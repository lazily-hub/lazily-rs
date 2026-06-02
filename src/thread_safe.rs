use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(feature = "instrumentation")]
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct ThreadSafeContextId(usize);

struct ThreadSafeTrackingFrame {
    context_id: ThreadSafeContextId,
    node_id: SlotId,
    dependencies: HashSet<SlotId>,
}

thread_local! {
    static THREAD_SAFE_TRACKING_STACK: RefCell<Vec<ThreadSafeTrackingFrame>> =
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

fn push_tracking_frame(context_id: ThreadSafeContextId, node_id: SlotId) -> TrackingGuard {
    THREAD_SAFE_TRACKING_STACK.with(|stack| {
        stack.borrow_mut().push(ThreadSafeTrackingFrame {
            context_id,
            node_id,
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
                frame.dependencies.insert(dependency_id);
                return Some(frame.node_id);
            }
        }
        None
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
    value: Option<Box<ThreadSafeAny>>,
    compute: Arc<ThreadSafeComputeFn>,
    equals: Option<Arc<ThreadSafeEqualsFn>>,
    dependencies: HashSet<SlotId>,
    dependents: HashSet<SlotId>,
    recompute_waiters: Arc<Condvar>,
    dirty: bool,
    force_recompute: bool,
    computing: bool,
    revision: u64,
}

struct ThreadSafeCellNode {
    value: Box<ThreadSafeAny>,
    dependents: HashSet<SlotId>,
}

struct ThreadSafeEffectNode {
    run: Arc<ThreadSafeEffectFn>,
    dependencies: HashSet<SlotId>,
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

#[derive(Default)]
struct ThreadSafeState {
    nodes: HashMap<SlotId, ThreadSafeNode>,
    next_id: u64,
    pending_effects: VecDeque<SlotId>,
    scheduled_effects: HashSet<SlotId>,
    flushing_effects: bool,
    batch_depth: usize,
    batched_cells: HashSet<SlotId>,
    batched_cell_clears: HashSet<SlotId>,
    batched_slots: HashSet<SlotId>,
    #[cfg(feature = "instrumentation")]
    instrumentation: crate::instrumentation::InstrumentationCounters,
}

struct ThreadSafeInner {
    state: Mutex<ThreadSafeState>,
    #[cfg(feature = "instrumentation")]
    lock_instrumentation: crate::instrumentation::ThreadSafeLockInstrumentation,
}

impl Default for ThreadSafeInner {
    fn default() -> Self {
        Self {
            state: Mutex::new(ThreadSafeState::default()),
            #[cfg(feature = "instrumentation")]
            lock_instrumentation: crate::instrumentation::ThreadSafeLockInstrumentation::default(),
        }
    }
}

#[cfg(feature = "instrumentation")]
struct ProfiledMutexGuard<'a> {
    guard: Option<MutexGuard<'a, ThreadSafeState>>,
    lock_instrumentation: &'a crate::instrumentation::ThreadSafeLockInstrumentation,
    site: ThreadSafeLockSite,
    acquired_at: Instant,
}

#[cfg(feature = "instrumentation")]
impl<'a> ProfiledMutexGuard<'a> {
    fn wait_on(mut self, condvar: &Condvar) -> Self {
        self.lock_instrumentation
            .record_lock_hold(self.site, self.acquired_at.elapsed());
        let guard = self
            .guard
            .take()
            .expect("profiled mutex guard missing while waiting");
        let wait_started = Instant::now();
        let guard = condvar
            .wait(guard)
            .expect("ThreadSafeContext mutex poisoned while waiting");
        self.lock_instrumentation
            .record_lock_wait(self.site, wait_started.elapsed());
        self.guard = Some(guard);
        self.acquired_at = Instant::now();
        self
    }
}

#[cfg(feature = "instrumentation")]
impl Deref for ProfiledMutexGuard<'_> {
    type Target = ThreadSafeState;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_ref()
            .expect("profiled mutex guard missing during deref")
    }
}

#[cfg(feature = "instrumentation")]
impl DerefMut for ProfiledMutexGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_mut()
            .expect("profiled mutex guard missing during mutable deref")
    }
}

