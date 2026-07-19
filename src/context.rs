use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

#[cfg(not(feature = "vec_edges"))]
use smallvec::SmallVec;

use crate::cell::CellHandle;
use crate::effect::{EffectCallbackResult, EffectHandle};
use crate::merge::{MergeCellHandle, MergePolicy};
use crate::signal::SignalHandle;
use crate::slot::SlotHandle;

/// Type alias for the erased compute function stored in slots.
type ComputeFn = dyn Fn(&Context) -> AnyValue;
/// Type alias for the erased equality function stored in slots.
type EqualsFn = dyn Fn(&AnyValue, &AnyValue) -> bool;
/// Type alias for the erased effect callback stored in effects.
type EffectFn = dyn Fn(&Context) -> Option<Box<dyn FnOnce()>>;

/// Unique identifier for a reactive node (slot or cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub(crate) u64);

#[cfg(not(feature = "vec_edges"))]
type EdgeVec = SmallVec<[SlotId; 2]>;
#[cfg(feature = "vec_edges")]
type EdgeVec = Vec<SlotId>;

// The per-node `TypeId` exists only to power belt-and-suspenders type-mismatch
// asserts. In release builds it is a zero-sized `()` so it costs nothing to
// store (16 B/node saved at 10M-node scale); in debug builds it is a real
// `TypeId` and the asserts run.
#[cfg(debug_assertions)]
type TypeTag = TypeId;
#[cfg(not(debug_assertions))]
type TypeTag = ();

#[inline]
fn node_type_tag<T: 'static>() -> TypeTag {
    #[cfg(debug_assertions)]
    {
        TypeId::of::<T>()
    }
    #[cfg(not(debug_assertions))]
    {
        ()
    }
}

/// Degree at which an edge list stops scanning and gains a hash index.
///
/// #lzspecedgeindex. Dedup is a linear scan while a node's degree is small —
/// measurably faster than hashing at low degree, which is the overwhelmingly
/// common case — and gains a hash index above this threshold so a wide-fanout
/// node stays amortized O(1) per registration instead of degrading to O(n^2)
/// per propagation. An unconditional hash set regresses the common case; an
/// unconditional scan regresses wide fanout.
///
/// Measured crossover with `SlotIdHasher`: the indexed path costs ~45 ns per
/// registration flat, and a linear scan passes that near width 40. With `std`'s
/// SipHash the indexed path cost ~83 ns and the crossover sat near 170 — the
/// hasher, not the scan, was what made a low threshold look wrong here.
///
/// The index is held in a side table on `Inner`, not on the node, so a node
/// below the threshold carries no extra bytes at all. See `EdgeIndex`.
const EDGE_INDEX_THRESHOLD: usize = 32;

/// Hysteresis: demote only well below the promote threshold.
///
/// A dependent list oscillates by one on every recompute — edges are removed
/// and re-registered — so a single shared boundary makes a list sitting exactly
/// at the threshold demote and rebuild its index on every recompute. Measured
/// at ~4x the steady-state cost. The gap absorbs that oscillation.
const EDGE_INDEX_DEMOTE_THRESHOLD: usize = 24;

/// `owner -> (edge -> position in owner's edge list)`, for promoted nodes only.
///
/// Absent for every node below the threshold, which is why an unpromoted node
/// pays nothing: no field, no branch on the node itself, no allocation.
///
/// Entries MUST be dropped whenever the edge list they describe is cleared or
/// its owner is removed — `SlotId`s are recycled (`free_ids` is LIFO), so a
/// stale entry would silently alias a different node's edges.
/// Hasher for `SlotId` keys (#lzspecedgeindex).
///
/// `std`'s default is SipHash, chosen to resist collision attacks on
/// attacker-controlled keys. `SlotId`s are internally allocated sequential
/// integers that never come from outside the process, so that resistance buys
/// nothing here and is paid on every index lookup — twice per wide
/// registration, once for the owner and once for the edge.
///
/// This is the splitmix64 finalizer: full avalanche in a handful of
/// multiply-xor-shift ops. Sequential ids land in well-separated buckets.
#[derive(Default, Clone, Copy)]
pub(crate) struct SlotIdHasher(u64);

impl std::hash::Hasher for SlotIdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, byte_a: &[u8]) {
        // SlotId hashes through write_u64; this exists only to satisfy the
        // trait, and is deliberately not tuned.
        for byte in byte_a {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_u64(&mut self, value: u64) {
        let mut mixed = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        self.0 = mixed ^ (mixed >> 31);
    }
}

#[derive(Default, Clone)]
pub(crate) struct SlotIdHashBuilder;

impl std::hash::BuildHasher for SlotIdHashBuilder {
    type Hasher = SlotIdHasher;

