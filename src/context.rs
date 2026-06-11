use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

#[cfg(not(feature = "vec_edges"))]
use smallvec::SmallVec;

use crate::cell::CellHandle;
use crate::effect::{EffectCallbackResult, EffectHandle};
use crate::slot::SlotHandle;

/// Type alias for the erased compute function stored in slots.
type ComputeFn = dyn Fn(&Context) -> Rc<dyn Any>;
/// Type alias for the erased equality function stored in slots.
type EqualsFn = dyn Fn(&dyn Any, &dyn Any) -> bool;
/// Type alias for the erased effect callback stored in effects.
type EffectFn = dyn Fn(&Context) -> Option<Box<dyn FnOnce()>>;

/// Unique identifier for a reactive node (slot or cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub(crate) u64);

#[cfg(not(feature = "vec_edges"))]
type EdgeVec = SmallVec<[SlotId; 4]>;
#[cfg(feature = "vec_edges")]
type EdgeVec = Vec<SlotId>;

fn edge_insert(edges: &mut EdgeVec, id: SlotId) -> bool {
    if edges.contains(&id) {
        false
    } else {
        edges.push(id);
        true
    }
}

fn edge_remove(edges: &mut EdgeVec, id: SlotId) -> bool {
    if let Some(pos) = edges.iter().position(|x| *x == id) {
        edges.swap_remove(pos);
        true
    } else {
        false
    }
}

#[inline]
unsafe fn downcast_ref_unchecked<T: 'static>(any: &Rc<dyn Any>) -> &T {
    unsafe { &*(&**any as *const dyn Any as *const T) }
}

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
    pub(crate) value: Option<Rc<dyn Any>>,
    pub(crate) type_id: TypeId,
    pub(crate) compute: Rc<ComputeFn>,
    pub(crate) equals: Option<Box<EqualsFn>>,
    pub(crate) dependencies: EdgeVec,
    pub(crate) dependents: EdgeVec,
    pub(crate) dirty: bool,
    pub(crate) force_recompute: bool,
}

pub(crate) struct CellNode {
    pub(crate) value: Rc<dyn Any>,
    pub(crate) type_id: TypeId,
    pub(crate) dependents: EdgeVec,
}

pub(crate) struct EffectNode {
    /// The effect callback.
    pub(crate) run: Rc<EffectFn>,
    /// Slots/cells that this effect depends on. Populated during each run.
    pub(crate) dependencies: EdgeVec,
    /// Cleanup returned by the latest effect run, if any.
    pub(crate) cleanup: Option<Box<dyn FnOnce()>>,
    /// Whether this scheduled effect must run without dependency freshness checks.
    pub(crate) force_run: bool,
}

pub(crate) enum Node {
    Slot(SlotNode),
    Cell(CellNode),
    Effect(EffectNode),
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

struct ContextInner {
    nodes: Vec<Option<Node>>,
    next_id: u64,
    free_ids: Vec<u64>,
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

/// Container for all reactive nodes. Owns allocations; uses a single
/// interior-mutability cell (`RefCell`) for single-threaded use.
pub struct Context {
    inner: RefCell<ContextInner>,
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
            inner: RefCell::new(ContextInner {
                nodes: Vec::new(),
                next_id: 0,
                free_ids: Vec::new(),
                pending_effects: VecDeque::new(),
                scheduled_effects: HashSet::new(),
                flushing_effects: false,
                batch_depth: 0,
                batched_cells: HashSet::new(),
                batched_cell_clears: HashSet::new(),
                batched_slots: HashSet::new(),
                #[cfg(feature = "instrumentation")]
                instrumentation: crate::instrumentation::InstrumentationCounters::default(),
            }),
        }
    }

    pub(crate) fn alloc_id(&self) -> SlotId {
        let mut inner = self.inner.borrow_mut();
        let slot_id = match inner.free_ids.pop() {
            Some(id) => SlotId(id),
            None => {
                let id = SlotId(inner.next_id);
                inner.next_id += 1;
                id
            }
        };
        #[cfg(feature = "instrumentation")]
        {
            inner.instrumentation.record_node_allocation();
        }
        slot_id
    }