#[cfg(feature = "instrumentation")]
impl Drop for ProfiledMutexGuard<'_> {
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

/// Mutex-backed context for sharing lazy reactive state across OS threads.
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
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        self.ctx.finish_batch();
    }
}

struct RecomputeGuard {
    ctx: ThreadSafeContext,
    id: SlotId,
    active: bool,
}

impl Drop for RecomputeGuard {
    fn drop(&mut self) {
        if self.active {
            self.ctx.finish_slot_recompute(self.id);
        }
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
    fn lock_state(&self) -> MutexGuard<'_, ThreadSafeState> {
        self.inner
            .state
            .lock()
            .expect("ThreadSafeContext mutex poisoned")
    }

    #[cfg(feature = "instrumentation")]
    fn lock_state(&self) -> ProfiledMutexGuard<'_> {
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
        ProfiledMutexGuard {
            guard: Some(guard),
            lock_instrumentation: &self.inner.lock_instrumentation,
            site,
            acquired_at: Instant::now(),
        }
    }

    fn alloc_id(&self) -> SlotId {
        let mut state = self.lock_state();
        let slot_id = SlotId(state.next_id);
        state.next_id += 1;
        #[cfg(feature = "instrumentation")]
        {
            state.instrumentation.record_node_allocation();
        }
        slot_id
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
        if let Some(node) = state.nodes.get_mut(&dependency_id) {
            match node {
                ThreadSafeNode::Slot(slot) => {
                    slot.dependents.insert(dependent_id);
                }
                ThreadSafeNode::Cell(cell) => {
                    cell.dependents.insert(dependent_id);
                }
                ThreadSafeNode::Effect(_) => {}
            }
        }

        if let Some(node) = state.nodes.get_mut(&dependent_id) {
            match node {
                ThreadSafeNode::Slot(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = parent.dependencies.insert(dependency_id);
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        parent.dependencies.insert(dependency_id);
                    }
                }
                ThreadSafeNode::Effect(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = parent.dependencies.insert(dependency_id);
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        parent.dependencies.insert(dependency_id);
                    }
                }
                ThreadSafeNode::Cell(_) => {}
            }
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
        old_dependencies: &HashSet<SlotId>,
        new_dependencies: &HashSet<SlotId>,
    ) {
        for dependency_id in old_dependencies.difference(new_dependencies) {
            Self::remove_parent_dependency_locked(state, dependent_id, *dependency_id);
            Self::remove_dependent_edge_locked(state, *dependency_id, dependent_id);
        }
    }

    fn remove_parent_dependency_locked(
        state: &mut ThreadSafeState,
        dependent_id: SlotId,
        dependency_id: SlotId,
    ) -> bool {
        match state.nodes.get_mut(&dependent_id) {
            Some(ThreadSafeNode::Slot(slot)) => slot.dependencies.remove(&dependency_id),
            Some(ThreadSafeNode::Effect(effect)) => effect.dependencies.remove(&dependency_id),
            Some(ThreadSafeNode::Cell(_)) | None => false,
        }
    }

    fn remove_dependent_edge_locked(
        state: &mut ThreadSafeState,
        dependency_id: SlotId,
        dependent_id: SlotId,
    ) {
        let _edge_removed = match state.nodes.get_mut(&dependency_id) {
            Some(ThreadSafeNode::Slot(slot)) => slot.dependents.remove(&dependent_id),
            Some(ThreadSafeNode::Cell(cell)) => cell.dependents.remove(&dependent_id),
            Some(ThreadSafeNode::Effect(_)) | None => false,
        };

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
        let node = ThreadSafeSlotNode {
            value: None,
            compute: Arc::new(move |ctx| Box::new(compute(ctx))),
            equals,
            dependencies: HashSet::new(),
            dependents: HashSet::new(),
            recompute_waiters: Arc::new(Condvar::new()),
            dirty: false,
            force_recompute: false,
            computing: false,
            revision: 0,
        };
        self.lock_state()
            .nodes
            .insert(id, ThreadSafeNode::Slot(node));
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
            self.refresh_slot(id);

            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::GetRefresh);
            let state = self.lock_state();
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    if let Some(value) = &slot.value {
                        return value
                            .downcast_ref::<T>()
                            .expect("type mismatch in slot")
                            .clone();
                    }
                }
                _ => panic!("get_slot called on non-slot id"),
            }
        }
    }

    fn refresh_slot(&self, id: SlotId) -> bool {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::GetRefresh);
        let dependencies: Vec<SlotId> = {
            let state = self.lock_state();
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot.dependencies.iter().copied().collect(),
                _ => return false,
            }
        };

        let mut dependency_changed = false;
        for dependency_id in dependencies {
            if self.refresh_slot(dependency_id) {
                dependency_changed = true;
            }
        }

        let needs_recompute = {
            let mut state = self.lock_state();
            let slot = match state.nodes.get_mut(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => return false,
            };

            if slot.value.is_none() || slot.force_recompute || dependency_changed {
                true
            } else {
                slot.dirty = false;
                slot.force_recompute = false;
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
        let (compute, old_dependencies, was_unset, start_revision) = {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::Publish);
            let mut state = self.lock_state();
            let result = {
                let slot = match state.nodes.get_mut(&id) {
                    Some(ThreadSafeNode::Slot(slot)) => slot,
                    _ => panic!("get_slot called on non-slot id"),
                };
                if slot.computing {
                    drop(state);
                    return self.wait_for_slot_recompute(id);
                }
                slot.computing = true;
                (
                    Arc::clone(&slot.compute),
                    slot.dependencies.clone(),
                    slot.value.is_none(),
                    slot.revision,
                )
            };
            #[cfg(feature = "instrumentation")]
            {
                state.instrumentation.record_slot_recompute();
            }
            result
        };
        let mut recompute_guard = RecomputeGuard {
            ctx: self.clone(),
            id,
            active: true,
        };

        let _tracking = push_tracking_frame(self.context_id(), id);
        let result = compute(self);
        let new_dependencies = _tracking.finish();

        {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::Publish);
            let mut state = self.lock_state();
            {
                let slot = match state.nodes.get_mut(&id) {
                    Some(ThreadSafeNode::Slot(slot)) => slot,
                    _ => {
                        recompute_guard.active = false;
                        return ThreadSafeRecomputeResult::Fresh(false);
                    }
                };
                slot.computing = false;
                let recompute_waiters = Arc::clone(&slot.recompute_waiters);
                recompute_waiters.notify_all();
                recompute_guard.active = false;

                if slot.revision != start_revision {
                    return ThreadSafeRecomputeResult::Stale;
                }
            }

            Self::remove_stale_dependencies_locked(
                &mut state,
                id,
                &old_dependencies,
                &new_dependencies,
            );

            let slot = match state.nodes.get_mut(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => return ThreadSafeRecomputeResult::Fresh(false),
            };
            if was_unset && slot.value.is_some() && !slot.dirty && !slot.force_recompute {
                #[cfg(feature = "instrumentation")]
                {
                    state
                        .instrumentation
                        .record_duplicate_speculative_recompute();
                }
                return ThreadSafeRecomputeResult::Fresh(false);
            }

            let had_value = slot.value.is_some();
            let unchanged = match (&slot.value, &slot.equals) {
                (Some(old), Some(equals)) => equals(old.as_ref(), result.as_ref()),
                _ => false,
            };
            slot.dirty = false;
            slot.force_recompute = false;
            if unchanged {
                ThreadSafeRecomputeResult::Fresh(false)
            } else {
                slot.value = Some(result);
                if had_value {
                    Self::notify_slot_value_changed_locked(&mut state, id);
                    ThreadSafeRecomputeResult::Fresh(true)
                } else {
                    ThreadSafeRecomputeResult::Fresh(false)
                }
            }
        }
    }

    fn wait_for_slot_recompute(&self, id: SlotId) -> ThreadSafeRecomputeResult {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::InFlightWait);
        let mut state = self.lock_state();
        loop {
            let recompute_waiters = {
                let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get(&id) else {
                    return ThreadSafeRecomputeResult::Fresh(false);
                };
                if slot.computing {
                    Some(Arc::clone(&slot.recompute_waiters))
                } else if slot.value.is_some() && !slot.dirty && !slot.force_recompute {
                    return ThreadSafeRecomputeResult::Fresh(false);
                } else {
                    return ThreadSafeRecomputeResult::Stale;
                }
            };
            state = self.wait_for_recompute_notification(
                state,
                recompute_waiters
                    .as_ref()
                    .expect("waiters should be present"),
            );
        }
    }

    #[cfg(not(feature = "instrumentation"))]
    fn wait_for_recompute_notification<'a>(
        &self,
        state: MutexGuard<'a, ThreadSafeState>,
        recompute_waiters: &Condvar,
    ) -> MutexGuard<'a, ThreadSafeState> {
        recompute_waiters
            .wait(state)
            .expect("ThreadSafeContext mutex poisoned while waiting")
    }

    #[cfg(feature = "instrumentation")]
    fn wait_for_recompute_notification<'a>(
        &self,
        state: ProfiledMutexGuard<'a>,
        recompute_waiters: &Condvar,
    ) -> ProfiledMutexGuard<'a> {
        state.wait_on(recompute_waiters)
    }

    fn finish_slot_recompute(&self, id: SlotId) {
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::Publish);
        let mut state = self.lock_state();
        if let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get_mut(&id) {
            slot.computing = false;
            let recompute_waiters = Arc::clone(&slot.recompute_waiters);
            recompute_waiters.notify_all();
        }
    }

    /// Create a mutable thread-safe cell.
    pub fn cell<T>(&self, value: T) -> CellHandle<T>
    where
        T: PartialEq + Send + Sync + 'static,
    {
        let id = self.alloc_id();
        let node = ThreadSafeCellNode {
            value: Box::new(value),
            dependents: HashSet::new(),
        };
        self.lock_state()
            .nodes
            .insert(id, ThreadSafeNode::Cell(node));
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

        let state = self.lock_state();
        if let Some(ThreadSafeNode::Cell(cell)) = state.nodes.get(&handle.id) {
            cell.value
                .downcast_ref::<T>()
                .expect("type mismatch in cell")
                .clone()
        } else {
            panic!("get_cell called on non-cell id");
        }
    }

    /// Set a cell value. Changed values invalidate dependents.
    pub fn set_cell<T>(&self, handle: &CellHandle<T>, new_value: T)
    where
        T: PartialEq + Send + Sync + 'static,
    {
        let should_flush = {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::SetCellInvalidation);
            let mut state = self.lock_state();
            let changed = match state.nodes.get(&handle.id) {
                Some(ThreadSafeNode::Cell(cell)) => {
                    let old = cell
                        .value
                        .downcast_ref::<T>()
                        .expect("type mismatch in cell set");
                    *old != new_value
                }
                _ => panic!("set_cell on non-cell id"),
            };

            if !changed {
                return;
            }

            let batching = state.batch_depth > 0;
            if let Some(ThreadSafeNode::Cell(cell)) = state.nodes.get_mut(&handle.id) {
                cell.value = Box::new(new_value);
            }
            if batching {
                state.batched_cells.insert(handle.id);
            } else {
                Self::invalidate_cell_dependents_locked(&mut state, handle.id);
            }
            !batching
        };

        if should_flush {
            self.flush_effects();
        }
    }

    /// Run several updates as one invalidation pass.
    pub fn batch<F, R>(&self, run: F) -> R
    where
        F: FnOnce(&ThreadSafeContext) -> R,
    {
        {
            let mut state = self.lock_state();
            state.batch_depth += 1;
        }
        let _guard = BatchGuard { ctx: self.clone() };
        run(self)
    }

    fn finish_batch(&self) {
        let should_flush = {
            let mut state = self.lock_state();
            assert!(state.batch_depth > 0, "finish_batch called without batch");
            state.batch_depth -= 1;
            state.batch_depth == 0
        };

        if should_flush {
            self.flush_batched_invalidations();
        }
    }

    fn is_batching(&self) -> bool {
        self.lock_state().batch_depth > 0
    }

    fn flush_batched_invalidations(&self) {
        {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::SetCellInvalidation);
            let mut state = self.lock_state();
            let cells = state.batched_cells.drain().collect::<Vec<_>>();
            let cell_clears = state.batched_cell_clears.drain().collect::<Vec<_>>();
            let slots = state.batched_slots.drain().collect::<Vec<_>>();

            for cell_id in cells {
                Self::invalidate_cell_dependents_locked(&mut state, cell_id);
            }
            for cell_id in cell_clears {
                Self::clear_cell_dependents_locked(&mut state, cell_id);
            }
            for slot_id in slots {
                Self::clear_slot_now_locked(&mut state, slot_id);
            }
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
            dependencies: HashSet::new(),
            cleanup: None,
            force_run: true,
        };
        self.lock_state()
            .nodes
            .insert(id, ThreadSafeNode::Effect(node));
        let handle = EffectHandle::new(id);
        self.schedule_effect(id, false);
        self.flush_effects();
        handle
    }

    /// Dispose an effect by handle.
    pub fn dispose_effect(&self, handle: &EffectHandle) {
        let (dependencies, cleanup) = {
            let mut state = self.lock_state();
            state.scheduled_effects.remove(&handle.id);
            state.pending_effects.retain(|queued| *queued != handle.id);
            let Some(ThreadSafeNode::Effect(effect)) = state.nodes.remove(&handle.id) else {
                return;
            };
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
        let state = self.lock_state();
        matches!(state.nodes.get(&handle.id), Some(ThreadSafeNode::Effect(_)))
    }

    fn schedule_effect(&self, id: SlotId, force: bool) {
        let mut state = self.lock_state();
        Self::schedule_effect_locked(&mut state, id, force);
    }

    fn schedule_effect_locked(state: &mut ThreadSafeState, id: SlotId, force: bool) {
        match state.nodes.get_mut(&id) {
            Some(ThreadSafeNode::Effect(effect)) => {
                if force {
                    effect.force_run = true;
                }
            }
            _ => return,
        }

        if state.scheduled_effects.insert(id) {
            state.pending_effects.push_back(id);
            #[cfg(feature = "instrumentation")]
            {
                let depth = state.pending_effects.len();
                state.instrumentation.record_effect_queue_push(depth);
            }
        }
    }

    fn remove_pending_effect(&self, id: SlotId) {
        let mut state = self.lock_state();
        state.pending_effects.retain(|queued| *queued != id);
        state.scheduled_effects.remove(&id);
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
                    state.scheduled_effects.remove(&id);
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
        self.remove_pending_effect(id);

        let (run, old_dependencies, cleanup) = {
            let mut state = self.lock_state();
            let effect = match state.nodes.get_mut(&id) {
                Some(ThreadSafeNode::Effect(effect)) => effect,
                _ => return,
            };
            let old_dependencies = effect.dependencies.drain().collect::<Vec<_>>();
            let cleanup = effect.cleanup.take();
            effect.force_run = false;
            (Arc::clone(&effect.run), old_dependencies, cleanup)
        };

        for dependency_id in old_dependencies {
            self.remove_dependent_edge(dependency_id, id);
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }

        let _tracking = push_tracking_frame(self.context_id(), id);
        let next_cleanup = run(self);
        drop(_tracking);

        let mut state = self.lock_state();
        if let Some(ThreadSafeNode::Effect(effect)) = state.nodes.get_mut(&id) {
            effect.cleanup = next_cleanup;
        } else if let Some(cleanup) = next_cleanup {
            drop(state);
            cleanup();
        }
    }

    fn effect_should_run(&self, id: SlotId) -> bool {
        let (force_run, dependencies) = {
            let state = self.lock_state();
            let Some(ThreadSafeNode::Effect(effect)) = state.nodes.get(&id) else {
                return false;
            };
            (
                effect.force_run,
                effect.dependencies.iter().copied().collect::<Vec<_>>(),
            )
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
        let should_clear = {
            let mut state = self.lock_state();
            if state.batch_depth > 0 {
                state.batched_slots.insert(id);
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
        let dependents = {
            if let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get_mut(&id) {
                if slot.value.is_none() && !slot.dirty {
                    return;
                }
                slot.value = None;
                slot.dirty = false;
                slot.force_recompute = false;
                slot.revision = slot.revision.wrapping_add(1);
                slot.dependents.iter().copied().collect::<Vec<_>>()
            } else {
                return;
            }
        };

        for dependent_id in dependents {
            Self::clear_dependent_locked(state, dependent_id);
        }
    }

    fn clear_slot_now(&self, id: SlotId) {
        let mut state = self.lock_state();
        Self::clear_slot_now_locked(&mut state, id);
    }

    /// Clear all dependent slots without changing the cell value.
    pub fn clear_cell_dependents<T>(&self, handle: &CellHandle<T>) {
        let should_flush = {
            let mut state = self.lock_state();
            if state.batch_depth > 0 {
                state.batched_cell_clears.insert(handle.id);
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
        let dependents = {
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Cell(cell)) => cell.dependents.iter().copied().collect(),
                _ => Vec::new(),
            }
        };

        for dependent_id in dependents {
            Self::invalidate_dependent_from_changed_value_locked(state, dependent_id);
        }
    }

    fn clear_cell_dependents_locked(state: &mut ThreadSafeState, id: SlotId) {
        let dependents = {
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Cell(cell)) => cell.dependents.iter().copied().collect(),
                _ => Vec::new(),
            }
        };

        for dependent_id in dependents {
            Self::clear_dependent_locked(state, dependent_id);
        }
    }

    fn clear_dependent_locked(state: &mut ThreadSafeState, id: SlotId) {
        let is_effect = matches!(state.nodes.get(&id), Some(ThreadSafeNode::Effect(_)));

        if is_effect {
            Self::schedule_effect_locked(state, id, true);
        } else {
            Self::clear_slot_now_locked(state, id);
        }
    }

    fn invalidate_dependent_from_changed_value_locked(state: &mut ThreadSafeState, id: SlotId) {
        let is_effect = matches!(state.nodes.get(&id), Some(ThreadSafeNode::Effect(_)));

        if is_effect {
            Self::schedule_effect_locked(state, id, true);
        } else {
            Self::mark_slot_dirty_locked(state, id, true);
        }
    }

    fn notify_slot_value_changed_locked(state: &mut ThreadSafeState, id: SlotId) {
        let dependents = {
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot.dependents.iter().copied().collect(),
                _ => Vec::new(),
            }
        };

        for dependent_id in dependents {
            Self::invalidate_dependent_from_changed_value_locked(state, dependent_id);
        }
    }

    fn mark_slot_dirty_locked(state: &mut ThreadSafeState, id: SlotId, force_recompute: bool) {
        let (dependents, should_propagate) = {
            let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get_mut(&id) else {
                return;
            };
            let should_propagate = !slot.dirty || (force_recompute && !slot.force_recompute);
            slot.revision = slot.revision.wrapping_add(1);
            slot.dirty = true;
            if force_recompute {
                slot.force_recompute = true;
            }
            (
                slot.dependents.iter().copied().collect::<Vec<_>>(),
                should_propagate,
            )
        };

        if !should_propagate {
            return;
        }

        for dependent_id in dependents {
            let is_effect = matches!(
                state.nodes.get(&dependent_id),
                Some(ThreadSafeNode::Effect(_))
            );

            if is_effect {
                Self::schedule_effect_locked(state, dependent_id, false);
            } else {
                Self::mark_slot_dirty_locked(state, dependent_id, false);
            }
        }
    }

    /// Check whether a slot currently has a cached, fresh value.
    pub fn is_set<T>(&self, handle: &SlotHandle<T>) -> bool
    where
        T: Send + Sync + 'static,
    {
        let state = self.lock_state();
        if let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get(&handle.id) {
            slot.value.is_some() && !slot.dirty
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
        snapshot
    }

    /// Return ThreadSafeContext graph-lock counters grouped by operation.
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
    }
}
