//! Spike: nested context for group-scoped lifetime (task 12).
//!
//! `cargo run --release --example child_context_spike`
//!
//! A third option alongside explicit disposal and Rc/Weak handles. A `Child`
//! borrows the context and records what it created; dropping it disposes that
//! group. Crucially the child is never captured by a compute closure — only
//! plain `Copy` handles are — so it sidesteps the `'static` problem that made a
//! borrowing per-node wrapper impossible (E0597, see 40b36c4).
//!
//! Claims under test:
//!
//! 1. Dropping a child disposes everything it created, with no per-node call.
//! 2. Handles stay `Copy`: a source is captured by two closures with no clone.
//! 3. Nodes in a child may freely read nodes owned by the parent or another
//!    child — grouping is about teardown, not visibility.
//! 4. The known gap: dropping a child tears out nodes another child still
//!    reads. Same contract as dispose_slot today; Rc/Weak is what fixes this.

use std::cell::RefCell;
use std::time::Instant;

use lazily::{Context, SlotHandle};

/// A teardown group. Nodes created through it are disposed when it drops.
struct Child<'ctx> {
    ctx: &'ctx Context,
    owned: RefCell<Vec<Box<dyn FnOnce(&Context)>>>,
}

impl<'ctx> Child<'ctx> {
    fn new(ctx: &'ctx Context) -> Self {
        Self {
            ctx,
            owned: RefCell::new(Vec::new()),
        }
    }

    /// Create a slot owned by this group. Returns a plain `Copy` handle.
    fn computed<T, F>(&self, compute: F) -> SlotHandle<T>
    where
        T: Clone + 'static,
        F: Fn(&Context) -> T + 'static,
    {
        let handle = self.ctx.computed(compute);
        self.owned
            .borrow_mut()
            .push(Box::new(move |ctx: &Context| ctx.dispose_slot(&handle)));
        handle
    }

    fn len(&self) -> usize {
        self.owned.borrow().len()
    }
}

impl Drop for Child<'_> {
    fn drop(&mut self) {
        // Reverse order: dependents before the things they read, so a group
        // tears itself down without transiently dangling inside itself.
        for dispose in self.owned.borrow_mut().drain(..).rev() {
            dispose(self.ctx);
        }
    }
}

fn claim_1_group_disposes_on_drop() {
    let ctx = Context::new();
    let topic = ctx.cell(1u64);
    let probe;
    {
        let conn = Child::new(&ctx);
        let a = conn.computed(move |c| c.get_cell(&topic) + 1);
        let _b = conn.computed(move |c| c.get(&a) * 10);
        assert_eq!(conn.len(), 2);
        probe = a;
        assert_eq!(ctx.get(&probe), 2);
    }
    // No public introspection, so probe behaviourally: a disposed node cannot
    // be read.
    let still_readable =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.get(&probe))).is_ok();
    assert!(
        !still_readable,
        "claim 1: the group's nodes were disposed on drop"
    );
    println!("  claim 1 ok — dropping a child disposed its whole group");
}

fn claim_2_handles_stay_copy() {
    let ctx = Context::new();
    let topic = ctx.cell(5u64);
    let conn = Child::new(&ctx);
    // `topic` captured twice, no clone: still a Copy handle.
    let a = conn.computed(move |c| c.get_cell(&topic) + 1);
    let b = conn.computed(move |c| c.get_cell(&topic) + 2);
    assert_eq!(ctx.get(&a) + ctx.get(&b), 13);
    println!("  claim 2 ok — source captured by two closures, no clone needed");
}

fn claim_3_cross_group_reads_work() {
    let ctx = Context::new();
    let topic = ctx.cell(2u64);
    let outer = ctx.computed(move |c| c.get_cell(&topic) * 3);
    let conn = Child::new(&ctx);
    let inner = conn.computed(move |c| c.get(&outer) + 1);
    assert_eq!(ctx.get(&inner), 7, "a child node reads a parent-owned node");
    println!("  claim 3 ok — grouping bounds teardown, not visibility");
}

fn cost_model(width: usize) {
    // build + teardown through a child, against explicit per-node disposal
    let ctx = Context::new();
    let topic = ctx.cell(0u64);

    let start = Instant::now();
    let plain: Vec<_> = (0..width)
        .map(|i| {
            let s = ctx.computed(move |c| c.get_cell(&topic) + i as u64);
            ctx.get(&s);
            s
        })
        .collect();
    let plain_build = start.elapsed().as_nanos() as f64 / width as f64;

    let start = Instant::now();
    for handle in &plain {
        ctx.dispose_slot(handle);
    }
    let plain_teardown = start.elapsed().as_nanos() as f64 / width as f64;

    let start = Instant::now();
    let child = Child::new(&ctx);
    for i in 0..width {
        let s = child.computed(move |c| c.get_cell(&topic) + i as u64);
        ctx.get(&s);
    }
    let child_build = start.elapsed().as_nanos() as f64 / width as f64;

    let start = Instant::now();
    drop(child);
    let child_teardown = start.elapsed().as_nanos() as f64 / width as f64;

    println!(
        "\n  width {width}\n  {:<12}{:>14}{:>14}",
        "path", "explicit (ns)", "child (ns)"
    );
    println!("  {:<12}{plain_build:>14.1}{child_build:>14.1}", "build");
    println!(
        "  {:<12}{plain_teardown:>14.1}{child_teardown:>14.1}",
        "teardown"
    );
}

fn main() {
    println!("Nested-context spike — claims:");
    claim_1_group_disposes_on_drop();
    claim_2_handles_stay_copy();
    claim_3_cross_group_reads_work();
    println!(
        "\n  claim 4 (known gap): a node in another group that reads into this\n  \
         one dangles after the drop — same contract as dispose_slot today.\n  \
         Rc/Weak is the option that closes it."
    );

    println!("\ncost model — per-node cost of build and teardown:");
    for width in [1024, 65_536] {
        cost_model(width);
    }
}
