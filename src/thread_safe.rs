use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

#[cfg(not(feature = "vec_edges"))]
use smallvec::SmallVec;
#[cfg(feature = "instrumentation")]
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock};
#[cfg(feature = "instrumentation")]
use std::time::Instant;

use crate::cell::CellHandle;
use crate::context::SlotId;
use crate::effect::EffectHandle;
#[cfg(feature = "instrumentation")]
use crate::instrumentation::ThreadSafeLockSite;
use crate::slot::SlotHandle;

type ThreadSafeAny = dyn Any + Send + Sync;
type ThreadSafeComputeFn = dyn Fn(&ThreadSafeContext) -> Box<ThreadSafeAny> + Send + Sync;
type ThreadSafeEqualsFn = dyn Fn(&ThreadSafeAny, &ThreadSafeAny) -> bool + Send + Sync;
type ThreadSafeCleanup = dyn FnOnce() + Send;
type ThreadSafeEffectFn =
    dyn Fn(&ThreadSafeContext) -> Option<Box<ThreadSafeCleanup>> + Send + Sync;

#[cfg(not(feature = "vec_edges"))]
type EdgeVec = SmallVec<[SlotId; 4]>;
#[cfg(feature = "vec_edges")]
type EdgeVec = Vec<SlotId>;

#[cfg(not(feature = "vec_edges"))]
type DependentEdgeVec = SmallVec<[(SlotId, ThreadSafeDependentKind); 4]>;
#[cfg(feature = "vec_edges")]
type DependentEdgeVec = Vec<(SlotId, ThreadSafeDependentKind)>;

fn edge_insert(edges: &mut EdgeVec, id: SlotId) -> bool {
    if edges.contains(&id) {
        false
    } else {
        edges.push(id);
        true
    }
}

fn edge_remove(edges: &mut EdgeVec, id: SlotId) -> bool {
    if let Some(pos) = edges.iter().position(|eid| *eid == id) {
        edges.swap_remove(pos);
        true
    } else {
        false
    }
}

fn dependent_edge_insert(edges: &mut DependentEdgeVec, id: SlotId, kind: ThreadSafeDependentKind) {
    if let Some(entry) = edges.iter_mut().find(|(eid, _)| *eid == id) {
        entry.1 = kind;
    } else {
        edges.push((id, kind));
    }
}

fn dependent_edge_remove(edges: &mut DependentEdgeVec, id: SlotId) {
    if let Some(pos) = edges.iter().position(|(eid, _)| *eid == id) {
        edges.swap_remove(pos);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThreadSafeContextId(usize);

struct ThreadSafeTrackingFrame {
    context_id: ThreadSafeContextId,
    node_id: SlotId,
    known_dependencies: EdgeVec,
    dependencies: HashSet<SlotId>,
}

#[derive(Default)]
struct ThreadSafeBatchChanges {
    cells: EdgeVec,
    cell_clears: EdgeVec,
    slots: EdgeVec,
}

struct ThreadSafeBatchFrame {
    context_id: ThreadSafeContextId,
    changes: ThreadSafeBatchChanges,
}

thread_local! {
    static THREAD_SAFE_TRACKING_STACK: RefCell<Vec<ThreadSafeTrackingFrame>> =
        const { RefCell::new(Vec::new()) };
}

thread_local! {
    static THREAD_SAFE_BATCH_STACK: RefCell<Vec<ThreadSafeBatchFrame>> =
        const { RefCell::new(Vec::new()) };
}

#[cfg(feature = "instrumentation")]
thread_local! {
    static THREAD_SAFE_LOCK_SITE_STACK: RefCell<Vec<ThreadSafeLockSite>> =
        const { RefCell::new(Vec::new()) };
}

struct TrackingGuard {
    active: bool,
}

impl TrackingGuard {
    fn finish(mut self) -> HashSet<SlotId> {
        self.active = false;
        THREAD_SAFE_TRACKING_STACK.with(|stack| {
            stack
                .borrow_mut()
                .pop()
                .map(|frame| frame.dependencies)
                .unwrap_or_default()
        })
    }
}

impl Drop for TrackingGuard {
    fn drop(&mut self) {
        if self.active {
            THREAD_SAFE_TRACKING_STACK.with(|stack| {
                stack.borrow_mut().pop();
            });
        }
    }
}

fn push_tracking_frame_with_known_dependencies(
    context_id: ThreadSafeContextId,
    node_id: SlotId,
    known_dependencies: EdgeVec,
) -> TrackingGuard {
    THREAD_SAFE_TRACKING_STACK.with(|stack| {
        stack.borrow_mut().push(ThreadSafeTrackingFrame {
            context_id,
            node_id,
            known_dependencies,
            dependencies: HashSet::new(),
        });
    });
    TrackingGuard { active: true }
}

fn track_dependency(context_id: ThreadSafeContextId, dependency_id: SlotId) -> Option<SlotId> {
    THREAD_SAFE_TRACKING_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        for frame in stack.iter_mut().rev() {
            if frame.context_id == context_id {
                let newly_tracked = frame.dependencies.insert(dependency_id);
                if newly_tracked && !frame.known_dependencies.contains(&dependency_id) {
                    return Some(frame.node_id);
                }
                return None;
            }
        }
        None
    })
}

fn push_batch_frame(context_id: ThreadSafeContextId) {
    THREAD_SAFE_BATCH_STACK.with(|stack| {
        stack.borrow_mut().push(ThreadSafeBatchFrame {
            context_id,
            changes: ThreadSafeBatchChanges::default(),
        });
    });
}

fn pop_batch_frame(context_id: ThreadSafeContextId) -> ThreadSafeBatchChanges {
    THREAD_SAFE_BATCH_STACK.with(|stack| {
        let frame = stack
            .borrow_mut()
            .pop()
            .expect("ThreadSafeContext batch frame stack underflow");
        assert_eq!(
            frame.context_id, context_id,
            "ThreadSafeContext batch frame mismatch"
        );
        frame.changes
    })
}

fn queue_batch_change<F>(context_id: ThreadSafeContextId, apply: F) -> bool
where
    F: FnOnce(&mut ThreadSafeBatchChanges),
{
    THREAD_SAFE_BATCH_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let Some(frame) = stack
            .iter_mut()
            .rev()
            .find(|frame| frame.context_id == context_id)
        else {
            return false;
        };
        apply(&mut frame.changes);
        true
    })
}

#[cfg(feature = "instrumentation")]
struct ThreadSafeLockSiteGuard;

