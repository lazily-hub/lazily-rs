//! Phase 3 spike tests for the paged durable tail (`SpillStore`, `#relaycell`).
//!
//! Operational forms of `LazilyFormal.Relay.spill_lossless` and
//! `spill_replay_idempotent`: reconstructing from cold pages + hot head equals
//! the flat fold, and crash-replaying unacked pages converges for an idempotent
//! policy. Also drives a `RelayCell` under `Spill` overflow into a `SpillStore`.

use lazily::{
    BackpressurePolicy, BoundDim, Context, Max, MergePolicy, Overflow, RelayCell, SetUnion, Source,
    SpillMode, SpillStore, Sum,
};
use std::collections::BTreeSet;

/// `spill_lossless`: reconstruct(cold pages + hot) == flat fold, for both modes.
#[test]
fn spill_reconstruct_is_lossless() {
    let ops = [5i64, 3, 8, 2, 9, 1, 7];
    let flat: i64 = ops.iter().sum();

    for mode in [SpillMode::AppendCompact, SpillMode::CompactOnWrite] {
        let mut store: SpillStore<i64, Sum> = SpillStore::new(mode, 2);
        // Spill each op as a one-op window; keep the last two in the hot head.
        let split = ops.len() - 2;
        for &op in &ops[..split] {
            store.spill(op, 8);
        }
        let hot: i64 = ops[split..].iter().sum();
        assert_eq!(store.reconstruct(0, Some(hot)), flat, "mode {mode:?}");
    }
}

/// CompactOnWrite bounds page count to `ceil(n / page_size)`; AppendCompact keeps
/// one page per window. Both stay lossless (checked above).
#[test]
fn compact_on_write_bounds_pages() {
    let mut compact: SpillStore<i64, Sum> = SpillStore::new(SpillMode::CompactOnWrite, 3);
    let mut append: SpillStore<i64, Sum> = SpillStore::new(SpillMode::AppendCompact, 3);
    for v in 0..9i64 {
        compact.spill(v, 1);
        append.spill(v, 1);
    }
    assert_eq!(compact.page_count(), 3); // 9 windows / page_size 3
    assert_eq!(append.page_count(), 9);
    // Same converged fold regardless of layout.
    assert_eq!(compact.fold_pages(0), append.fold_pages(0));
}

/// `spill_replay_idempotent`: after a crash the egress replays unacked pages;
/// for an idempotent policy (Max / SetUnion) re-delivering an already-applied
/// page is a no-op, so at-least-once replay converges.
#[test]
fn crash_replay_converges_for_idempotent_policy() {
    let mut store: SpillStore<i64, Max> = SpillStore::new(SpillMode::AppendCompact, 1);
    for v in [3i64, 9, 5, 9, 2] {
        store.spill(v, 8);
    }
    // Downstream consumed everything once (converged = max = 9).
    let converged = store.replay_unacked(0);
    assert_eq!(converged, 9);

    // Crash before acking: replay the SAME unacked pages into the converged
    // state. Idempotent ⊕ ⇒ no change.
    let after_replay = store.replay_unacked(converged);
    assert_eq!(after_replay, converged, "idempotent replay must be a no-op");

    // A second full crash-replay from scratch still converges to the same value.
    assert_eq!(store.replay_unacked(0), converged);
}

/// SetUnion (grow-only semilattice) also survives at-least-once replay.
#[test]
fn crash_replay_set_union_idempotent() {
    let mut store: SpillStore<BTreeSet<u8>, SetUnion> =
        SpillStore::new(SpillMode::AppendCompact, 1);
    store.spill([1u8, 2].into_iter().collect(), 8);
    store.spill([2u8, 3].into_iter().collect(), 8);
    let converged = store.replay_unacked(BTreeSet::new());
    let expected: BTreeSet<u8> = [1, 2, 3].into_iter().collect();
    assert_eq!(converged, expected);
    // Replay again over the converged state — no change.
    assert_eq!(store.replay_unacked(converged.clone()), expected);
}

/// ack-before-reclaim: acking advances the reclaim cursor; reclaim drops acked
/// pages while keeping the manifest/cursor consistent and pending pages intact.
#[test]
fn ack_before_reclaim() {
    let mut store: SpillStore<i64, Sum> = SpillStore::new(SpillMode::AppendCompact, 1);
    for v in [10i64, 20, 30] {
        store.spill(v, 4);
    }
    assert_eq!(store.page_count(), 3);
    assert_eq!(store.manifest(), vec![(0, 4), (1, 4), (2, 4)]);

    // Egress acks pages 0 and 1.
    store.ack_through(1);
    assert_eq!(store.pending_pages().len(), 1); // only page 2 remains unacked
    assert_eq!(store.pending_pages()[0].summary, 30);

    store.reclaim();
    assert_eq!(store.page_count(), 1);
    assert_eq!(store.pending_pages()[0].summary, 30);
}

/// End-to-end: a `RelayCell` under `Spill` overflow pages full windows into a
/// `SpillStore`; the durable tail + final hot head reconstruct the flat fold.
#[test]
fn relay_spills_full_windows_losslessly() {
    let ctx = Context::new();
    let policy = BackpressurePolicy::new(&ctx, BoundDim::Count, 3, 1, Overflow::Spill);
    let relay: RelayCell<i64, Sum> = RelayCell::new(&ctx, policy).unwrap();
    let mut store: SpillStore<i64, Sum> = SpillStore::new(SpillMode::AppendCompact, 4);

    let ops = [1i64, 2, 3, 4, 5, 6, 7];
    let flat: i64 = ops.iter().sum();

    for &op in &ops {
        // When the window is full, page it out to the durable tail before merging.
        if relay.is_full().get(&ctx)
            && let Some(window) = relay.drain(&ctx)
        {
            store.spill(window, 8);
        }
        relay.ingress(&ctx, op);
    }
    let hot = relay.drain(&ctx);
    assert_eq!(store.reconstruct(0, hot), flat);

    // The egress accumulates cold pages then the hot head — same converged value.
    let egress: Source<i64, Sum> = ctx.merge_cell(0);
    for page in store.pending_pages() {
        egress.merge(&ctx, page.summary);
    }
    assert_eq!(egress.get(&ctx), flat - hot.unwrap_or(0));
    let sum_conflates = <Sum as MergePolicy<i64>>::CONFLATES;
    assert!(sum_conflates);
}
