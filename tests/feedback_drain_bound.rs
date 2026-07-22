//! Effect-drain iteration bound (`#lzfeedbackdrain`).
//!
//! A scheduler-closed feedback loop — an effect that writes into its own
//! dependency cone — closes through the *scheduler*, not the graph. It is not a
//! dependency cycle, so acyclicity checks never fire. The write calls
//! `set_cell` -> `flush_effects`, which hits the re-entrancy guard and returns
//! immediately, appending the rescheduled effect to the worklist for the outer
//! drain. The loop therefore runs flat, at constant stack depth: it will not
//! self-terminate by stack overflow, and a recursion-depth bound could never
//! fire.
//!
//! Before this bound existed the drain's only exit was an empty worklist, so a
//! divergent loop hung the process. These tests pin the two halves of the fix:
//! a divergent loop reports exhaustion, and a terminating cascade does not.

use lazily::Context;

/// A loop under `KeepLatest` that never converges must report exhaustion
/// rather than hang.
///
/// `KeepLatest` is the reachable case — `Cell ≡ MergeCell<KeepLatest>` — and
/// its recurrence collapses to `x_{n+1} = f(x_n)`, unrestricted iteration of an
/// arbitrary function over unbounded state. No analysis decides whether that
/// halts, which is exactly why the budget has to exist.
#[test]
fn divergent_loop_reports_exhaustion_instead_of_hanging() {
    let ctx = Context::new();
    // Small budget so the test exercises divergence in microseconds rather
    // than at the default 100k iterations.
    ctx.set_drain_budget(64);

    let counter = ctx.source(0i64);

    // Writes into its own dependency cone: reads `counter`, then writes it.
    // Each run reschedules the next.
    let _effect = ctx.effect(move |c| {
        let n = c.get(&counter);
        c.set(&counter, n + 1);
    });

    // Kick the drain. Without the bound this call never returns.
    ctx.set(&counter, 1);

    let report = ctx
        .last_drain_exhaustion()
        .expect("divergent scheduler-closed loop must report drain exhaustion");

    assert_eq!(report.budget, 64, "report carries the budget that was hit");
    assert_eq!(
        report.iterations, 64,
        "drain stops at the budget, not past it"
    );

    // Attribution is the point: exhaustion must name what was cycling, not
    // merely say a counter was hit. A scheduler-closed loop concentrates runs
    // in one effect, which is what separates it from a wide cascade.
    let (_, top_runs) = report
        .top_effects
        .first()
        .copied()
        .expect("exhaustion report must attribute runs to an effect");
    assert!(
        top_runs as usize >= report.iterations / 2,
        "a self-rescheduling loop concentrates runs in one effect: {:?}",
        report.top_effects
    );
}

/// The complement, and the reason the budget is large by default: a cascade
/// that terminates must not be reported as exhausted.
///
/// Without this, the bound could be "satisfied" by a implementation that
/// reports exhaustion on every flush.
#[test]
fn terminating_cascade_does_not_report_exhaustion() {
    let ctx = Context::new();
    ctx.set_drain_budget(64);

    let source = ctx.source(0i64);
    let mut effects = Vec::new();
    // A wide fan-out: many effects, each running once per write. Spread thin
    // across effects rather than concentrated in one.
    for _ in 0..16 {
        effects.push(ctx.effect(move |c| {
            let _ = c.get(&source);
        }));
    }

    ctx.set(&source, 1);

    assert!(
        ctx.last_drain_exhaustion().is_none(),
        "a terminating cascade must drain to an empty worklist, not exhaustion"
    );
}

/// A loop that stops writing ends the cascade. The effect body is the only
/// yield point in a synchronous context — the calling thread is not running
/// while the drain proceeds, so an external cancellation has nowhere to land.
/// Not writing is what ends it.
#[test]
fn stop_condition_in_effect_body_terminates_the_loop() {
    let ctx = Context::new();
    ctx.set_drain_budget(1_000);

    let counter = ctx.source(0i64);

    let _effect = ctx.effect(move |c| {
        let n = c.get(&counter);
        // The decreasing measure the caller owes. Without it this is the
        // divergent case above.
        if n < 10 {
            c.set(&counter, n + 1);
        }
    });

    ctx.set(&counter, 1);

    assert!(
        ctx.last_drain_exhaustion().is_none(),
        "a loop with a stop condition converges within budget"
    );
    assert_eq!(
        ctx.get(&counter),
        10,
        "loop ran to its fixed point rather than being cut short"
    );
}