    fn build_hasher(&self) -> Self::Hasher {
        SlotIdHasher(0)
    }
}

type OwnerEdgeIndex = HashMap<SlotId, usize, SlotIdHashBuilder>;
type EdgeIndex = HashMap<SlotId, OwnerEdgeIndex, SlotIdHashBuilder>;

/// Insert `id` into `edges` if absent. Returns whether an edge was added.
fn edge_insert(edges: &mut EdgeVec, id: SlotId, owner: SlotId, index: &mut EdgeIndex) -> bool {
    // Invariant: an owner has an index entry exactly while its edge list is
    // longer than the threshold. Gating on the length keeps a low-degree node
    // off the hash path entirely — hashing an absent key costs more than the
    // short scan it would replace.
    // `SmallVec::len` branches on inline-vs-spilled, so read it once: the
    // common path is then one compare on top of the scan it already did.
    let len = edges.len();
    // Below the demote threshold there is provably no index, so a short list
    // never touches the map. Between the thresholds one may or may not exist.
    if len > EDGE_INDEX_DEMOTE_THRESHOLD
        && let Some(owner_index) = index.get_mut(&owner)
    {
        if owner_index.contains_key(&id) {
            return false;
        }
        owner_index.insert(id, len);
        edges.push(id);
        return true;
    }
    if edges.contains(&id) {
        return false;
    }
    edges.push(id);
    if len + 1 > EDGE_INDEX_THRESHOLD && !index.contains_key(&owner) {
        // crossed the threshold on this push: build the index once
        index.insert(
            owner,
            edges
                .iter()
                .enumerate()
                .map(|(pos, edge)| (*edge, pos))
                .collect(),
        );
    }
    true
}

/// Remove `id` from `edges` if present. Returns whether an edge was removed.
///
/// Swap-removes, so edge order is not preserved — matching the previous
/// behaviour, which callers already tolerate.
fn edge_remove(edges: &mut EdgeVec, id: SlotId, owner: SlotId, index: &mut EdgeIndex) -> bool {
    if edges.len() > EDGE_INDEX_DEMOTE_THRESHOLD
        && let Some(owner_index) = index.get_mut(&owner)
    {
        let Some(pos) = owner_index.remove(&id) else {
            return false;
        };
        let last = edges
            .pop()
            .expect("index non-empty implies edges non-empty");
        if pos < edges.len() {
            edges[pos] = last;
            owner_index.insert(last, pos);
        }
        // Demote only well below the promote threshold, so a list hovering at
        // the boundary does not rebuild its index on every recompute.
        if edges.len() <= EDGE_INDEX_DEMOTE_THRESHOLD {
            index.remove(&owner);
        }
        return true;
    }
    if let Some(pos) = edges.iter().position(|edge| *edge == id) {
        edges.swap_remove(pos);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// SmallAny inline value storage (#lzsmallany)
// ---------------------------------------------------------------------------

// Inline storage envelope (mirrors the thread-safe INLINE_CAP/INLINE_ALIGN in
// thread_safe.rs): small, trivially-droppable values (i64/f64 and similar
// scalars) are stored inline in the node instead of behind a heap `Rc` box.
// lazily-cpp's SmallAny gives it a ~3x cold-recalc lead purely from skipping
// one heap allocation per recompute; this closes that gap.
const VALUE_INLINE_CAP: usize = 24;
const VALUE_INLINE_ALIGN: usize = 16;

// SAFETY: the `align(16)` guarantees any T with `align_of::<T>() <= 16` can be
// written into the buffer at its required alignment.
#[repr(C, align(16))]
pub(crate) struct InlineBuf([core::mem::MaybeUninit<u8>; VALUE_INLINE_CAP]);

/// Type-erased reactive value. `None` is the unset slot state; `Inline` holds a
/// small, trivially-droppable T bitwise (no `Rc` heap allocation); `Heap` falls
/// back to the classic `Rc<dyn Any>` for large or `Drop`-bearing types.
///
/// `Inline` is only ever used for T with `size_of::<T>() <= VALUE_INLINE_CAP`,
/// `align_of::<T>() <= VALUE_INLINE_ALIGN`, and `needs_drop::<T>() == false`,
/// so overwriting or dropping an `Inline` variant never needs to run a
/// destructor — the derived `Drop` only has to release the `Heap` `Rc`.
pub(crate) enum AnyValue {
    None,
    Inline(InlineBuf),
    Heap(Rc<dyn Any>),
}

impl AnyValue {
    /// Erase `value` into either inline storage or a heap `Rc`, picking inline
    /// only when it is safe to store bitwise (small, well-aligned, no drop).
    #[inline]
    pub(crate) fn from_value<T: 'static>(value: T) -> Self {
        if core::mem::size_of::<T>() <= VALUE_INLINE_CAP
            && core::mem::align_of::<T>() <= VALUE_INLINE_ALIGN
            && !core::mem::needs_drop::<T>()
        {
            let mut buf = InlineBuf([core::mem::MaybeUninit::uninit(); VALUE_INLINE_CAP]);
            // SAFETY: `buf` is 16-aligned (InlineBuf) and 24 bytes wide; T fits
            // both and is being written at its required alignment.
            unsafe {
                core::ptr::write(buf.0.as_mut_ptr() as *mut T, value);
            }
            AnyValue::Inline(buf)
        } else {
            AnyValue::Heap(Rc::new(value))
        }
    }

    /// Borrow the stored value as `&T`, trusting the caller's type (the node's
    /// `TypeTag` assert has already proven T matches). Works for both inline
    /// and heap storage.
    #[inline]
    pub(crate) unsafe fn as_t_ref_unchecked<T: 'static>(&self) -> &T {
        match self {
            AnyValue::Inline(buf) => unsafe { &*(buf.0.as_ptr() as *const T) },
            AnyValue::Heap(rc) => unsafe { &*(&**rc as *const dyn Any as *const T) },
            AnyValue::None => unsafe { core::hint::unreachable_unchecked() },
        }
    }

    /// Clone the stored value into a fresh `Rc<T>`. For heap storage this is a
    /// refcount bump (no deep clone); for inline storage there is no shared box
    /// to refcount, so a new `Rc` is materialized (inline-eligible T is trivially
    /// droppable, so owning it in an `Rc` is sound).
    #[inline]
    pub(crate) unsafe fn rc_clone_unchecked<T: 'static>(&self) -> Rc<T> {
        match self {
            AnyValue::Heap(rc) => {
                let rc: Rc<dyn Any> = Rc::clone(rc);
                unsafe {
                    let ptr = Rc::into_raw(rc) as *const T;
                    Rc::from_raw(ptr)
                }
            }
            AnyValue::Inline(buf) => {
                Rc::new(unsafe { core::ptr::read(buf.0.as_ptr() as *const T) })
            }
            AnyValue::None => unsafe { core::hint::unreachable_unchecked() },
        }
    }

    #[inline]
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, AnyValue::None)
    }
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
    pub(crate) value: AnyValue,
    pub(crate) type_id: TypeTag,
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
    /// #lzspecrevisionengine: last global revision at which this slot was
    /// verified clean. In revision mode, staleness is `verified_at < revision`
    /// (O(1) write — no dirty walk) rather than the `dirty` flag.
    pub(crate) verified_at: u64,
}

