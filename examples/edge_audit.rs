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

    pub fn run() {
        println!("ThreadSafeContext effect fan-out shape control");
        println!("(total effects fixed at {TOTAL_NODES}; only fan-out width varies)\n");
        println!("{:>10}{:>10}{:>22}", "width", "topics", "notify ns/effect");
        effect_shape_rung(64);
        for width in WIDTHS {
            let ns = effect_shape_rung(width);
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

    println!("\nContext teardown fan-out shape control");
    println!("(total slots fixed at {TOTAL_NODES}; only fan-out width varies)\n");
    println!("{:>10}{:>10}{:>22}", "width", "topics", "teardown ns/sub");
    teardown_shape_rung(64); // warm up
    for width in WIDTHS {
        let ns = teardown_shape_rung(width);
        println!("{:>10}{:>10}{:>22.1}", width, TOTAL_NODES / width, ns);
    }
}
