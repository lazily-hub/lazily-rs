//! #lzspecedgeindex audit harness: effect-flush cost and edge-removal cost.
//!
//! ```sh
//! cargo run --release --example edge_audit
//! cargo run --release --features thread-safe --example edge_audit -- thread-safe
//! ```
//!
//! `pubsub_load` measures pull-based reads through computed slots. Neither the
//! effect-flush path nor the edge-*removal* path is on that route, so a
//! quadratic in either is invisible there. This measures both, and it measures
//! them as a *shape* control rather than as absolute growth.
//!
//! ## Why a shape control, not a growth ladder
//!
//! "cost per unit grows < 2x from 1k to 1M" is unsound: cache effects alone
//! move a correct engine by several times over that range. So each ladder here
//! holds the **total node count and total work fixed** and varies only the
//! fan-out width:
//!
//!   N effects total, split across N/W topics of width W each.
//!   Publish once into every topic => exactly N notifications, at every rung.
//!
//! Every rung does identical total work. A flat ns/notification column is the
//! correct result. A column that climbs with W is a quadratic in W, and cache
//! effects cannot explain it because the working set is the same size.

use std::time::Instant;

use lazily::Context;

/// Fan-out widths. Total effect count is held fixed at `TOTAL_NODES`.
const WIDTHS: [usize; 7] = [16, 64, 256, 1024, 4096, 16_384, 65_536];
/// Fixed total node count across every rung of the shape control.
const TOTAL_NODES: usize = 65_536;

/// Effect fan-out shape control: `TOTAL_NODES` effects in `TOTAL_NODES / width`
/// topics of `width` each. Returns ns per notification.
fn effect_shape_rung(width: usize) -> f64 {
    let topics = (TOTAL_NODES / width).max(1);
    let ctx = Context::new();

    let topic_a: Vec<_> = (0..topics).map(|_| ctx.cell(0u64)).collect();
    for topic in &topic_a {
        let topic = *topic;
        for _ in 0..width {
            ctx.effect(move |ctx| {
                std::hint::black_box(ctx.get_cell(&topic));
            });
        }
    }

    // One publish per topic => topics * width == TOTAL_NODES notifications.
    let start = Instant::now();
    for (revision, topic) in topic_a.iter().enumerate() {
        ctx.set_cell(topic, revision as u64 + 1);
    }
    let elapsed = start.elapsed().as_nanos() as f64;

    elapsed / (topics * width) as f64
}

/// Edge-removal shape control: `TOTAL_NODES` subscriber slots in
/// `TOTAL_NODES / width` topics of `width` each, then dispose every slot.
/// Returns ns per disposal. Disposal is what walks the dependency edges and
/// removes each subscriber from its topic's dependent list.
fn teardown_shape_rung(width: usize) -> f64 {
    let topics = (TOTAL_NODES / width).max(1);
    let ctx = Context::new();

    let mut subscriber_a = Vec::with_capacity(topics * width);
    for _ in 0..topics {
        let topic = ctx.cell(0u64);
        for i in 0..width {
            let slot = ctx.computed(move |ctx| ctx.get_cell(&topic) + i as u64);
            ctx.get(&slot);
            subscriber_a.push(slot);
        }
    }

    let start = Instant::now();
    for slot in &subscriber_a {
        ctx.dispose_slot(slot);
    }
    let elapsed = start.elapsed().as_nanos() as f64;

    elapsed / subscriber_a.len() as f64
}

