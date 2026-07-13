//! Phase 1 of the RelayCell backpressure plan — the merge algebra and the
//! `Reactive` read supertype.
//!
//! See `lazily-spec/docs/relaycell-backpressure-analysis.md`:
//!
//! - §4.0 — the reactive primitives. `Reactive<T>` is the read supertype
//!   (`get` + `subscribe`); `Source<T>: Reactive<T>` adds `set`/`merge`, so a
//!   non-settable reader-kind (a derived Slot) is correctly typed read-only.
//! - §4.3 — `MergePolicy`, the algebra trait. A merge `⊕ : T × T → T` folds
//!   accumulated ops; the *properties* it satisfies (associativity always;
//!   commutativity/idempotency per transport contract) select which overflow
//!   behaviour is sound. `MergeCell<T, M>` generalizes `Cell`
//!   (`Cell ≡ MergeCell<KeepLatest>`): a source whose write is a merge.
//!
//! Associativity is the irreducible core (§2) and is *not* a runtime flag — it
//! is a law every policy must satisfy, verified by the property tests in
//! `tests/merge_laws.rs`. The two independent branches — commutativity (the
//! reordering tax) and idempotency (the durability tax) — are surfaced as
//! `const` flags so a relay can validate its (overflow, transport) choice
//! against the algebra at construction (Phase 2+).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::ops::Add;

use crate::Context;
use crate::cell::CellHandle;
#[cfg(feature = "distributed")]
use crate::crdt::CellCrdt;
use crate::effect::EffectHandle;
use crate::signal::SignalHandle;
use crate::slot::SlotHandle;

/// The coalescence algebra: an associative fold `⊕ : T × T → T`.
///
/// `merge(old, op)` accumulates the operation `op` into the current state
/// `old`. The single non-negotiable requirement is **associativity**
/// (`merge(merge(a, b), c) == merge(a, merge(b, c))`), which licenses the
/// variable flush points a bounded relay uses: regrouping a run of merged ops
/// never changes the converged state (analysis §2). Associativity is enforced
/// by the law-tests, not by a flag.
///
/// The two independent, transport-selected properties are exposed as `const`
/// flags:
///
/// - [`COMMUTATIVE`](MergePolicy::COMMUTATIVE) — needed only when ops may be
///   applied out of order (the *reordering tax*).
/// - [`IDEMPOTENT`](MergePolicy::IDEMPOTENT) — needed only for at-least-once /
///   crash-replay durability (the *durability tax*). For an idempotent `⊕`,
///   re-applying the same op is a no-op — which is exactly the `PartialEq`
///   store-guard one layer up, giving free dedup.
pub trait MergePolicy<T> {
    /// Fold `op` into `old`. MUST be associative.
    fn merge(old: &T, op: T) -> T;

    /// `true` iff `⊕` is commutative — reordering ops converges to the same
    /// state (`merge(merge(a, b), c) == merge(merge(a, c), b)`).
    const COMMUTATIVE: bool;

    /// `true` iff `⊕` is idempotent — re-applying an op is a no-op
    /// (`merge(merge(a, b), b) == merge(a, b)`).
    const IDEMPOTENT: bool;
}

/// Keep-latest (right-zero) band: `old ⊕ op = op`. Associative and idempotent,
/// **not** commutative. This is the merge behind a plain [`Cell`](CellHandle) —
/// `Cell ≡ MergeCell<KeepLatest>` (analysis §4.0). Positional last-writer-wins;
/// distinct from timestamped [`Lww`](crate::LwwRegister) (which is commutative).
pub struct KeepLatest;

impl<T> MergePolicy<T> for KeepLatest {
    #[inline]
    fn merge(_old: &T, op: T) -> T {
        op
    }
    const COMMUTATIVE: bool = false;
    const IDEMPOTENT: bool = true;
}

/// Additive commutative monoid: `old ⊕ op = old + op`. Associative and
/// commutative, **not** idempotent (re-adding double-counts). The
/// unordered-exactly-once tier (analysis §2): a running counter / sum.
pub struct Sum;

