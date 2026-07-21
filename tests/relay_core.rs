//! Phase 2 spike tests for the in-proc `RelayCell` core (`#relaycell`).
//!
//! The load-bearing test is `converged_egress_independent_of_drain_schedule` —
//! the operational form of `LazilyFormal.Relay.relay_converges`: folding the
//! relay's drained windows downstream equals the flat fold of every ingested op,
//! no matter when the egress drains. The rest cover overflow behaviour, the
//! reactive reader-kinds, and construction-time flag validation.

use lazily::{
    BackpressurePolicy, BoundDim, Context, IngressOutcome, KeepLatest, Max, MergePolicy, Overflow,
    RawFifo, RelayCell, RelayConfigError, Source, Sum,
};

fn policy(ctx: &Context, high: u64, overflow: Overflow) -> BackpressurePolicy {
    BackpressurePolicy::new(ctx, BoundDim::Count, high, high / 2, overflow)
}

/// Fold the relay's drained window summaries into a downstream `MergeCell<T, M>`
/// and return the converged egress value.
fn drive<M>(ctx: &Context, ops: &[i64], drain_at: &[usize]) -> i64
where
    M: MergePolicy<i64> + 'static,
{
    // A generous high_water + Conflate so nothing blocks/drops: pure coalescing.
    let relay: RelayCell<i64, M> =
        RelayCell::new(ctx, policy(ctx, u64::MAX, Overflow::Conflate)).unwrap();
    let egress: Source<i64, M> = ctx.merge_cell(0);

    let mut drain_at = drain_at.iter().copied().peekable();
    for (i, &op) in ops.iter().enumerate() {
        relay.ingress(ctx, op);
        if drain_at.peek() == Some(&i) {
            drain_at.next();
            if let Some(window) = relay.drain(ctx) {
                egress.merge(ctx, window);
            }
        }
    }
    // Final drain of whatever remains.
    if let Some(window) = relay.drain(ctx) {
        egress.merge(ctx, window);
    }
    egress.get(ctx)
}

/// The §9 invariant, operationally: the drained-window fold equals the flat fold
/// of the ops, independent of the drain schedule — across policies.
#[test]
fn converged_egress_independent_of_drain_schedule() {
    let ops: Vec<i64> = vec![5, -3, 8, 2, -1, 7, 4];

    // Flat-fold references.
    let ctx = Context::new();
    let sum_flat: i64 = ops.iter().sum();
    let max_flat: i64 = *ops.iter().max().unwrap();

    let schedules: [&[usize]; 4] = [&[], &[0], &[2, 4], &[0, 1, 2, 3, 4, 5, 6]];
    for sched in schedules {
        assert_eq!(
            drive::<Sum>(&ctx, &ops, sched),
            sum_flat,
            "Sum sched {sched:?}"
        );
        assert_eq!(
            drive::<Max>(&ctx, &ops, sched),
            max_flat.max(0),
            "Max sched {sched:?}"
        );
        // KeepLatest downstream = last drained window's last op = last op overall.
        assert_eq!(
            drive::<KeepLatest>(&ctx, &ops, sched),
            *ops.last().unwrap(),
            "KeepLatest sched {sched:?}"
        );
    }
}

/// Conflate keeps merging past `high_water` (the coalescence is the bound); the
/// window depth counts ingested ops while the state stays coalesced.
#[test]
fn conflate_never_blocks() {
    let ctx = Context::new();
    let relay: RelayCell<i64, Sum> =
        RelayCell::new(&ctx, policy(&ctx, 2, Overflow::Conflate)).unwrap();
    for v in [1, 2, 3, 4, 5] {
        assert_ne!(relay.ingress(&ctx, v), IngressOutcome::Blocked);
    }
    assert_eq!(relay.drain(&ctx), Some(15));
}

