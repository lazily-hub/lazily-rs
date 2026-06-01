use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::cell::CellHandle;
use crate::effect::{EffectCallbackResult, EffectHandle};
use crate::slot::SlotHandle;

/// Type alias for the erased compute function stored in slots.
type ComputeFn = dyn Fn(&Context) -> Box<dyn Any>;
/// Type alias for the erased effect callback stored in effects.
type EffectFn = dyn Fn(&Context) -> Option<Box<dyn FnOnce()>>;

/// Unique identifier for a reactive node (slot or cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(u64);

// ---------------------------------------------------------------------------
// Thread-local tracking stack for automatic dependency discovery
// ---------------------------------------------------------------------------

thread_local! {
    static TRACKING_STACK: RefCell<Vec<SlotId>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn push_tracking_frame(id: SlotId) {
    TRACKING_STACK.with(|stack| stack.borrow_mut().push(id));
}

pub(crate) fn pop_tracking_frame() {
    TRACKING_STACK.with(|stack| stack.borrow_mut().pop());
}

/// If there is an active tracking frame, return the id of the slot currently
/// being computed (i.e. the dependent that should subscribe to whatever is
/// being accessed).
pub(crate) fn current_tracking_frame() -> Option<SlotId> {
    TRACKING_STACK.with(|stack| stack.borrow().last().copied())
}

// ---------------------------------------------------------------------------
// Internal node kinds stored inside Context
// ---------------------------------------------------------------------------

pub(crate) struct SlotNode {
    /// The cached value, if set.
    pub(crate) value: Option<Box<dyn Any>>,
    /// The compute closure (type-erased).
    pub(crate) compute: Box<ComputeFn>,
    /// Slots/cells that this slot depends on (parents). Populated during compute.
    pub(crate) dependencies: HashSet<SlotId>,
    /// Slots that depend on this node (children / subscribers).
    pub(crate) dependents: HashSet<SlotId>,
}

pub(crate) struct CellNode {
    pub(crate) value: Box<dyn Any>,
    /// Slots that depend on this cell.
    pub(crate) dependents: HashSet<SlotId>,
}

pub(crate) struct EffectNode {
    /// The effect callback.
    pub(crate) run: Box<EffectFn>,
    /// Slots/cells that this effect depends on. Populated during each run.
    pub(crate) dependencies: HashSet<SlotId>,
    /// Cleanup returned by the latest effect run, if any.
    pub(crate) cleanup: Option<Box<dyn FnOnce()>>,
}

pub(crate) enum Node {
    Slot(SlotNode),
    Cell(CellNode),
    Effect(EffectNode),
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// Container for all reactive nodes. Owns allocations; uses interior
/// mutability (`RefCell`) for single-threaded use.
pub struct Context {
    pub(crate) nodes: RefCell<HashMap<SlotId, Node>>,
    pub(crate) next_id: RefCell<u64>,
    pub(crate) pending_effects: RefCell<VecDeque<SlotId>>,
    pub(crate) scheduled_effects: RefCell<HashSet<SlotId>>,
    pub(crate) flushing_effects: RefCell<bool>,
    pub(crate) batch_depth: RefCell<usize>,
    pub(crate) batched_cells: RefCell<HashSet<SlotId>>,
    pub(crate) batched_slots: RefCell<HashSet<SlotId>>,
}

struct BatchGuard<'a> {
    ctx: &'a Context,
}

impl Drop for BatchGuard<'_> {
    fn drop(&mut self) {
        self.ctx.finish_batch();
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            nodes: RefCell::new(HashMap::new()),
            next_id: RefCell::new(0),
            pending_effects: RefCell::new(VecDeque::new()),
            scheduled_effects: RefCell::new(HashSet::new()),
            flushing_effects: RefCell::new(false),
            batch_depth: RefCell::new(0),
            batched_cells: RefCell::new(HashSet::new()),
            batched_slots: RefCell::new(HashSet::new()),
        }
    }

    pub(crate) fn alloc_id(&self) -> SlotId {
        let mut id = self.next_id.borrow_mut();
        let slot_id = SlotId(*id);
        *id += 1;
        slot_id
    }

    fn register_dependency(&self, dependency_id: SlotId, dependent_id: SlotId) {
        if dependency_id == dependent_id {
            return;
        }

        let mut nodes = self.nodes.borrow_mut();
        // The node being accessed gets `dependent_id` as a dependent.
        if let Some(node) = nodes.get_mut(&dependency_id) {
            match node {
                Node::Slot(s) => {
                    s.dependents.insert(dependent_id);
                }
                Node::Cell(c) => {
                    c.dependents.insert(dependent_id);
                }
                Node::Effect(_) => {}
            }
        }

        // The currently-running slot/effect records the accessed node as a
        // dependency. Cells never run and therefore never track dependencies.
        if let Some(node) = nodes.get_mut(&dependent_id) {
            match node {
                Node::Slot(parent) => {
                    parent.dependencies.insert(dependency_id);
                }
                Node::Effect(parent) => {
                    parent.dependencies.insert(dependency_id);
                }
                Node::Cell(_) => {}
            }
        }
    }

    fn remove_dependent_edge(&self, dependency_id: SlotId, dependent_id: SlotId) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(dep_node) = nodes.get_mut(&dependency_id) {
            match dep_node {
                Node::Slot(s) => {
                    s.dependents.remove(&dependent_id);
                }
                Node::Cell(c) => {
                    c.dependents.remove(&dependent_id);
                }
                Node::Effect(_) => {}
            }
        }
    }

    fn invalidate_dependent(&self, id: SlotId) {
        let is_effect = {
            let nodes = self.nodes.borrow();
            matches!(nodes.get(&id), Some(Node::Effect(_)))
        };

        if is_effect {
            self.schedule_effect(id);
        } else {
            self.clear_slot(id);
        }
    }

    // -- Slot API ----------------------------------------------------------

    /// Create a new lazily-computed slot.
    pub fn slot<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: 'static,
        F: Fn(&Context) -> T + 'static,
    {
        let id = self.alloc_id();
        let node = SlotNode {
            value: None,
            compute: Box::new(move |ctx| Box::new(compute(ctx))),
            dependencies: HashSet::new(),
            dependents: HashSet::new(),
        };
        self.nodes.borrow_mut().insert(id, Node::Slot(node));
        SlotHandle::new(id)
    }

    /// Get the value of a slot, computing it if necessary.
    pub fn get<T: Clone + 'static>(&self, handle: &SlotHandle<T>) -> T {
        self.get_slot(handle.id)
    }

    /// Internal: get a slot value by id, performing computation if unset and
    /// registering dependency tracking.
    fn get_slot<T: Clone + 'static>(&self, id: SlotId) -> T {
        // Register dependency if someone is tracking.
        if let Some(parent_id) = current_tracking_frame() {
            self.register_dependency(id, parent_id);
        }

        // Check if value is already cached.
        {
            let nodes = self.nodes.borrow();
            if let Some(Node::Slot(slot)) = nodes.get(&id)
                && let Some(ref val) = slot.value
            {
                return val
                    .downcast_ref::<T>()
                    .expect("type mismatch in slot")
                    .clone();
            }
        }

        // Need to compute. First, collect old dependencies and get a pointer
        // to the compute fn. We must drop the borrow before calling compute
        // since it will recursively call get.
        let compute: Box<ComputeFn>;
        let old_deps: Vec<SlotId>;
        {
            let mut nodes = self.nodes.borrow_mut();
            let slot = match nodes.get_mut(&id) {
                Some(Node::Slot(s)) => s,
                _ => panic!("get_slot called on non-slot id"),
            };
            // Collect old dependencies to clear edges in a second pass.
            old_deps = slot.dependencies.drain().collect();
            // Get a pointer to the compute fn.
            // Safety: we drop the borrow before calling compute, and the
            // pointer is valid because nodes keeps the allocation alive and
            // the compute fn is never mutated.
            compute = unsafe {
                let ptr = &*slot.compute as *const ComputeFn;
                Box::new(move |ctx| (*ptr)(ctx))
            };
        }
        // Clear old dependency edges (remove `id` from each old dep's dependents).
        for dep_id in old_deps {
            self.remove_dependent_edge(dep_id, id);
        }

        // Push tracking frame so nested gets register as dependencies.
        push_tracking_frame(id);
        let result = compute(self);
        pop_tracking_frame();

        // Store the computed value.
        let cloned = result
            .downcast_ref::<T>()
            .expect("type mismatch in slot compute")
            .clone();
        {
            let mut nodes = self.nodes.borrow_mut();
            if let Some(Node::Slot(slot)) = nodes.get_mut(&id) {
                slot.value = Some(result);
            }
        }
        cloned
    }

    /// Get the value of a cell.
    pub fn get_cell<T: Clone + 'static>(&self, handle: &CellHandle<T>) -> T {
        // Register dependency tracking.
        if let Some(parent_id) = current_tracking_frame() {
            self.register_dependency(handle.id, parent_id);
        }

        let nodes = self.nodes.borrow();
        if let Some(Node::Cell(c)) = nodes.get(&handle.id) {
            c.value
                .downcast_ref::<T>()
                .expect("type mismatch in cell")
                .clone()
        } else {
            panic!("get_cell called on non-cell id");
        }
    }

    // -- Cell API ----------------------------------------------------------

    /// Create a new mutable cell with an initial value.
    pub fn cell<T: PartialEq + 'static>(&self, value: T) -> CellHandle<T> {
        let id = self.alloc_id();
        let node = CellNode {
            value: Box::new(value),
            dependents: HashSet::new(),
        };
        self.nodes.borrow_mut().insert(id, Node::Cell(node));
        CellHandle::new(id)
    }

    /// Set the value of a cell. If the value differs (via PartialEq), all
    /// dependent slots are cleared.
    pub fn set_cell<T: PartialEq + 'static>(&self, handle: &CellHandle<T>, new_value: T) {
        let changed = {
            let nodes = self.nodes.borrow();
            if let Some(Node::Cell(c)) = nodes.get(&handle.id) {
                let old = c
                    .value
                    .downcast_ref::<T>()
                    .expect("type mismatch in cell set");
                *old != new_value
            } else {
                panic!("set_cell on non-cell id");
            }
        };

        if changed {
            {
                let mut nodes = self.nodes.borrow_mut();
                if let Some(Node::Cell(c)) = nodes.get_mut(&handle.id) {
                    c.value = Box::new(new_value);
                }
            }
            if self.is_batching() {
                self.batched_cells.borrow_mut().insert(handle.id);
            } else {
                self.invalidate_cell_dependents_now(handle.id);
                self.flush_effects();
            }
        }
    }

    // -- Batch API ---------------------------------------------------------

    /// Run several updates as one invalidation pass.
    ///
    /// Cell updates and explicit clears inside the callback are collected and
    /// applied when the outermost batch completes. Direct cell reads see the
    /// latest values immediately; dependent slots keep their previous cached
    /// values until the batch exits.
    pub fn batch<F, R>(&self, run: F) -> R
    where
        F: FnOnce(&Context) -> R,
    {
        *self.batch_depth.borrow_mut() += 1;
        let _guard = BatchGuard { ctx: self };
        run(self)
    }

    fn finish_batch(&self) {
        let should_flush = {
            let mut depth = self.batch_depth.borrow_mut();
            assert!(*depth > 0, "finish_batch called without active batch");
            *depth -= 1;
            *depth == 0
        };

        if should_flush {
            self.flush_batched_invalidations();
        }
    }

    fn is_batching(&self) -> bool {
        *self.batch_depth.borrow() > 0
    }

    fn flush_batched_invalidations(&self) {
        let cells: Vec<SlotId> = self.batched_cells.borrow_mut().drain().collect();
        let slots: Vec<SlotId> = self.batched_slots.borrow_mut().drain().collect();

        for cell_id in cells {
            self.invalidate_cell_dependents_now(cell_id);
        }
        for slot_id in slots {
            self.clear_slot_now(slot_id);
        }
        self.flush_effects();
    }

    // -- Effect API --------------------------------------------------------

    /// Create an effect, run it immediately, and automatically rerun it after
    /// any cells/slots it read are invalidated.
    ///
    /// The callback may return `()` for no cleanup or a `FnOnce() + 'static`
    /// cleanup closure. Cleanup runs before each rerun and when the effect is
    /// disposed.
    pub fn effect<F, R>(&self, run: F) -> EffectHandle
    where
        F: Fn(&Context) -> R + 'static,
        R: EffectCallbackResult + 'static,
    {
        let id = self.alloc_id();
        let node = EffectNode {
            run: Box::new(move |ctx| run(ctx).into_cleanup()),
            dependencies: HashSet::new(),
            cleanup: None,
        };
        self.nodes.borrow_mut().insert(id, Node::Effect(node));
        let handle = EffectHandle::new(id);
        self.schedule_effect(id);
        self.flush_effects();
        handle
    }

    /// Dispose an effect by handle.
    pub fn dispose_effect(&self, handle: &EffectHandle) {
        let (dependencies, cleanup) = {
            let mut nodes = self.nodes.borrow_mut();
            let Some(Node::Effect(effect)) = nodes.remove(&handle.id) else {
                return;
            };
            (effect.dependencies, effect.cleanup)
        };

        self.scheduled_effects.borrow_mut().remove(&handle.id);
        for dep_id in dependencies {
            self.remove_dependent_edge(dep_id, handle.id);
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }
    }

    /// Check whether an effect is still registered.
    pub fn is_effect_active(&self, handle: &EffectHandle) -> bool {
        let nodes = self.nodes.borrow();
        matches!(nodes.get(&handle.id), Some(Node::Effect(_)))
    }

    fn schedule_effect(&self, id: SlotId) {
        let exists = {
            let nodes = self.nodes.borrow();
            matches!(nodes.get(&id), Some(Node::Effect(_)))
        };
        if !exists {
            return;
        }

        let inserted = self.scheduled_effects.borrow_mut().insert(id);
        if inserted {
            self.pending_effects.borrow_mut().push_back(id);
        }
    }

    pub(crate) fn flush_effects(&self) {
        {
            let mut flushing = self.flushing_effects.borrow_mut();
            if *flushing {
                return;
            }
            *flushing = true;
        }

        loop {
            let Some(id) = ({ self.pending_effects.borrow_mut().pop_front() }) else {
                break;
            };
            self.scheduled_effects.borrow_mut().remove(&id);
            self.run_effect(id);
        }

        *self.flushing_effects.borrow_mut() = false;
    }

    fn run_effect(&self, id: SlotId) {
        // Collect old dependencies and callback pointer, then drop the borrow
        // before running user code because the effect may read or write context.
        let run: Box<EffectFn>;
        let old_deps: Vec<SlotId>;
        let cleanup: Option<Box<dyn FnOnce()>>;
        {
            let mut nodes = self.nodes.borrow_mut();
            let effect = match nodes.get_mut(&id) {
                Some(Node::Effect(effect)) => effect,
                _ => return,
            };
            old_deps = effect.dependencies.drain().collect();
            cleanup = effect.cleanup.take();
            // Safety: nodes keeps the boxed callback allocation alive while the
            // effect node exists, and this method is single-threaded.
            run = unsafe {
                let ptr = &*effect.run as *const EffectFn;
                Box::new(move |ctx| (*ptr)(ctx))
            };
        }

        for dep_id in old_deps {
            self.remove_dependent_edge(dep_id, id);
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }

        push_tracking_frame(id);
        let next_cleanup = run(self);
        pop_tracking_frame();

        let mut nodes = self.nodes.borrow_mut();
        if let Some(Node::Effect(effect)) = nodes.get_mut(&id) {
            effect.cleanup = next_cleanup;
        } else if let Some(cleanup) = next_cleanup {
            drop(nodes);
            cleanup();
        }
    }

    // -- Clearing ----------------------------------------------------------

    /// Clear a slot's cached value and recursively clear all dependents.
    pub(crate) fn clear_slot(&self, id: SlotId) {
        if self.is_batching() {
            self.batched_slots.borrow_mut().insert(id);
            return;
        }
        self.clear_slot_now(id);
    }

    pub(crate) fn flush_effects_after_invalidation(&self) {
        if !self.is_batching() {
            self.flush_effects();
        }
    }

    fn clear_slot_now(&self, id: SlotId) {
        let dependents: Vec<SlotId>;
        {
            let mut nodes = self.nodes.borrow_mut();
            if let Some(Node::Slot(slot)) = nodes.get_mut(&id) {
                if slot.value.is_none() {
                    return; // Already cleared, stop recursion.
                }
                slot.value = None;
                dependents = slot.dependents.iter().copied().collect();
            } else {
                return;
            }
        }
        for dep_id in dependents {
            self.invalidate_dependent(dep_id);
        }
    }

    pub(crate) fn clear_cell_dependents(&self, id: SlotId) {
        if self.is_batching() {
            self.batched_cells.borrow_mut().insert(id);
            return;
        }
        self.invalidate_cell_dependents_now(id);
        self.flush_effects();
    }

    fn invalidate_cell_dependents_now(&self, id: SlotId) {
        let dependents: Vec<SlotId> = {
            let nodes = self.nodes.borrow();
            match nodes.get(&id) {
                Some(Node::Cell(c)) => c.dependents.iter().copied().collect(),
                _ => vec![],
            }
        };
        for dep_id in dependents {
            self.invalidate_dependent(dep_id);
        }
    }

    /// Check whether a slot currently has a cached value (for testing).
    pub fn is_set<T: 'static>(&self, handle: &SlotHandle<T>) -> bool {
        let nodes = self.nodes.borrow();
        if let Some(Node::Slot(slot)) = nodes.get(&handle.id) {
            slot.value.is_some()
        } else {
            false
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
