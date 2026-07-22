//! Phase-0 acceptance: demand-driven reader-kinds + store-without-cascade
//! (`relaycell-backpressure-analysis.md` §5 / §4.0). These assert the *merge
//! cost law* deterministically via instrumentation counters rather than
//! wall-clock ns (which is CI-flaky): an **unobserved** queue op does no reader
//! derivation and schedules no effect, and a **burst** of writes between two
//! reads coalesces to a single recompute ("dirty-once").
//!
//! Run with `cargo test --features instrumentation --test queue_demand_driven`.
#![cfg(feature = "instrumentation")]

use lazily::{Context, QueueCell};

/// Zero subscribers → an op derives no reader value, allocates no node, and
/// schedules no effect. The reactive shell is charged only along a path an
/// effect actually observes (§4.0 merge cost law); an unsubscribed `QueueCell`
/// collapses toward raw-storage cost.
#[test]
fn unsubscribed_ops_do_not_derive_or_schedule() {
    let ctx = Context::new();
    let q: QueueCell<i32> = QueueCell::new(&ctx);

    // Warm-up push so the queue is non-empty; then measure a steady-state run.
    q.try_push(&ctx, 0).unwrap();
    ctx.reset_instrumentation();

    const OPS: i32 = 1000;
    for i in 0..OPS {
        q.try_push(&ctx, i).unwrap();
        q.try_pop(&ctx).unwrap();
    }

    let snap = ctx.instrumentation_snapshot();
    assert_eq!(
        snap.slot_recomputes, 0,
        "unsubscribed ops must derive no reader-kind value"
    );
    assert_eq!(
        snap.effect_queue_pushes, 0,
        "unsubscribed ops must schedule no effect (store-without-cascade)"
    );
    assert_eq!(
        snap.node_allocations, 0,
        "steady-state ops must allocate no new graph nodes"
    );
}

/// A burst of N writes between two reads costs ONE recompute at the next read,
/// not N — the dirty-once law. `mark`/`clear` of an already-dirty (uncached)
/// Slot does not re-derive; derivation is paid once, lazily, on the next `Get`.
#[test]
fn burst_writes_coalesce_to_one_recompute() {
    for burst in [1usize, 8, 64, 512] {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);

        // First read materializes the len Slot (one recompute), then reset so we
        // measure only the burst + the single trailing read.
        assert_eq!(q.len(&ctx), 0);
        ctx.reset_instrumentation();

        for i in 0..burst {
            q.try_push(&ctx, i as i32).unwrap();
        }
        // No read happened during the burst → no derivation yet.
        assert_eq!(
            ctx.instrumentation_snapshot().slot_recomputes,
            0,
            "a write-only burst must not derive the len Slot"
        );

        // One trailing read pays exactly one recompute regardless of burst size.
        assert_eq!(q.len(&ctx), burst);
        assert_eq!(
            ctx.instrumentation_snapshot().slot_recomputes,
            1,
            "burst of {burst} writes + 1 read must recompute len exactly once"
        );
    }
}

/// Store-without-cascade write dual: setting a cell whose dependent cone holds
/// no Effect stores the latest value (glitch-free for a *future* subscriber) but
/// schedules no effect. A subscriber that attaches later reads the current value.
#[test]
fn store_without_cascade_skips_flush_but_stays_glitch_free() {
    let ctx = Context::new();
    let cell = ctx.source(0i32);

    // A lazy Slot dependent (no Effect in the cone).
    let doubled = ctx.computed(move |ctx| ctx.get(&cell) * 2);
    assert_eq!(ctx.get(&doubled), 0);

    ctx.reset_instrumentation();
    for v in 1..=100 {
        ctx.set(&cell, v);
    }
    assert_eq!(
        ctx.instrumentation_snapshot().effect_queue_pushes,
        0,
        "writes with no Effect in the dependent cone must schedule no effect"
    );

    // Late read sees the latest stored value, glitch-free.
    assert_eq!(ctx.get(&doubled), 200);
}
