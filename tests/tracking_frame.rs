//! The tracking frame must be popped on the unwind path (`#lzspecedgeindex`).
//!
//! Reading a disposed node panics — that is this library's expression of the
//! spec's `read_after_dispose`. Such a read happens *inside* a compute closure
//! whenever a surviving dependent is recomputed after its dependency was
//! disposed, which is exactly the survivor shape
//! `disposal_does_not_run_surviving_effects.json` describes. A bare
//! `push_tracking_frame` / `pop_tracking_frame` pair skips the pop on unwind,
//! leaving the dead node as the current frame so every *later* top-level read
//! silently registers a dependency edge against it. The corruption surfaces
//! arbitrarily far from the throw, which is what makes it worth pinning.

use std::panic::{self, AssertUnwindSafe};

use lazily::Context;

/// Swallow a panic and its message.
fn quiet<R>(f: impl FnOnce() -> R) -> Result<R, ()> {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let out = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(prev);
    out.map_err(|_| ())
}

#[test]
fn a_panic_out_of_a_slot_compute_does_not_strand_the_frame() {
    let ctx = Context::new();
    let src = ctx.cell(1i64);
    let mid = ctx.computed(move |c| c.get(&src) + 1);
    let survivor = ctx.computed(move |c| c.get(&mid) * 10);
    assert_eq!(ctx.get(&survivor), 20);

    ctx.dispose_slot(&mid);
    assert!(
        quiet(|| ctx.get(&survivor)).is_err(),
        "reading through a disposed dependency must error"
    );

    // A top-level read is not inside any compute, so it must register nothing.
    let fresh = ctx.cell(100i64);
    assert_eq!(ctx.get(&fresh), 100);
    assert_eq!(
        ctx.dependent_count(&fresh),
        0,
        "stranded tracking frame: a top-level read registered a dependent"
    );
}

#[test]
fn a_panic_out_of_an_effect_body_does_not_strand_the_frame() {
    let ctx = Context::new();
    let src = ctx.cell(1i64);
    let mid = ctx.computed(move |c| c.get(&src) + 1);
    // The effect reads `src` directly as well as through `mid`. The direct edge
    // is what survives the disposal and gives the write below something to
    // invalidate, so the effect actually reruns and reaches the disposed `mid`.
    // Reading only `mid` would not do: disposal detaches that edge, the write
    // would reach nothing, and the effect body would never run again.
    let _watch = ctx.effect(move |c| {
        let _ = c.get(&src);
        let _ = c.get(&mid);
    });

    ctx.dispose_slot(&mid);
    // Disposal must not run the survivor, so force the rerun explicitly to
    // reach the effect-body unwind path this test is about.
    assert!(
        quiet(|| ctx.set(&src, 2)).is_err(),
        "rerunning an effect over a disposed dependency must error"
    );

    let fresh = ctx.cell(100i64);
    assert_eq!(ctx.get(&fresh), 100);
    assert_eq!(
        ctx.dependent_count(&fresh),
        0,
        "stranded tracking frame: a top-level read registered a dependent"
    );
}
