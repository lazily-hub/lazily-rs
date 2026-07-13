//! Phase 1 law-tests for the merge algebra (`MergePolicy`) and the
//! `Reactive`/`Source` supertypes.
//!
//! See `lazily-spec/docs/relaycell-backpressure-analysis.md` §2 (the algebra
//! theorem) and §4.0/§4.3. Every policy MUST be **associative** (the irreducible
//! core that licenses variable flush points); commutativity and idempotency are
//! per-policy branches surfaced as `const` flags and asserted only when the flag
//! is set. The idempotent-`⊕`-is-a-no-op property is the same mechanism as the
//! `Cell` `PartialEq` store-guard one layer up.

use std::collections::BTreeSet;

use lazily::{
    Context, KeepLatest, Max, MergeCellHandle, MergePolicy, RawFifo, Reactive, SetUnion, Source,
    Sum,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Pure-algebra law helpers (property-based). A `MergePolicy<T>` merge is a
// binary fold `⊕: T×T→T`; the three laws are expressed purely over T values.
// ---------------------------------------------------------------------------

/// Associativity — the non-negotiable core: `(a⊕b)⊕c == a⊕(b⊕c)`.
fn assert_associative<T, M>(a: T, b: T, c: T)
where
    T: Clone + PartialEq + std::fmt::Debug,
    M: MergePolicy<T>,
{
    let left = M::merge(&M::merge(&a, b.clone()), c.clone());
    let right = M::merge(&a, M::merge(&b, c));
    assert_eq!(left, right, "associativity violated");
}

/// Commutativity — reordering two ops onto the same state converges:
/// `(a⊕b)⊕c == (a⊕c)⊕b`. Asserted only when `M::COMMUTATIVE`.
fn assert_commutative<T, M>(a: T, b: T, c: T)
where
    T: Clone + PartialEq + std::fmt::Debug,
    M: MergePolicy<T>,
{
    let left = M::merge(&M::merge(&a, b.clone()), c.clone());
    let right = M::merge(&M::merge(&a, c), b);
    assert_eq!(left, right, "commutativity flag set but law fails");
}

/// Idempotency — re-applying an op is a no-op: `(a⊕b)⊕b == a⊕b`.
/// Asserted only when `M::IDEMPOTENT`.
fn assert_idempotent<T, M>(a: T, b: T)
where
    T: Clone + PartialEq + std::fmt::Debug,
    M: MergePolicy<T>,
{
    let once = M::merge(&a, b.clone());
    let twice = M::merge(&once, b);
    assert_eq!(twice, once, "idempotency flag set but law fails");
}

proptest! {
    // KeepLatest: band — assoc + idem, NOT commutative.
    #[test]
    fn keep_latest_laws(a: i64, b: i64, c: i64) {
        assert_associative::<i64, KeepLatest>(a, b, c);
        prop_assert!(!<KeepLatest as MergePolicy<i64>>::COMMUTATIVE);
        prop_assert!(<KeepLatest as MergePolicy<i64>>::IDEMPOTENT);
        assert_idempotent::<i64, KeepLatest>(a, b);
    }

    // Sum: comm-monoid — assoc + comm, NOT idempotent. Use i32 to dodge overflow.
    #[test]
    fn sum_laws(a in -1_000_000i64..1_000_000, b in -1_000_000i64..1_000_000, c in -1_000_000i64..1_000_000) {
        assert_associative::<i64, Sum>(a, b, c);
        prop_assert!(<Sum as MergePolicy<i64>>::COMMUTATIVE);
        assert_commutative::<i64, Sum>(a, b, c);
        prop_assert!(!<Sum as MergePolicy<i64>>::IDEMPOTENT);
    }

    // Max: semilattice — all three.
    #[test]
    fn max_laws(a: i64, b: i64, c: i64) {
        assert_associative::<i64, Max>(a, b, c);
        assert_commutative::<i64, Max>(a, b, c);
        assert_idempotent::<i64, Max>(a, b);
        prop_assert!(<Max as MergePolicy<i64>>::COMMUTATIVE);
        prop_assert!(<Max as MergePolicy<i64>>::IDEMPOTENT);
    }

    // SetUnion: semilattice — all three.
    #[test]
    fn set_union_laws(a: Vec<u8>, b: Vec<u8>, c: Vec<u8>) {
        let sa: BTreeSet<u8> = a.into_iter().collect();
        let sb: BTreeSet<u8> = b.into_iter().collect();
        let sc: BTreeSet<u8> = c.into_iter().collect();
        assert_associative::<BTreeSet<u8>, SetUnion>(sa.clone(), sb.clone(), sc.clone());
        assert_commutative::<BTreeSet<u8>, SetUnion>(sa.clone(), sb.clone(), sc);
        assert_idempotent::<BTreeSet<u8>, SetUnion>(sa, sb);
    }

    // RawFifo: free semigroup (concat) — assoc ONLY, neither comm nor idem.
    #[test]
    fn raw_fifo_laws(a: Vec<u8>, b: Vec<u8>, c: Vec<u8>) {
        assert_associative::<Vec<u8>, RawFifo>(a, b, c);
        prop_assert!(!<RawFifo as MergePolicy<Vec<u8>>>::COMMUTATIVE);
        prop_assert!(!<RawFifo as MergePolicy<Vec<u8>>>::IDEMPOTENT);
    }
}

/// Concat is genuinely non-commutative and non-idempotent — assert the flags do
/// not lie by exhibiting a counterexample (order and multiplicity are meaning).
#[test]
fn raw_fifo_flags_are_honest() {
    // non-commutative: [1]++[2] != [2]++[1]
    let ab = RawFifo::merge(&vec![1u8], vec![2u8]);
    let ba = RawFifo::merge(&vec![2u8], vec![1u8]);
    assert_ne!(ab, ba);
    // non-idempotent: ([]++[1])++[1] != []++[1]
    let once = RawFifo::merge(&Vec::<u8>::new(), vec![1u8]);
    let twice = RawFifo::merge(&once, vec![1u8]);
    assert_ne!(twice, once);
}

/// `Sum` is genuinely non-idempotent.
#[test]
fn sum_flag_is_honest() {
    let once = Sum::merge(&0i64, 5);
    let twice = Sum::merge(&once, 5);
    assert_ne!(twice, once);
    assert_eq!(twice, 10);
}

// ---------------------------------------------------------------------------
// Runtime: MergeCell over a live Context.
// ---------------------------------------------------------------------------

/// `Cell ≡ MergeCell<KeepLatest>`: a keep-latest MergeCell observably behaves
/// exactly like a plain Cell — `merge` replaces, and equal writes no-op.
#[test]
fn cell_is_merge_cell_keep_latest() {
    let ctx = Context::new();
    let cell = ctx.cell(0i64);
    let mc: MergeCellHandle<i64, KeepLatest> = ctx.merge_cell(0i64);

    for v in [3i64, 3, 7, 7, 1] {
        ctx.set_cell(&cell, v);
        mc.merge(&ctx, v);
        assert_eq!(cell.get(&ctx), mc.get(&ctx), "keep-latest diverged at {v}");
    }
    assert_eq!(mc.get(&ctx), 1);
}

/// A `Sum` MergeCell accumulates; a burst of merges folds into the running sum.
#[test]
fn merge_cell_sum_accumulates() {
    let ctx = Context::new();
    let mc: MergeCellHandle<i64, Sum> = ctx.merge_cell(0i64);
    for d in [1i64, 2, 3, 4] {
        mc.merge(&ctx, d);
    }
    assert_eq!(mc.get(&ctx), 10);
}

/// Converged-state determinism (analysis §2 invariant): the same op multiset,
/// merged in any order into a commutative policy, yields the same egress state —
/// independent of flush grouping. This is the property Phase 2's relay relies on.
#[test]
fn sum_converges_regardless_of_order() {
    let ctx = Context::new();
    let ops = [5i64, -3, 8, 2, -1];

    let a: MergeCellHandle<i64, Sum> = ctx.merge_cell(0);
    for &d in &ops {
        a.merge(&ctx, d);
    }

    let b: MergeCellHandle<i64, Sum> = ctx.merge_cell(0);
    for &d in ops.iter().rev() {
        b.merge(&ctx, d);
    }

    assert_eq!(a.get(&ctx), b.get(&ctx));
    assert_eq!(a.get(&ctx), 11);
}

/// Idempotent-`⊕` gives free dedup through the `PartialEq` store-guard: merging
/// a value already present (Max at/below the current max) does not fire an
/// Effect. This is the write-side merge cost law (analysis §5) meeting §2's
/// idempotency branch.
#[test]
fn idempotent_merge_no_ops_via_partial_eq_guard() {
    use std::cell::Cell as StdCell;
    use std::rc::Rc;

    let ctx = Context::new();
    let mc: MergeCellHandle<i64, Max> = ctx.merge_cell(10i64);

    let runs = Rc::new(StdCell::new(0u32));
    let runs2 = runs.clone();
    let _eff = mc.subscribe(&ctx, move |_c, _v| {
        runs2.set(runs2.get() + 1);
    });
    // subscribe runs once immediately.
    assert_eq!(runs.get(), 1);

    // Merge values <= current max: no state change → guard no-ops → no rerun.
    mc.merge(&ctx, 5);
    mc.merge(&ctx, 10);
    mc.merge(&ctx, 0);
    assert_eq!(
        runs.get(),
        1,
        "idempotent no-op should not rerun the effect"
    );

    // A value above the max changes state → effect reruns exactly once.
    mc.merge(&ctx, 42);
    assert_eq!(mc.get(&ctx), 42);
    assert_eq!(runs.get(), 2);
}

/// The `Reactive`/`Source` supertypes are usable through dynamic dispatch of the
/// generic bound: a `Source<i64>` can be driven and observed uniformly.
#[test]
fn reactive_source_supertype_uniform() {
    fn drive<S: Source<i64> + Copy + 'static>(ctx: &Context, s: S) -> i64 {
        s.set(ctx, 1);
        s.merge(ctx, 2); // for a plain Cell this replaces (KeepLatest)
        s.get(ctx)
    }

    let ctx = Context::new();
    let cell = ctx.cell(0i64);
    assert_eq!(drive(&ctx, cell), 2); // Cell ≡ MergeCell<KeepLatest>: merge == replace

    let mc: MergeCellHandle<i64, Sum> = ctx.merge_cell(0i64);
    // set to 1, then merge(+2) => 3 under Sum.
    assert_eq!(drive(&ctx, mc), 3);
}