#[cfg(feature = "instrumentation")]
impl Drop for ThreadSafeLockSiteGuard {
    fn drop(&mut self) {
        THREAD_SAFE_LOCK_SITE_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

#[cfg(feature = "instrumentation")]
fn push_thread_safe_lock_site(site: ThreadSafeLockSite) -> ThreadSafeLockSiteGuard {
    THREAD_SAFE_LOCK_SITE_STACK.with(|stack| {
        stack.borrow_mut().push(site);
    });
    ThreadSafeLockSiteGuard
}

#[cfg(feature = "instrumentation")]
fn current_thread_safe_lock_site() -> ThreadSafeLockSite {
    THREAD_SAFE_LOCK_SITE_STACK.with(|stack| {
        stack
            .borrow()
            .last()
            .copied()
            .unwrap_or(ThreadSafeLockSite::Other)
    })
}

struct ThreadSafeSlotNode {
    value: Option<Arc<ThreadSafeAny>>,
    equals: Option<Arc<ThreadSafeEqualsFn>>,
    dependencies: EdgeVec,
    dependents: EdgeVec,
    fast_path: Arc<ThreadSafeSlotFastPath>,
    dirty: bool,
    force_recompute: bool,
    revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreadSafeDependentKind {
    Slot,
    Effect,
}

struct ThreadSafeSlotFastPath {
    value: RwLock<Option<Arc<ThreadSafeAny>>>,
    cache_revision: AtomicU64,
    dirty: AtomicBool,
    force_recompute: AtomicBool,
    compute: Arc<ThreadSafeComputeFn>,
    dependencies: Mutex<EdgeVec>,
    slot_dependency_count: AtomicUsize,
    recompute: Mutex<ThreadSafeSlotRecomputeState>,
    recompute_condvar: Condvar,
    dependents: Mutex<DependentEdgeVec>,
}

impl ThreadSafeSlotFastPath {
    fn new(compute: Arc<ThreadSafeComputeFn>, initial_dependencies: EdgeVec) -> Self {
        let slot_dependency_count = initial_dependencies.len();
        Self {
            value: RwLock::new(None),
            cache_revision: AtomicU64::default(),
            dirty: AtomicBool::default(),
            force_recompute: AtomicBool::default(),
            compute,
            dependencies: Mutex::new(initial_dependencies),
            slot_dependency_count: AtomicUsize::new(slot_dependency_count),
            recompute: Mutex::new(ThreadSafeSlotRecomputeState::default()),
            recompute_condvar: Condvar::new(),
            dependents: Mutex::new(DependentEdgeVec::new()),
        }
    }

    fn compute(&self) -> Arc<ThreadSafeComputeFn> {
        Arc::clone(&self.compute)
    }

    fn read_fresh<T>(&self) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let cache_revision = self.cache_revision.load(Ordering::Acquire);
        if self.dirty.load(Ordering::Acquire) || self.force_recompute.load(Ordering::Acquire) {
            return None;
        }

        let value = self
            .value
            .read()
            .expect("ThreadSafeContext slot fast path rwlock poisoned")
            .as_ref()
            .map(|value| {
                value
                    .downcast_ref::<T>()
                    .expect("type mismatch in slot")
                    .clone()
            });
        if self.cache_revision.load(Ordering::Acquire) != cache_revision
            || self.dirty.load(Ordering::Acquire)
            || self.force_recompute.load(Ordering::Acquire)
        {
            return None;
        }

        value
    }

    fn needs_refresh(&self) -> bool {
        self.dirty.load(Ordering::Acquire) || self.force_recompute.load(Ordering::Acquire)
    }

    fn needs_refresh_without_slot_dependencies(&self) -> bool {
        self.needs_refresh() && self.slot_dependency_count.load(Ordering::Acquire) == 0
    }

    fn dirty_force(&self) -> (bool, bool) {
        let recompute = self.lock_recompute_state();
        (recompute.dirty, recompute.force_recompute)
    }

    fn store_value(&self, value: Option<Arc<ThreadSafeAny>>) {
        *self
            .value
            .write()
            .expect("ThreadSafeContext slot fast path rwlock poisoned") = value;
        self.cache_revision.fetch_add(1, Ordering::AcqRel);
    }

    fn mark_dirty(&self, force_recompute: bool) {
        {
            let mut recompute = self.lock_recompute_state();
            recompute.revision = recompute.revision.wrapping_add(1);
            recompute.dirty = true;
            recompute.force_recompute |= force_recompute;
        }
        self.cache_revision.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
        if force_recompute {
            self.force_recompute.store(true, Ordering::Release);
        }
    }

    fn mark_fresh(&self, has_value: bool) {
        {
            let mut recompute = self.lock_recompute_state();
            recompute.has_value = has_value;
            recompute.dirty = false;
            recompute.force_recompute = false;
        }
        self.force_recompute.store(false, Ordering::Release);
        self.dirty.store(false, Ordering::Release);
    }

    fn clear(&self) {
        self.store_value(None);
        {
            let mut recompute = self.lock_recompute_state();
            recompute.revision = recompute.revision.wrapping_add(1);
            recompute.has_value = false;
            recompute.dirty = false;
            recompute.force_recompute = false;
        }
        self.force_recompute.store(false, Ordering::Release);
        self.dirty.store(false, Ordering::Release);
    }

    fn lock_recompute_state(&self) -> MutexGuard<'_, ThreadSafeSlotRecomputeState> {
        self.recompute
            .lock()
            .expect("ThreadSafeContext slot recompute mutex poisoned")
    }

    fn begin_recompute(&self) -> Option<ThreadSafeRecomputeStart> {
        let mut recompute = self.lock_recompute_state();
        if recompute.computing {
            return None;
        }
        recompute.computing = true;
        Some(ThreadSafeRecomputeStart {
            revision: recompute.revision,
            was_unset: !recompute.has_value,
        })
    }

    fn recompute_in_flight(&self) -> bool {
        self.lock_recompute_state().computing
    }

    fn current_recompute_revision(&self) -> u64 {
        self.lock_recompute_state().revision
    }

    fn finish_recompute(&self) {
        let notify_waiter = {
            let mut recompute = self.lock_recompute_state();
            recompute.computing = false;
            recompute.waiters > 0
        };
        if notify_waiter {
            self.recompute_condvar.notify_one();
        }
    }

    fn wait_for_recompute(&self) -> ThreadSafeRecomputeResult {
        let mut recompute = self.lock_recompute_state();
        let mut registered_waiter = false;
        if recompute.computing {
            recompute.waiters = recompute.waiters.saturating_add(1);
            registered_waiter = true;
        }
        while recompute.computing {
            recompute = self
                .recompute_condvar
                .wait(recompute)
                .expect("ThreadSafeContext slot recompute mutex poisoned while waiting");
        }

        let notify_next_waiter = if registered_waiter {
            debug_assert!(recompute.waiters > 0);
            recompute.waiters -= 1;
            recompute.waiters > 0
        } else {
            false
        };
        if recompute.has_value && !recompute.dirty && !recompute.force_recompute {
            drop(recompute);
            if notify_next_waiter {
                self.recompute_condvar.notify_one();
            }
            ThreadSafeRecomputeResult::Fresh(false)
        } else {
            drop(recompute);
            if notify_next_waiter {
                self.recompute_condvar.notify_one();
            }
            ThreadSafeRecomputeResult::Stale
        }
    }

    fn dependencies_snapshot(&self) -> EdgeVec {
        self.dependencies
            .lock()
            .expect("ThreadSafeContext slot dependencies mutex poisoned")
            .clone()
    }

    fn insert_dependency(&self, dependency_id: SlotId, dependency_is_slot: bool) {
        let inserted = edge_insert(
            &mut self
                .dependencies
                .lock()
                .expect("ThreadSafeContext slot dependencies mutex poisoned"),
            dependency_id,
        );
        if inserted && dependency_is_slot {
            self.slot_dependency_count.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn remove_dependency(&self, dependency_id: SlotId, dependency_is_slot: bool) {
        let removed = edge_remove(
            &mut self
                .dependencies
                .lock()
                .expect("ThreadSafeContext slot dependencies mutex poisoned"),
            dependency_id,
        );
        if removed && dependency_is_slot {
            self.slot_dependency_count.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn insert_dependent(&self, dependent_id: SlotId, kind: ThreadSafeDependentKind) {
        dependent_edge_insert(
            &mut self
                .dependents
                .lock()
                .expect("ThreadSafeContext slot dependents mutex poisoned"),
            dependent_id,
            kind,
        );
    }

    fn remove_dependent(&self, dependent_id: SlotId) {
        dependent_edge_remove(
            &mut self
                .dependents
                .lock()
                .expect("ThreadSafeContext slot dependents mutex poisoned"),
            dependent_id,
        );
    }

    fn dependents_snapshot(&self) -> Vec<(SlotId, ThreadSafeDependentKind)> {
        self.dependents
            .lock()
            .expect("ThreadSafeContext slot dependents mutex poisoned")
            .to_vec()
    }
}

#[derive(Default)]
struct ThreadSafeSlotRecomputeState {
    has_value: bool,
    dirty: bool,
    force_recompute: bool,
    computing: bool,
    waiters: usize,
    revision: u64,
}

struct ThreadSafeRecomputeStart {
    revision: u64,
    was_unset: bool,
}

struct ThreadSafeCellNode {
    dependents: EdgeVec,
    fast_path: Arc<ThreadSafeCellFastPath>,
}

struct ThreadSafeCellFastPath {
    value: Mutex<Box<ThreadSafeAny>>,
    dependents: Mutex<DependentEdgeVec>,
}

impl ThreadSafeCellFastPath {
    fn new<T>(value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            value: Mutex::new(Box::new(value)),
            dependents: Mutex::new(DependentEdgeVec::new()),
        }
    }

    fn get<T>(&self) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.value
            .lock()
            .expect("ThreadSafeContext cell value mutex poisoned")
            .downcast_ref::<T>()
            .expect("type mismatch in cell")
            .clone()
    }

    fn set_if_changed<T>(&self, new_value: T) -> bool
    where
        T: PartialEq + Send + Sync + 'static,
    {
        let mut value = self
            .value
            .lock()
            .expect("ThreadSafeContext cell value mutex poisoned");
        let old = value
            .downcast_ref::<T>()
            .expect("type mismatch in cell set");
        if *old == new_value {
            return false;
        }
        *value = Box::new(new_value);
        true
    }

    fn insert_dependent(&self, dependent_id: SlotId, kind: ThreadSafeDependentKind) {
        dependent_edge_insert(
            &mut self
                .dependents
                .lock()
                .expect("ThreadSafeContext cell dependents mutex poisoned"),
            dependent_id,
            kind,
        );
    }

    fn remove_dependent(&self, dependent_id: SlotId) {
        dependent_edge_remove(
            &mut self
                .dependents
                .lock()
                .expect("ThreadSafeContext cell dependents mutex poisoned"),
            dependent_id,
        );
    }

    fn dependents_snapshot(&self) -> Vec<(SlotId, ThreadSafeDependentKind)> {
        self.dependents
            .lock()
            .expect("ThreadSafeContext cell dependents mutex poisoned")
            .to_vec()
    }
}

struct ThreadSafeEffectNode {
    run: Arc<ThreadSafeEffectFn>,
    dependencies: EdgeVec,
    cleanup: Option<Box<ThreadSafeCleanup>>,
    force_run: bool,
}

enum ThreadSafeNode {
    Slot(ThreadSafeSlotNode),
    Cell(ThreadSafeCellNode),
    Effect(ThreadSafeEffectNode),
}

enum ThreadSafeRecomputeResult {
    Fresh(bool),
    Stale,
}

enum ThreadSafeSlotRead<T> {
    Fresh(T),
    Refresh(EdgeVec),
}

#[derive(Clone, Copy)]
struct ThreadSafeInvalidationRoot {
    id: SlotId,
    force_recompute: bool,
}

#[derive(Default)]
struct ThreadSafeInvalidationPlan {
    slot_marks: Vec<(SlotId, bool)>,
    slot_clears: Vec<SlotId>,
    effect_schedules: Vec<(SlotId, bool)>,
}

impl ThreadSafeInvalidationPlan {
    fn from_roots_locked<I>(state: &ThreadSafeState, roots: I) -> Self
    where
        I: IntoIterator<Item = ThreadSafeInvalidationRoot>,
    {
        let mut queue = VecDeque::new();
        let mut requested_force: Vec<(SlotId, bool)> = Vec::new();
        for root in roots {
            Self::enqueue_root(&mut queue, &mut requested_force, root);
        }

        let mut plan = Self::default();
        let mut simulated_slots: Vec<(SlotId, (bool, bool))> = Vec::new();

        while let Some(root) = queue.pop_front() {
            let force_recompute = match Self::find_force(&requested_force, root.id) {
                Some(f) if root.force_recompute == f => f,
                _ => continue,
            };

            let dependents = match state.get_node(root.id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    let (dirty, force_state) = match Self::find_simulated(&simulated_slots, root.id)
                    {
                        Some(state) => state,
                        None => (slot.dirty, slot.force_recompute),
                    };
                    let should_propagate = !dirty || (force_recompute && !force_state);
                    Self::set_simulated(
                        &mut simulated_slots,
                        root.id,
                        (true, force_state || force_recompute),
                    );

                    plan.add_slot_mark(root.id, force_recompute);

                    if should_propagate {
                        sorted_slot_ids(slot.dependents.iter().copied())
                    } else {
                        Vec::new()
                    }
                }
                Some(ThreadSafeNode::Effect(_)) => {
                    plan.add_effect_schedule(root.id, force_recompute);
                    Vec::new()
                }
                Some(ThreadSafeNode::Cell(_)) | None => Vec::new(),
            };

            for dependent_id in dependents {
                Self::enqueue_root(
                    &mut queue,
                    &mut requested_force,
                    ThreadSafeInvalidationRoot {
                        id: dependent_id,
                        force_recompute: false,
                    },
                );
            }
        }

        plan
    }

    fn from_clear_roots_locked<I>(state: &ThreadSafeState, roots: I) -> Self
    where
        I: IntoIterator<Item = SlotId>,
    {
        let mut plan = Self::default();
        let mut queue = roots.into_iter().collect::<VecDeque<_>>();
        let mut visited_slots: Vec<SlotId> = Vec::new();

        while let Some(id) = queue.pop_front() {
            match state.get_node(id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    if visited_slots.contains(&id) {
                        continue;
                    }
                    visited_slots.push(id);
                    if slot.value.is_none() && !slot.dirty {
                        continue;
                    }
                    plan.add_slot_clear(id);
                    for dependent_id in sorted_slot_ids(slot.dependents.iter().copied()) {
                        queue.push_back(dependent_id);
                    }
                }
                Some(ThreadSafeNode::Effect(_)) => {
                    plan.add_effect_schedule(id, true);
                }
                Some(ThreadSafeNode::Cell(_)) | None => {}
            }
        }

        plan
    }

    fn apply_locked(self, state: &mut ThreadSafeState) {
        for id in &self.slot_clears {
            let Some(ThreadSafeNode::Slot(slot)) = state.get_node_mut(*id) else {
                continue;
            };
            slot.value = None;
            slot.dirty = false;
            slot.force_recompute = false;
            slot.revision = slot.revision.wrapping_add(1);
            slot.fast_path.clear();
        }

        #[cfg(feature = "instrumentation")]
        let mut dirty_epoch_advances = 0usize;
        for &(id, force_recompute) in &self.slot_marks {
            if self.slot_clears.contains(&id) {
                continue;
            }
            let Some(ThreadSafeNode::Slot(slot)) = state.get_node_mut(id) else {
                continue;
            };
            slot.revision = slot.revision.wrapping_add(1);
            slot.dirty = true;
            if force_recompute {
                slot.force_recompute = true;
            }
            slot.fast_path.mark_dirty(slot.force_recompute);
            #[cfg(feature = "instrumentation")]
            {
                dirty_epoch_advances = dirty_epoch_advances.saturating_add(1);
            }
        }
        #[cfg(feature = "instrumentation")]
        if dirty_epoch_advances > 0 {
            state
                .instrumentation
                .record_dirty_epoch_advances(dirty_epoch_advances);
        }

        for &(id, force) in &self.effect_schedules {
            ThreadSafeContext::schedule_effect_locked(state, id, force);
        }
    }

    fn add_slot_mark(&mut self, id: SlotId, force_recompute: bool) {
        if let Some((_, force)) = self.slot_marks.iter_mut().find(|(sid, _)| *sid == id) {
            *force |= force_recompute;
        } else {
            self.slot_marks.push((id, force_recompute));
        }
    }

    fn add_slot_clear(&mut self, id: SlotId) {
        if !self.slot_clears.contains(&id) {
            self.slot_clears.push(id);
        }
    }

    fn add_effect_schedule(&mut self, id: SlotId, force: bool) {
        if let Some((_, existing_force)) =
            self.effect_schedules.iter_mut().find(|(sid, _)| *sid == id)
        {
            *existing_force |= force;
        } else {
            self.effect_schedules.push((id, force));
        }
    }

    fn find_force(vec: &[(SlotId, bool)], id: SlotId) -> Option<bool> {
        vec.iter().find(|(sid, _)| *sid == id).map(|(_, f)| *f)
    }

    fn find_simulated(vec: &[(SlotId, (bool, bool))], id: SlotId) -> Option<(bool, bool)> {
        vec.iter()
            .find(|(sid, _)| *sid == id)
            .map(|(_, state)| *state)
    }

    fn set_simulated(vec: &mut Vec<(SlotId, (bool, bool))>, id: SlotId, value: (bool, bool)) {
        if let Some((_, state)) = vec.iter_mut().find(|(sid, _)| *sid == id) {
            *state = value;
        } else {
            vec.push((id, value));
        }
    }

    fn enqueue_root(
        queue: &mut VecDeque<ThreadSafeInvalidationRoot>,
        force_vec: &mut Vec<(SlotId, bool)>,
        root: ThreadSafeInvalidationRoot,
    ) {
        if let Some((_, force)) = force_vec.iter_mut().find(|(sid, _)| *sid == root.id) {
            *force |= root.force_recompute;
        } else {
            force_vec.push((root.id, root.force_recompute));
            queue.push_back(root);
        }
    }
}

fn sorted_slot_ids<I>(ids: I) -> Vec<SlotId>
where
    I: IntoIterator<Item = SlotId>,
{
    let mut ids = ids.into_iter().collect::<Vec<_>>();
    ids.sort_by_key(|id| id.0);
    ids
}

#[derive(Default)]
struct ThreadSafeState {
    nodes: Vec<Option<ThreadSafeNode>>,
    next_id: u64,
    free_ids: Vec<u64>,
    pending_effects: VecDeque<SlotId>,
    scheduled_effects: Vec<bool>,
    flushing_effects: bool,
    batch_depth: usize,
    batched_cells: EdgeVec,
    batched_cell_clears: EdgeVec,
    batched_slots: EdgeVec,
    dependent_scratch: Vec<SlotId>,
    #[cfg(feature = "instrumentation")]
    instrumentation: crate::instrumentation::InstrumentationCounters,
}

impl ThreadSafeState {
    fn get_node(&self, id: SlotId) -> Option<&ThreadSafeNode> {
        self.nodes.get(id.0 as usize).and_then(|opt| opt.as_ref())
    }