/// Teardown-during-flush shape control: dispose many effects while the pending
/// queue is *non-empty*.
///
/// `dispose_effect` scans `pending_effects` to drop the disposed id. With an
/// empty queue that scan is free (`Vec::retain` walks `len`, not capacity), so
/// disposing after a flush settles measures nothing. The shape that bites is
/// mass teardown *during* a flush, which is what a teardown scope does.
///
/// Per topic: a disposer effect is created FIRST, so it is queued first and
/// pops first, while the queue still holds all `width - 1` victims behind it.
/// Its body then disposes every victim against that full queue.
///
/// Total disposals are held fixed at ~`TOTAL_NODES`; only fan-out width varies.
fn dispose_during_flush_rung(width: usize) -> f64 {
    use std::cell::{Cell as StdCell, RefCell};
    use std::rc::Rc;

    let topics = (TOTAL_NODES / width).max(1);
    let ctx = Context::new();

    let mut topic_a = Vec::with_capacity(topics);
    for _ in 0..topics {
        let topic = ctx.cell(0u64);
        let all_a: Rc<RefCell<Vec<lazily::Effect>>> = Rc::new(RefCell::new(Vec::new()));
        let armed = Rc::new(StdCell::new(false));
        let done = Rc::new(StdCell::new(false));

        // Creation order does NOT determine flush order — the dependents list
        // is swap-removed and reordered, so a "disposer created first" does not
        // reliably pop first (an earlier version of this harness measured a
        // queue depth of 0 at every rung for exactly that reason). Instead
        // EVERY effect is a disposer, and whichever one the flush happens to
        // pop first tears down all the others. That one runs with the full
        // remaining queue behind it, which is the shape under test.
        for i in 0..width {
            let all_inner = Rc::clone(&all_a);
            let armed_inner = Rc::clone(&armed);
            let done_inner = Rc::clone(&done);
            let handle = ctx.effect(move |ctx| {
                std::hint::black_box(ctx.get_cell(&topic));
                if armed_inner.get() && !done_inner.get() {
                    done_inner.set(true);
                    let victim_a: Vec<_> = all_inner.borrow().clone();
                    for (j, victim) in victim_a.iter().enumerate() {
                        if j != i {
                            ctx.dispose_effect(victim);
                        }
                    }
                }
            });
            all_a.borrow_mut().push(handle);
        }
        armed.set(true);
        topic_a.push(topic);
    }

    let disposals = topics * width.saturating_sub(1).max(1);
    let start = Instant::now();
    for (revision, topic) in topic_a.iter().enumerate() {
        ctx.set_cell(topic, revision as u64 + 1);
    }
    let elapsed = start.elapsed().as_nanos() as f64;

    elapsed / disposals as f64
}

#[cfg(feature = "thread-safe")]
mod thread_safe_audit {
    use super::{TOTAL_NODES, WIDTHS};
    use lazily::ThreadSafeContext;
    use std::time::Instant;

    fn effect_shape_rung(width: usize) -> f64 {
        let topics = (TOTAL_NODES / width).max(1);
        let ctx = ThreadSafeContext::new();

        let topic_a: Vec<_> = (0..topics).map(|_| ctx.cell_copy(0u64)).collect();
        for topic in &topic_a {
            let topic = *topic;
            for _ in 0..width {
                ctx.effect(move |ctx| {
                    std::hint::black_box(ctx.get_cell(&topic));
                });
            }
        }

        let start = Instant::now();
        for (revision, topic) in topic_a.iter().enumerate() {
            ctx.set_cell(topic, revision as u64 + 1);
        }
        let elapsed = start.elapsed().as_nanos() as f64;

        elapsed / (topics * width) as f64
    }

    /// Mirror of `dispose_during_flush_rung` over `ThreadSafeContext`, whose
    /// `dispose_effect` carried the same unguarded queue scan behind the
    /// non-default `thread-safe` feature.
    fn dispose_during_flush_rung(width: usize) -> f64 {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};

        let topics = (TOTAL_NODES / width).max(1);
        let ctx = ThreadSafeContext::new();

