//! Spike: Rc/Weak handles for automatic reclamation (#lzspecedgeindex, task 12).
//!
//! `cargo run --release --example rc_handle_spike`
//!
//! Tests the design WITHOUT refactoring the library. `RcSlot` wraps a plain
//! `Computed` in an `Rc` whose `Drop` disposes the node, and holds a `Weak`
//! back to the context. That is `'static`, so a compute closure can capture it —
//! the property a borrowing wrapper could never have (E0597, see 40b36c4).
//!
//! Verifies four claims, then measures the cost model:
//!
//! 1. A dropped leaf handle disposes its node, with no explicit call.
//! 2. A node a dependent still reads stays alive when the user drops their
//!    handle — the strong reference is the dependent's captured clone.
//! 3. Disposing that dependent cascades: releasing its captures can transitively
//!    dispose what it read.
//! 4. No reference cycle: the context drops cleanly with live nodes in it.
//!
//! Cost model under test — refcount traffic should land on build and churn, and
//! NOT on publish, because propagation walks bare ids and clones no handle.

use std::cell::Cell as StdCell;
use std::rc::{Rc, Weak};
use std::time::Instant;

use lazily::{Computed, Context, Source};

// Counts disposals so the tests can assert on them.
thread_local! {
    static DISPOSED: StdCell<usize> = const { StdCell::new(0) };
}

fn disposed_count() -> usize {
    DISPOSED.with(|d| d.get())
}

struct RcSlotInner<T> {
    handle: Computed<T>,
    ctx: Weak<Context>,
}

impl<T> Drop for RcSlotInner<T> {
    fn drop(&mut self) {
        // Weak, so a dead context is simply nothing to dispose into.
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.dispose_slot(&self.handle);
            DISPOSED.with(|d| d.set(d.get() + 1));
        }
    }
}

/// A slot whose node lives exactly as long as some clone of this handle.
#[derive(Clone)]
struct RcSlot<T> {
    inner: Rc<RcSlotInner<T>>,
}

impl<T> RcSlot<T> {
    fn handle(&self) -> &Computed<T> {
        &self.inner.handle
    }

    fn strong_count(&self) -> usize {
        Rc::strong_count(&self.inner)
    }
}

fn rc_slot<T, F>(ctx: &Rc<Context>, compute: F) -> RcSlot<T>
where
    T: Clone + PartialEq + 'static,
    F: Fn(&Context) -> T + 'static,
{
    RcSlot {
        inner: Rc::new(RcSlotInner {
            handle: ctx.computed(compute),
            ctx: Rc::downgrade(ctx),
        }),
    }
}

/// Is `id` still listed as a dependent of `cell`?
fn cell_has_dependent<T>(ctx: &Context, cell: &Source<T>, slot_id_of: &Computed<u64>) -> bool {
    // No public introspection, so probe behaviourally: a disposed slot cannot be
    // read. `get` on a disposed node panics, so catch it.
    let _ = cell;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.get(slot_id_of))).is_ok()
}

fn claim_1_leaf_disposes_on_drop() {
    let ctx = Rc::new(Context::new());
    let topic = ctx.cell(1u64);
    let before = disposed_count();
    {
        let leaf = rc_slot(&ctx, move |c| c.get_cell(&topic) + 1);
        assert_eq!(ctx.get(leaf.handle()), 2);
        assert_eq!(leaf.strong_count(), 1);
    }
    assert_eq!(
        disposed_count(),
        before + 1,
        "claim 1: dropping the last handle to a leaf disposes it"
    );
    println!("  claim 1 ok — leaf disposed on drop, no explicit call");
}

fn claim_2_and_3_dependent_keeps_alive_then_cascades() {
    let ctx = Rc::new(Context::new());
    let topic = ctx.cell(1u64);
    let mid = rc_slot(&ctx, move |c| c.get_cell(&topic) + 10);

    // sink captures a clone of mid: the strong reference is upward.
    let mid_for_sink = mid.clone();
    let sink = rc_slot(&ctx, move |c| c.get(mid_for_sink.handle()) * 2);
    assert_eq!(ctx.get(sink.handle()), 22);
    assert_eq!(mid.strong_count(), 2, "user handle + sink's capture");

    let mid_handle = *mid.handle();
    let before = disposed_count();
    drop(mid);
    assert_eq!(
        disposed_count(),
        before,
        "claim 2: mid survives — sink still reads it"
    );
    assert!(
        cell_has_dependent(&ctx, &topic, &mid_handle),
        "claim 2: mid is still readable"
    );
    println!("  claim 2 ok — a node a dependent reads survives the user's drop");

    drop(sink);
    assert_eq!(
        disposed_count(),
        before + 2,
        "claim 3: disposing sink released its capture, cascading into mid"
    );
    println!("  claim 3 ok — disposal cascades transitively");
}

