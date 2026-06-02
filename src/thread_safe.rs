use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(feature = "instrumentation")]
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct ThreadSafeContextId(usize);

struct ThreadSafeTrackingFrame {
    context_id: ThreadSafeContextId,
    node_id: SlotId,
    known_dependencies: HashSet<SlotId>,
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
            known_dependencies: HashSet::new(),
            dependencies: HashSet::new(),
        });
    });
    TrackingGuard { active: true }
}

fn push_tracking_frame_with_known_dependencies(
    context_id: ThreadSafeContextId,
    node_id: SlotId,
    known_dependencies: HashSet<SlotId>,
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
    compute: Arc<ThreadSafeComputeFn>,
    equals: Option<Arc<ThreadSafeEqualsFn>>,
    dependencies: HashSet<SlotId>,
    dependents: HashSet<SlotId>,
    recompute_waiters: Arc<ThreadSafeRecomputeWaiters>,
    fast_path: Arc<ThreadSafeSlotFastPath>,
    dirty: bool,
    force_recompute: bool,
    computing: bool,
    revision: u64,
}

#[derive(Default)]
struct ThreadSafeSlotFastPath {
    value: RwLock<Option<Arc<ThreadSafeAny>>>,
    dirty: AtomicBool,
    force_recompute: AtomicBool,
}

impl ThreadSafeSlotFastPath {
    fn read_fresh<T>(&self) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        if self.dirty.load(Ordering::Acquire) || self.force_recompute.load(Ordering::Acquire) {
            return None;
        }

        self.value
            .read()
            .expect("ThreadSafeContext slot fast path rwlock poisoned")
            .as_ref()
            .map(|value| {
                value
                    .downcast_ref::<T>()
                    .expect("type mismatch in slot")
                    .clone()
            })
    }

    fn store_value(&self, value: Option<Arc<ThreadSafeAny>>) {
        *self
            .value
            .write()
            .expect("ThreadSafeContext slot fast path rwlock poisoned") = value;
    }

    fn mark_dirty(&self, force_recompute: bool) {
        self.dirty.store(true, Ordering::Release);
        if force_recompute {
            self.force_recompute.store(true, Ordering::Release);
        }
    }

    fn mark_fresh(&self) {
        self.force_recompute.store(false, Ordering::Release);
        self.dirty.store(false, Ordering::Release);
    }

    fn clear(&self) {
        self.store_value(None);
        self.force_recompute.store(false, Ordering::Release);
        self.dirty.store(false, Ordering::Release);
    }
}

#[derive(Default)]
struct ThreadSafeRecomputeWaiters {
    generation: Mutex<u64>,
    condvar: Condvar,
}

impl ThreadSafeRecomputeWaiters {
    fn lock_generation(&self) -> MutexGuard<'_, u64> {
        self.generation
            .lock()
            .expect("ThreadSafeContext recompute waiter mutex poisoned")
    }

    fn wait_until_changed(&self, mut generation: MutexGuard<'_, u64>, observed_generation: u64) {
        while *generation == observed_generation {
            generation = self
                .condvar
                .wait(generation)
                .expect("ThreadSafeContext recompute waiter mutex poisoned while waiting");
        }
    }

    fn notify_all(&self) {
        let mut generation = self.lock_generation();
        *generation = generation.wrapping_add(1);
        self.condvar.notify_all();
    }
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

enum ThreadSafeSlotRead<T> {
    Fresh(T),
    Refresh(Vec<SlotId>),
}

#[derive(Clone, Copy)]
struct ThreadSafeInvalidationRoot {
    id: SlotId,
    force_recompute: bool,
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
    slot_fast_paths: RwLock<HashMap<SlotId, Arc<ThreadSafeSlotFastPath>>>,
    #[cfg(feature = "instrumentation")]
    lock_instrumentation: crate::instrumentation::ThreadSafeLockInstrumentation,
}