        let mut topic_a = Vec::with_capacity(topics);
        for _ in 0..topics {
            let topic = ctx.cell_copy(0u64);
            let all_a: Arc<Mutex<Vec<lazily::Effect>>> = Arc::new(Mutex::new(Vec::new()));
            let armed = Arc::new(AtomicBool::new(false));
            let done = Arc::new(AtomicBool::new(false));

            for i in 0..width {
                let all_inner = Arc::clone(&all_a);
                let armed_inner = Arc::clone(&armed);
                let done_inner = Arc::clone(&done);
                let handle = ctx.effect(move |ctx| {
                    std::hint::black_box(ctx.get_cell(&topic));
                    if armed_inner.load(Ordering::Relaxed)
                        && !done_inner.swap(true, Ordering::Relaxed)
                    {
                        let victim_a: Vec<_> = all_inner.lock().unwrap().clone();
                        for (j, victim) in victim_a.iter().enumerate() {
                            if j != i {
                                ctx.dispose_effect(victim);
                            }
                        }
                    }
                });
                all_a.lock().unwrap().push(handle);
            }
            armed.store(true, Ordering::Relaxed);
            topic_a.push(topic);
        }

        let disposals = topics * width.saturating_sub(1).max(1);
        let start = Instant::now();
        for (revision, topic) in topic_a.iter().enumerate() {
            ctx.set_cell(topic, revision as u64 + 1);
        }
        let elapsed = start.elapsed().as_nanos() as f64;

        elapsed / disposals as f64
    }

    pub fn run() {
        println!("ThreadSafeContext effect fan-out shape control");
        println!("(total effects fixed at {TOTAL_NODES}; only fan-out width varies)\n");
        println!("{:>10}{:>10}{:>22}", "width", "topics", "notify ns/effect");
        effect_shape_rung(64);
        for width in WIDTHS {
            let ns = effect_shape_rung(width);
            println!("{:>10}{:>10}{:>22.1}", width, TOTAL_NODES / width, ns);
        }

        println!("\nThreadSafeContext dispose-during-flush shape control");
        println!("(total disposals fixed at ~{TOTAL_NODES}; only fan-out width varies)\n");
        println!("{:>10}{:>10}{:>22}", "width", "topics", "dispose ns/effect");
        dispose_during_flush_rung(64);
        for width in WIDTHS {
            let ns = dispose_during_flush_rung(width);
            println!("{:>10}{:>10}{:>22.1}", width, TOTAL_NODES / width, ns);
        }
    }
}

fn main() {
    let thread_safe = std::env::args().any(|arg| arg == "thread-safe");

    if thread_safe {
        #[cfg(feature = "thread-safe")]
        {
            thread_safe_audit::run();
            return;
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            eprintln!("rebuild with --features thread-safe");
            return;
        }
    }

    println!("Context effect fan-out shape control");
    println!("(total effects fixed at {TOTAL_NODES}; only fan-out width varies)\n");
    println!("{:>10}{:>10}{:>22}", "width", "topics", "notify ns/effect");
    effect_shape_rung(64); // warm up
    for width in WIDTHS {
        let ns = effect_shape_rung(width);
        println!("{:>10}{:>10}{:>22.1}", width, TOTAL_NODES / width, ns);
    }

    println!("\nContext dispose-during-flush shape control");
    println!("(total disposals fixed at ~{TOTAL_NODES}; only fan-out width varies)\n");
    println!("{:>10}{:>10}{:>22}", "width", "topics", "dispose ns/effect");
    dispose_during_flush_rung(64); // warm up
    for width in WIDTHS {
        let ns = dispose_during_flush_rung(width);
        #[cfg(audit_probe)]
        {
            let (max, mean, calls) = lazily::audit_probe::take();
            println!(
                "{:>10}{:>10}{:>22.1}   [queue max {max}, mean {mean:.0}, {calls} disposes]",
                width,
                TOTAL_NODES / width,
                ns
            );
        }
        #[cfg(not(audit_probe))]
        println!("{:>10}{:>10}{:>22.1}", width, TOTAL_NODES / width, ns);
    }

    println!("\nContext teardown fan-out shape control");
    println!("(total slots fixed at {TOTAL_NODES}; only fan-out width varies)\n");
    println!("{:>10}{:>10}{:>22}", "width", "topics", "teardown ns/sub");
    teardown_shape_rung(64); // warm up
    for width in WIDTHS {
        let ns = teardown_shape_rung(width);
        println!("{:>10}{:>10}{:>22.1}", width, TOTAL_NODES / width, ns);
    }
}
