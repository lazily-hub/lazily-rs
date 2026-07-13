//! Phase 7 — example systems as integration tests (`#relaycell`). Each drives
//! the RelayCell stack end-to-end per `relaycell-backpressure-analysis.md` §7.

use lazily::{
    BackpressurePolicy, BoundDim, Context, KeepLatest, KeyedRelay, Outbox, Overflow, RatePolicy,
    RelayCell, SpillMode, SpillStore, Sum,
};

/// §7.2 — high-volume telemetry pipeline (bounded, lossless).
/// `ingest → RelayCell<Sum>(rate-paced) → SpillStore(AppendCompact) → batch egress`.
/// Counters accumulate losslessly (O(keys)); spill bounds memory; the rate policy
/// paces the sink. The sink's converged total equals the flat fold — lossless.
#[test]
fn telemetry_pipeline_bounded_lossless() {
    let ctx = Context::new();
    let relay: RelayCell<i64, Sum> = RelayCell::new(
        &ctx,
        BackpressurePolicy::new(&ctx, BoundDim::Count, 3, 1, Overflow::Spill),
    )
    .unwrap();
    let mut spill: SpillStore<i64, Sum> = SpillStore::new(SpillMode::AppendCompact, 8);
    let mut rate = RatePolicy::new(2, 1);

    let samples: Vec<i64> = (1..=20).collect();
    let flat: i64 = samples.iter().sum();

    for &s in &samples {
        // Page a full window to the durable tail (Spill overflow).
        if relay.is_full().get(&ctx)
            && let Some(win) = relay.drain(&ctx)
        {
            spill.spill(win, 8);
        }
        relay.ingress(&ctx, s);
    }
    if let Some(win) = relay.drain(&ctx) {
        spill.spill(win, 8);
    }

    // Batch egress to the sink, rate-paced. Refill until all pages drain.
    let mut sink = 0i64;
    let mut pending: Vec<i64> = spill.pending_pages().iter().map(|p| p.summary).collect();
    let mut i = 0;
    while i < pending.len() {
        if rate.try_egress() {
            sink += pending[i];
            i += 1;
        } else {
            rate.tick();
        }
    }
    pending.clear();

    assert_eq!(sink, flat, "telemetry sink must be lossless");
}

/// §7.4 — reactive document sync (the `#lzsync` use case). Per-cell `RelayCell<LWW>`
/// (keep-latest) → transport → converge. Each document cell converges to its
/// latest edit; the plane state equals the last write per cell.
#[test]
fn document_sync_plane_converges_per_cell() {
    let ctx = Context::new();
    // Two document cells, each a keyed keep-latest relay shard.
    let mut plane: KeyedRelay<&str, i64, KeepLatest> =
        KeyedRelay::new(u64::MAX, Overflow::Conflate);

    // Interleaved edits to cells "title" and "body".
    let edits = [
        ("title", 1),
        ("body", 100),
        ("title", 2),
        ("body", 200),
        ("title", 3),
    ];
    for (cell, rev) in edits {
        plane.ingress(&ctx, cell, rev);
    }
    // Transport delivers the coalesced latest per cell.
    assert_eq!(plane.drain(&ctx, &"title"), Some(3)); // last title edit
    assert_eq!(plane.drain(&ctx, &"body"), Some(200)); // last body edit
}

/// §7.1 — embedded WS server: per-connection `Outbox<KeepLatest>` state broadcast.
/// Each subscriber conflates to the latest state per its own drain schedule; all
/// converge to the same latest value (KeepLatest is associative).
#[test]
fn broadcast_per_subscriber_conflation() {
    let ctx = Context::new();
    let fast: Outbox<i64, KeepLatest> = Outbox::new(&ctx, u64::MAX).unwrap();
    let slow: Outbox<i64, KeepLatest> = Outbox::new(&ctx, u64::MAX).unwrap();

    // Broadcast a stream of state deltas to both per-connection outboxes.
    let states = [10i64, 20, 30, 40];
    for (i, &st) in states.iter().enumerate() {
        fast.send(&ctx, st);
        slow.send(&ctx, st);
        // The fast client drains every tick; the slow client only reads at the end.
        if i % 2 == 0 {
            let _ = fast.drain(&ctx);
        }
    }
    // Both clients' latest observed state is the final broadcast value.
    assert_eq!(fast.drain(&ctx), Some(40));
    assert_eq!(slow.drain(&ctx), Some(40)); // slow client conflated to latest, no backlog
}
