use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::cell::CellHandle;
use crate::slot::SlotHandle;

/// Type alias for the erased compute function stored in slots.
type ComputeFn = dyn Fn(&Context) -> Box<dyn Any>;

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

pub(crate) enum Node {
    Slot(SlotNode),
    Cell(CellNode),
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// Container for all reactive nodes. Owns allocations; uses interior
/// mutability (`RefCell`) for single-threaded use.
pub struct Context {
    pub(crate) nodes: RefCell<HashMap<SlotId, Node>>,
    pub(crate) next_id: RefCell<u64>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            nodes: RefCell::new(HashMap::new()),
            next_id: RefCell::new(0),
        }
    }

    pub(crate) fn alloc_id(&self) -> SlotId {
        let mut id = self.next_id.borrow_mut();
        let slot_id = SlotId(*id);
        *id += 1;
        slot_id
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
        if let Some(parent_id) = current_tracking_frame()
            && parent_id != id
        {
            let mut nodes = self.nodes.borrow_mut();
            // The node being accessed gets `parent_id` as a dependent.
            if let Some(node) = nodes.get_mut(&id) {
                match node {
                    Node::Slot(s) => {
                        s.dependents.insert(parent_id);
                    }
                    Node::Cell(c) => {
                        c.dependents.insert(parent_id);
                    }
                }
            }
            // The parent records this node as a dependency.
            if let Some(Node::Slot(parent)) = nodes.get_mut(&parent_id) {
                parent.dependencies.insert(id);
            }
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
        {
            let mut nodes = self.nodes.borrow_mut();
            for dep_id in old_deps {
                if let Some(dep_node) = nodes.get_mut(&dep_id) {
                    match dep_node {
                        Node::Slot(s) => {
                            s.dependents.remove(&id);
                        }
                        Node::Cell(c) => {
                            c.dependents.remove(&id);
                        }
                    }
                }
            }
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
            let mut nodes = self.nodes.borrow_mut();
            if let Some(Node::Cell(c)) = nodes.get_mut(&handle.id) {
                c.dependents.insert(parent_id);
            }
            if let Some(Node::Slot(parent)) = nodes.get_mut(&parent_id) {
                parent.dependencies.insert(handle.id);
            }
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
            // Collect dependents and clear them.
            let dependents: Vec<SlotId> = {
                let nodes = self.nodes.borrow();
                if let Some(Node::Cell(c)) = nodes.get(&handle.id) {
                    c.dependents.iter().copied().collect()
                } else {
                    vec![]
                }
            };
            for dep_id in dependents {
                self.clear_slot(dep_id);
            }
        }
    }

    // -- Clearing ----------------------------------------------------------

    /// Clear a slot's cached value and recursively clear all dependents.
    fn clear_slot(&self, id: SlotId) {
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
            self.clear_slot(dep_id);
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
