//! `Context::computed_ripple_when` (#lzcellkernel) — a guarded computed with an
//! explicit, PURE change predicate (`true` = propagate). Covers the two
//! motivating shapes: a custom significance policy, and "propagate every N"
//! where the increment evidence lives in the value (so the predicate stays
//! pure). `computed(f) == computed_ripple_when(f, !=)`; `slot(f)` = always-propagate.

use lazily::Context;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn computed_ripple_when_custom_significance_propagates_on_proxy_change() {
    let ctx = Context::new();
    let input = ctx.source(0u64);

    // Derived value carries a `bucket` proxy; propagate only when the bucket
    // changes, ignoring the raw payload.
    let derived = ctx.computed_ripple_when(
        move |c| {
            let v = c.get(&input);
            (v, v / 10) // (payload, bucket)
        },
        |old, new| old.1 != new.1, // propagate when bucket changed
    );

    let recomputes = Rc::new(Cell::new(0u32));
    let r = recomputes.clone();
    let observer = ctx.computed(move |c| {
        r.set(r.get() + 1);
        c.get(&derived).0
    });

    assert_eq!(ctx.get(&observer), 0);
    let base = recomputes.get();

    // Same bucket (0..9): dependent stays cached.
    ctx.set(&input, 3);
    assert_eq!(ctx.get(&observer), 0, "suppressed: proxy bucket unchanged");
    assert_eq!(
        recomputes.get(),
        base,
        "no dependent recompute within a bucket"
    );

    // Crossing a bucket boundary propagates.
    ctx.set(&input, 12);
    assert_eq!(ctx.get(&observer), 12, "propagated: bucket changed");
    assert_eq!(recomputes.get(), base + 1);
}

#[test]
fn computed_ripple_when_propagate_every_n_via_value_carried_counter() {
    let ctx = Context::new();
    let input = ctx.source(0u64);

    // "Propagate every 3rd increment" — evidence (the counter) is IN the value,
    // so the predicate is a pure function of (old, new): propagate only when the
    // count crosses a size-3 window boundary.
    let sampled = ctx.computed_ripple_when(move |c| c.get(&input), |old, new| new / 3 != old / 3);

    let seen = Rc::new(Cell::new(0u32));
    let s = seen.clone();
    let observer = ctx.computed(move |c| {
        s.set(s.get() + 1);
        c.get(&sampled)
    });

    assert_eq!(ctx.get(&observer), 0);
    let base = seen.get();

    // 0 -> 1 -> 2 stay in window [0,3): suppressed.
    ctx.set(&input, 1);
    ctx.set(&input, 2);
    assert_eq!(ctx.get(&observer), 0);
    assert_eq!(seen.get(), base, "window not crossed yet");

    // 3 crosses into [3,6): propagate.
    ctx.set(&input, 3);
    assert_eq!(ctx.get(&observer), 3);
    assert_eq!(seen.get(), base + 1);
}

#[test]
fn computed_is_computed_ripple_when_not_equal() {
    // `computed(f)` behaves as `computed_ripple_when(f, |o, n| o != n)`.
    let ctx = Context::new();
    let input = ctx.source(0i64);

    let via_computed = ctx.computed(move |c| c.get(&input).min(1));
    let via_when = ctx.computed_ripple_when(move |c| c.get(&input).min(1), |o, n| o != n);

    let ca = Rc::new(Cell::new(0u32));
    let cb = Rc::new(Cell::new(0u32));
    let (a, b) = (ca.clone(), cb.clone());
    let obs_a = ctx.computed(move |c| {
        a.set(a.get() + 1);
        c.get(&via_computed)
    });
    let obs_b = ctx.computed(move |c| {
        b.set(b.get() + 1);
        c.get(&via_when)
    });
    assert_eq!(ctx.get(&obs_a), 0);
    assert_eq!(ctx.get(&obs_b), 0);
    let (base_a, base_b) = (ca.get(), cb.get());

    // 0 -> 5 both clamp to 1: both guards suppress identically.
    ctx.set(&input, 5);
    assert_eq!(ctx.get(&obs_a), 1);
    assert_eq!(ctx.get(&obs_b), 1);
    assert_eq!(ca.get(), base_a + 1);
    assert_eq!(cb.get(), base_b + 1);

    // 5 -> 9 both stay 1: both suppress the dependent.
    ctx.set(&input, 9);
    assert_eq!(ctx.get(&obs_a), 1);
    assert_eq!(ctx.get(&obs_b), 1);
    assert_eq!(ca.get(), base_a + 1, "computed suppressed equal recompute");
    assert_eq!(
        cb.get(),
        base_b + 1,
        "computed_ripple_when(!=) matches computed"
    );
}

#[test]
fn slot_is_pass_through_always_propagates() {
    let ctx = Context::new();
    let input = ctx.source(0u64);
    // slot() installs no guard: even an equal recompute propagates.
    let passthrough = ctx.slot(move |c| {
        let _ = c.get(&input); // depend on input, but always yield the same value
        0u64
    });

    let recomputes = Rc::new(Cell::new(0u32));
    let r = recomputes.clone();
    let observer = ctx.computed(move |c| {
        r.set(r.get() + 1);
        c.get(&passthrough)
    });

    assert_eq!(ctx.get(&observer), 0);
    let base = recomputes.get();

    // Value stays 0, but slot has no guard, so the dependent re-fires.
    ctx.set(&input, 5);
    assert_eq!(ctx.get(&observer), 0);
    assert!(
        recomputes.get() > base,
        "pass-through slot propagates even when the value is unchanged"
    );
}
