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

use std::collections::BTreeSet;
#[cfg(feature = "distributed")]
use std::marker::PhantomData;
use std::ops::Add;

#[cfg(feature = "distributed")]
use crate::crdt::CellCrdt;

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

    /// `true` iff coalescing (merging accumulated ops into one) actually *bounds*
    /// the state — the precondition for the `Conflate` overflow action. All
    /// conflating policies (band / monoid / semilattice) bound; only `RawFifo`
    /// (concat: order + multiplicity are meaning) grows without bound, so it
    /// cannot conflate and a relay MUST reject `Conflate` for it (analysis §4.3).
    const CONFLATES: bool = true;
}

/// Keep-latest (right-zero) band: `old ⊕ op = op`. Associative and idempotent,
/// **not** commutative. This is the merge behind a plain [`Cell`](Source) —
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
    // Concat grows without bound: order + multiplicity are meaning, so a RawFifo
    // relay cannot conflate — only Block / Drop / Spill are sound.
    const CONFLATES: bool = false;
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

// ---------------------------------------------------------------------------
// Cell kernel migration (`#lzcellkernel`)
// ---------------------------------------------------------------------------
//
// The former `MergeCellHandle<T, M>` struct and the vestigial `Reactive<T>` /
// `Source<T>` read/write traits are **deleted**. A "merge cell" is now just a
// `Source<T, M>` with `M != KeepLatest`, and a plain source cell is `Source<T>`
// = `Source<T, KeepLatest>` — the identity `Source ≡ Source<T, KeepLatest>` is a
// default type parameter, not a spec assertion. Write protection lives on the
// inherent `impl<T, M: MergePolicy<T>> Source<T, M>` (see `cell.rs`), so
// `computed.set(…)` fails to compile with no trait in sight.
