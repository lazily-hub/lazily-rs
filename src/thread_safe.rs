use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::cell::CellHandle;
use crate::context::SlotId;
use crate::effect::EffectHandle;
use crate::slot::SlotHandle;

type ThreadSafeAny = dyn Any + Send + Sync;
type ThreadSafeComputeFn = dyn Fn(&ThreadSafeContext) -> Box<ThreadSafeAny> + Send + Sync;
type ThreadSafeEqualsFn = dyn Fn(&ThreadSafeAny, &ThreadSafeAny) -> bool + Send + Sync;
type ThreadSafeCleanup = dyn FnOnce() + Send;
type ThreadSafeEffectFn =
    dyn Fn(&ThreadSafeContext) -> Option<Box<ThreadSafeCleanup>> + Send + Sync;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ThreadSafeContextId(usize);

#[derive(Clone, Copy)]
struct ThreadSafeTrackingFrame {
    context_id: ThreadSafeContextId,
    node_id: SlotId,
}

thread_local! {
    static THREAD_SAFE_TRACKING_STACK: RefCell<Vec<ThreadSafeTrackingFrame>> =
        const { RefCell::new(Vec::new()) };
}

struct TrackingGuard;

impl Drop for TrackingGuard {
    fn drop(&mut self) {
        THREAD_SAFE_TRACKING_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn push_tracking_frame(context_id: ThreadSafeContextId, node_id: SlotId) -> TrackingGuard {
    THREAD_SAFE_TRACKING_STACK.with(|stack| {
        stack.borrow_mut().push(ThreadSafeTrackingFrame {
            context_id,
            node_id,
        });
    });
    TrackingGuard
}

fn current_tracking_frame(context_id: ThreadSafeContextId) -> Option<SlotId> {
    THREAD_SAFE_TRACKING_STACK.with(|stack| {
        stack
            .borrow()
            .iter()
            .rev()
            .find(|frame| frame.context_id == context_id)
            .map(|frame| frame.node_id)
    })
}

struct ThreadSafeSlotNode {
    value: Option<Box<ThreadSafeAny>>,
    compute: Arc<ThreadSafeComputeFn>,
    equals: Option<Arc<ThreadSafeEqualsFn>>,
    dependencies: HashSet<SlotId>,
    dependents: HashSet<SlotId>,
    dirty: bool,
    force_recompute: bool,
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
}

#[derive(Default)]
struct ThreadSafeInner {
    state: Mutex<ThreadSafeState>,
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

    fn lock_state(&self) -> MutexGuard<'_, ThreadSafeState> {
        self.inner
            .state
            .lock()
            .expect("ThreadSafeContext mutex poisoned")
    }

    fn alloc_id(&self) -> SlotId {
        let mut state = self.lock_state();
        let slot_id = SlotId(state.next_id);
        state.next_id += 1;
        slot_id
    }

    fn register_dependency(&self, dependency_id: SlotId, dependent_id: SlotId) {
        if dependency_id == dependent_id {
            return;
        }

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
                    parent.dependencies.insert(dependency_id);
                }
                ThreadSafeNode::Effect(parent) => {
                    parent.dependencies.insert(dependency_id);
                }
                ThreadSafeNode::Cell(_) => {}
            }
        }
    }

    fn remove_dependent_edge(&self, dependency_id: SlotId, dependent_id: SlotId) {
        let mut state = self.lock_state();
        if let Some(node) = state.nodes.get_mut(&dependency_id) {
            match node {
                ThreadSafeNode::Slot(slot) => {
                    slot.dependents.remove(&dependent_id);
                }
                ThreadSafeNode::Cell(cell) => {
                    cell.dependents.remove(&dependent_id);
                }
                ThreadSafeNode::Effect(_) => {}
            }
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
            dirty: false,
            force_recompute: false,
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
        if let Some(parent_id) = current_tracking_frame(self.context_id()) {
            self.register_dependency(id, parent_id);
        }

        self.refresh_slot(id);

        let state = self.lock_state();
        if let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get(&id)
            && let Some(value) = &slot.value
        {
            return value
                .downcast_ref::<T>()
                .expect("type mismatch in slot")
                .clone();
        }
        panic!("get_slot called on unset or non-slot id");
    }

    fn refresh_slot(&self, id: SlotId) -> bool {
        let dependencies: Vec<SlotId> = {
            let state = self.lock_state();
            match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot.dependencies.iter().copied().collect(),
                _ => return false,
            }
        };

        let mut dependency_changed = false;
        for dependency_id in dependencies {
            if self.is_slot_node(dependency_id) && self.refresh_slot(dependency_id) {
                dependency_changed = true;
            }
        }

        let needs_recompute = {
            let state = self.lock_state();
            let slot = match state.nodes.get(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => return false,
            };
            slot.value.is_none() || slot.force_recompute || dependency_changed
        };

        if !needs_recompute {
            self.clear_slot_dirty_flags(id);
            return false;
        }

        self.recompute_slot_now(id)
    }

    fn is_slot_node(&self, id: SlotId) -> bool {
        let state = self.lock_state();
        matches!(state.nodes.get(&id), Some(ThreadSafeNode::Slot(_)))
    }

    fn clear_slot_dirty_flags(&self, id: SlotId) {
        let mut state = self.lock_state();
        if let Some(ThreadSafeNode::Slot(slot)) = state.nodes.get_mut(&id) {
            slot.dirty = false;
            slot.force_recompute = false;
        }
    }

    fn recompute_slot_now(&self, id: SlotId) -> bool {
        let (compute, old_dependencies, was_unset) = {
            let mut state = self.lock_state();
            let slot = match state.nodes.get_mut(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => panic!("get_slot called on non-slot id"),
            };
            (
                Arc::clone(&slot.compute),
                slot.dependencies.drain().collect::<Vec<_>>(),
                slot.value.is_none(),
            )
        };

        for dependency_id in old_dependencies {
            self.remove_dependent_edge(dependency_id, id);
        }

        let _tracking = push_tracking_frame(self.context_id(), id);
        let result = compute(self);
        drop(_tracking);

        {
            let mut state = self.lock_state();
            let slot = match state.nodes.get_mut(&id) {
                Some(ThreadSafeNode::Slot(slot)) => slot,
                _ => return false,
            };

            if was_unset && slot.value.is_some() && !slot.dirty && !slot.force_recompute {
                return false;
            }

            let had_value = slot.value.is_some();
            let unchanged = match (&slot.value, &slot.equals) {
                (Some(old), Some(equals)) => equals(old.as_ref(), result.as_ref()),
                _ => false,
            };
            slot.dirty = false;
            slot.force_recompute = false;
            if unchanged {
                false
            } else {
                slot.value = Some(result);
                if had_value {
                    Self::notify_slot_value_changed_locked(&mut state, id);
                    true
                } else {
                    false
                }
            }
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
        if let Some(parent_id) = current_tracking_frame(self.context_id()) {
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

        dependencies.into_iter().any(|dependency_id| {
            self.is_slot_node(dependency_id) && self.refresh_slot(dependency_id)
        })
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
}