/// Block refuses ingress at `high_water`; a drain re-opens the relay.
#[test]
fn block_refuses_at_high_water_then_reopens() {
    let ctx = Context::new();
    let relay: RelayCell<i64, Sum> =
        RelayCell::new(&ctx, policy(&ctx, 2, Overflow::Block)).unwrap();
    assert_eq!(relay.ingress(&ctx, 1), IngressOutcome::Accepted);
    assert_eq!(relay.ingress(&ctx, 2), IngressOutcome::Conflated);
    // depth == 2 == high_water → full → blocked, losslessly.
    assert_eq!(relay.ingress(&ctx, 3), IngressOutcome::Blocked);
    assert_eq!(relay.drain(&ctx), Some(3)); // 1 + 2
    // After drain the window is empty → ingress accepted again.
    assert_eq!(relay.ingress(&ctx, 9), IngressOutcome::Accepted);
    assert_eq!(relay.drain(&ctx), Some(9));
}

/// DropNewest discards the incoming op at capacity; DropOldest restarts the window.
#[test]
fn drop_policies() {
    let ctx = Context::new();

    let newest: RelayCell<i64, Sum> =
        RelayCell::new(&ctx, policy(&ctx, 2, Overflow::DropNewest)).unwrap();
    newest.ingress(&ctx, 1);
    newest.ingress(&ctx, 2);
    assert_eq!(newest.ingress(&ctx, 100), IngressOutcome::Dropped);
    assert_eq!(newest.drain(&ctx), Some(3)); // 100 dropped

    let oldest: RelayCell<i64, Sum> =
        RelayCell::new(&ctx, policy(&ctx, 2, Overflow::DropOldest)).unwrap();
    oldest.ingress(&ctx, 1);
    oldest.ingress(&ctx, 2);
    assert_eq!(oldest.ingress(&ctx, 100), IngressOutcome::Dropped);
    assert_eq!(oldest.drain(&ctx), Some(100)); // window reset to the newest op
}

/// The reader-kinds are reactive: an effect on `is_full` reruns as depth crosses
/// the watermark, and demand-driven reads reflect ingress/drain.
#[test]
fn reactive_reads_track_depth() {
    use std::cell::Cell as StdCell;
    use std::rc::Rc;

    let ctx = Context::new();
    let relay: RelayCell<i64, Sum> =
        RelayCell::new(&ctx, policy(&ctx, 2, Overflow::Conflate)).unwrap();

    let full = relay.is_full();
    let flips = Rc::new(StdCell::new(0u32));
    let flips2 = flips.clone();
    let _eff = ctx.effect(move |c| {
        let _ = full.get(c);
        flips2.set(flips2.get() + 1);
    });
    assert_eq!(flips.get(), 1); // initial run, not full
    assert!(!relay.is_full().get(&ctx));
    assert!(relay.is_empty().get(&ctx));

    relay.ingress(&ctx, 1);
    assert_eq!(relay.depth().get(&ctx), 1);
    assert!(!relay.is_full().get(&ctx));

    relay.ingress(&ctx, 1);
    assert!(relay.is_full().get(&ctx)); // depth 2 >= high_water 2
    assert!(flips.get() >= 2, "is_full effect should rerun on crossing");

    relay.drain(&ctx);
    assert_eq!(relay.depth().get(&ctx), 0);
    assert!(relay.is_empty().get(&ctx));
}

/// Construction rejects `Conflate` for `RawFifo` (order + multiplicity are
/// meaning — concat does not bound). Overflow flag validation, analysis §4.3.
#[test]
fn conflate_rejected_for_raw_fifo() {
    let ctx = Context::new();
    let bad = RelayCell::<Vec<u8>, RawFifo>::new(&ctx, policy(&ctx, 4, Overflow::Conflate));
    assert_eq!(bad.err(), Some(RelayConfigError::ConflateNotBounding));

    // RawFifo with Block is fine (lossless, propagates backpressure).
    let ok = RelayCell::<Vec<u8>, RawFifo>::new(&ctx, policy(&ctx, 4, Overflow::Block));
    assert!(ok.is_ok());
    // Flags: RawFifo does not conflate; a bounding policy (Sum) does.
    let raw_conflates = <RawFifo as MergePolicy<Vec<u8>>>::CONFLATES;
    let sum_conflates = <Sum as MergePolicy<i64>>::CONFLATES;
    assert!(!raw_conflates);
    assert!(sum_conflates);
}