    fn node_index(id: SlotId) -> Option<usize> {
        usize::try_from(id.0).ok()
    }

    fn get_node(nodes: &[Option<Node>], id: SlotId) -> Option<&Node> {
        nodes.get(Self::node_index(id)?)?.as_ref()
    }

    fn get_node_mut(nodes: &mut [Option<Node>], id: SlotId) -> Option<&mut Node> {
        nodes.get_mut(Self::node_index(id)?)?.as_mut()
    }

    fn take_node(nodes: &mut [Option<Node>], id: SlotId) -> Option<Node> {
        nodes.get_mut(Self::node_index(id)?)?.take()
    }

    fn insert_node(&self, id: SlotId, node: Node) {
        let index = Self::node_index(id).expect("SlotId does not fit usize");
        let mut inner = self.inner.borrow_mut();
        if inner.nodes.len() <= index {
            inner.nodes.resize_with(index + 1, || None);
        }
        inner.nodes[index] = Some(node);
    }

    fn register_dependency(&self, dependency_id: SlotId, dependent_id: SlotId) {
        if dependency_id == dependent_id {
            return;
        }

        #[cfg(feature = "instrumentation")]
        let mut edge_added = false;
        let mut inner = self.inner.borrow_mut();
        if let Some(node) = Self::get_node_mut(&mut inner.nodes, dependency_id) {
            match node {
                Node::Slot(s) => {
                    edge_insert(&mut s.dependents, dependent_id);
                }
                Node::Cell(c) => {
                    edge_insert(&mut c.dependents, dependent_id);
                }
                Node::Effect(_) => {}
            }
        }

        if let Some(node) = Self::get_node_mut(&mut inner.nodes, dependent_id) {
            match node {
                Node::Slot(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = edge_insert(&mut parent.dependencies, dependency_id);
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        edge_insert(&mut parent.dependencies, dependency_id);
                    }
                }
                Node::Effect(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = edge_insert(&mut parent.dependencies, dependency_id);
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        edge_insert(&mut parent.dependencies, dependency_id);
                    }
                }
                Node::Cell(_) => {}
            }
        }

        #[cfg(feature = "instrumentation")]
        if edge_added {
            inner.instrumentation.record_dependency_edge_added();
        }
    }

    fn remove_dependent_edge(&self, dependency_id: SlotId, dependent_id: SlotId) {
        #[cfg(feature = "instrumentation")]
        let mut edge_removed = false;
        {
            let mut inner = self.inner.borrow_mut();
            if let Some(dep_node) = Self::get_node_mut(&mut inner.nodes, dependency_id) {
                match dep_node {
                    Node::Slot(s) => {
                        #[cfg(feature = "instrumentation")]
                        {
                            edge_removed = edge_remove(&mut s.dependents, dependent_id);
                        }
                        #[cfg(not(feature = "instrumentation"))]
                        {
                            edge_remove(&mut s.dependents, dependent_id);
                        }
                    }
                    Node::Cell(c) => {
                        #[cfg(feature = "instrumentation")]
                        {
                            edge_removed = edge_remove(&mut c.dependents, dependent_id);
                        }
                        #[cfg(not(feature = "instrumentation"))]
                        {
                            edge_remove(&mut c.dependents, dependent_id);
                        }
                    }
                    Node::Effect(_) => {}
                }
            }

            #[cfg(feature = "instrumentation")]
            if edge_removed {
                inner.instrumentation.record_dependency_edge_removed();
            }
        }
    }

    // -- Slot API ----------------------------------------------------------

    /// Create a new lazily-computed slot.
    pub fn slot<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: 'static,
        F: Fn(&Context) -> T + 'static,
    {
        self.slot_with_equals(compute, None)
    }

    /// Create a derived lazily-computed value.
    ///
    /// This is an ergonomic alias for [`Context::slot`].
    pub fn computed<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: 'static,
        F: Fn(&Context) -> T + 'static,
    {
        self.slot(compute)
    }

    /// Create a new lazily-computed slot with a `PartialEq` memoization guard.
    pub fn memo<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: PartialEq + 'static,
        F: Fn(&Context) -> T + 'static,
    {
        self.slot_with_equals(
            compute,
            Some(Box::new(|old, new| {
                let old = old.downcast_ref::<T>().expect("type mismatch in slot");
                let new = new.downcast_ref::<T>().expect("type mismatch in slot");
                old == new
            })),
        )
    }

    fn slot_with_equals<T, F>(&self, compute: F, equals: Option<Box<EqualsFn>>) -> SlotHandle<T>
    where
        T: 'static,
        F: Fn(&Context) -> T + 'static,
    {
        let id = self.alloc_id();
        let node = SlotNode {
            value: None,
            type_id: TypeId::of::<T>(),
            compute: Rc::new(move |ctx| Rc::new(compute(ctx))),
            equals,
            dependencies: EdgeVec::new(),
            dependents: EdgeVec::new(),
            dirty: false,
            force_recompute: false,
        };
        self.insert_node(id, Node::Slot(node));
        SlotHandle::new(id)
    }

    /// Get the value of a slot, computing it if necessary.
    pub fn get<T: Clone + 'static>(&self, handle: &SlotHandle<T>) -> T {
        self.get_slot(handle.id)
    }

    /// Get the value of a slot as `Rc<T>`, avoiding a deep clone.
    ///
    /// Returns a reference-counted pointer to the stored value. Use this when
    /// you only need to read the value without owning a separate copy.
    pub fn get_rc<T: 'static>(&self, handle: &SlotHandle<T>) -> Rc<T> {
        if let Some(parent_id) = current_tracking_frame() {
            self.register_dependency(handle.id, parent_id);
        }

        self.refresh_slot(handle.id);

        let inner = self.inner.borrow();
        if let Some(Node::Slot(slot)) = Self::get_node(&inner.nodes, handle.id)
            && let Some(ref val) = slot.value
        {
            assert!(slot.type_id == TypeId::of::<T>(), "type mismatch in slot");
            let rc: Rc<dyn Any> = val.clone();
            return unsafe {
                let ptr = Rc::into_raw(rc) as *const T;
                Rc::from_raw(ptr)
            };
        }
        panic!("get_rc called on unset or non-slot id");
    }

    /// Internal: get a slot value by id, performing computation if unset and
    /// registering dependency tracking.
    fn get_slot<T: Clone + 'static>(&self, id: SlotId) -> T {
        if let Some(parent_id) = current_tracking_frame() {
            self.register_dependency(id, parent_id);
        }

        self.refresh_slot(id);

        let inner = self.inner.borrow();
        if let Some(Node::Slot(slot)) = Self::get_node(&inner.nodes, id)
            && let Some(ref val) = slot.value
        {
            assert!(slot.type_id == TypeId::of::<T>(), "type mismatch in slot");
            return unsafe { downcast_ref_unchecked::<T>(val) }.clone();
        }
        panic!("get_slot called on unset or non-slot id");
    }

    /// Refresh a slot if its cached value may be stale.
    ///
    /// Returns true only when the slot's computed value changed. Downstream
    /// dependents use this as the memoization guard: a dirty dependency whose
    /// value recomputes equal does not force them to recompute.
    fn refresh_slot(&self, id: SlotId) -> bool {
        let dependencies = {
            let inner = self.inner.borrow();
            match Self::get_node(&inner.nodes, id) {
                Some(Node::Slot(slot)) => slot.dependencies.clone(),
                _ => return false,
            }
        };

        let mut dependency_changed = false;
        for dep_id in dependencies {
            if self.is_slot_node(dep_id) && self.refresh_slot(dep_id) {
                dependency_changed = true;
            }
        }

        let needs_recompute = {
            let inner = self.inner.borrow();
            let slot = match Self::get_node(&inner.nodes, id) {
                Some(Node::Slot(slot)) => slot,
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
        let inner = self.inner.borrow();
        matches!(Self::get_node(&inner.nodes, id), Some(Node::Slot(_)))
    }

    fn clear_slot_dirty_flags(&self, id: SlotId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(Node::Slot(slot)) = Self::get_node_mut(&mut inner.nodes, id) {
            slot.dirty = false;
            slot.force_recompute = false;
        }
    }

    fn recompute_slot_now(&self, id: SlotId) -> bool {
        let compute: Rc<ComputeFn>;
        let old_deps;
        {
            let mut inner = self.inner.borrow_mut();
            #[cfg(feature = "instrumentation")]
            {
                inner.instrumentation.record_slot_recompute();
            }
            let slot = match Self::get_node_mut(&mut inner.nodes, id) {
                Some(Node::Slot(s)) => s,
                _ => panic!("get_slot called on non-slot id"),
            };
            old_deps = std::mem::take(&mut slot.dependencies);
            compute = Rc::clone(&slot.compute);
        }
        for dep_id in old_deps {
            self.remove_dependent_edge(dep_id, id);
        }

        push_tracking_frame(id);
        let result = (compute.as_ref())(self);
        pop_tracking_frame();

        let changed = {
            let mut inner = self.inner.borrow_mut();
            let slot = match Self::get_node_mut(&mut inner.nodes, id) {
                Some(Node::Slot(slot)) => slot,
                _ => return false,
            };
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
                had_value
            }
        };

        if changed {
            self.notify_slot_value_changed(id);
        }

        changed
    }

    /// Get the value of a cell.
    pub fn get_cell<T: Clone + 'static>(&self, handle: &CellHandle<T>) -> T {
        if let Some(parent_id) = current_tracking_frame() {
            self.register_dependency(handle.id, parent_id);
        }

        let inner = self.inner.borrow();
        if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, handle.id) {
            assert!(c.type_id == TypeId::of::<T>(), "type mismatch in cell");
            unsafe { downcast_ref_unchecked::<T>(&c.value) }.clone()
        } else {
            panic!("get_cell called on non-cell id");
        }
    }

    /// Get the value of a cell as `Rc<T>`, avoiding a deep clone.
    pub fn get_cell_rc<T: 'static>(&self, handle: &CellHandle<T>) -> Rc<T> {
        if let Some(parent_id) = current_tracking_frame() {
            self.register_dependency(handle.id, parent_id);
        }

        let inner = self.inner.borrow();
        if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, handle.id) {
            assert!(c.type_id == TypeId::of::<T>(), "type mismatch in cell");
            let rc: Rc<dyn Any> = c.value.clone();
            unsafe {
                let ptr = Rc::into_raw(rc) as *const T;
                Rc::from_raw(ptr)
            }
        } else {
            panic!("get_cell_rc called on non-cell id");
        }
    }

    // -- Cell API ----------------------------------------------------------

    /// Create a new mutable cell with an initial value.
    pub fn cell<T: PartialEq + 'static>(&self, value: T) -> CellHandle<T> {
        let id = self.alloc_id();
        let node = CellNode {
            value: Rc::new(value),
            type_id: TypeId::of::<T>(),
            dependents: EdgeVec::new(),
        };
        self.insert_node(id, Node::Cell(node));
        CellHandle::new(id)
    }

    /// Set the value of a cell. If the value differs (via PartialEq),
    /// dependent slots are marked dirty for memoized validation.
    pub fn set_cell<T: PartialEq + 'static>(&self, handle: &CellHandle<T>, new_value: T) {
        let changed = {
            let inner = self.inner.borrow();
            if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, handle.id) {
                assert!(c.type_id == TypeId::of::<T>(), "type mismatch in cell set");
                let old = unsafe { downcast_ref_unchecked::<T>(&c.value) };
                *old != new_value
            } else {
                panic!("set_cell on non-cell id");
            }
        };

        if changed {
            {
                let mut inner = self.inner.borrow_mut();
                if let Some(Node::Cell(c)) = Self::get_node_mut(&mut inner.nodes, handle.id) {
                    c.value = Rc::new(new_value);
                }
            }
            if self.is_batching() {
                self.inner.borrow_mut().batched_cells.insert(handle.id);
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
        self.inner.borrow_mut().batch_depth += 1;
        let _guard = BatchGuard { ctx: self };
        run(self)
    }

    fn finish_batch(&self) {
        let should_flush = {
            let mut inner = self.inner.borrow_mut();
            assert!(
                inner.batch_depth > 0,
                "finish_batch called without active batch"
            );
            inner.batch_depth -= 1;
            inner.batch_depth == 0
        };

        if should_flush {
            self.flush_batched_invalidations();
        }
    }

    fn is_batching(&self) -> bool {
        self.inner.borrow().batch_depth > 0
    }

    fn flush_batched_invalidations(&self) {
        let cells: Vec<SlotId> = self.inner.borrow_mut().batched_cells.drain().collect();
        let cell_clears: Vec<SlotId> = self
            .inner
            .borrow_mut()
            .batched_cell_clears
            .drain()
            .collect();
        let slots: Vec<SlotId> = self.inner.borrow_mut().batched_slots.drain().collect();

        for cell_id in cells {
            self.invalidate_cell_dependents_now(cell_id);
        }
        for cell_id in cell_clears {
            self.clear_cell_dependents_now(cell_id);
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
            run: Rc::new(move |ctx| run(ctx).into_cleanup()),
            dependencies: EdgeVec::new(),
            cleanup: None,
            force_run: true,
        };
        self.insert_node(id, Node::Effect(node));
        let handle = EffectHandle::new(id);
        self.schedule_effect(id, false);
        self.flush_effects();
        handle
    }

    /// Dispose an effect by handle.
    pub fn dispose_effect(&self, handle: &EffectHandle) {
        let (dependencies, cleanup) = {
            let mut inner = self.inner.borrow_mut();
            let Some(Node::Effect(effect)) = Self::take_node(&mut inner.nodes, handle.id) else {
                return;
            };
            inner.scheduled_effects.remove(&handle.id);
            inner.free_ids.push(handle.id.0);
            (effect.dependencies, effect.cleanup)
        };

        for dep_id in dependencies {
            self.remove_dependent_edge(dep_id, handle.id);
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }
    }

    /// Check whether an effect is still registered.
    pub fn is_effect_active(&self, handle: &EffectHandle) -> bool {
        let inner = self.inner.borrow();
        matches!(
            Self::get_node(&inner.nodes, handle.id),
            Some(Node::Effect(_))
        )
    }

    fn schedule_effect(&self, id: SlotId, force: bool) {
        let mut inner = self.inner.borrow_mut();
        let exists = match Self::get_node_mut(&mut inner.nodes, id) {
            Some(Node::Effect(effect)) => {
                if force {
                    effect.force_run = true;
                }
                true
            }
            _ => false,
        };
        if !exists {
            return;
        }

        if inner.scheduled_effects.insert(id) {
            inner.pending_effects.push_back(id);
            #[cfg(feature = "instrumentation")]
            {
                let depth = inner.pending_effects.len();
                inner.instrumentation.record_effect_queue_push(depth);
            }
        }
    }

    fn remove_pending_effect(&self, id: SlotId) {
        let mut inner = self.inner.borrow_mut();
        inner.pending_effects.retain(|queued| *queued != id);
        inner.scheduled_effects.remove(&id);
    }

    pub(crate) fn flush_effects(&self) {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.flushing_effects {
                return;
            }
            inner.flushing_effects = true;
        }

        loop {
            let id = {
                let mut inner = self.inner.borrow_mut();
                match inner.pending_effects.pop_front() {
                    Some(id) => {
                        inner.scheduled_effects.remove(&id);
                        id
                    }
                    None => {
                        inner.flushing_effects = false;
                        return;
                    }
                }
            };
            self.run_effect(id);
        }
    }

    fn run_effect(&self, id: SlotId) {
        if !self.effect_should_run(id) {
            return;
        }
        self.remove_pending_effect(id);

        let run: Rc<EffectFn>;
        let old_deps;
        let cleanup: Option<Box<dyn FnOnce()>>;
        {
            let mut inner = self.inner.borrow_mut();
            let effect = match Self::get_node_mut(&mut inner.nodes, id) {
                Some(Node::Effect(effect)) => effect,
                _ => return,
            };
            old_deps = std::mem::take(&mut effect.dependencies);
            cleanup = effect.cleanup.take();
            effect.force_run = false;
            run = Rc::clone(&effect.run);
        }

        for dep_id in old_deps {
            self.remove_dependent_edge(dep_id, id);
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }

        push_tracking_frame(id);
        let next_cleanup = (run.as_ref())(self);
        pop_tracking_frame();

        let mut inner = self.inner.borrow_mut();
        if let Some(Node::Effect(effect)) = Self::get_node_mut(&mut inner.nodes, id) {
            effect.cleanup = next_cleanup;
        } else if let Some(cleanup) = next_cleanup {
            drop(inner);
            cleanup();
        }
    }

    fn effect_should_run(&self, id: SlotId) -> bool {
        let (force_run, dependencies) = {
            let inner = self.inner.borrow();
            let Some(Node::Effect(effect)) = Self::get_node(&inner.nodes, id) else {
                return false;
            };
            (effect.force_run, effect.dependencies.clone())
        };

        if force_run {
            return true;
        }

        dependencies
            .into_iter()
            .any(|dep_id| self.is_slot_node(dep_id) && self.refresh_slot(dep_id))
    }

    // -- Clearing ----------------------------------------------------------

    /// Hard-clear a slot's cached value and recursively clear all dependents.
    pub(crate) fn clear_slot(&self, id: SlotId) {
        if self.is_batching() {
            self.inner.borrow_mut().batched_slots.insert(id);
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
        let dependents;
        {
            let mut inner = self.inner.borrow_mut();
            if let Some(Node::Slot(slot)) = Self::get_node_mut(&mut inner.nodes, id) {
                if slot.value.is_none() && !slot.dirty {
                    return;
                }
                slot.value = None;
                slot.dirty = false;
                slot.force_recompute = false;
                dependents = slot.dependents.clone();
            } else {
                return;
            }
        }
        for dep_id in dependents {
            self.clear_dependent_now(dep_id);
        }
    }

    pub(crate) fn clear_cell_dependents(&self, id: SlotId) {
        if self.is_batching() {
            self.inner.borrow_mut().batched_cell_clears.insert(id);
            return;
        }
        self.clear_cell_dependents_now(id);
        self.flush_effects();
    }

    fn invalidate_cell_dependents_now(&self, id: SlotId) {
        let dependents = {
            let inner = self.inner.borrow();
            match Self::get_node(&inner.nodes, id) {
                Some(Node::Cell(c)) => c.dependents.clone(),
                _ => EdgeVec::new(),
            }
        };
        for dep_id in dependents {
            self.invalidate_dependent_from_changed_value(dep_id);
        }
    }

    fn clear_cell_dependents_now(&self, id: SlotId) {
        let dependents = {
            let inner = self.inner.borrow();
            match Self::get_node(&inner.nodes, id) {
                Some(Node::Cell(c)) => c.dependents.clone(),
                _ => EdgeVec::new(),
            }
        };
        for dep_id in dependents {
            self.clear_dependent_now(dep_id);
        }
    }

    fn clear_dependent_now(&self, id: SlotId) {
        let is_effect = {
            let inner = self.inner.borrow();
            matches!(Self::get_node(&inner.nodes, id), Some(Node::Effect(_)))
        };

        if is_effect {
            self.schedule_effect(id, true);
        } else {
            self.clear_slot_now(id);
        }
    }

    fn invalidate_dependent_from_changed_value(&self, id: SlotId) {
        let is_effect = {
            let inner = self.inner.borrow();
            matches!(Self::get_node(&inner.nodes, id), Some(Node::Effect(_)))
        };

        if is_effect {
            self.schedule_effect(id, true);
        } else {
            self.mark_slot_dirty(id, true);
        }
    }

    fn notify_slot_value_changed(&self, id: SlotId) {
        let dependents = {
            let inner = self.inner.borrow();
            match Self::get_node(&inner.nodes, id) {
                Some(Node::Slot(slot)) => slot.dependents.clone(),
                _ => EdgeVec::new(),
            }
        };
        for dep_id in dependents {
            self.invalidate_dependent_from_changed_value(dep_id);
        }
    }

    fn mark_slot_dirty(&self, id: SlotId, force_recompute: bool) {
        let dependents;
        let should_propagate: bool;
        {
            let mut inner = self.inner.borrow_mut();
            let Some(Node::Slot(slot)) = Self::get_node_mut(&mut inner.nodes, id) else {
                return;
            };
            should_propagate = !slot.dirty || (force_recompute && !slot.force_recompute);
            slot.dirty = true;
            if force_recompute {
                slot.force_recompute = true;
            }
            dependents = slot.dependents.clone();
        }

        if !should_propagate {
            return;
        }

        for dep_id in dependents {
            let is_effect = {
                let inner = self.inner.borrow();
                matches!(Self::get_node(&inner.nodes, dep_id), Some(Node::Effect(_)))
            };

            if is_effect {
                self.schedule_effect(dep_id, false);
            } else {
                self.mark_slot_dirty(dep_id, false);
            }
        }
    }

    /// Check whether a slot currently has a cached, fresh value (for testing).
    pub fn is_set<T: 'static>(&self, handle: &SlotHandle<T>) -> bool {
        let inner = self.inner.borrow();
        if let Some(Node::Slot(slot)) = Self::get_node(&inner.nodes, handle.id) {
            slot.value.is_some() && !slot.dirty
        } else {
            false
        }
    }

    /// Return the current benchmark instrumentation counters.
    #[cfg(feature = "instrumentation")]
    pub fn instrumentation_snapshot(&self) -> crate::instrumentation::InstrumentationSnapshot {
        self.inner.borrow().instrumentation.snapshot()
    }

    /// Reset benchmark instrumentation counters to zero.
    #[cfg(feature = "instrumentation")]
    pub fn reset_instrumentation(&self) {
        self.inner.borrow_mut().instrumentation.reset();
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_nodes_are_vec_indexed_by_sequential_slot_ids_and_reuse_effect_ids() {
        let ctx = Context::new();
        let cell = ctx.cell(1i32);
        let slot = ctx.slot(|_| 2i32);
        let effect = ctx.effect(move |ctx| {
            let _ = ctx.get(&slot);
        });

        assert_eq!(cell.id, SlotId(0));
        assert_eq!(slot.id, SlotId(1));
        assert_eq!(effect.id, SlotId(2));
        {
            let inner = ctx.inner.borrow();
            assert_eq!(inner.nodes.len(), 3);
            assert_eq!(inner.next_id, 3);
            assert!(inner.free_ids.is_empty());
            assert!(matches!(inner.nodes[0].as_ref(), Some(Node::Cell(_))));
            assert!(matches!(inner.nodes[1].as_ref(), Some(Node::Slot(_))));
            assert!(matches!(inner.nodes[2].as_ref(), Some(Node::Effect(_))));
        }

        effect.dispose(&ctx);
        {
            let inner = ctx.inner.borrow();
            assert_eq!(inner.nodes.len(), 3);
            assert_eq!(inner.next_id, 3);
            assert_eq!(inner.free_ids.as_slice(), &[2]);
            assert!(inner.nodes[2].is_none());
        }

        let reused = ctx.computed(|_| 3i32);
        assert_eq!(reused.id, SlotId(2));
        {
            let inner = ctx.inner.borrow();
            assert_eq!(inner.nodes.len(), 3);
            assert_eq!(inner.next_id, 3);
            assert!(inner.free_ids.is_empty());
            assert!(matches!(inner.nodes[2].as_ref(), Some(Node::Slot(_))));
        }
    }
}
