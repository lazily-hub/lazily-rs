use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

#[cfg(not(feature = "vec_edges"))]
use smallvec::SmallVec;

use crate::cell::CellHandle;
use crate::effect::{EffectCallbackResult, EffectHandle};
use crate::signal::SignalHandle;
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
    /// True while this slot is actively refreshing/recomputing. Used to detect
    /// dependency cycles (a slot that reads itself directly or transitively)
    /// before the pull-based recompute walk overflows the stack.
    pub(crate) in_progress: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FactoryKind {
    Slot,
    Cell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FactoryKey {
    kind: FactoryKind,
    factory_type: TypeId,
}

struct FactoryEntry {
    value_type: TypeId,
    handle: Rc<dyn Any>,
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
    batched_cells: EdgeVec,
    batched_cell_clears: EdgeVec,
    batched_slots: EdgeVec,
    factory_handles: HashMap<FactoryKey, FactoryEntry>,
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

/// RAII guard that clears a slot's `in_progress` cycle-detection flag when the
/// refresh of that slot completes (or unwinds).
struct RefreshGuard<'a> {
    ctx: &'a Context,
    id: SlotId,
}

impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.ctx.inner.borrow_mut();
        if let Some(Node::Slot(slot)) = Context::get_node_mut(&mut inner.nodes, self.id) {
            slot.in_progress = false;
        }
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
                batched_cells: EdgeVec::new(),
                batched_cell_clears: EdgeVec::new(),
                batched_slots: EdgeVec::new(),
                factory_handles: HashMap::new(),
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

    /// Return the context-local slot handle for factory `K`, creating it on
    /// first use.
    ///
    /// This supports decorator-style factory functions: callers do not store
    /// handles in wrapper structs; the context memoizes one handle per factory
    /// key. Later calls with the same key return the same slot handle and ignore
    /// the supplied compute callback.
    pub fn memoized_slot<K, T, F>(&self, compute: F) -> SlotHandle<T>
    where
        K: 'static,
        T: 'static,
        F: Fn(&Context) -> T + 'static,
    {
        let key = FactoryKey {
            kind: FactoryKind::Slot,
            factory_type: TypeId::of::<K>(),
        };
        if let Some(handle) = self.factory_handle::<SlotHandle<T>>(key, TypeId::of::<T>()) {
            return handle;
        }

        let handle = self.slot(compute);
        self.insert_factory_handle(key, TypeId::of::<T>(), handle);
        handle
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
            in_progress: false,
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
    /// Mark a slot as actively refreshing, returning a RAII guard that clears
    /// the flag on drop. Returns `None` for non-slot ids (nothing to refresh).
    ///
    /// Panics if the slot is already in-progress: that means the recompute walk
    /// has re-entered a slot still on the call stack, i.e. a dependency cycle.
    /// Surfacing it as a deterministic panic turns an otherwise-divergent
    /// infinite recompute / stack overflow into a recoverable error a caller
    /// can `catch_unwind` and render as a `#CIRCULAR!`-style value.
    fn enter_refresh(&self, id: SlotId) -> Option<RefreshGuard<'_>> {
        let mut inner = self.inner.borrow_mut();
        match Self::get_node_mut(&mut inner.nodes, id) {
            Some(Node::Slot(slot)) => {
                if slot.in_progress {
                    drop(inner);
                    panic!(
                        "lazily: circular dependency detected at slot {id:?}; a \
                         computed/memo slot depends on itself (directly or \
                         transitively) and would recompute infinitely. Break the \
                         cycle (e.g. via a base case or an untracked read)."
                    );
                }
                slot.in_progress = true;
                Some(RefreshGuard { ctx: self, id })
            }
            _ => None,
        }
    }

    fn refresh_slot(&self, id: SlotId) -> bool {
        // Fast path: clean cache hit. When the slot holds a value and is
        // neither dirty nor force-recompute, no upstream value can have
        // changed since the last compute — invalidation always sets
        // `dirty=true` on dependents via `mark_slot_dirty` (called from
        // `invalidate_dependent_from_changed_value` with `force_recompute=true`
        // for both cell- and slot-driven changes). The dependency-refresh walk,
        // the cycle guard, and the dirty-flag clear are therefore all
        // unnecessary on this path. This is the hot path for cached slot
        // reads: it collapses the borrowMut (enter_refresh) + guard-drop
        // borrowMut + dependencies Vec clone + per-dep `is_slot_node` borrows
        // + needs_recompute borrow + clear_slot_dirty_flags borrowMut down to a
        // single shared borrow.
        {
            let inner = self.inner.borrow();
            match Self::get_node(&inner.nodes, id) {
                Some(Node::Slot(slot)) => {
                    if slot.value.is_some() && !slot.dirty && !slot.force_recompute {
                        return false;
                    }
                }
                _ => return false,
            }
        }

        // Cycle guard: mark this slot in-progress for the duration of the
        // refresh. If the pull-based walk re-enters the same slot (a slot that
        // depends on itself directly or transitively), `enter_refresh` panics
        // with a diagnostic instead of recursing into a stack overflow.
        let Some(_cycle_guard) = self.enter_refresh(id) else {
            return false;
        };

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

    /// Return the context-local cell handle for factory `K`, creating it on
    /// first use.
    ///
    /// The initializer belongs to the factory. It runs only when this context
    /// has not seen `K` before; callers should mutate the returned cell handle
    /// with [`CellHandle::set`] / [`Context::set_cell`].
    pub fn memoized_cell<K, T, F>(&self, init: F) -> CellHandle<T>
    where
        K: 'static,
        T: PartialEq + 'static,
        F: FnOnce(&Context) -> T,
    {
        let key = FactoryKey {
            kind: FactoryKind::Cell,
            factory_type: TypeId::of::<K>(),
        };
        if let Some(handle) = self.factory_handle::<CellHandle<T>>(key, TypeId::of::<T>()) {
            return handle;
        }

        let value = init(self);
        let handle = self.cell(value);
        self.insert_factory_handle(key, TypeId::of::<T>(), handle);
        handle
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
                self.inner.borrow_mut().batched_cells.push(handle.id);
            } else {
                // Store-without-cascade: dirty-mark the dependent cone, then flush
                // effects ONLY when the cone actually contains an Effect. A cell
                // with no active (Effect-bearing) dependent stores its latest
                // value (already done above, so a late subscriber reads it
                // glitch-free) and marks lazy Slot dependents dirty, but pays no
                // effect-scheduling flush — the write side of the merge cost law
                // (relaycell-backpressure-analysis.md §4.0 / §5).
                if self.invalidate_cell_dependents_now(handle.id) {
                    self.flush_effects();
                }
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
        // Batch ALL invalidation/clear roots from all changed cells/slots into
        // ONE DFS pass under a SINGLE `borrow_mut` — avoids N separate
        // `borrow_mut` + DFS-queue allocations for N batched cells (#lzbatchborrow).
        let all_effects = {
            let mut inner = self.inner.borrow_mut();
            inner.batched_cells.sort_unstable();
            inner.batched_cells.dedup();
            inner.batched_cell_clears.sort_unstable();
            inner.batched_cell_clears.dedup();
            inner.batched_slots.sort_unstable();
            inner.batched_slots.dedup();

            let cells = std::mem::take(&mut inner.batched_cells);
            let cell_clears = std::mem::take(&mut inner.batched_cell_clears);
            let slots = std::mem::take(&mut inner.batched_slots);

            // Collect invalidation roots from all changed cells.
            let mut roots: Vec<SlotId> = Vec::new();
            for cell_id in &cells {
                if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, *cell_id) {
                    roots.extend_from_slice(&c.dependents);
                }
            }
            let mark_effects = Self::mark_frontier_locked(&mut inner.nodes, &roots);

            // Collect clear roots from all cleared cells.
            let mut clear_roots: Vec<SlotId> = Vec::new();
            for cell_id in &cell_clears {
                if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, *cell_id) {
                    clear_roots.extend_from_slice(&c.dependents);
                }
            }
            let clear_effects = Self::clear_frontier_locked(&mut inner.nodes, &clear_roots);

            // Clear slots directly.
            let slot_effects = Self::clear_frontier_locked(&mut inner.nodes, &slots);

            mark_effects
                .into_iter()
                .chain(clear_effects)
                .chain(slot_effects)
                .collect::<Vec<_>>()
        };
        for (effect_id, force) in all_effects {
            self.schedule_effect(effect_id, force);
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
            // Deschedule and drain any pending flush entry BEFORE recycling the
            // id, mirroring ThreadSafeContext::dispose_effect. A stale
            // pending_effects entry can alias a recycled id (free_ids is LIFO)
            // and trigger a spurious run of a freshly allocated node.
            inner.pending_effects.retain(|queued| *queued != handle.id);
            inner.scheduled_effects.remove(&handle.id);
            let Some(Node::Effect(effect)) = Self::take_node(&mut inner.nodes, handle.id) else {
                return;
            };
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

    // -- Signal API --------------------------------------------------------

    /// Create an **eager** derived value that recomputes immediately whenever
    /// one of its dependencies is invalidated.
    ///
    /// Where [`Context::computed`] is lazy (recomputed on the next read), a
    /// signal is materialized eagerly: by the time the invalidating
    /// `set_cell`/`set`/`batch` call returns, the signal already holds its new
    /// value. The value is always set, so observers never see an intermediate
    /// unset state — a dependency change drives the value directly from `v1`
    /// to `v2`.
    ///
    /// The signal is backed by a memoized slot, so a recomputation that yields
    /// an equal value (via `PartialEq`) does not invalidate downstream
    /// dependents. Recomputation is pull-based and therefore glitch-free: a
    /// signal that reads other signals/slots always observes values consistent
    /// with the current inputs.
    pub fn signal<T, F>(&self, compute: F) -> SignalHandle<T>
    where
        T: PartialEq + 'static,
        F: Fn(&Context) -> T + 'static,
    {
        let slot = self.memo(compute);
        // Eager puller: re-materializes the slot after every invalidation.
        // `get_rc` refreshes and registers the dependency without deep-cloning
        // the value on each refresh.
        let effect = self.effect(move |ctx| {
            let _ = ctx.get_rc(&slot);
        });
        SignalHandle::new(slot, effect)
    }

    /// Read a signal's current value. Always returns a materialized value.
    pub fn get_signal<T: Clone + 'static>(&self, handle: &SignalHandle<T>) -> T {
        self.get(&handle.slot)
    }

    /// Read a signal's current value as `Rc<T>`, avoiding a deep clone.
    pub fn get_signal_rc<T: 'static>(&self, handle: &SignalHandle<T>) -> Rc<T> {
        self.get_rc(&handle.slot)
    }

    /// Dispose a signal's eager puller.
    ///
    /// Stops eager recomputation; the backing value remains readable and
    /// reverts to lazy (recomputed on next read) behavior.
    pub fn dispose_signal<T>(&self, handle: &SignalHandle<T>) {
        self.dispose_effect(&handle.effect);
    }

    /// Check whether a signal's eager puller is still active.
    pub fn is_signal_active<T>(&self, handle: &SignalHandle<T>) -> bool {
        self.is_effect_active(&handle.effect)
    }

    // -- Clearing ----------------------------------------------------------

    /// Hard-clear a slot's cached value and recursively clear all dependents.
    pub(crate) fn clear_slot(&self, id: SlotId) {
        if self.is_batching() {
            self.inner.borrow_mut().batched_slots.push(id);
            return;
        }
        self.clear_slot_now(id);
    }

    /// Batch-aware multi-root slot invalidation. Clears each id's cached value
    /// and recursively clears dependents in ONE frontier walk, then flushes any
    /// scheduled effects exactly once. Used by demand-driven derived readers
    /// (e.g. [`QueueCell`](crate::QueueCell) reader-kinds) that own an
    /// out-of-graph mutation source and must invalidate several derived slots
    /// atomically on a single op — a push/pop whose `len`/`is_full` transition
    /// together must never glitch. Unsubscribed + uncached roots hit the
    /// `clear_frontier` no-op fast path, so an op nobody observes costs ~O(roots)
    /// with no derivation, no effect scheduling, and no flush.
    pub(crate) fn clear_slots(&self, ids: &[SlotId]) {
        if ids.is_empty() {
            return;
        }
        if self.is_batching() {
            self.inner.borrow_mut().batched_slots.extend_from_slice(ids);
            return;
        }
        let effects_to_schedule = {
            let mut inner = self.inner.borrow_mut();
            Self::clear_frontier_locked(&mut inner.nodes, ids)
        };
        // Store-without-cascade (read-side dual): if clearing the roots reached
        // no Effect, there is nothing to flush — an unobserved op skips the
        // flush machinery entirely and returns after a single frontier walk.
        if effects_to_schedule.is_empty() {
            return;
        }
        for (effect_id, force) in effects_to_schedule {
            self.schedule_effect(effect_id, force);
        }
        self.flush_effects();
    }

    pub(crate) fn flush_effects_after_invalidation(&self) {
        if !self.is_batching() {
            self.flush_effects();
        }
    }

    fn clear_slot_now(&self, id: SlotId) {
        let effects_to_schedule = {
            let mut inner = self.inner.borrow_mut();
            let roots = [id];
            Self::clear_frontier_locked(&mut inner.nodes, &roots)
        };
        for (effect_id, force) in effects_to_schedule {
            self.schedule_effect(effect_id, force);
        }
    }

    pub(crate) fn clear_cell_dependents(&self, id: SlotId) {
        if self.is_batching() {
            self.inner.borrow_mut().batched_cell_clears.push(id);
            return;
        }
        self.clear_cell_dependents_now(id);
        self.flush_effects();
    }

    /// Returns `true` iff at least one Effect was scheduled (i.e. the dependent
    /// cone contains an active reactor that must flush). A `false` result is the
    /// store-without-cascade fast path: the value is already stored and lazy Slot
    /// dependents are dirty-marked, but no effect flush is owed.
    fn invalidate_cell_dependents_now(&self, id: SlotId) -> bool {
        self.invalidate_dependents_now(id)
    }

    fn clear_cell_dependents_now(&self, id: SlotId) {
        let effects_to_schedule = {
            let mut inner = self.inner.borrow_mut();
            let roots = match Self::get_node(&inner.nodes, id) {
                Some(Node::Cell(c)) => c.dependents.clone(),
                _ => return,
            };
            Self::clear_frontier_locked(&mut inner.nodes, &roots)
        };
        for (effect_id, force) in effects_to_schedule {
            self.schedule_effect(effect_id, force);
        }
    }

    /// Batched BFS invalidation: marks all reachable slots dirty under a SINGLE
    /// `borrow_mut`, then schedules collected effects after the borrow is released.
    /// Replaces the former recursive `mark_slot_dirty` / `invalidate_dependent_from_changed_value`
    /// which re-borrowed per node — for fan-out 256 this cuts ~768 RefCell operations
    /// to 1 (#lzbatchborrow).
    fn invalidate_dependents_now(&self, id: SlotId) -> bool {
        let effects_to_schedule = {
            let mut inner = self.inner.borrow_mut();
            let roots = match Self::get_node(&inner.nodes, id) {
                Some(Node::Cell(c)) if c.dependents.is_empty() => return false,
                Some(Node::Slot(s)) if s.dependents.is_empty() => return false,
                Some(Node::Cell(c)) => c.dependents.clone(),
                Some(Node::Slot(s)) => s.dependents.clone(),
                _ => return false,
            };
            Self::mark_frontier_locked(&mut inner.nodes, &roots)
        };
        let scheduled = !effects_to_schedule.is_empty();
        for (effect_id, force) in effects_to_schedule {
            self.schedule_effect(effect_id, force);
        }
        scheduled
    }

    /// Single-borrow DFS dirty-marking. Roots get `force=true`; transitive
    /// descendants get `force=false` (matching the former recursive semantics).
    /// Returns `(effect_id, force)` pairs for the caller to schedule after the
    /// borrow is released. Uses stack-based DFS with SmallVec to avoid heap
    /// allocation for the common small-fan-out case (#lzbatchborrow).
    fn mark_frontier_locked(nodes: &mut [Option<Node>], roots: &[SlotId]) -> Vec<(SlotId, bool)> {
        let mut effects: Vec<(SlotId, bool)> = Vec::new();
        // DFS stack — order doesn't matter for invalidation marking.
        let mut stack: Vec<SlotId> = Vec::with_capacity(roots.len());
        let mut force_stack: Vec<bool> = Vec::with_capacity(roots.len());
        for &root in roots {
            stack.push(root);
            force_stack.push(true);
        }
        while let (Some(id), Some(force)) = (stack.pop(), force_stack.pop()) {
            match Self::get_node_mut(nodes, id) {
                Some(Node::Slot(slot)) => {
                    let should_propagate = !slot.dirty || (force && !slot.force_recompute);
                    slot.dirty = true;
                    if force {
                        slot.force_recompute = true;
                    }
                    if should_propagate {
                        for dep_id in &slot.dependents {
                            stack.push(*dep_id);
                            force_stack.push(false);
                        }
                    }
                }
                Some(Node::Effect(_)) => {
                    effects.push((id, force));
                }
                _ => {}
            }
        }
        effects
    }

    /// Single-borrow DFS value-clearing. Clears slot values and dirty flags
    /// recursively, collecting effects to schedule.
    fn clear_frontier_locked(nodes: &mut [Option<Node>], roots: &[SlotId]) -> Vec<(SlotId, bool)> {
        let mut effects: Vec<(SlotId, bool)> = Vec::new();
        let mut stack: Vec<SlotId> = Vec::with_capacity(roots.len());
        for &root in roots {
            stack.push(root);
        }
        while let Some(id) = stack.pop() {
            match Self::get_node_mut(nodes, id) {
                Some(Node::Slot(slot)) => {
                    if slot.value.is_none() && !slot.dirty {
                        continue;
                    }
                    slot.value = None;
                    slot.dirty = false;
                    slot.force_recompute = false;
                    for dep_id in &slot.dependents {
                        stack.push(*dep_id);
                    }
                }
                Some(Node::Effect(_)) => {
                    effects.push((id, true));
                }
                _ => {}
            }
        }
        effects
    }

    fn notify_slot_value_changed(&self, id: SlotId) {
        self.invalidate_dependents_now(id);
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

    fn factory_handle<H>(&self, key: FactoryKey, value_type: TypeId) -> Option<H>
    where
        H: Copy + 'static,
    {
        let inner = self.inner.borrow();
        let entry = inner.factory_handles.get(&key)?;
        assert!(
            entry.value_type == value_type,
            "lazily: factory key {:?} was reused with an incompatible value type",
            key.factory_type
        );
        Some(
            *entry
                .handle
                .downcast_ref::<H>()
                .expect("lazily: factory handle type mismatch"),
        )
    }

    fn insert_factory_handle<H>(&self, key: FactoryKey, value_type: TypeId, handle: H)
    where
        H: Copy + 'static,
    {
        self.inner.borrow_mut().factory_handles.insert(
            key,
            FactoryEntry {
                value_type,
                handle: Rc::new(handle),
            },
        );
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

    #[test]
    fn dispose_effect_drains_pending_effects_queue() {
        // A disposed effect must not leave a stale entry in `pending_effects`.
        // Such an entry can alias a recycled id (free_ids is LIFO) and trigger
        // a spurious run of a freshly allocated node during a later flush.
        // Mirrors ThreadSafeContext::dispose_effect (thread_safe.rs:2577-2578).
        let ctx = Context::new();
        let cell = ctx.cell(0i32);
        let effect = ctx.effect(move |ctx| {
            let _ = ctx.get_cell(&cell);
        });

        // Simulate the effect being scheduled (pending) but not yet flushed,
        // which happens when another effect's body invalidates it mid-flush.
        ctx.schedule_effect(effect.id, true);
        {
            let inner = ctx.inner.borrow();
            assert!(inner.pending_effects.contains(&effect.id));
            assert!(inner.scheduled_effects.contains(&effect.id));
        }

        effect.dispose(&ctx);

        let inner = ctx.inner.borrow();
        assert!(
            !inner.pending_effects.contains(&effect.id),
            "dispose must drain the pending_effects queue"
        );
        assert!(!inner.scheduled_effects.contains(&effect.id));
        assert_eq!(inner.free_ids.as_slice(), &[effect.id.0]);
    }

    // -- Cycle detection (#lzcycledetect) ----------------------------------

    fn cycle_panic_message(result: Box<dyn std::any::Any + Send>) -> String {
        result
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| result.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    }

    #[test]
    fn two_slot_dependency_cycle_panics_instead_of_overflowing() {
        let ctx = Context::new();
        // `a` reads `b` (wired after both exist); `b` reads `a`.
        let link: std::rc::Rc<std::cell::RefCell<Option<SlotHandle<i32>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let link_a = std::rc::Rc::clone(&link);
        let a = ctx.computed(move |ctx| match *link_a.borrow() {
            Some(b) => ctx.get(&b) + 1,
            None => 0,
        });
        let b = ctx.computed(move |ctx| ctx.get(&a) + 1);
        *link.borrow_mut() = Some(b);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.get(&a)));
        assert!(
            result.is_err(),
            "circular dependency must panic, not diverge"
        );
        let msg = cycle_panic_message(result.unwrap_err());
        assert!(
            msg.contains("circular dependency"),
            "unexpected panic message: {msg}"
        );

        // The in-progress flags must be cleared by the RAII guards so the
        // context is not wedged after the panic is caught.
        {
            let inner = ctx.inner.borrow();
            for node in inner.nodes.iter().flatten() {
                if let Node::Slot(slot) = node {
                    assert!(!slot.in_progress, "in_progress must be cleared on unwind");
                }
            }
        }
        let fresh = ctx.computed(|_| 42i32);
        assert_eq!(ctx.get(&fresh), 42, "context must still work after a cycle");
    }

    #[test]
    fn self_referential_slot_panics() {
        let ctx = Context::new();
        let link: std::rc::Rc<std::cell::RefCell<Option<SlotHandle<i32>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let link_self = std::rc::Rc::clone(&link);
        let s = ctx.computed(move |ctx| match *link_self.borrow() {
            Some(me) => ctx.get(&me) + 1,
            None => 0,
        });
        *link.borrow_mut() = Some(s);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.get(&s)));
        assert!(result.is_err(), "self-referential slot must panic");
        let msg = cycle_panic_message(result.unwrap_err());
        assert!(msg.contains("circular dependency"), "got: {msg}");
    }

    #[test]
    fn diamond_dependencies_do_not_false_positive() {
        // A diamond (`d` <- b,c <- a) reads `a` twice but is acyclic; the
        // cycle guard must not misfire on shared, non-nested dependencies.
        let ctx = Context::new();
        let a = ctx.cell(1i32);
        let b = ctx.computed(move |ctx| ctx.get_cell(&a) + 1);
        let c = ctx.computed(move |ctx| ctx.get_cell(&a) + 2);
        let d = ctx.computed(move |ctx| ctx.get(&b) + ctx.get(&c));
        assert_eq!(ctx.get(&d), (1 + 1) + (1 + 2));
        a.set(&ctx, 10);
        assert_eq!(ctx.get(&d), (10 + 1) + (10 + 2));
    }

    #[test]
    fn batch_dedup_invalidates_shared_dependent_once(/* #lzbatchalloc */) {
        // Multiple writes to the same cell inside one batch, plus writes to
        // several cells sharing one dependent, must coalesce: the shared
        // dependent is invalidated exactly once on flush. Guards the sort+dedup
        // replacement for the former HashSet-backed batch sets.
        let ctx = Context::new();
        let a = ctx.cell(0i32);
        let b = ctx.cell(0i32);
        let total = ctx.computed(move |ctx| ctx.get_cell(&a) + ctx.get_cell(&b));
        assert_eq!(ctx.get(&total), 0);

        ctx.batch(|ctx| {
            a.set(ctx, 1);
            a.set(ctx, 2); // duplicate cell `a` in the batch set
            a.set(ctx, 3); // triplicate
            b.set(ctx, 10);
        });
        // After the batch flush, the dependent sees the latest coalesced values.
        assert_eq!(ctx.get(&total), 13);
    }
}