impl Default for ThreadSafeInner {
    fn default() -> Self {
        Self {
            state: Mutex::new(ThreadSafeState::default()),
            slot_fast_paths: RwLock::new(HashMap::new()),
            #[cfg(feature = "instrumentation")]
            lock_instrumentation: crate::instrumentation::ThreadSafeLockInstrumentation::default(),
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

    fn slot_fast_path(&self, id: SlotId) -> Option<Arc<ThreadSafeSlotFastPath>> {
        self.inner
            .slot_fast_paths
            .read()
            .expect("ThreadSafeContext slot fast path registry poisoned")
            .get(&id)
            .cloned()
    }

    fn try_read_fresh_slot_fast_path<T>(&self, id: SlotId) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.slot_fast_path(id)
            .and_then(|fast_path| fast_path.read_fresh())
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
        let fast_path = Arc::new(ThreadSafeSlotFastPath::default());
        let node = ThreadSafeSlotNode {
            value: None,
            compute: Arc::new(move |ctx| Box::new(compute(ctx))),
            equals,
            dependencies: HashSet::new(),
            dependents: HashSet::new(),
            recompute_waiters: Arc::new(ThreadSafeRecomputeWaiters::default()),
            fast_path: Arc::clone(&fast_path),
            dirty: false,
            force_recompute: false,
            computing: false,
            revision: 0,
        };
        self.inner
            .slot_fast_paths
            .write()
            .expect("ThreadSafeContext slot fast path registry poisoned")
            .insert(id, fast_path);
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
        match state.nodes.get(&id) {
            Some(ThreadSafeNode::Slot(slot)) => {
                if let (false, false, Some(value)) = (slot.dirty, slot.force_recompute, &slot.value)
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
                                    state.nodes.get(dependency_id),
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
        #[cfg(feature = "instrumentation")]
        let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::GetRefresh);
        let dependencies: Vec<SlotId> = {
            let state = self.read_state();
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot
                    .dependencies
                    .iter()
                    .filter(|dependency_id| {
                        matches!(
                            state.nodes.get(dependency_id),
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

    fn refresh_slot_with_dependencies(&self, id: SlotId, dependencies: Vec<SlotId>) -> bool {
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
            let slot = match state.nodes.get_mut(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => return false,
            };

            if slot.value.is_none() || slot.force_recompute || dependency_changed {
                true
            } else {
                slot.dirty = false;
                slot.force_recompute = false;
                slot.fast_path.mark_fresh();
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

        let _tracking = push_tracking_frame_with_known_dependencies(
            self.context_id(),
            id,
            old_dependencies.clone(),
        );
        let result = compute(self);
        let new_dependencies = _tracking.finish();
        let result = Arc::<ThreadSafeAny>::from(result);

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
                slot.fast_path.mark_fresh();
                ThreadSafeRecomputeResult::Fresh(false)
            } else {
                slot.value = Some(Arc::clone(&result));
                slot.fast_path.store_value(Some(result));
                slot.fast_path.mark_fresh();
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
        loop {
            let state = self.read_state();
            let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get(&id) else {
                return ThreadSafeRecomputeResult::Fresh(false);
            };

            if slot.computing {
                let recompute_waiters = Arc::clone(&slot.recompute_waiters);
                let generation = recompute_waiters.lock_generation();
                let observed_generation = *generation;
                drop(state);
                recompute_waiters.wait_until_changed(generation, observed_generation);
                continue;
            }

            if slot.value.is_some() && !slot.dirty && !slot.force_recompute {
                return ThreadSafeRecomputeResult::Fresh(false);
            }

            return ThreadSafeRecomputeResult::Stale;
        }
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

        let state = self.read_state();
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
        self.read_state().batch_depth > 0
    }

    fn flush_batched_invalidations(&self) {
        {
            #[cfg(feature = "instrumentation")]
            let _lock_site = push_thread_safe_lock_site(ThreadSafeLockSite::SetCellInvalidation);
            let mut state = self.lock_state();
            let cells = state.batched_cells.drain().collect::<Vec<_>>();
            let cell_clears = state.batched_cell_clears.drain().collect::<Vec<_>>();
            let slots = state.batched_slots.drain().collect::<Vec<_>>();

            let invalidation_roots = cells
                .into_iter()
                .flat_map(|cell_id| {
                    Self::dependents_locked(&state, cell_id)
                        .into_iter()
                        .map(|id| ThreadSafeInvalidationRoot {
                            id,
                            force_recompute: true,
                        })
                })
                .collect::<Vec<_>>();
            Self::invalidate_frontier_locked(&mut state, invalidation_roots);

            let mut clear_roots = cell_clears
                .into_iter()
                .flat_map(|cell_id| Self::dependents_locked(&state, cell_id))
                .collect::<Vec<_>>();
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
        let state = self.read_state();
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
            let state = self.read_state();
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
        Self::clear_frontier_locked(state, [id]);
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
        let roots =
            Self::dependents_locked(state, id)
                .into_iter()
                .map(|id| ThreadSafeInvalidationRoot {
                    id,
                    force_recompute: true,
                });
        Self::invalidate_frontier_locked(state, roots);
    }

    fn clear_cell_dependents_locked(state: &mut ThreadSafeState, id: SlotId) {
        Self::clear_frontier_locked(state, Self::dependents_locked(state, id));
    }

    fn notify_slot_value_changed_locked(state: &mut ThreadSafeState, id: SlotId) {
        let roots =
            Self::dependents_locked(state, id)
                .into_iter()
                .map(|id| ThreadSafeInvalidationRoot {
                    id,
                    force_recompute: true,
                });
        Self::invalidate_frontier_locked(state, roots);
    }

    fn dependents_locked(state: &ThreadSafeState, id: SlotId) -> Vec<SlotId> {
        match state.nodes.get(&id) {
            Some(ThreadSafeNode::Slot(slot)) => slot.dependents.iter().copied().collect(),
            Some(ThreadSafeNode::Cell(cell)) => cell.dependents.iter().copied().collect(),
            Some(ThreadSafeNode::Effect(_)) | None => Vec::new(),
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
        let mut queue = VecDeque::new();
        let mut requested_force = HashMap::new();
        for root in roots {
            Self::enqueue_invalidation_root(&mut queue, &mut requested_force, root);
        }

        let mut simulated_slots = HashMap::<SlotId, (bool, bool)>::new();
        let mut slots_to_mark = HashMap::<SlotId, bool>::new();
        let mut slot_order = Vec::new();
        let mut effects_to_schedule = HashMap::<SlotId, bool>::new();
        let mut effect_order = Vec::new();

        while let Some(root) = queue.pop_front() {
            let Some(force_recompute) = requested_force.get(&root.id).copied() else {
                continue;
            };
            if root.force_recompute != force_recompute {
                continue;
            }

            let dependents = match state.nodes.get(&root.id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    let (dirty, force_state) = simulated_slots
                        .get(&root.id)
                        .copied()
                        .unwrap_or((slot.dirty, slot.force_recompute));
                    let should_propagate = !dirty || (force_recompute && !force_state);
                    simulated_slots.insert(root.id, (true, force_state || force_recompute));

                    match slots_to_mark.get_mut(&root.id) {
                        Some(force) => *force |= force_recompute,
                        None => {
                            slots_to_mark.insert(root.id, force_recompute);
                            slot_order.push(root.id);
                        }
                    }

                    if should_propagate {
                        slot.dependents.iter().copied().collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
                }
                Some(ThreadSafeNode::Effect(_)) => {
                    match effects_to_schedule.get_mut(&root.id) {
                        Some(force) => *force |= force_recompute,
                        None => {
                            effects_to_schedule.insert(root.id, force_recompute);
                            effect_order.push(root.id);
                        }
                    }
                    Vec::new()
                }
                Some(ThreadSafeNode::Cell(_)) | None => Vec::new(),
            };

            for dependent_id in dependents {
                Self::enqueue_invalidation_root(
                    &mut queue,
                    &mut requested_force,
                    ThreadSafeInvalidationRoot {
                        id: dependent_id,
                        force_recompute: false,
                    },
                );
            }
        }

        for id in slot_order {
            let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get_mut(&id) else {
                continue;
            };
            slot.revision = slot.revision.wrapping_add(1);
            slot.dirty = true;
            if slots_to_mark.get(&id).copied().unwrap_or(false) {
                slot.force_recompute = true;
            }
            slot.fast_path.mark_dirty(slot.force_recompute);
        }

        for id in effect_order {
            Self::schedule_effect_locked(
                state,
                id,
                effects_to_schedule.get(&id).copied().unwrap_or(false),
            );
        }
    }

    fn clear_frontier_locked<I>(state: &mut ThreadSafeState, roots: I)
    where
        I: IntoIterator<Item = SlotId>,
    {
        let mut queue = roots.into_iter().collect::<VecDeque<_>>();
        let mut visited_slots = HashSet::new();
        let mut slots_to_clear = Vec::new();
        let mut effects_to_schedule = HashSet::new();
        let mut effect_order = Vec::new();

        while let Some(id) = queue.pop_front() {
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => {
                    if !visited_slots.insert(id) {
                        continue;
                    }
                    if slot.value.is_none() && !slot.dirty {
                        continue;
                    }
                    slots_to_clear.push(id);
                    for dependent_id in slot.dependents.iter().copied() {
                        queue.push_back(dependent_id);
                    }
                }
                Some(ThreadSafeNode::Effect(_)) => {
                    if effects_to_schedule.insert(id) {
                        effect_order.push(id);
                    }
                }
                Some(ThreadSafeNode::Cell(_)) | None => {}
            }
        }

        for id in slots_to_clear {
            let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get_mut(&id) else {
                continue;
            };
            slot.value = None;
            slot.dirty = false;
            slot.force_recompute = false;
            slot.revision = slot.revision.wrapping_add(1);
            slot.fast_path.clear();
        }

        for id in effect_order {
            Self::schedule_effect_locked(state, id, true);
        }
    }

    /// Check whether a slot currently has a cached, fresh value.
    pub fn is_set<T>(&self, handle: &SlotHandle<T>) -> bool
    where
        T: Send + Sync + 'static,
    {
        let state = self.read_state();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn slot_revision<T>(ctx: &ThreadSafeContext, handle: &SlotHandle<T>) -> u64
    where
        T: Send + Sync + 'static,
    {
        let state = ctx.lock_state();
        match state.nodes.get(&handle.id) {
            Some(ThreadSafeNode::Slot(slot)) => slot.revision,
            _ => panic!("slot_revision called on non-slot id"),
        }
    }

    fn slot_dirty_force<T>(ctx: &ThreadSafeContext, handle: &SlotHandle<T>) -> (bool, bool)
    where
        T: Send + Sync + 'static,
    {
        let state = ctx.lock_state();
        match state.nodes.get(&handle.id) {
            Some(ThreadSafeNode::Slot(slot)) => (slot.dirty, slot.force_recompute),
            _ => panic!("slot_dirty_force called on non-slot id"),
        }
    }

    fn cell_dependents_len<T>(ctx: &ThreadSafeContext, handle: &CellHandle<T>) -> usize
    where
        T: Send + Sync + 'static,
    {
        let state = ctx.lock_state();
        match state.nodes.get(&handle.id) {
            Some(ThreadSafeNode::Cell(cell)) => cell.dependents.len(),
            _ => panic!("cell_dependents_len called on non-cell id"),
        }
    }

    fn effect_is_scheduled(ctx: &ThreadSafeContext, handle: &EffectHandle) -> bool {
        let state = ctx.lock_state();
        state.scheduled_effects.contains(&handle.id)
    }

    fn pending_effect_count(ctx: &ThreadSafeContext) -> usize {
        ctx.lock_state().pending_effects.len()
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