fn claim_4_no_reference_cycle() {
    let weak_probe;
    {
        let ctx = Rc::new(Context::new());
        weak_probe = Rc::downgrade(&ctx);
        let topic = ctx.cell(1u64);
        let a = rc_slot(&ctx, move |c| c.get_cell(&topic) + 1);
        let a2 = a.clone();
        let _b = rc_slot(&ctx, move |c| c.get(a2.handle()) + 1);
        assert_eq!(Rc::strong_count(&ctx), 1, "handles hold Weak, not Rc");
    }
    assert!(
        weak_probe.upgrade().is_none(),
        "claim 4: the context dropped — no cycle kept it alive"
    );
    println!("  claim 4 ok — context freed, no reference cycle");
}

/// Rc-handle costs against plain Copy handles, on the three paths.
fn cost_model(width: usize) {
    // --- build ---
    let ctx_plain = Context::new();
    let topic_plain = ctx_plain.cell(0u64);
    let start = Instant::now();
    let plain_a: Vec<_> = (0..width)
        .map(|i| {
            let slot = ctx_plain.computed(move |c| c.get_cell(&topic_plain) + i as u64);
            ctx_plain.get(&slot);
            slot
        })
        .collect();
    let plain_build = start.elapsed().as_nanos() as f64 / width as f64;

    let ctx_rc = Rc::new(Context::new());
    let topic_rc = ctx_rc.cell(0u64);
    let start = Instant::now();
    let rc_a: Vec<_> = (0..width)
        .map(|i| {
            let slot = rc_slot(&ctx_rc, move |c| c.get_cell(&topic_rc) + i as u64);
            ctx_rc.get(slot.handle());
            slot
        })
        .collect();
    let rc_build = start.elapsed().as_nanos() as f64 / width as f64;

    // --- publish (no handle is cloned or dropped: expect no difference) ---
    let publishes = (200_000 / width).max(1);
    let start = Instant::now();
    for publish in 1..=publishes {
        ctx_plain.set_cell(&topic_plain, publish as u64);
        for slot in &plain_a {
            std::hint::black_box(ctx_plain.get(slot));
        }
    }
    let plain_publish = start.elapsed().as_nanos() as f64 / (publishes * width) as f64;

    let start = Instant::now();
    for publish in 1..=publishes {
        ctx_rc.set_cell(&topic_rc, publish as u64);
        for slot in &rc_a {
            std::hint::black_box(ctx_rc.get(slot.handle()));
        }
    }
    let rc_publish = start.elapsed().as_nanos() as f64 / (publishes * width) as f64;

    // --- churn: plain needs an explicit dispose, Rc needs nothing ---
    const CYCLES: usize = 100_000;
    let mut plain_live: Vec<_> = plain_a.iter().copied().take(64).collect();
    let start = Instant::now();
    for cycle in 0..CYCLES {
        let victim = plain_live.swap_remove(cycle % plain_live.len());
        ctx_plain.dispose_slot(&victim);
        let slot = ctx_plain.computed(move |c| c.get_cell(&topic_plain) + cycle as u64);
        ctx_plain.get(&slot);
        plain_live.push(slot);
    }
    let plain_churn = start.elapsed().as_nanos() as f64 / CYCLES as f64;

    let mut rc_live: Vec<_> = rc_a.iter().take(64).cloned().collect();
    let start = Instant::now();
    for cycle in 0..CYCLES {
        rc_live.swap_remove(cycle % rc_live.len()); // dropped => disposed
        let slot = rc_slot(&ctx_rc, move |c| c.get_cell(&topic_rc) + cycle as u64);
        ctx_rc.get(slot.handle());
        rc_live.push(slot);
    }
    let rc_churn = start.elapsed().as_nanos() as f64 / CYCLES as f64;

    println!(
        "\n  width {width}\n  {:<10}{:>14}{:>14}{:>12}",
        "path", "Copy id (ns)", "Rc/Weak (ns)", "delta"
    );
    for (name, plain, rc) in [
        ("build", plain_build, rc_build),
        ("publish", plain_publish, rc_publish),
        ("churn", plain_churn, rc_churn),
    ] {
        println!(
            "  {name:<10}{plain:>14.1}{rc:>14.1}{:>11.0}%",
            (rc - plain) / plain * 100.0
        );
    }
}

fn main() {
    println!("Rc/Weak handle spike — correctness claims:");
    claim_1_leaf_disposes_on_drop();
    claim_2_and_3_dependent_keeps_alive_then_cascades();
    claim_4_no_reference_cycle();

    println!("\ncost model (refcount traffic should hit build & churn, not publish):");
    for width in [1024, 65_536] {
        cost_model(width);
    }
}