    fn get_node_mut(&mut self, id: SlotId) -> Option<&mut ThreadSafeNode> {
        self.nodes
            .get_mut(id.0 as usize)
            .and_then(|opt| opt.as_mut())
    }

    fn insert_node(&mut self, id: SlotId, node: ThreadSafeNode) {
        let idx = id.0 as usize;
        if idx >= self.nodes.len() {
            self.nodes.resize_with(idx + 1, || None);
        }
        self.nodes[idx] = Some(node);
    }

    fn remove_node(&mut self, id: SlotId) -> Option<ThreadSafeNode> {
        let idx = id.0 as usize;
        self.nodes.get_mut(idx).and_then(|slot| slot.take())
    }

    fn deschedule_effect(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if idx < self.scheduled_effects.len() {
            self.scheduled_effects[idx] = false;
        }
    }

    #[cfg(test)]
    fn is_effect_scheduled(&self, id: SlotId) -> bool {
        let idx = id.0 as usize;
        idx < self.scheduled_effects.len() && self.scheduled_effects[idx]
    }

    fn fill_dependent_scratch(&mut self, id: SlotId) {
        self.dependent_scratch.clear();
        let idx = id.0 as usize;
        let deps: &[SlotId] = match self.nodes.get(idx).and_then(|opt| opt.as_ref()) {
            Some(ThreadSafeNode::Slot(slot)) => slot.dependents.as_slice(),
            Some(ThreadSafeNode::Cell(cell)) => cell.dependents.as_slice(),
            _ => return,
        };
        self.dependent_scratch.extend_from_slice(deps);
    }
}

struct ThreadSafeInner {
    state: Mutex<ThreadSafeState>,
    slot_fast_paths: RwLock<Vec<Option<Arc<ThreadSafeSlotFastPath>>>>,
    cell_fast_paths: RwLock<Vec<Option<Arc<ThreadSafeCellFastPath>>>>,
    batch_depth: AtomicUsize,
    active_callbacks: AtomicUsize,
    #[cfg(feature = "instrumentation")]
    lock_instrumentation: crate::instrumentation::ThreadSafeLockInstrumentation,
    #[cfg(feature = "instrumentation")]
    invalidation_instrumentation: crate::instrumentation::ThreadSafeInvalidationInstrumentation,
}

impl Default for ThreadSafeInner {
    fn default() -> Self {
        Self {
            state: Mutex::new(ThreadSafeState::default()),
            slot_fast_paths: RwLock::new(Vec::new()),
            cell_fast_paths: RwLock::new(Vec::new()),
            batch_depth: AtomicUsize::new(0),
            active_callbacks: AtomicUsize::new(0),
            #[cfg(feature = "instrumentation")]
            lock_instrumentation: crate::instrumentation::ThreadSafeLockInstrumentation::default(),
            #[cfg(feature = "instrumentation")]
            invalidation_instrumentation:
                crate::instrumentation::ThreadSafeInvalidationInstrumentation::default(),
        }
    }
}

#[cfg(feature = "instrumentation")]
struct ProfiledReadGuard<'a> {
    guard: Option<MutexGuard<'a, ThreadSafeState>>,
    lock_instrumentation: &'a crate::instrumentation::ThreadSafeLockInstrumentation,
    site: ThreadSafeLockSite,
    acquired_at: Instant,
}

#[cfg(feature = "instrumentation")]
struct ProfiledWriteGuard<'a> {
    guard: Option<MutexGuard<'a, ThreadSafeState>>,
    lock_instrumentation: &'a crate::instrumentation::ThreadSafeLockInstrumentation,
    site: ThreadSafeLockSite,
    acquired_at: Instant,
}

#[cfg(feature = "instrumentation")]
impl Deref for ProfiledReadGuard<'_> {
    type Target = ThreadSafeState;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_ref()
            .expect("profiled mutex read guard missing during deref")
    }
}

#[cfg(feature = "instrumentation")]
impl Drop for ProfiledReadGuard<'_> {
    fn drop(&mut self) {
        if self.guard.is_some() {
            self.lock_instrumentation
                .record_lock_hold(self.site, self.acquired_at.elapsed());
        }
    }
}

#[cfg(feature = "instrumentation")]
impl Deref for ProfiledWriteGuard<'_> {
    type Target = ThreadSafeState;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_ref()
            .expect("profiled mutex write guard missing during deref")
    }
}

#[cfg(feature = "instrumentation")]
impl DerefMut for ProfiledWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_mut()
            .expect("profiled mutex write guard missing during mutable deref")
    }
}

#[cfg(feature = "instrumentation")]
impl Drop for ProfiledWriteGuard<'_> {
    fn drop(&mut self) {
        if self.guard.is_some() {
            self.lock_instrumentation
                .record_lock_hold(self.site, self.acquired_at.elapsed());
        }
    }
}