pub(crate) struct CellNode {
    pub(crate) value: AnyValue,
    pub(crate) type_id: TypeTag,
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
    /// #lzspecedgeindex. Hash indexes for promoted (wide) edge lists only; see
    /// `EdgeIndex`. Kept off the nodes so low-degree nodes cost nothing.
    dependents_index: EdgeIndex,
    dependencies_index: EdgeIndex,
    /// Reused buffer for invalidation roots. Swapped with a node's dependent
    /// list so a publish does not copy it — see `invalidate_dependents_now`.
    roots_scratch: EdgeVec,
    pending_effects: VecDeque<SlotId>,
    /// Effect-schedule membership bitset, indexed by node slot. Mirrors the
    /// thread-safe variant (thread_safe.rs): a `Vec<bool>` beats `HashSet` for
    /// the bounded, dense node-id space and avoids per-schedule hashing.
    scheduled_effects: Vec<bool>,
    flushing_effects: bool,
    batch_depth: usize,
    batched_cells: EdgeVec,
    batched_cell_clears: EdgeVec,
    batched_slots: EdgeVec,
    /// Reusable DFS stack for `mark_frontier_locked` / `clear_frontier_locked`.
    /// Holds `(id, force)` so the former separate `stack` + `force_stack`
    /// allocations collapse into one with better pop locality (#lzbatchborrow).
    mark_scratch: Vec<(SlotId, bool)>,
    /// Reusable sink for `(effect_id, force)` pairs collected during a frontier
    /// walk. Taken out for one invalidation and restored afterward so its
    /// capacity survives across invalidations instead of reallocating per call.
    effects_scratch: Vec<(SlotId, bool)>,
    factory_handles: HashMap<FactoryKey, FactoryEntry>,
    /// #lzspecrevisionengine: global revision counter, bumped once per
    /// value-changing write. In revision mode, slot staleness is detected by
    /// `verified_at < revision` instead of the `dirty` flag, giving O(1)
    /// writes (no dependent cone walk). Push mode (default) leaves this at 0.
    revision: u64,
    /// #lzspecrevisionengine: whether this Context uses the revision (pull)
    /// invalidation engine instead of the default push (dirty-walk) engine.
    /// Per-Context choice; never mixed within one graph.
    revision_mode: bool,
    /// #lzspecrevisionengine: all effect node ids, for the revision-mode
    /// effect-flush scan (O(effects) per flush, not O(cone) per write).
    all_effect_ids: Vec<SlotId>,
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
        Self::new_impl(false)
    }

    /// Create a Context using the **revision (pull) invalidation engine** instead
    /// of the default push (dirty-walk) engine (`#lzspecrevisionengine`).
    ///
    /// In revision mode, a cell write bumps a global revision counter (O(1),
    /// no dependent cone walk). Slot staleness is detected lazily on read via
    /// `verified_at < revision`. Observable values are provably identical to
    /// push mode (`get_equiv_push`, lazily-formal). Pick revision for
    /// write-heavy / high-fan-out workloads; keep push (default) for read-heavy.
    pub fn with_revision_engine() -> Self {
        Self::new_impl(true)
    }

    fn new_impl(revision_mode: bool) -> Self {
        Self {
            inner: RefCell::new(ContextInner {
                nodes: Vec::new(),
                next_id: 0,
                free_ids: Vec::new(),
                dependents_index: EdgeIndex::default(),
                dependencies_index: EdgeIndex::default(),
                roots_scratch: EdgeVec::new(),
                pending_effects: VecDeque::new(),
                scheduled_effects: Vec::new(),
                flushing_effects: false,
                batch_depth: 0,
                batched_cells: EdgeVec::new(),
                batched_cell_clears: EdgeVec::new(),
                batched_slots: EdgeVec::new(),
                mark_scratch: Vec::new(),
                effects_scratch: Vec::new(),
                factory_handles: HashMap::new(),
                revision: 0,
                revision_mode,
                all_effect_ids: Vec::new(),
                #[cfg(feature = "instrumentation")]
                instrumentation: crate::instrumentation::InstrumentationCounters::default(),
            }),
        }
    }

    pub(crate) fn alloc_id(&self) -> SlotId {
        let mut inner = self.inner.borrow_mut();
        let slot_id = match inner.free_ids.pop() {
            Some(id) => {
                let id = SlotId(id);
                // Belt and braces against id recycling: a fresh node must never
                // inherit an index entry. Disposal already clears these; this
                // makes any future removal path safe by construction.
                if !inner.dependents_index.is_empty() {
                    inner.dependents_index.remove(&id);
                }
                if !inner.dependencies_index.is_empty() {
                    inner.dependencies_index.remove(&id);
                }
                id
            }
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
        // Disjoint field borrows: the node comes from `nodes`, the index from
        // its own field, so both can be held at once.
        let inner_mut = &mut *inner;
        if let Some(node) = Self::get_node_mut(&mut inner_mut.nodes, dependency_id) {
            let index = &mut inner_mut.dependents_index;
            match node {
                Node::Slot(s) => {
                    edge_insert(&mut s.dependents, dependent_id, dependency_id, index);
                }
                Node::Cell(c) => {
                    edge_insert(&mut c.dependents, dependent_id, dependency_id, index);
                }
                Node::Effect(_) => {}
            }
        }

        if let Some(node) = Self::get_node_mut(&mut inner_mut.nodes, dependent_id) {
            let index = &mut inner_mut.dependencies_index;
            match node {
                Node::Slot(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = edge_insert(
                            &mut parent.dependencies,
                            dependency_id,
                            dependent_id,
                            index,
                        );
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        edge_insert(&mut parent.dependencies, dependency_id, dependent_id, index);
                    }
                }
                Node::Effect(parent) => {
                    #[cfg(feature = "instrumentation")]
                    {
                        edge_added = edge_insert(
                            &mut parent.dependencies,
                            dependency_id,
                            dependent_id,
                            index,
                        );
                    }
                    #[cfg(not(feature = "instrumentation"))]
                    {
                        edge_insert(&mut parent.dependencies, dependency_id, dependent_id, index);
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

    /// Remove every `dependent_id` edge from `old_deps`'s dependents under a
    /// SINGLE `ContextInner` borrow. Mirrors the thread-safe variant's
    /// `remove_stale_dependencies_locked` (thread_safe.rs): the former per-edge
    /// `remove_dependent_edge` calls each re-borrowed the `RefCell`, so a
    /// fan-out-256 recompute paid 256 borrow_mut acquisitions instead of one
    /// (#lzbatchborrow).
    /// Detach `dependency_id` from each of `dependent_a`'s dependency lists.
    ///
    /// The mirror of `remove_dependent_edges_locked`, needed when a node is
    /// torn down while others still point at it.
    fn remove_dependency_edges_locked(
        inner: &mut ContextInner,
        dependency_id: SlotId,
        dependent_a: &[SlotId],
    ) {
        for dependent_id in dependent_a {
            let inner_mut = &mut *inner;
            if let Some(node) = Self::get_node_mut(&mut inner_mut.nodes, *dependent_id) {
                let index = &mut inner_mut.dependencies_index;
                match node {
                    Node::Slot(slot) => {
                        edge_remove(&mut slot.dependencies, dependency_id, *dependent_id, index);
                    }
                    Node::Effect(effect) => {
                        edge_remove(
                            &mut effect.dependencies,
                            dependency_id,
                            *dependent_id,
                            index,
                        );
                    }
                    Node::Cell(_) => {}
                }
            }
        }
    }

    /// Drop both index side-table entries for `id`.
    ///
    /// Mandatory before recycling an id: `free_ids` is LIFO, so a stale entry
    /// would alias the very next node allocated.
    fn drop_edge_index_entries(inner: &mut ContextInner, id: SlotId) {
        if !inner.dependencies_index.is_empty() {
            inner.dependencies_index.remove(&id);
        }
        if !inner.dependents_index.is_empty() {
            inner.dependents_index.remove(&id);
        }
    }

    fn remove_dependent_edges_locked(
        inner: &mut ContextInner,
        dependent_id: SlotId,
        old_deps: &[SlotId],
    ) {
        for dependency_id in old_deps {
            #[cfg(feature = "instrumentation")]
            let mut edge_removed = false;
            let inner_mut = &mut *inner;
            if let Some(dep_node) = Self::get_node_mut(&mut inner_mut.nodes, *dependency_id) {
                let index = &mut inner_mut.dependents_index;
                match dep_node {
                    Node::Slot(s) => {
                        #[cfg(feature = "instrumentation")]
                        {
                            edge_removed =
                                edge_remove(&mut s.dependents, dependent_id, *dependency_id, index);
                        }
                        #[cfg(not(feature = "instrumentation"))]
                        {
                            edge_remove(&mut s.dependents, dependent_id, *dependency_id, index);
                        }
                    }
                    Node::Cell(c) => {
                        #[cfg(feature = "instrumentation")]
                        {
                            edge_removed =
                                edge_remove(&mut c.dependents, dependent_id, *dependency_id, index);
                        }
                        #[cfg(not(feature = "instrumentation"))]
                        {
                            edge_remove(&mut c.dependents, dependent_id, *dependency_id, index);
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
            Some(Box::new(|old: &AnyValue, new: &AnyValue| {
                let old = unsafe { old.as_t_ref_unchecked::<T>() };
                let new = unsafe { new.as_t_ref_unchecked::<T>() };
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
            value: AnyValue::None,
            type_id: node_type_tag::<T>(),
            compute: Rc::new(move |ctx| AnyValue::from_value(compute(ctx))),
            equals,
            dependencies: EdgeVec::new(),
            dependents: EdgeVec::new(),
            dirty: false,
            force_recompute: false,
            in_progress: false,
            verified_at: 0,
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
            && !slot.value.is_none()
        {
            assert!(
                slot.type_id == node_type_tag::<T>(),
                "type mismatch in slot"
            );
            return unsafe { slot.value.rc_clone_unchecked::<T>() };
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
            && !slot.value.is_none()
        {
            assert!(
                slot.type_id == node_type_tag::<T>(),
                "type mismatch in slot"
            );
            return unsafe { slot.value.as_t_ref_unchecked::<T>() }.clone();
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
                    if inner.revision_mode {
                        // #lzspecrevisionengine: staleness = verified_at < revision.
                        // Clean (verified_at == revision) and has a value → cache hit.
                        if !slot.value.is_none() && slot.verified_at == inner.revision {
                            return false;
                        }
                    } else if !slot.value.is_none() && !slot.dirty && !slot.force_recompute {
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
            // refresh_slot returns false for non-slot ids (its fast path and
            // every later borrow fall through to `_ => ...`), so the explicit
            // is_slot_node borrow+lookup is redundant — calling refresh_slot
            // directly collapses two RefCell borrows per dependency into one.
            if self.refresh_slot(dep_id) {
                dependency_changed = true;
            }
        }

        let needs_recompute = {
            let inner = self.inner.borrow();
            let slot = match Self::get_node(&inner.nodes, id) {
                Some(Node::Slot(slot)) => slot,
                _ => return false,
            };
            if inner.revision_mode {
                // #lzspecrevisionengine: reaching here means verified_at <
                // revision (the fast path didn't short-circuit). The slot is
                // stale and must recompute. The memo guard in recompute_slot_now
                // handles the value early-cutoff (if the recomputed value
                // equals the cache, downstream caches are preserved).
                true
            } else {
                slot.value.is_none() || slot.force_recompute || dependency_changed
            }
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
            // The list this described is gone; its index must go with it.
            // `is_empty` first: with no promoted node anywhere this is a
            // pointer compare, where `remove` would hash on every recompute.
            if !inner.dependencies_index.is_empty() {
                inner.dependencies_index.remove(&id);
            }
            Self::remove_dependent_edges_locked(&mut inner, id, &old_deps);
        }

        push_tracking_frame(id);
        let result = (compute.as_ref())(self);
        pop_tracking_frame();

        let changed = {
            let mut inner = self.inner.borrow_mut();
            let (rev_mode, rev) = (inner.revision_mode, inner.revision);
            let slot = match Self::get_node_mut(&mut inner.nodes, id) {
                Some(Node::Slot(slot)) => slot,
                _ => return false,
            };
            let had_value = !slot.value.is_none();
            let unchanged = match (&slot.value, &slot.equals) {
                (AnyValue::None, _) | (_, None) => false,
                (old, Some(equals)) => equals(old, &result),
            };
            slot.dirty = false;
            slot.force_recompute = false;
            if rev_mode {
                slot.verified_at = rev;
            }
            if unchanged {
                false
            } else {
                slot.value = result;
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
            assert!(c.type_id == node_type_tag::<T>(), "type mismatch in cell");
            unsafe { c.value.as_t_ref_unchecked::<T>() }.clone()
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
            assert!(c.type_id == node_type_tag::<T>(), "type mismatch in cell");
            unsafe { c.value.rc_clone_unchecked::<T>() }
        } else {
            panic!("get_cell_rc called on non-cell id");
        }
    }

    // -- Cell API ----------------------------------------------------------

    /// Create a new mutable cell with an initial value.
    pub fn cell<T: PartialEq + 'static>(&self, value: T) -> CellHandle<T> {
        let id = self.alloc_id();
        let node = CellNode {
            value: AnyValue::from_value(value),
            type_id: node_type_tag::<T>(),
            dependents: EdgeVec::new(),
        };
        self.insert_node(id, Node::Cell(node));
        CellHandle::new(id)
    }

    /// Create a [`MergeCellHandle`] — a cell whose write is a *merge* under
    /// policy `M`, rather than a replace. `Cell ≡ MergeCell<KeepLatest>`
    /// (relaycell-backpressure-analysis.md §4.0). Backed by an ordinary cell
    /// node, so it inherits the store-without-cascade write fast path.
    pub fn merge_cell<T, M>(&self, initial: T) -> MergeCellHandle<T, M>
    where
        T: PartialEq + 'static,
        M: MergePolicy<T>,
    {
        MergeCellHandle::new(self.cell(initial))
    }

    /// Fold `op` into a cell's value under policy `M` (the merge write). Reads
    /// the current value untracked, computes `M::merge(old, op)`, then routes
    /// through [`set_cell`](Context::set_cell) so the `PartialEq` store-guard
    /// (free dedup when `⊕(old, op) == old`), batching, and
    /// store-without-cascade all apply unchanged.
    pub fn apply_merge<T, M>(&self, handle: &CellHandle<T>, op: T)
    where
        T: PartialEq + Clone + 'static,
        M: MergePolicy<T>,
    {
        let merged = {
            let inner = self.inner.borrow();
            if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, handle.id) {
                assert!(
                    c.type_id == node_type_tag::<T>(),
                    "type mismatch in apply_merge"
                );
                let old = unsafe { c.value.as_t_ref_unchecked::<T>() };
                M::merge(old, op)
            } else {
                panic!("apply_merge on non-cell id");
            }
        };
        self.set_cell(handle, merged);
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
                assert!(
                    c.type_id == node_type_tag::<T>(),
                    "type mismatch in cell set"
                );
                let old = unsafe { c.value.as_t_ref_unchecked::<T>() };
                *old != new_value
            } else {
                panic!("set_cell on non-cell id");
            }
        };

        if changed {
            {
                let mut inner = self.inner.borrow_mut();
                if let Some(Node::Cell(c)) = Self::get_node_mut(&mut inner.nodes, handle.id) {
                    c.value = AnyValue::from_value(new_value);
                }
            }
            if self.is_batching() {
                self.inner.borrow_mut().batched_cells.push(handle.id);
            } else if self.inner.borrow().revision_mode {
                // #lzspecrevisionengine: O(1) write — bump the global revision
                // counter; no dependent cone walk. Slot staleness is detected
                // lazily on read via `verified_at < revision`. Effects are
                // notified via the revision-mode flush scan.
                self.inner.borrow_mut().revision += 1;
                self.flush_effects_revision();
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
        // #lzspecrevisionengine: in revision mode, bump the global revision
        // once for the entire batch (O(1)), then scan effects for staleness.
        // The push-mode DFS (below) is skipped entirely.
        if self.inner.borrow().revision_mode {
            self.inner.borrow_mut().revision += 1;
            self.flush_effects_revision();
            self.inner.borrow_mut().batched_cells.clear();
            self.inner.borrow_mut().batched_cell_clears.clear();
            self.inner.borrow_mut().batched_slots.clear();
            return;
        }
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

            // Reusable effects sink: cleared once here, appended across all
            // frontier walks, then taken out (so it can outlive the borrow while
            // effects are scheduled) and restored afterward so its capacity
            // survives to the next invalidation instead of reallocating.
            inner.effects_scratch.clear();

            // Collect invalidation roots from all changed cells.
            let mut roots: Vec<SlotId> = Vec::new();
            for cell_id in &cells {
                if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, *cell_id) {
                    roots.extend_from_slice(&c.dependents);
                }
            }
            Self::mark_frontier_locked(&mut inner, &roots);

            // Collect clear roots from all cleared cells.
            let mut clear_roots: Vec<SlotId> = Vec::new();
            for cell_id in &cell_clears {
                if let Some(Node::Cell(c)) = Self::get_node(&inner.nodes, *cell_id) {
                    clear_roots.extend_from_slice(&c.dependents);
                }
            }
            Self::clear_frontier_locked(&mut inner, &clear_roots);

            // Clear slots directly.
            Self::clear_frontier_locked(&mut inner, &slots);

            std::mem::take(&mut inner.effects_scratch)
        };
        for (effect_id, force) in &all_effects {
            self.schedule_effect(*effect_id, *force);
        }
        self.inner.borrow_mut().effects_scratch = all_effects;
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
        self.inner.borrow_mut().all_effect_ids.push(id);
        let handle = EffectHandle::new(id);
        self.schedule_effect(id, false);
        self.flush_effects();
        handle
    }

    /// Dispose an effect by handle.
    pub fn dispose_effect(&self, handle: &EffectHandle) {
        let torn_down = {
            let mut inner = self.inner.borrow_mut();
            // Deschedule and drain any pending flush entry BEFORE recycling the
            // id, mirroring ThreadSafeContext::dispose_effect. A stale
            // pending_effects entry can alias a recycled id (free_ids is LIFO)
            // and trigger a spurious run of a freshly allocated node.
            inner.pending_effects.retain(|queued| *queued != handle.id);
            Self::deschedule_effect(&mut inner, handle.id);
            let Some(Node::Effect(effect)) = Self::take_node(&mut inner.nodes, handle.id) else {
                return;
            };
            Self::remove_dependent_edges_locked(&mut inner, handle.id, &effect.dependencies);
            // The id is about to be recycled, so drop both index entries — a
            // stale one would alias the next node allocated with this id.
            Self::drop_edge_index_entries(&mut inner, handle.id);
            inner.free_ids.push(handle.id.0);
            effect
        };
        // Outside the borrow: the effect owns its run closure and everything it
        // captured, whose Drop may re-enter the context.
        let mut torn_down = torn_down;
        let cleanup = torn_down.cleanup.take();
        drop(torn_down);

        if let Some(cleanup) = cleanup {
            cleanup();
        }
    }

    /// Tear down a derived slot: detach both edge directions, clear the node,
    /// and recycle its id.
    ///
    /// Without this a slot is permanent. `SlotHandle` is `Copy` — an id, not an
    /// owner — so dropping every handle reclaims nothing, and the node and its
    /// edge on each dependency survive for the life of the context. Under
    /// subscribe/unsubscribe churn that is unbounded growth in both memory and
    /// propagation cost: the dependent list keeps lengthening even though the
    /// live subscriber count does not.
    ///
    /// Callers must ensure nothing still reads the slot in a live compute.
    /// Reading a disposed node throws on the next recompute — the same contract
    /// as [`Context::dispose_effect`] and the JS binding's `disposeSlot`.
    pub fn dispose_slot<T>(&self, handle: &SlotHandle<T>) {
        let torn_down = {
            let mut inner = self.inner.borrow_mut();
            // Check the kind BEFORE taking: a stale handle whose id has been
            // recycled must not tear down whatever now owns it.
            if !matches!(Self::get_node(&inner.nodes, handle.id), Some(Node::Slot(_))) {
                return;
            }
            let Some(Node::Slot(slot)) = Self::take_node(&mut inner.nodes, handle.id) else {
                return;
            };
            Self::remove_dependent_edges_locked(&mut inner, handle.id, &slot.dependencies);
            Self::remove_dependency_edges_locked(&mut inner, handle.id, &slot.dependents);
            Self::drop_edge_index_entries(&mut inner, handle.id);
            inner.free_ids.push(handle.id.0);
            slot
        };
        // Drop the node outside the borrow. It owns the compute closure and
        // everything that closure captured, so dropping it under the borrow
        // panics if any capture's Drop re-enters the context — which a
        // self-disposing handle type does by construction.
        drop(torn_down);
    }

    /// Tear down a source cell: detach its dependents, clear the node, and
    /// recycle its id.
    ///
    /// Cells are pure sources with no dependencies, so only downstream edges
    /// need detaching. Same contract as [`Context::dispose_slot`].
    pub fn dispose_cell<T>(&self, handle: &CellHandle<T>) {
        let mut inner = self.inner.borrow_mut();
        if !matches!(Self::get_node(&inner.nodes, handle.id), Some(Node::Cell(_))) {
            return;
        }
        let Some(Node::Cell(cell)) = Self::take_node(&mut inner.nodes, handle.id) else {
            return;
        };
        Self::remove_dependency_edges_locked(&mut inner, handle.id, &cell.dependents);
        Self::drop_edge_index_entries(&mut inner, handle.id);
        inner.free_ids.push(handle.id.0);
        drop(inner);
        // Same reason as dispose_slot: the cell owns its value, whose Drop may
        // re-enter.
        drop(cell);
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

        let idx = Self::node_index(id).expect("SlotId does not fit usize");
        let already_scheduled = idx < inner.scheduled_effects.len() && inner.scheduled_effects[idx];
        if !already_scheduled {
            if idx >= inner.scheduled_effects.len() {
                inner.scheduled_effects.resize(idx + 1, false);
            }
            inner.scheduled_effects[idx] = true;
            inner.pending_effects.push_back(id);
            #[cfg(feature = "instrumentation")]
            {
                let depth = inner.pending_effects.len();
                inner.instrumentation.record_effect_queue_push(depth);
            }
        }
    }

    fn deschedule_effect(inner: &mut ContextInner, id: SlotId) {
        let idx = Self::node_index(id).expect("SlotId does not fit usize");
        if idx < inner.scheduled_effects.len() {
            inner.scheduled_effects[idx] = false;
        }
    }

    #[cfg(test)]
    fn is_effect_scheduled(&self, id: SlotId) -> bool {
        let inner = self.inner.borrow();
        let idx = Self::node_index(id).expect("SlotId does not fit usize");
        idx < inner.scheduled_effects.len() && inner.scheduled_effects[idx]
    }

    fn remove_pending_effect(&self, id: SlotId) {
        let mut inner = self.inner.borrow_mut();
        inner.pending_effects.retain(|queued| *queued != id);
        Self::deschedule_effect(&mut inner, id);
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
                        Self::deschedule_effect(&mut inner, id);
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

    /// #lzspecrevisionengine: revision-mode effect flush. Scans all registered
    /// effects and schedules those whose dependencies are stale
    /// (`verified_at < revision`). O(effects) per flush, not O(cone) per write —
    /// the effect-side cost is decoupled from the write-path cone walk that
    /// revision mode eliminates.
    fn flush_effects_revision(&self) {
        let stale_effects: Vec<SlotId> = {
            let inner = self.inner.borrow();
            inner
                .all_effect_ids
                .iter()
                .filter(|&&eid| match Self::get_node(&inner.nodes, eid) {
                    Some(Node::Effect(e)) => {
                        e.dependencies.iter().any(|dep| {
                            matches!(Self::get_node(&inner.nodes, *dep),
                                Some(Node::Slot(s)) if s.verified_at < inner.revision)
                        }) || e.force_run
                    }
                    _ => false,
                })
                .copied()
                .collect()
        };
        for eid in stale_effects {
            self.schedule_effect(eid, true);
        }
        self.flush_effects();
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
            if !inner.dependencies_index.is_empty() {
                inner.dependencies_index.remove(&id);
            }
            Self::remove_dependent_edges_locked(&mut inner, id, &old_deps);
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
            inner.effects_scratch.clear();
            Self::clear_frontier_locked(&mut inner, ids);
            std::mem::take(&mut inner.effects_scratch)
        };
        // Store-without-cascade (read-side dual): if clearing the roots reached
        // no Effect, there is nothing to flush — an unobserved op skips the
        // flush machinery entirely and returns after a single frontier walk.
        if !effects_to_schedule.is_empty() {
            for (effect_id, force) in effects_to_schedule.iter().copied() {
                self.schedule_effect(effect_id, force);
            }
            self.flush_effects();
        }
        // Restore the reusable sink so its capacity survives to the next call.
        self.inner.borrow_mut().effects_scratch = effects_to_schedule;
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
            inner.effects_scratch.clear();
            Self::clear_frontier_locked(&mut inner, &roots);
            std::mem::take(&mut inner.effects_scratch)
        };
        for (effect_id, force) in effects_to_schedule.iter().copied() {
            self.schedule_effect(effect_id, force);
        }
        self.inner.borrow_mut().effects_scratch = effects_to_schedule;
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
            inner.effects_scratch.clear();
            Self::clear_frontier_locked(&mut inner, &roots);
            std::mem::take(&mut inner.effects_scratch)
        };
        for (effect_id, force) in effects_to_schedule.iter().copied() {
            self.schedule_effect(effect_id, force);
        }
        self.inner.borrow_mut().effects_scratch = effects_to_schedule;
    }

    /// Batched BFS invalidation: marks all reachable slots dirty under a SINGLE
    /// `borrow_mut`, then schedules collected effects after the borrow is released.
    /// Replaces the former recursive `mark_slot_dirty` / `invalidate_dependent_from_changed_value`
    /// which re-borrowed per node — for fan-out 256 this cuts ~768 RefCell operations
    /// to 1 (#lzbatchborrow).
    fn invalidate_dependents_now(&self, id: SlotId) -> bool {
        let effects_to_schedule = {
            let mut inner = self.inner.borrow_mut();
            let inner_mut = &mut *inner;
            // Swap the dependent list into a reused buffer instead of cloning
            // it. Cloning cost 8 bytes per dependent plus an allocation on
            // every set_cell — 10.7ms per publish at width 1M — even when
            // nothing downstream was read. The graph is acyclic, so marking
            // cannot reach `id` again and observe the borrowed-out list.
            match Self::get_node_mut(&mut inner_mut.nodes, id) {
                Some(Node::Cell(c)) if c.dependents.is_empty() => return false,
                Some(Node::Slot(s)) if s.dependents.is_empty() => return false,
                Some(Node::Cell(c)) => {
                    std::mem::swap(&mut c.dependents, &mut inner_mut.roots_scratch)
                }
                Some(Node::Slot(s)) => {
                    std::mem::swap(&mut s.dependents, &mut inner_mut.roots_scratch)
                }
                _ => return false,
            }
            let roots = std::mem::take(&mut inner.roots_scratch);
            inner.effects_scratch.clear();
            Self::mark_frontier_locked(&mut inner, &roots);
            inner.roots_scratch = roots;
            // put the list back on its node
            let inner_mut = &mut *inner;
            match Self::get_node_mut(&mut inner_mut.nodes, id) {
                Some(Node::Cell(c)) => {
                    std::mem::swap(&mut c.dependents, &mut inner_mut.roots_scratch)
                }
                Some(Node::Slot(s)) => {
                    std::mem::swap(&mut s.dependents, &mut inner_mut.roots_scratch)
                }
                _ => {}
            }
            std::mem::take(&mut inner.effects_scratch)
        };
        let scheduled = !effects_to_schedule.is_empty();
        for (effect_id, force) in effects_to_schedule.iter().copied() {
            self.schedule_effect(effect_id, force);
        }
        self.inner.borrow_mut().effects_scratch = effects_to_schedule;
        scheduled
    }

    /// Single-borrow DFS dirty-marking. Roots get `force=true`; transitive
    /// descendants get `force=false` (matching the former recursive semantics).
    /// Appends `(effect_id, force)` pairs to `effects_scratch` for the caller to
    /// schedule after the borrow is released. Reuses the `ContextInner`'s
    /// `mark_scratch`/`effects_scratch` buffers so an invalidation no longer
    /// allocates a DFS stack + force stack per call: the former separate
    /// `stack` and `force_stack` collapse into one `Vec<(SlotId, bool)>` with
    /// better pop locality (#lzbatchborrow).
    fn mark_frontier_locked(inner: &mut ContextInner, roots: &[SlotId]) {
        let nodes = &mut inner.nodes;
        let stack = &mut inner.mark_scratch;
        let effects = &mut inner.effects_scratch;
        stack.clear();
        for &root in roots {
            stack.push((root, true));
        }
        while let Some((id, force)) = stack.pop() {
            match Self::get_node_mut(nodes, id) {
                Some(Node::Slot(slot)) => {
                    let should_propagate = !slot.dirty || (force && !slot.force_recompute);
                    slot.dirty = true;
                    if force {
                        slot.force_recompute = true;
                    }
                    if should_propagate {
                        for dep_id in &slot.dependents {
                            stack.push((*dep_id, false));
                        }
                    }
                }
                Some(Node::Effect(_)) => {
                    effects.push((id, force));
                }
                _ => {}
            }
        }
    }

    /// Single-borrow DFS value-clearing. Clears slot values and dirty flags
    /// recursively, appending effects to schedule to `effects_scratch`.
    fn clear_frontier_locked(inner: &mut ContextInner, roots: &[SlotId]) {
        let nodes = &mut inner.nodes;
        let stack = &mut inner.mark_scratch;
        let effects = &mut inner.effects_scratch;
        stack.clear();
        for &root in roots {
            stack.push((root, true));
        }
        while let Some((id, _)) = stack.pop() {
            match Self::get_node_mut(nodes, id) {
                Some(Node::Slot(slot)) => {
                    if slot.value.is_none() && !slot.dirty {
                        continue;
                    }
                    slot.value = AnyValue::None;
                    slot.dirty = false;
                    slot.force_recompute = false;
                    for dep_id in &slot.dependents {
                        stack.push((*dep_id, true));
                    }
                }
                Some(Node::Effect(_)) => {
                    effects.push((id, true));
                }
                _ => {}
            }
        }
    }

    fn notify_slot_value_changed(&self, id: SlotId) {
        // #lzspecrevisionengine: in revision mode, downstream slots detect
        // staleness via `verified_at < revision` (the global revision was bumped
        // on the write). No dirty walk needed — the cone walk is the push cost
        // revision mode eliminates.
        if self.inner.borrow().revision_mode {
            return;
        }
        self.invalidate_dependents_now(id);
    }

    /// Check whether a slot currently has a cached, fresh value (for testing).
    pub fn is_set<T: 'static>(&self, handle: &SlotHandle<T>) -> bool {
        let inner = self.inner.borrow();
        if let Some(Node::Slot(slot)) = Self::get_node(&inner.nodes, handle.id) {
            !slot.value.is_none() && !slot.dirty
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

    // -- #lzspecedgeindex --------------------------------------------------
    //
    // The index side table is only correct while its invariant holds: an owner
    // has an entry exactly while its edge list is longer than the threshold.
    // Both fast paths assert on that, so a violation panics rather than
    // silently reading a stale position.

    #[test]
    fn disposal_tolerates_a_capture_whose_drop_reenters() {
        // A node owns its compute closure and everything that closure captured.
        // If the node is dropped while the RefCell borrow is held, any capture
        // whose Drop touches the context panics with "already borrowed" — which
        // a self-disposing handle type does by construction.
        struct ReentersOnDrop {
            ctx: std::rc::Weak<Context>,
            victim: SlotId,
        }
        impl Drop for ReentersOnDrop {
            fn drop(&mut self) {
                if let Some(ctx) = self.ctx.upgrade() {
                    // any context call that borrows would do
                    ctx.dispose_slot(&SlotHandle::<u64> {
                        id: self.victim,
                        _marker: std::marker::PhantomData,
                    });
                }
            }
        }

        let ctx = std::rc::Rc::new(Context::new());
        let base = ctx.cell(1u64);
        let victim = ctx.computed(move |c| c.get_cell(&base) + 1);
        assert_eq!(ctx.get(&victim), 2);

        let reentrant = ReentersOnDrop {
            ctx: std::rc::Rc::downgrade(&ctx),
            victim: victim.id,
        };
        let holder = ctx.computed(move |c| {
            let _ = &reentrant;
            c.get_cell(&base) + 100
        });
        assert_eq!(ctx.get(&holder), 101);

        // Disposing `holder` drops its closure, dropping `reentrant`, whose Drop
        // disposes `victim`. Must not panic.
        ctx.dispose_slot(&holder);
        assert!(
            ctx.inner.borrow().free_ids.contains(&victim.id.0),
            "the re-entrant disposal must have completed"
        );
    }

    #[test]
    fn dispose_slot_detaches_both_edge_directions_and_recycles_the_id() {
        let ctx = Context::new();
        let src = ctx.cell(1usize);
        let mid = ctx.computed(move |ctx| ctx.get_cell(&src) + 1);
        let sink = ctx.computed(move |ctx| ctx.get(&mid) * 10);
        assert_eq!(ctx.get(&sink), 20);

        let recycled = mid.id;
        ctx.dispose_slot(&mid);

        {
            let inner = ctx.inner.borrow();
            // upstream: the cell no longer lists the disposed slot
            match Context::get_node(&inner.nodes, src.id) {
                Some(Node::Cell(c)) => assert!(
                    !c.dependents.contains(&recycled),
                    "dependency must drop the disposed dependent"
                ),
                _ => panic!("expected cell"),
            }
            // downstream: the sink no longer lists the disposed slot
            match Context::get_node(&inner.nodes, sink.id) {
                Some(Node::Slot(s)) => assert!(
                    !s.dependencies.contains(&recycled),
                    "dependent must drop the disposed dependency"
                ),
                _ => panic!("expected slot"),
            }
            assert!(inner.free_ids.contains(&recycled.0), "id must be recycled");
        }

        // the recycled id is handed to the next node and behaves normally
        let reused = ctx.computed(move |ctx| ctx.get_cell(&src) + 100);
        assert_eq!(reused.id, recycled);
        assert_eq!(ctx.get(&reused), 101);
        ctx.set_cell(&src, 5);
        assert_eq!(ctx.get(&reused), 105);
    }

    #[test]
    fn dispose_cell_detaches_dependents_and_recycles_the_id() {
        let ctx = Context::new();
        let src = ctx.cell(2usize);
        let derived = ctx.computed(move |ctx| ctx.get_cell(&src) * 3);
        assert_eq!(ctx.get(&derived), 6);

        let recycled = src.id;
        ctx.dispose_cell(&src);

        let inner = ctx.inner.borrow();
        match Context::get_node(&inner.nodes, derived.id) {
            Some(Node::Slot(s)) => assert!(
                !s.dependencies.contains(&recycled),
                "dependent must drop the disposed cell"
            ),
            _ => panic!("expected slot"),
        }
        assert!(inner.free_ids.contains(&recycled.0), "id must be recycled");
    }

    #[test]
    fn dispose_slot_ignores_a_handle_naming_another_kind() {
        // free_ids is LIFO, so a stale handle can name a live node of a
        // different kind. Disposing through it must be a no-op, not a teardown.
        let ctx = Context::new();
        let cell = ctx.cell(1usize);
        let stale: SlotHandle<usize> = SlotHandle {
            id: cell.id,
            _marker: std::marker::PhantomData,
        };
        ctx.dispose_slot(&stale);
        assert_eq!(ctx.get_cell(&cell), 1, "the cell must survive");
        assert!(
            !ctx.inner.borrow().free_ids.contains(&cell.id.0),
            "a live node's id must not be recycled"
        );
    }

    #[test]
    fn dispose_slot_returns_the_graph_to_its_prior_size() {
        // The churn case: repeatedly subscribe and unsubscribe against one
        // topic. Without disposal the dependent list grows without bound even
        // though the live count is constant.
        let ctx = Context::new();
        let topic = ctx.cell(0usize);
        let live_width = 8usize;
        let mut live_a: Vec<_> = (0..live_width)
            .map(|i| {
                let slot = ctx.computed(move |ctx| ctx.get_cell(&topic) + i);
                ctx.get(&slot);
                slot
            })
            .collect();

        for cycle in 0..500usize {
            let victim = live_a.swap_remove(cycle % live_a.len());
            ctx.dispose_slot(&victim);
            let slot = ctx.computed(move |ctx| ctx.get_cell(&topic) + cycle);
            ctx.get(&slot);
            live_a.push(slot);
        }

        let inner = ctx.inner.borrow();
        match Context::get_node(&inner.nodes, topic.id) {
            Some(Node::Cell(c)) => assert_eq!(
                c.dependents.len(),
                live_width,
                "dependent list must track live subscribers, not total ever created"
            ),
            _ => panic!("expected cell"),
        }
    }

    #[test]
    fn edge_index_promotes_past_the_threshold_and_still_dedups() {
        let ctx = Context::new();
        let src = ctx.cell(0usize);
        // comfortably past EDGE_INDEX_THRESHOLD so the index is built
        let width = EDGE_INDEX_THRESHOLD * 4;
        let dep_a: Vec<_> = (0..width)
            .map(|i| ctx.computed(move |ctx| ctx.get_cell(&src) + i))
            .collect();
        for slot in &dep_a {
            ctx.get(slot);
        }

        {
            let inner = ctx.inner.borrow();
            let index = inner
                .dependents_index
                .get(&src.id)
                .expect("wide cell must be indexed");
            assert_eq!(index.len(), width, "one index entry per dependent");
        }

        // re-reading must not duplicate edges
        for slot in &dep_a {
            ctx.get(slot);
        }
        {
            let inner = ctx.inner.borrow();
            match Context::get_node(&inner.nodes, src.id) {
                Some(Node::Cell(c)) => assert_eq!(c.dependents.len(), width, "no duplicate edges"),
                _ => panic!("expected cell"),
            }
        }

        ctx.set_cell(&src, 7);
        for (i, slot) in dep_a.iter().enumerate() {
            assert_eq!(ctx.get(slot), 7 + i, "every dependent recomputed");
        }
    }

    #[test]
    fn edge_index_demotes_when_the_list_shrinks_below_the_threshold() {
        let ctx = Context::new();
        let src = ctx.cell(0usize);
        let toggle = ctx.cell(true);
        // each slot reads src only while `toggle` is set, so flipping it drops
        // every dependent edge and must demote the index
        let width = EDGE_INDEX_THRESHOLD * 2;
        let dep_a: Vec<_> = (0..width)
            .map(|i| {
                ctx.computed(move |ctx| {
                    if ctx.get_cell(&toggle) {
                        ctx.get_cell(&src) + i
                    } else {
                        i
                    }
                })
            })
            .collect();
        for slot in &dep_a {
            ctx.get(slot);
        }
        assert!(
            ctx.inner.borrow().dependents_index.contains_key(&src.id),
            "wide list must be indexed"
        );

        ctx.set_cell(&toggle, false);
        for slot in &dep_a {
            ctx.get(slot);
        }
        assert!(
            !ctx.inner.borrow().dependents_index.contains_key(&src.id),
            "index entry must be dropped once the list is short again"
        );

        // and the graph still behaves
        ctx.set_cell(&src, 99);
        for (i, slot) in dep_a.iter().enumerate() {
            assert_eq!(ctx.get(slot), i, "src is no longer a dependency");
        }
    }

    #[test]
    fn edge_index_does_not_survive_id_recycling() {
        // free_ids is LIFO, so a disposed effect's id is handed straight to the
        // next node. A leftover index entry would alias it.
        let ctx = Context::new();
        let src = ctx.cell(0usize);
        let width = EDGE_INDEX_THRESHOLD * 2;
        let cell_a: Vec<_> = (0..width).map(|i| ctx.cell(i)).collect();
        let effect = ctx.effect(move |ctx| {
            for cell in &cell_a {
                let _ = ctx.get_cell(cell);
            }
        });
        assert!(
            ctx.inner
                .borrow()
                .dependencies_index
                .contains_key(&effect.id),
            "wide effect must be indexed"
        );

        let recycled = effect.id;
        ctx.dispose_effect(&effect);
        assert!(
            !ctx.inner
                .borrow()
                .dependencies_index
                .contains_key(&recycled),
            "disposal must drop the index entry before the id is recycled"
        );

        let reused = ctx.computed(move |ctx| ctx.get_cell(&src) + 1);
        assert_eq!(reused.id, recycled, "id was recycled as expected");
        assert_eq!(ctx.get(&reused), 1, "recycled node computes normally");
        ctx.set_cell(&src, 41);
        assert_eq!(ctx.get(&reused), 42, "recycled node propagates normally");
    }

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
        }
        assert!(ctx.is_effect_scheduled(effect.id));

        effect.dispose(&ctx);

        let inner = ctx.inner.borrow();
        assert!(
            !inner.pending_effects.contains(&effect.id),
            "dispose must drain the pending_effects queue"
        );
        assert!(!ctx.is_effect_scheduled(effect.id));
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
