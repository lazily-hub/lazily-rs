//! The fortified `Compute` view is the sole tracking surface (`#lzcellkernel`).
//!
//! These tests pin the two halves of the fortification contract:
//!
//! 1. A **tracked** read through the `&Compute` handed to a compute/effect
//!    closure registers a dependency edge against the *recomputing node*, so a
//!    change to the dependency recomputes the dependent.
//! 2. The explicit **untracked** escape (`Compute::untracked()`) registers
//!    **no** edge, so the dependent neither gains a dependency nor recomputes.
//!
//! The recomputing node id is threaded as a *value* (`Compute::slot_id`), not an
//! ambient thread-local, so the attribution is correct by construction.

use std::cell::Cell;
use std::rc::Rc;

use lazily::Context;

#[test]
fn tracked_read_registers_edge_against_the_recomputing_node() {
    let ctx = Context::new();
    let a = ctx.source(1i32);

    let calls = Rc::new(Cell::new(0usize));
    let b = ctx.computed({
        let calls = Rc::clone(&calls);
        move |c| {
            calls.set(calls.get() + 1);
            // Tracked read: the edge must attribute to `b`, the node being
            // recomputed — not to any ambient frame.
            c.get(&a) * 10
        }
    });

    assert_eq!(ctx.get(&b), 10);
    assert_eq!(calls.get(), 1, "first read computes once");

    // Structural: the edge exists in both directions.
    assert_eq!(
        ctx.dependent_count(&a),
        1,
        "a must have b as its single tracked dependent"
    );
    assert_eq!(ctx.dependency_count(&b), 1, "b must depend on a");

    // Behavioural: changing a recomputes b.
    ctx.set(&a, 5);
    assert_eq!(ctx.get(&b), 50);
    assert_eq!(
        calls.get(),
        2,
        "changing the tracked dependency recomputes b"
    );
}

#[test]
fn untracked_read_registers_no_edge_and_does_not_recompute() {
    let ctx = Context::new();
    let a = ctx.source(1i32);

    let calls = Rc::new(Cell::new(0usize));
    let d = ctx.computed({
        let calls = Rc::clone(&calls);
        move |c| {
            calls.set(calls.get() + 1);
            // The explicit untracked escape: read `a` through the owning
            // `Context`, which forms no dependency edge.
            c.untracked().get(&a) * 10
        }
    });

    assert_eq!(ctx.get(&d), 10);
    assert_eq!(calls.get(), 1);

    // Structural: no edge was formed by the untracked read.
    assert_eq!(
        ctx.dependent_count(&a),
        0,
        "an untracked read must not register a dependent"
    );
    assert_eq!(
        ctx.dependency_count(&d),
        0,
        "d must have acquired no dependency"
    );

    // Behavioural: changing a does NOT recompute d — its cached value stands.
    ctx.set(&a, 5);
    assert_eq!(ctx.get(&d), 10, "untracked dependent keeps its stale value");
    assert_eq!(calls.get(), 1, "untracked dependent never recomputes");
}

#[test]
fn effect_tracks_through_its_compute_view() {
    let ctx = Context::new();
    let a = ctx.source(1i32);

    let runs = Rc::new(Cell::new(0usize));
    let _watch = ctx.effect({
        let runs = Rc::clone(&runs);
        move |c| {
            runs.set(runs.get() + 1);
            let _ = c.get(&a);
        }
    });

    assert_eq!(runs.get(), 1, "effect runs once on creation");
    assert_eq!(ctx.dependent_count(&a), 1, "effect owns the edge to a");

    ctx.set(&a, 2);
    assert_eq!(runs.get(), 2, "a change reruns the tracking effect");
}