/// Return value accepted by [`ThreadSafeContext::effect`].
///
/// Returning `()` registers no cleanup. Returning a `Send` cleanup closure
/// registers that closure for the current effect run.
pub trait ThreadSafeEffectCallbackResult {
    fn into_thread_safe_cleanup(self) -> Option<Box<ThreadSafeCleanup>>;
}

impl ThreadSafeEffectCallbackResult for () {
    fn into_thread_safe_cleanup(self) -> Option<Box<ThreadSafeCleanup>> {
        None
    }
}

impl<F> ThreadSafeEffectCallbackResult for F
where
    F: FnOnce() + Send + 'static,
{
    fn into_thread_safe_cleanup(self) -> Option<Box<ThreadSafeCleanup>> {
        Some(Box::new(self))
    }
}

/// Lock-backed context for sharing lazy reactive state across OS threads.
///
/// This type mirrors the core [`crate::Context`] API while requiring
/// `Send + Sync + 'static` values and callbacks. The graph lock is released
/// before user compute/effect/cleanup callbacks run, so callbacks may re-enter
/// the same context without deadlocking.
#[derive(Clone, Default)]
pub struct ThreadSafeContext {
    inner: Arc<ThreadSafeInner>,
}

struct BatchGuard {
    ctx: ThreadSafeContext,
    context_id: ThreadSafeContextId,
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        let changes = pop_batch_frame(self.context_id);
        self.ctx.finish_batch(changes);
    }
}

struct RecomputeGuard {
    fast_path: Arc<ThreadSafeSlotFastPath>,
    active: bool,
}

impl Drop for RecomputeGuard {
    fn drop(&mut self) {
        if self.active {
            self.fast_path.finish_recompute();
        }
    }
}

struct CallbackActivityGuard {
    inner: Arc<ThreadSafeInner>,
}

impl Drop for CallbackActivityGuard {
    fn drop(&mut self) {
        self.inner.active_callbacks.fetch_sub(1, Ordering::AcqRel);
    }
}

struct FlushGuard {
    ctx: ThreadSafeContext,
    active: bool,
}

impl Drop for FlushGuard {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.ctx.lock_state();
            state.flushing_effects = false;
        }
    }
}

impl ThreadSafeContext {
    pub fn new() -> Self {
        Self::default()
    }

    fn context_id(&self) -> ThreadSafeContextId {
        ThreadSafeContextId(Arc::as_ptr(&self.inner) as usize)
    }