impl<T> MergePolicy<T> for Sum
where
    T: Add<Output = T> + Clone,
{
    #[inline]
    fn merge(old: &T, op: T) -> T {
        old.clone() + op
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = false;
}

/// Max semilattice: `old ⊕ op = max(old, op)`. Associative, commutative, and
/// idempotent — the full-CRDT corner for a totally-ordered value.
pub struct Max;

impl<T> MergePolicy<T> for Max
where
    T: Ord + Clone,
{
    #[inline]
    fn merge(old: &T, op: T) -> T {
        if op > *old { op } else { old.clone() }
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

/// Set-union (grow-only) semilattice: `old ⊕ op = old ∪ op`. Associative,
/// commutative, idempotent. The unordered-at-least-once tier (analysis §2).
pub struct SetUnion;

impl<E> MergePolicy<BTreeSet<E>> for SetUnion
where
    E: Ord + Clone,
{
    #[inline]
    fn merge(old: &BTreeSet<E>, op: BTreeSet<E>) -> BTreeSet<E> {
        let mut out = old.clone();
        out.extend(op);
        out
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

/// Raw FIFO append: `old ⊕ op = old ++ op`. Associative (concatenation is a
/// free semigroup) but **neither** commutative nor idempotent — order and
/// multiplicity are meaning (analysis §2, `protocol.md` §176). A `RawFifo`
/// stream cannot conflate; its only bounded-lossless option is Spill.
pub struct RawFifo;

impl<E> MergePolicy<Vec<E>> for RawFifo
where
    E: Clone,
{
    #[inline]
    fn merge(old: &Vec<E>, op: Vec<E>) -> Vec<E> {
        let mut out = old.clone();
        out.extend(op);
        out
    }
    const COMMUTATIVE: bool = false;
    const IDEMPOTENT: bool = false;
}

/// Blanket semilattice policy over any existing [`CellCrdt`] unit — wires the
/// `#lzsync` CRDT registers (`LwwRegister`, `MvRegister`, `PnCounter`) into the
/// merge algebra without reimplementing their join. `merge` folds via
/// [`CellCrdt::merge_from`], which is contractually commutative, associative,
/// and idempotent (a join semilattice), so all three properties hold.
///
/// - `CrdtJoin<LwwRegister<T>>` — timestamped last-writer-wins (commutative,
///   unlike positional [`KeepLatest`]).
/// - `CrdtJoin<PnCounter>` — increment/decrement counter.
/// - `CrdtJoin<MvRegister<T>>` — multi-value (concurrency-retaining) register.
#[cfg(feature = "distributed")]
pub struct CrdtJoin<C>(PhantomData<C>);

#[cfg(feature = "distributed")]
impl<C> MergePolicy<C> for CrdtJoin<C>
where
    C: CellCrdt + Clone,
{
    #[inline]
    fn merge(old: &C, op: C) -> C {
        let mut out = old.clone();
        out.merge_from(&op);
        out
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

/// A [`Cell`](CellHandle) whose write is a **merge** under policy `M`, rather
/// than a replace. `Cell ≡ MergeCell<KeepLatest>` (analysis §4.0): a plain cell
/// is the keep-latest instance of this generalization.
///
/// Backed by an ordinary cell node, so it inherits the Phase-0 write fast path:
/// the `PartialEq` store-guard no-ops when `⊕(old, op) == old` (free dedup for
/// an idempotent policy), and store-without-cascade skips the effect flush when
/// no active reactor is downstream (the merge cost law, analysis §5).
pub struct MergeCellHandle<T, M> {
    pub(crate) cell: CellHandle<T>,
    pub(crate) _marker: PhantomData<M>,
}

impl<T, M> MergeCellHandle<T, M> {
    pub(crate) fn new(cell: CellHandle<T>) -> Self {
        Self {
            cell,
            _marker: PhantomData,
        }
    }

    /// The underlying cell handle, for wiring into derived readers.
    pub fn cell(&self) -> CellHandle<T> {
        self.cell
    }

    /// Read the current converged value (tracks a dependency when read inside a
    /// reactive computation).
    pub fn get(&self, ctx: &Context) -> T
    where
        T: Clone + 'static,
    {
        ctx.get_cell(&self.cell)
    }

    /// Fold `op` into the current value under policy `M`.
    pub fn merge(&self, ctx: &Context, op: T)
    where
        T: PartialEq + Clone + 'static,
        M: MergePolicy<T>,
    {
        ctx.apply_merge::<T, M>(&self.cell, op);
    }

    /// Replace the value outright (the `KeepLatest` write), bypassing `M`.
    pub fn set(&self, ctx: &Context, value: T)
    where
        T: PartialEq + 'static,
    {
        ctx.set_cell(&self.cell, value);
    }
}

impl<T, M> Clone for MergeCellHandle<T, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, M> Copy for MergeCellHandle<T, M> {}

impl<T, M> std::fmt::Debug for MergeCellHandle<T, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeCellHandle")
            .field("id", &self.cell.id)
            .finish()
    }
}

/// The **read** supertype of every reactive node (analysis §4.0). Exposes only
/// `get` and `subscribe`; writability is the [`Source`] sub-interface. A
/// composite reader-kind can be declared `Reactive<T>` and the backend chooses
/// the impl (pull-Slot / push-Cell / polling-Slot) behind one interface.
pub trait Reactive<T: Clone + 'static> {
    /// Read the current value. Registers a dependency when called inside a
    /// reactive computation (a Slot compute or an Effect run).
    fn get(&self, ctx: &Context) -> T;

    /// Run `on_change` now and again on every change to this value. Returns the
    /// backing [`EffectHandle`]; dispose it to unsubscribe.
    fn subscribe(
        &self,
        ctx: &Context,
        on_change: impl FnMut(&Context, &T) + 'static,
    ) -> EffectHandle
    where
        Self: Copy + 'static,
    {
        let this = *self;
        let cb = RefCell::new(on_change);
        ctx.effect(move |c| {
            let v = this.get(c);
            (cb.borrow_mut())(c, &v);
        })
    }
}

/// A writable [`Reactive`] — adds `set` (replace) and `merge` (fold under the
/// node's policy). `Cell`/`MergeCell` are `Source`; a derived Slot is not.
pub trait Source<T: Clone + 'static>: Reactive<T> {
    /// Replace the value (the keep-latest write).
    fn set(&self, ctx: &Context, value: T);
    /// Fold `op` into the value under this source's merge policy. For a plain
    /// cell this is `set` (`Cell ≡ MergeCell<KeepLatest>`).
    fn merge(&self, ctx: &Context, op: T);
}

impl<T: Clone + 'static> Reactive<T> for CellHandle<T> {
    fn get(&self, ctx: &Context) -> T {
        ctx.get_cell(self)
    }
}

impl<T: PartialEq + Clone + 'static> Source<T> for CellHandle<T> {
    fn set(&self, ctx: &Context, value: T) {
        ctx.set_cell(self, value);
    }
    fn merge(&self, ctx: &Context, op: T) {
        // Cell ≡ MergeCell<KeepLatest>: the fold is a replace.
        ctx.set_cell(self, op);
    }
}

impl<T: Clone + 'static> Reactive<T> for SlotHandle<T> {
    fn get(&self, ctx: &Context) -> T {
        ctx.get(self)
    }
}

impl<T: Clone + 'static> Reactive<T> for SignalHandle<T> {
    fn get(&self, ctx: &Context) -> T {
        ctx.get_signal(self)
    }
}

impl<T: Clone + 'static, M> Reactive<T> for MergeCellHandle<T, M> {
    fn get(&self, ctx: &Context) -> T {
        ctx.get_cell(&self.cell)
    }
}

impl<T: PartialEq + Clone + 'static, M: MergePolicy<T>> Source<T> for MergeCellHandle<T, M> {
    fn set(&self, ctx: &Context, value: T) {
        ctx.set_cell(&self.cell, value);
    }
    fn merge(&self, ctx: &Context, op: T) {
        ctx.apply_merge::<T, M>(&self.cell, op);
    }
}