    #[cfg(not(feature = "instrumentation"))]
    fn read_state(&self) -> MutexGuard<'_, ThreadSafeState> {
        self.inner
            .state
            .lock()
            .expect("ThreadSafeContext mutex poisoned")
    }

    #[cfg(not(feature = "instrumentation"))]
    fn lock_state(&self) -> MutexGuard<'_, ThreadSafeState> {
        self.inner
            .state
            .lock()
            .expect("ThreadSafeContext mutex poisoned")
    }

    #[cfg(feature = "instrumentation")]
    fn read_state(&self) -> ProfiledReadGuard<'_> {
        let site = current_thread_safe_lock_site();
        let wait_started = Instant::now();
        let guard = self
            .inner
            .state
            .lock()
            .expect("ThreadSafeContext mutex poisoned");
        self.inner
            .lock_instrumentation
            .record_lock_wait(site, wait_started.elapsed());
        ProfiledReadGuard {
            guard: Some(guard),
            lock_instrumentation: &self.inner.lock_instrumentation,
            site,
            acquired_at: Instant::now(),
        }
    }

    #[cfg(feature = "instrumentation")]
    fn lock_state(&self) -> ProfiledWriteGuard<'_> {
        let site = current_thread_safe_lock_site();
        let wait_started = Instant::now();
        let guard = self
            .inner
            .state
            .lock()
            .expect("ThreadSafeContext mutex poisoned");
        self.inner
            .lock_instrumentation
            .record_lock_wait(site, wait_started.elapsed());
        ProfiledWriteGuard {
            guard: Some(guard),
            lock_instrumentation: &self.inner.lock_instrumentation,
            site,
            acquired_at: Instant::now(),
        }
    }

    #[cfg(feature = "instrumentation")]
    fn record_coordination_lock(&self, site: ThreadSafeLockSite) {
        self.inner
            .lock_instrumentation
            .record_lock_wait(site, std::time::Duration::ZERO);
        self.inner
            .lock_instrumentation
            .record_lock_hold(site, std::time::Duration::ZERO);
    }

    fn alloc_id(&self) -> SlotId {
        let mut state = self.lock_state();
        let slot_id = match state.free_ids.pop() {
            Some(id) => SlotId(id),
            None => {
                let id = SlotId(state.next_id);
                state.next_id += 1;
                id
            }
        };
        #[cfg(feature = "instrumentation")]
        {
            state.instrumentation.record_node_allocation();
        }
        slot_id
    }

    fn slot_fast_path(&self, id: SlotId) -> Option<Arc<ThreadSafeSlotFastPath>> {
        self.inner
            .slot_fast_paths
            .read()
            .expect("ThreadSafeContext slot fast path registry poisoned")
            .get(id.0 as usize)
            .and_then(|opt| opt.as_ref().cloned())
    }

    fn cell_fast_path(&self, id: SlotId) -> Option<Arc<ThreadSafeCellFastPath>> {
        self.inner
            .cell_fast_paths
            .read()
            .expect("ThreadSafeContext cell fast path registry poisoned")
            .get(id.0 as usize)
            .and_then(|opt| opt.as_ref().cloned())
    }

    fn try_read_fresh_slot_fast_path<T>(&self, id: SlotId) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.slot_fast_path(id)
            .and_then(|fast_path| fast_path.read_fresh())
    }

    fn slot_recompute_in_flight(&self, id: SlotId) -> bool {
        self.slot_fast_path(id)
            .map(|fast_path| fast_path.recompute_in_flight())
            .unwrap_or(false)
    }

    fn slot_needs_refresh_without_slot_dependencies(&self, id: SlotId) -> bool {
        self.slot_fast_path(id)
            .map(|fast_path| fast_path.needs_refresh_without_slot_dependencies())
            .unwrap_or(false)
    }

    fn callbacks_active(&self) -> bool {
        self.inner.active_callbacks.load(Ordering::Acquire) > 0
    }

    fn callback_activity(&self) -> CallbackActivityGuard {
        self.inner.active_callbacks.fetch_add(1, Ordering::AcqRel);
        CallbackActivityGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    fn dependent_kind_locked(
        state: &ThreadSafeState,
        dependent_id: SlotId,
    ) -> Option<ThreadSafeDependentKind> {
        match state.get_node(dependent_id) {
            Some(ThreadSafeNode::Slot(_)) => Some(ThreadSafeDependentKind::Slot),
            Some(ThreadSafeNode::Effect(_)) => Some(ThreadSafeDependentKind::Effect),
            Some(ThreadSafeNode::Cell(_)) | None => None,
        }
    }

    fn insert_dependent_sidecar_locked(
        state: &ThreadSafeState,
        dependency_id: SlotId,
        dependent_id: SlotId,
        dependent_kind: ThreadSafeDependentKind,
    ) {
        match state.get_node(dependency_id) {
            Some(ThreadSafeNode::Slot(slot)) => {
                slot.fast_path
                    .insert_dependent(dependent_id, dependent_kind);
            }
            Some(ThreadSafeNode::Cell(cell)) => {
                cell.fast_path
                    .insert_dependent(dependent_id, dependent_kind);
            }
            Some(ThreadSafeNode::Effect(_)) | None => {}
        }
    }

    fn remove_dependent_sidecar_locked(
        state: &ThreadSafeState,
        dependency_id: SlotId,
        dependent_id: SlotId,
    ) {
        match state.get_node(dependency_id) {
            Some(ThreadSafeNode::Slot(slot)) => {
                slot.fast_path.remove_dependent(dependent_id);
            }
            Some(ThreadSafeNode::Cell(cell)) => {
                cell.fast_path.remove_dependent(dependent_id);
            }
            Some(ThreadSafeNode::Effect(_)) | None => {}
        }
    }

    fn register_dependency(&self, dependency_id: SlotId, dependent_id: SlotId) {
        if dependency_id == dependent_id {
            return;
        }

        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::DependencyEdge);
        #[cfg(feature = "instrumentation")]
        let mut edge_added = false;
        let mut state = self.lock_state();
        let dependency_is_slot =
            matches!(state.get_node(dependency_id), Some(ThreadSafeNode::Slot(_)));
        let dependent_kind = Self::dependent_kind_locked(&state, dependent_id);
        if let Some(node) = state.get_node_mut(dependency_id) {
            match node {
                ThreadSafeNode::Slot(slot) => {
                    edge_insert(&mut slot.dependents, dependent_id);
                }
                ThreadSafeNode::Cell(cell) => {
                    edge_insert(&mut cell.dependents, dependent_id);
                }
                ThreadSafeNode::Effect(_) => {}
            }
        }

        if let Some(node) = state.get_node_mut(dependent_id) {
            match node {
                ThreadSafeNode::Slot(parent) => {
                    let inserted = edge_insert(&mut parent.dependencies, dependency_id);
                    if inserted {
                        parent
                            .fast_path
                            .insert_dependency(dependency_id, dependency_is_slot);
                    }
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = inserted;
                    }
                }
                ThreadSafeNode::Effect(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = edge_insert(&mut parent.dependencies, dependency_id);
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        edge_insert(&mut parent.dependencies, dependency_id);
                    }
                }
                ThreadSafeNode::Cell(_) => {}
            }
        }
        if let Some(dependent_kind) = dependent_kind {
            Self::insert_dependent_sidecar_locked(
                &state,
                dependency_id,
                dependent_id,
                dependent_kind,
            );
        }
        #[cfg(feature = "instrumentation")]
        if edge_added {
            state.instrumentation.record_dependency_edge_added();
        }
    }

    fn remove_dependent_edge(&self, dependency_id: SlotId, dependent_id: SlotId) {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::DependencyEdge);
        let mut state = self.lock_state();
        Self::remove_dependent_edge_locked(&mut state, dependency_id, dependent_id);
    }

    fn remove_stale_dependencies_locked(
        state: &mut ThreadSafeState,
        dependent_id: SlotId,
        old_dependencies: &EdgeVec,
        new_dependencies: &HashSet<SlotId>,
    ) {
        for dependency_id in old_dependencies.iter() {
            if !new_dependencies.contains(dependency_id) {
                Self::remove_parent_dependency_locked(state, dependent_id, *dependency_id);
                Self::remove_dependent_edge_locked(state, *dependency_id, dependent_id);
            }
        }
    }

    fn remove_parent_dependency_locked(
        state: &mut ThreadSafeState,
        dependent_id: SlotId,
        dependency_id: SlotId,
    ) -> bool {
        let dependency_is_slot =
            matches!(state.get_node(dependency_id), Some(ThreadSafeNode::Slot(_)));
        match state.get_node_mut(dependent_id) {
            Some(ThreadSafeNode::Slot(slot)) => {
                let removed = edge_remove(&mut slot.dependencies, dependency_id);
                if removed {
                    slot.fast_path
                        .remove_dependency(dependency_id, dependency_is_slot);
                }
                removed
            }
            Some(ThreadSafeNode::Effect(effect)) => {
                edge_remove(&mut effect.dependencies, dependency_id)
            }
            Some(ThreadSafeNode::Cell(_)) | None => false,
        }
    }

    fn remove_dependent_edge_locked(
        state: &mut ThreadSafeState,
        dependency_id: SlotId,
        dependent_id: SlotId,
    ) {
        let _edge_removed = match state.get_node_mut(dependency_id) {
            Some(ThreadSafeNode::Slot(slot)) => edge_remove(&mut slot.dependents, dependent_id),
            Some(ThreadSafeNode::Cell(cell)) => edge_remove(&mut cell.dependents, dependent_id),
            Some(ThreadSafeNode::Effect(_)) | None => false,
        };
        if _edge_removed {
            Self::remove_dependent_sidecar_locked(state, dependency_id, dependent_id);
        }

        #[cfg(feature = "instrumentation")]
        if _edge_removed {
            state.instrumentation.record_dependency_edge_removed();
        }
    }

    /// Create a new lazily-computed thread-safe slot.
    pub fn slot<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: Send + Sync + 'static,
        F: Fn(&ThreadSafeContext) -> T + Send + Sync + 'static,
    {
        self.slot_with_equals(compute, None)
    }

    /// Create a derived lazily-computed thread-safe value.
    ///
    /// This is an ergonomic alias for [`ThreadSafeContext::slot`].
    pub fn computed<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: Send + Sync + 'static,
        F: Fn(&ThreadSafeContext) -> T + Send + Sync + 'static,
    {
        self.slot(compute)
    }

    /// Create a lazily-computed thread-safe slot with a `PartialEq` guard.
    pub fn memo<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: PartialEq + Send + Sync + 'static,
        F: Fn(&ThreadSafeContext) -> T + Send + Sync + 'static,
    {
        self.slot_with_equals(
            compute,
            Some(Arc::new(|old, new| {
                let old = old.downcast_ref::<T>().expect("type mismatch in slot");
                let new = new.downcast_ref::<T>().expect("type mismatch in slot");
                old == new
            })),
        )
    }

    fn slot_with_equals<T, F>(
        &self,
        compute: F,
        equals: Option<Arc<ThreadSafeEqualsFn>>,
    ) -> SlotHandle<T>
    where
        T: Send + Sync + 'static,
        F: Fn(&ThreadSafeContext) -> T + Send + Sync + 'static,
    {
        let id = self.alloc_id();
        let compute: Arc<ThreadSafeComputeFn> =
            Arc::new(move |ctx: &ThreadSafeContext| Box::new(compute(ctx)));
        let fast_path = Arc::new(ThreadSafeSlotFastPath::new(
            Arc::clone(&compute),
            EdgeVec::new(),
        ));
        let node = ThreadSafeSlotNode {
            value: None,
            equals,
            dependencies: EdgeVec::new(),
            dependents: EdgeVec::new(),
            fast_path: Arc::clone(&fast_path),
            dirty: false,
            force_recompute: false,
            revision: 0,
        };
        let mut slot_fast_paths = self
            .inner
            .slot_fast_paths
            .write()
            .expect("ThreadSafeContext slot fast path registry poisoned");
        let idx = id.0 as usize;
        if idx >= slot_fast_paths.len() {
            slot_fast_paths.resize_with(idx + 1, || None);
        }
        slot_fast_paths[idx] = Some(fast_path);
        self.lock_state()
            .insert_node(id, ThreadSafeNode::Slot(node));
        SlotHandle::new(id)
    }

    /// Get a slot value, computing or validating it if needed.
    pub fn get<T>(&self, handle: &SlotHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get_slot(handle.id)
    }

    fn get_slot<T>(&self, id: SlotId) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        if let Some(parent_id) = track_dependency(self.context_id(), id) {
            self.register_dependency(id, parent_id);
        }

        loop {
            if self.slot_recompute_in_flight(id) {
                let _ = self.wait_for_slot_recompute(id);
                continue;
            }

            if self.slot_needs_refresh_without_slot_dependencies(id) {
                let _ = self.recompute_slot_now(id);
                continue;
            }

            match self.read_slot_or_dependencies(id) {
                ThreadSafeSlotRead::Fresh(value) => return value,
                ThreadSafeSlotRead::Refresh(dependencies) => {
                    self.refresh_slot_with_dependencies(id, dependencies);
                }
            }
        }
    }

    fn read_slot_or_dependencies<T>(&self, id: SlotId) -> ThreadSafeSlotRead<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::GetRefresh);
        if let Some(value) = self.try_read_fresh_slot_fast_path(id) {
            return ThreadSafeSlotRead::Fresh(value);
        }

        let state = self.read_state();
        match state.get_node(id) {
            Some(ThreadSafeNode::Slot(slot)) => {
                if !slot.fast_path.needs_refresh()
                    && let (false, false, Some(value)) =
                        (slot.dirty, slot.force_recompute, &slot.value)
                {
                    ThreadSafeSlotRead::Fresh(
                        value
                            .downcast_ref::<T>()
                            .expect("type mismatch in slot")
                            .clone(),
                    )
                } else {
                    ThreadSafeSlotRead::Refresh(
                        slot.dependencies
                            .iter()
                            .filter(|dependency_id| {
                                matches!(
                                    state.get_node(**dependency_id),
                                    Some(ThreadSafeNode::Slot(_))
                                )
                            })
                            .copied()
                            .collect(),
                    )
                }
            }
            _ => panic!("get_slot called on non-slot id"),
        }
    }

    fn refresh_slot(&self, id: SlotId) -> bool {
        if self.slot_recompute_in_flight(id) {
            return match self.wait_for_slot_recompute(id) {
                ThreadSafeRecomputeResult::Fresh(changed) => changed,
                ThreadSafeRecomputeResult::Stale => self.refresh_slot(id),
            };
        }

        if self.slot_needs_refresh_without_slot_dependencies(id) {
            return match self.recompute_slot_now(id) {
                ThreadSafeRecomputeResult::Fresh(changed) => changed,
                ThreadSafeRecomputeResult::Stale => self.refresh_slot(id),
            };
        }

        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::GetRefresh);
        let dependencies = {
            let state = self.read_state();
            match state.get_node(id) {
                Some(ThreadSafeNode::Slot(slot)) => slot
                    .dependencies
                    .iter()
                    .filter(|dependency_id| {
                        matches!(
                            state.get_node(**dependency_id),
                            Some(ThreadSafeNode::Slot(_))
                        )
                    })
                    .copied()
                    .collect(),
                _ => return false,
            }
        };

        self.refresh_slot_with_dependencies(id, dependencies)
    }

    fn refresh_slot_with_dependencies(&self, id: SlotId, dependencies: EdgeVec) -> bool {
        let mut dependency_changed = false;
        for dependency_id in dependencies {
            if self.refresh_slot(dependency_id) {
                dependency_changed = true;
            }
        }

        let needs_recompute = {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::GetRefresh);
            let mut state = self.lock_state();
            let slot = match state.get_node_mut(id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => return false,
            };

            if slot.value.is_none()
                || slot.force_recompute
                || slot.fast_path.needs_refresh()
                || dependency_changed
            {
                true
            } else {
                slot.dirty = false;
                slot.force_recompute = false;
                slot.fast_path.mark_fresh(true);
                false
            }
        };

        if !needs_recompute {
            return false;
        }

        loop {
            match self.recompute_slot_now(id) {
                ThreadSafeRecomputeResult::Fresh(changed) => return changed,
                ThreadSafeRecomputeResult::Stale => {}
            }
        }
    }

    fn recompute_slot_now(&self, id: SlotId) -> ThreadSafeRecomputeResult {
        if self.slot_recompute_in_flight(id) {
            return self.wait_for_slot_recompute(id);
        }

        let fast_path = self
            .slot_fast_path(id)
            .unwrap_or_else(|| panic!("get_slot called on non-slot id"));
        let Some(recompute_start) = fast_path.begin_recompute() else {
            return self.wait_for_slot_recompute(id);
        };
        let compute = fast_path.compute();
        let old_dependencies = fast_path.dependencies_snapshot();
        let mut recompute_guard = RecomputeGuard {
            fast_path: Arc::clone(&fast_path),
            active: true,
        };

        let _tracking = push_tracking_frame_with_known_dependencies(
            self.context_id(),
            id,
            old_dependencies.clone(),
        );
        let _callback_activity = self.callback_activity();
        let result = compute(self);
        drop(_callback_activity);
        let new_dependencies = _tracking.finish();
        let result = Arc::<ThreadSafeAny>::from(result);

        {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::Publish);
            let mut state = self.lock_state();
            #[cfg(feature = "instrumentation")]
            {
                state.instrumentation.record_slot_recompute();
            }
            {
                let slot = match state.get_node_mut(id) {
                    Some(ThreadSafeNode::Slot(slot)) => slot,
                    _ => {
                        recompute_guard.active = false;
                        fast_path.finish_recompute();
                        return ThreadSafeRecomputeResult::Fresh(false);
                    }
                };

                if slot.fast_path.current_recompute_revision() != recompute_start.revision {
                    slot.fast_path.finish_recompute();
                    recompute_guard.active = false;
                    return ThreadSafeRecomputeResult::Stale;
                }
            }

            Self::remove_stale_dependencies_locked(
                &mut state,
                id,
                &old_dependencies,
                &new_dependencies,
            );

            let (publish_fast_path, duplicate_speculative, notify_dependents, changed) = {
                let slot = match state.get_node_mut(id) {
                    Some(ThreadSafeNode::Slot(slot)) => slot,
                    _ => {
                        recompute_guard.active = false;
                        fast_path.finish_recompute();
                        return ThreadSafeRecomputeResult::Fresh(false);
                    }
                };
                let publish_fast_path = Arc::clone(&slot.fast_path);
                if recompute_start.was_unset
                    && slot.value.is_some()
                    && !slot.dirty
                    && !slot.force_recompute
                {
                    (publish_fast_path, true, false, false)
                } else {
                    let had_value = slot.value.is_some();
                    let unchanged = match (&slot.value, &slot.equals) {
                        (Some(old), Some(equals)) => equals(old.as_ref(), result.as_ref()),
                        _ => false,
                    };
                    slot.dirty = false;
                    slot.force_recompute = false;
                    if unchanged {
                        (publish_fast_path, false, false, false)
                    } else {
                        slot.value = Some(Arc::clone(&result));
                        publish_fast_path.store_value(Some(result));
                        (publish_fast_path, false, had_value, had_value)
                    }
                }
            };

            if duplicate_speculative {
                #[cfg(feature = "instrumentation")]
                state
                    .instrumentation
                    .record_duplicate_speculative_recompute();
                publish_fast_path.mark_fresh(true);
                publish_fast_path.finish_recompute();
                recompute_guard.active = false;
                return ThreadSafeRecomputeResult::Fresh(false);
            }

            if notify_dependents {
                Self::notify_slot_value_changed_locked(&mut state, id);
            }
            publish_fast_path.mark_fresh(true);
            publish_fast_path.finish_recompute();
            recompute_guard.active = false;
            ThreadSafeRecomputeResult::Fresh(changed)
        }
    }

    fn wait_for_slot_recompute(&self, id: SlotId) -> ThreadSafeRecomputeResult {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::InFlightWait);
        #[cfg(feature = "instrumentation")]
        self.record_coordination_lock(ThreadSafeLockSite::InFlightWait);
        self.slot_fast_path(id)
            .map(|fast_path| fast_path.wait_for_recompute())
            .unwrap_or(ThreadSafeRecomputeResult::Fresh(false))
    }

    /// Create a mutable thread-safe cell.
    pub fn cell<T>(&self, value: T) -> CellHandle<T>
    where
        T: PartialEq + Send + Sync + 'static,
    {
        let id = self.alloc_id();
        let fast_path = Arc::new(ThreadSafeCellFastPath::new(value));
        let node = ThreadSafeCellNode {
            dependents: EdgeVec::new(),
            fast_path: Arc::clone(&fast_path),
        };
        let mut cell_fast_paths = self
            .inner
            .cell_fast_paths
            .write()
            .expect("ThreadSafeContext cell fast path registry poisoned");
        let idx = id.0 as usize;
        if idx >= cell_fast_paths.len() {
            cell_fast_paths.resize_with(idx + 1, || None);
        }
        cell_fast_paths[idx] = Some(fast_path);
        self.lock_state()
            .insert_node(id, ThreadSafeNode::Cell(node));
        CellHandle::new(id)
    }

    /// Get the value of a thread-safe cell.
    pub fn get_cell<T>(&self, handle: &CellHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        if let Some(parent_id) = track_dependency(self.context_id(), handle.id) {
            self.register_dependency(handle.id, parent_id);
        }

        self.cell_fast_path(handle.id)
            .map(|fast_path| fast_path.get())
            .unwrap_or_else(|| panic!("get_cell called on non-cell id"))
    }

    /// Set a cell value. Changed values invalidate dependents.
    pub fn set_cell<T>(&self, handle: &CellHandle<T>, new_value: T)
    where
        T: PartialEq + Send + Sync + 'static,
    {
        let fast_path = self
            .cell_fast_path(handle.id)
            .unwrap_or_else(|| panic!("set_cell on non-cell id"));
        if !fast_path.set_if_changed(new_value) {
            return;
        }

        if self.queue_batched_cell(handle.id) {
            return;
        }

        let should_flush = if let Some(should_flush) =
            self.try_invalidate_cell_dependents_fast(handle.id, &fast_path)
        {
            should_flush
        } else {
            #[cfg(feature = "instrumentation")]
            self.inner
                .invalidation_instrumentation
                .record_sidecar_fallback();
            self.invalidate_changed_cell_locked(handle.id)
        };

        if should_flush {
            self.flush_effects();
        }
    }

    fn invalidate_changed_cell_locked(&self, id: SlotId) -> bool {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::SetCellInvalidation);
        let mut state = self.lock_state();
        match state.get_node(id) {
            Some(ThreadSafeNode::Cell(_)) => {}
            _ => panic!("set_cell on non-cell id"),
        }
        let batching = state.batch_depth > 0;
        if batching {
            edge_insert(&mut state.batched_cells, id);
        } else {
            Self::invalidate_cell_dependents_locked(&mut state, id);
        }
        !batching
    }

    fn try_invalidate_cell_dependents_fast(
        &self,
        id: SlotId,
        fast_path: &ThreadSafeCellFastPath,
    ) -> Option<bool> {
        if self.callbacks_active() {
            return None;
        }

        if self.inner.batch_depth.load(Ordering::Acquire) > 0 {
            let mut state = self.lock_state();
            if state.batch_depth > 0 {
                edge_insert(&mut state.batched_cells, id);
                return Some(false);
            }
            return None;
        }

        let roots = fast_path
            .dependents_snapshot()
            .into_iter()
            .map(|(id, kind)| (id, kind, true));
        self.try_mark_slot_frontier_fast(roots)
    }

    fn try_mark_slot_frontier_fast<I>(&self, roots: I) -> Option<bool>
    where
        I: IntoIterator<Item = (SlotId, ThreadSafeDependentKind, bool)>,
    {
        let mut queue = VecDeque::new();
        let mut requested_force = HashMap::new();
        for (id, kind, force_recompute) in roots {
            match kind {
                ThreadSafeDependentKind::Slot => Self::enqueue_invalidation_root(
                    &mut queue,
                    &mut requested_force,
                    ThreadSafeInvalidationRoot {
                        id,
                        force_recompute,
                    },
                ),
                ThreadSafeDependentKind::Effect => return None,
            }
        }

        let mut slots_to_mark = HashMap::<SlotId, bool>::new();
        let mut slot_order = Vec::new();

        while let Some(root) = queue.pop_front() {
            let Some(force_recompute) = requested_force.get(&root.id).copied() else {
                continue;
            };
            if root.force_recompute != force_recompute {
                continue;
            }

            let fast_path = self.slot_fast_path(root.id)?;
            let (dirty, force_state) = fast_path.dirty_force();
            let should_propagate = !dirty || (force_recompute && !force_state);

            match slots_to_mark.get_mut(&root.id) {
                Some(force) => *force |= force_recompute,
                None => {
                    slots_to_mark.insert(root.id, force_recompute);
                    slot_order.push(root.id);
                }
            }

            if should_propagate {
                for (dependent_id, dependent_kind) in fast_path.dependents_snapshot() {
                    match dependent_kind {
                        ThreadSafeDependentKind::Slot => Self::enqueue_invalidation_root(
                            &mut queue,
                            &mut requested_force,
                            ThreadSafeInvalidationRoot {
                                id: dependent_id,
                                force_recompute: false,
                            },
                        ),
                        ThreadSafeDependentKind::Effect => return None,
                    }
                }
            }
        }

        #[cfg(feature = "instrumentation")]
        let dirty_marks = slot_order.len();
        for id in slot_order {
            let force_recompute = slots_to_mark.get(&id).copied().unwrap_or(false);
            self.slot_fast_path(id)?.mark_dirty(force_recompute);
        }
        #[cfg(feature = "instrumentation")]
        self.inner
            .invalidation_instrumentation
            .record_sidecar_frontier(dirty_marks);

        Some(false)
    }

    /// Run several updates as one invalidation pass.
    pub fn batch<F, R>(&self, run: F) -> R
    where
        F: FnOnce(&ThreadSafeContext) -> R,
    {
        let context_id = self.context_id();
        {
            let mut state = self.lock_state();
            state.batch_depth += 1;
            self.inner
                .batch_depth
                .store(state.batch_depth, Ordering::Release);
        }
        push_batch_frame(context_id);
        let _guard = BatchGuard {
            ctx: self.clone(),
            context_id,
        };
        run(self)
    }

    fn finish_batch(&self, changes: ThreadSafeBatchChanges) {
        let should_flush = {
            let mut state = self.lock_state();
            assert!(state.batch_depth > 0, "finish_batch called without batch");
            for id in changes.cells {
                edge_insert(&mut state.batched_cells, id);
            }
            for id in changes.cell_clears {
                edge_insert(&mut state.batched_cell_clears, id);
            }
            for id in changes.slots {
                edge_insert(&mut state.batched_slots, id);
            }
            state.batch_depth -= 1;
            self.inner
                .batch_depth
                .store(state.batch_depth, Ordering::Release);
            state.batch_depth == 0
        };

        if should_flush {
            self.flush_batched_invalidations();
        }
    }

    fn is_batching(&self) -> bool {
        self.read_state().batch_depth > 0
    }

    fn queue_batched_cell(&self, id: SlotId) -> bool {
        queue_batch_change(self.context_id(), |changes| {
            edge_insert(&mut changes.cells, id);
        })
    }

    fn queue_batched_cell_clear(&self, id: SlotId) -> bool {
        queue_batch_change(self.context_id(), |changes| {
            edge_insert(&mut changes.cell_clears, id);
        })
    }

    fn queue_batched_slot_clear(&self, id: SlotId) -> bool {
        queue_batch_change(self.context_id(), |changes| {
            edge_insert(&mut changes.slots, id);
        })
    }

    fn flush_batched_invalidations(&self) {
        {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::SetCellInvalidation);
            let mut state = self.lock_state();
            let cells = std::mem::take(&mut state.batched_cells)
                .into_iter()
                .collect::<Vec<_>>();
            let cell_clears = std::mem::take(&mut state.batched_cell_clears)
                .into_iter()
                .collect::<Vec<_>>();
            let slots = std::mem::take(&mut state.batched_slots)
                .into_iter()
                .collect::<Vec<_>>();

            let mut invalidation_roots = Vec::new();
            for cell_id in &cells {
                state.fill_dependent_scratch(*cell_id);
                for &id in &state.dependent_scratch {
                    invalidation_roots.push(ThreadSafeInvalidationRoot {
                        id,
                        force_recompute: true,
                    });
                }
            }
            Self::invalidate_frontier_locked(&mut state, invalidation_roots);

            let mut clear_roots = Vec::new();
            for cell_id in &cell_clears {
                state.fill_dependent_scratch(*cell_id);
                let deps = state.dependent_scratch.clone();
                clear_roots.extend(deps);
            }
            clear_roots.extend(slots);
            Self::clear_frontier_locked(&mut state, clear_roots);
        }
        self.flush_effects();
    }

    /// Create an effect, run it immediately, and rerun it after tracked
    /// dependencies invalidate.
    pub fn effect<F, R>(&self, run: F) -> EffectHandle
    where
        F: Fn(&ThreadSafeContext) -> R + Send + Sync + 'static,
        R: ThreadSafeEffectCallbackResult + 'static,
    {
        let id = self.alloc_id();
        let node = ThreadSafeEffectNode {
            run: Arc::new(move |ctx| run(ctx).into_thread_safe_cleanup()),
            dependencies: EdgeVec::new(),
            cleanup: None,
            force_run: true,
        };
        self.lock_state()
            .insert_node(id, ThreadSafeNode::Effect(node));
        let handle = EffectHandle::new(id);
        self.schedule_effect(id, false);
        self.flush_effects();
        handle
    }

    /// Dispose an effect by handle.
    pub fn dispose_effect(&self, handle: &EffectHandle) {
        let (dependencies, cleanup) = {
            let mut state = self.lock_state();
            state.deschedule_effect(handle.id);
            state.pending_effects.retain(|queued| *queued != handle.id);
            let Some(ThreadSafeNode::Effect(effect)) = state.remove_node(handle.id) else {
                return;
            };
            state.free_ids.push(handle.id.0);
            (effect.dependencies, effect.cleanup)
        };

        for dependency_id in dependencies {
            self.remove_dependent_edge(dependency_id, handle.id);
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }
    }

    /// Check whether an effect is still registered.
    pub fn is_effect_active(&self, handle: &EffectHandle) -> bool {
        let state = self.read_state();
        matches!(state.get_node(handle.id), Some(ThreadSafeNode::Effect(_)))
    }

    fn schedule_effect(&self, id: SlotId, force: bool) {
        let mut state = self.lock_state();
        Self::schedule_effect_locked(&mut state, id, force);
    }

    fn schedule_effect_locked(state: &mut ThreadSafeState, id: SlotId, force: bool) {
        match state.get_node_mut(id) {
            Some(ThreadSafeNode::Effect(effect)) => {
                if force {
                    effect.force_run = true;
                }
            }
            _ => return,
        }

        let idx = id.0 as usize;
        let already_scheduled = if idx < state.scheduled_effects.len() {
            state.scheduled_effects[idx]
        } else {
            false
        };
        if !already_scheduled {
            if idx >= state.scheduled_effects.len() {
                state.scheduled_effects.resize(idx + 1, false);
            }
            state.scheduled_effects[idx] = true;
            state.pending_effects.push_back(id);
            #[cfg(feature = "instrumentation")]
            {
                let depth = state.pending_effects.len();
                state.instrumentation.record_effect_queue_push(depth);
            }
        }
    }

    fn flush_effects(&self) {
        {
            let mut state = self.lock_state();
            if state.flushing_effects {
                return;
            }
            state.flushing_effects = true;
        }
        let mut guard = FlushGuard {
            ctx: self.clone(),
            active: true,
        };

        loop {
            let id = {
                let mut state = self.lock_state();
                if let Some(id) = state.pending_effects.pop_front() {
                    state.deschedule_effect(id);
                    Some(id)
                } else {
                    state.flushing_effects = false;
                    guard.active = false;
                    None
                }
            };
            let Some(id) = id else {
                break;
            };
            self.run_effect(id);
        }
    }

    fn run_effect(&self, id: SlotId) {
        if !self.effect_should_run(id) {
            return;
        }

        let (run, old_dependencies, cleanup) = {
            let mut state = self.lock_state();
            state.pending_effects.retain(|queued| *queued != id);
            state.deschedule_effect(id);
            let effect = match state.get_node_mut(id) {
                Some(ThreadSafeNode::Effect(effect)) => effect,
                _ => return,
            };
            let old_dependencies = effect.dependencies.clone();
            let cleanup = effect.cleanup.take();
            effect.force_run = false;
            (Arc::clone(&effect.run), old_dependencies, cleanup)
        };

        if let Some(cleanup) = cleanup {
            cleanup();
        }

        let _tracking = push_tracking_frame_with_known_dependencies(
            self.context_id(),
            id,
            old_dependencies.clone(),
        );
        let _callback_activity = self.callback_activity();
        let next_cleanup = run(self);
        drop(_callback_activity);
        let new_dependencies = _tracking.finish();

        let mut state = self.lock_state();
        if matches!(state.get_node(id), Some(ThreadSafeNode::Effect(_))) {
            Self::remove_stale_dependencies_locked(
                &mut state,
                id,
                &old_dependencies,
                &new_dependencies,
            );
        }
        if let Some(ThreadSafeNode::Effect(effect)) = state.get_node_mut(id) {
            effect.cleanup = next_cleanup;
        } else if let Some(cleanup) = next_cleanup {
            drop(state);
            cleanup();
        }
    }

    fn effect_should_run(&self, id: SlotId) -> bool {
        let (force_run, dependencies) = {
            let state = self.read_state();
            let Some(ThreadSafeNode::Effect(effect)) = state.get_node(id) else {
                return false;
            };
            (effect.force_run, effect.dependencies.clone())
        };

        if force_run {
            return true;
        }

        dependencies
            .into_iter()
            .any(|dependency_id| self.refresh_slot(dependency_id))
    }

    /// Hard-clear a slot and recursively clear dependents.
    pub fn clear<T>(&self, handle: &SlotHandle<T>) {
        self.clear_slot(handle.id);
        self.flush_effects_after_invalidation();
    }

    fn clear_slot(&self, id: SlotId) {
        if self.queue_batched_slot_clear(id) {
            return;
        }

        let should_clear = {
            let mut state = self.lock_state();
            if state.batch_depth > 0 {
                edge_insert(&mut state.batched_slots, id);
                false
            } else {
                true
            }
        };

        if should_clear {
            self.clear_slot_now(id);
        }
    }

    fn flush_effects_after_invalidation(&self) {
        if !self.is_batching() {
            self.flush_effects();
        }
    }

    fn clear_slot_now_locked(state: &mut ThreadSafeState, id: SlotId) {
        Self::clear_frontier_locked(state, [id]);
    }

    fn clear_slot_now(&self, id: SlotId) {
        let mut state = self.lock_state();
        Self::clear_slot_now_locked(&mut state, id);
    }

    /// Clear all dependent slots without changing the cell value.
    pub fn clear_cell_dependents<T>(&self, handle: &CellHandle<T>) {
        if self.queue_batched_cell_clear(handle.id) {
            return;
        }

        let should_flush = {
            let mut state = self.lock_state();
            if state.batch_depth > 0 {
                edge_insert(&mut state.batched_cell_clears, handle.id);
                false
            } else {
                Self::clear_cell_dependents_locked(&mut state, handle.id);
                true
            }
        };

        if should_flush {
            self.flush_effects();
        }
    }

    fn invalidate_cell_dependents_locked(state: &mut ThreadSafeState, id: SlotId) {
        state.fill_dependent_scratch(id);
        let roots: Vec<_> = state
            .dependent_scratch
            .iter()
            .map(|&id| ThreadSafeInvalidationRoot {
                id,
                force_recompute: true,
            })
            .collect();
        Self::invalidate_frontier_locked(state, roots);
    }

    fn clear_cell_dependents_locked(state: &mut ThreadSafeState, id: SlotId) {
        state.fill_dependent_scratch(id);
        let deps: Vec<SlotId> = state.dependent_scratch.clone();
        Self::clear_frontier_locked(state, deps);
    }

    fn notify_slot_value_changed_locked(state: &mut ThreadSafeState, id: SlotId) {
        state.fill_dependent_scratch(id);
        let roots: Vec<_> = state
            .dependent_scratch
            .iter()
            .map(|&id| ThreadSafeInvalidationRoot {
                id,
                force_recompute: true,
            })
            .collect();
        Self::invalidate_frontier_locked(state, roots);
    }

    #[cfg(test)]
    fn dependents_locked(state: &ThreadSafeState, id: SlotId) -> EdgeVec {
        match state.get_node(id) {
            Some(ThreadSafeNode::Slot(slot)) => slot.dependents.clone(),
            Some(ThreadSafeNode::Cell(cell)) => cell.dependents.clone(),
            Some(ThreadSafeNode::Effect(_)) | None => EdgeVec::new(),
        }
    }

    fn enqueue_invalidation_root(
        queue: &mut VecDeque<ThreadSafeInvalidationRoot>,
        requested_force: &mut HashMap<SlotId, bool>,
        root: ThreadSafeInvalidationRoot,
    ) {
        match requested_force.get_mut(&root.id) {
            Some(force_recompute) if root.force_recompute && !*force_recompute => {
                *force_recompute = true;
                queue.push_back(root);
            }
            Some(_) => {}
            None => {
                requested_force.insert(root.id, root.force_recompute);
                queue.push_back(root);
            }
        }
    }

    fn invalidate_frontier_locked<I>(state: &mut ThreadSafeState, roots: I)
    where
        I: IntoIterator<Item = ThreadSafeInvalidationRoot>,
    {
        ThreadSafeInvalidationPlan::from_roots_locked(state, roots).apply_locked(state);
    }

    fn clear_frontier_locked<I>(state: &mut ThreadSafeState, roots: I)
    where
        I: IntoIterator<Item = SlotId>,
    {
        ThreadSafeInvalidationPlan::from_clear_roots_locked(state, roots).apply_locked(state);
    }

    /// Check whether a slot currently has a cached, fresh value.
    pub fn is_set<T>(&self, handle: &SlotHandle<T>) -> bool
    where
        T: Send + Sync + 'static,
    {
        let state = self.read_state();
        if let Some(ThreadSafeNode::Slot(slot)) = state.get_node(handle.id) {
            slot.value.is_some() && !slot.dirty && !slot.fast_path.needs_refresh()
        } else {
            false
        }
    }

    /// Return the current benchmark instrumentation counters.
    #[cfg(feature = "instrumentation")]
    pub fn instrumentation_snapshot(&self) -> crate::instrumentation::InstrumentationSnapshot {
        let mut snapshot = {
            let state = self
                .inner
                .state
                .lock()
                .expect("ThreadSafeContext mutex poisoned");
            state.instrumentation.snapshot()
        };
        self.inner
            .lock_instrumentation
            .apply_to_snapshot(&mut snapshot);
        self.inner
            .invalidation_instrumentation
            .apply_to_snapshot(&mut snapshot);
        snapshot
    }

    /// Return ThreadSafeContext lock and coordination counters grouped by
    /// operation.
    #[cfg(feature = "instrumentation")]
    pub fn lock_profile_snapshot(
        &self,
    ) -> [crate::instrumentation::ThreadSafeLockSiteSnapshot;
        crate::instrumentation::THREAD_SAFE_LOCK_SITE_COUNT] {
        self.inner.lock_instrumentation.site_snapshots()
    }

    /// Reset benchmark instrumentation counters to zero.
    #[cfg(feature = "instrumentation")]
    pub fn reset_instrumentation(&self) {
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("ThreadSafeContext mutex poisoned");
            state.instrumentation.reset();
        }
        self.inner.lock_instrumentation.reset();
        self.inner.invalidation_instrumentation.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn slot_revision<T>(ctx: &ThreadSafeContext, handle: &SlotHandle<T>) -> u64
    where
        T: Send + Sync + 'static,
    {
        let state = ctx.lock_state();
        match state.get_node(handle.id) {
            Some(ThreadSafeNode::Slot(slot)) => slot.revision,
            _ => panic!("slot_revision called on non-slot id"),
        }
    }

    fn slot_dirty_force<T>(ctx: &ThreadSafeContext, handle: &SlotHandle<T>) -> (bool, bool)
    where
        T: Send + Sync + 'static,
    {
        let state = ctx.lock_state();
        match state.get_node(handle.id) {
            Some(ThreadSafeNode::Slot(slot)) => (slot.dirty, slot.force_recompute),
            _ => panic!("slot_dirty_force called on non-slot id"),
        }
    }

    fn cell_dependents_len<T>(ctx: &ThreadSafeContext, handle: &CellHandle<T>) -> usize
    where
        T: Send + Sync + 'static,
    {
        let state = ctx.lock_state();
        match state.get_node(handle.id) {
            Some(ThreadSafeNode::Cell(cell)) => cell.dependents.len(),
            _ => panic!("cell_dependents_len called on non-cell id"),
        }
    }

    fn effect_is_scheduled(ctx: &ThreadSafeContext, handle: &EffectHandle) -> bool {
        let state = ctx.lock_state();
        state.is_effect_scheduled(handle.id)
    }

    fn pending_effect_count(ctx: &ThreadSafeContext) -> usize {
        ctx.lock_state().pending_effects.len()
    }

    #[test]
    fn invalidation_plan_snapshots_frontier_before_apply() {
        let ctx = ThreadSafeContext::new();
        let root = ctx.cell(0usize);
        let left = ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1));
        let right = ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(2));
        let joined = ctx.computed(move |ctx| ctx.get(&left).wrapping_add(ctx.get(&right)));
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_for_effect = Arc::clone(&runs);
        let effect = ctx.effect(move |ctx| {
            runs_for_effect.fetch_add(1, Ordering::SeqCst);
            let _ = ctx.get(&joined);
        });

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(ctx.get(&joined), 3);

        let plan = {
            let state = ctx.lock_state();
            let roots = ThreadSafeContext::dependents_locked(&state, root.id)
                .into_iter()
                .map(|id| ThreadSafeInvalidationRoot {
                    id,
                    force_recompute: true,
                });
            let plan = ThreadSafeInvalidationPlan::from_roots_locked(&state, roots);
            let planned_slots: HashSet<SlotId> =
                plan.slot_marks.iter().map(|(id, _)| *id).collect();
            let expected_slots = [left.id, right.id, joined.id]
                .into_iter()
                .collect::<HashSet<_>>();

            assert_eq!(planned_slots, expected_slots);
            assert_eq!(
                plan.slot_marks
                    .iter()
                    .find(|(sid, _)| *sid == left.id)
                    .map(|(_, f)| *f),
                Some(true)
            );
            assert_eq!(
                plan.slot_marks
                    .iter()
                    .find(|(sid, _)| *sid == right.id)
                    .map(|(_, f)| *f),
                Some(true)
            );
            assert_eq!(
                plan.slot_marks
                    .iter()
                    .find(|(sid, _)| *sid == joined.id)
                    .map(|(_, f)| *f),
                Some(false)
            );
            assert_eq!(
                plan.effect_schedules
                    .iter()
                    .find(|(sid, _)| *sid == effect.id)
                    .map(|(_, f)| *f),
                Some(false)
            );
            match state.get_node(joined.id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    assert!(!slot.dirty);
                    assert!(!slot.force_recompute);
                }
                _ => panic!("joined should be a slot"),
            }
            plan
        };

        assert_eq!(slot_dirty_force(&ctx, &joined), (false, false));
        assert!(!effect_is_scheduled(&ctx, &effect));

        {
            let mut state = ctx.lock_state();
            plan.apply_locked(&mut state);
        }

        assert_eq!(slot_dirty_force(&ctx, &left), (true, true));
        assert_eq!(slot_dirty_force(&ctx, &right), (true, true));
        assert_eq!(slot_dirty_force(&ctx, &joined), (true, false));
        assert!(effect_is_scheduled(&ctx, &effect));
    }

    #[test]
    fn invalidation_plan_snapshots_hard_clears_before_apply() {
        let ctx = ThreadSafeContext::new();
        let root = ctx.cell(1usize);
        let doubled = ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_mul(2));
        let labeled = ctx.computed(move |ctx| ctx.get(&doubled).wrapping_add(1));
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_for_effect = Arc::clone(&runs);
        let effect = ctx.effect(move |ctx| {
            runs_for_effect.fetch_add(1, Ordering::SeqCst);
            let _ = ctx.get(&labeled);
        });

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(ctx.get(&labeled), 3);
        assert!(ctx.is_set(&doubled));
        assert!(ctx.is_set(&labeled));

        let plan = {
            let state = ctx.lock_state();
            let roots = ThreadSafeContext::dependents_locked(&state, root.id);
            let plan = ThreadSafeInvalidationPlan::from_clear_roots_locked(&state, roots);
            let planned_clears: HashSet<SlotId> = plan.slot_clears.iter().copied().collect();
            let expected_clears = [doubled.id, labeled.id].into_iter().collect::<HashSet<_>>();

            assert_eq!(planned_clears, expected_clears);
            assert_eq!(
                plan.effect_schedules
                    .iter()
                    .find(|(sid, _)| *sid == effect.id)
                    .map(|(_, f)| *f),
                Some(true)
            );
            match state.get_node(labeled.id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    assert!(slot.value.is_some());
                    assert!(!slot.dirty);
                }
                _ => panic!("labeled should be a slot"),
            }
            plan
        };

        assert!(ctx.is_set(&doubled));
        assert!(ctx.is_set(&labeled));
        assert!(!effect_is_scheduled(&ctx, &effect));

        {
            let mut state = ctx.lock_state();
            plan.apply_locked(&mut state);
        }

        assert!(!ctx.is_set(&doubled));
        assert!(!ctx.is_set(&labeled));
        assert!(effect_is_scheduled(&ctx, &effect));
    }

    #[test]
    fn batched_cell_invalidations_mark_shared_dependent_once() {
        let ctx = ThreadSafeContext::new();
        let cells = [
            ctx.cell(0usize),
            ctx.cell(0usize),
            ctx.cell(0usize),
            ctx.cell(0usize),
        ];
        let total = ctx.computed(move |ctx| {
            cells
                .iter()
                .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)))
        });

        assert_eq!(ctx.get(&total), 0);
        assert_eq!(slot_revision(&ctx, &total), 0);

        ctx.batch(|ctx| {
            for (offset, cell) in cells.iter().enumerate() {
                ctx.set_cell(cell, offset + 1);
            }
        });

        assert_eq!(
            slot_revision(&ctx, &total),
            1,
            "one batch should apply one coalesced dirty/revision mark to the shared frontier"
        );
        assert_eq!(slot_dirty_force(&ctx, &total), (true, true));
        assert_eq!(ctx.get(&total), 10);
        assert_eq!(
            slot_revision(&ctx, &total),
            1,
            "recompute should publish the new value without additional invalidation marks"
        );
    }

    #[test]
    fn batched_cell_invalidations_schedule_shared_effect_once() {
        let ctx = ThreadSafeContext::new();
        let left = ctx.cell(0usize);
        let right = ctx.cell(0usize);
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_for_effect = Arc::clone(&runs);
        let effect = ctx.effect(move |ctx| {
            runs_for_effect.fetch_add(1, Ordering::SeqCst);
            let _ = ctx.get_cell(&left);
            let _ = ctx.get_cell(&right);
        });

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(cell_dependents_len(&ctx, &left), 1);
        assert_eq!(cell_dependents_len(&ctx, &right), 1);

        ctx.batch(|ctx| {
            ctx.set_cell(&left, 1);
            ctx.set_cell(&right, 1);
        });

        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(pending_effect_count(&ctx), 0);
        assert!(!effect_is_scheduled(&ctx, &effect));
    }
}
