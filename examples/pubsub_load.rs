//! In-memory pub/sub: fan-out width ladder, per-subscriber memory, churn soak.
//!
//! ```sh
//! cargo run --release --example pubsub_load
//! LAZILY_PUBSUB_MAX_WIDTH=100000000 cargo run --release --example pubsub_load
//! ```
//!
//! `benches/scale.rs` measures scale as node *count* — 1M cells — but fixes the
//! graph shape at fan-in 2 / fan-out 2. Width is never a variable, so nothing in
//! that suite can see a cost that grows with the number of dependents on a
//! single node. This measures that axis, in the shape a broadcast hub produces:
//!
//!   topic       a cell
//!   subscriber  a slot reading that cell
//!   publish     set_cell, then every subscriber reads through
//!
//! Questions, each with an answer that could embarrass the library:
//!
//! 1. **Is per-subscriber cost flat as width grows?** If it climbs, edge
//!    bookkeeping is still superlinear somewhere.
//! 2. **Where is the real crossover?** `EDGE_INDEX_THRESHOLD` is 32, a figure
//!    taken from a JS measurement. The ladder straddles it closely so the Rust
//!    crossover is visible rather than assumed.
//! 3. **How wide can one topic actually get?** Registration is what bounds this.
//!    An unconditional linear-scan dedup makes building a width-N topic O(N^2) —
//!    at 100M that is ~5e15 comparisons, i.e. it never finishes. With the index
//!    it is N amortized-O(1) inserts.
//! 4. **Does memory return to baseline after churn?** Subscribe/unsubscribe
//!    recycles ids through a LIFO free list and adds and removes index entries.
//!
//! The ladder climbs, measuring bytes/subscriber at each rung and extrapolating
//! before attempting the next, and stops when the projection would not fit in
//! available memory. Deliberately NOT measured: throughput/sec and latency
//! percentiles — those describe a service with a queue in front of it, and
//! criterion measures per-operation cost here more accurately anyway.

use std::time::Instant;

use lazily::Context;

/// Rungs, smallest first. Straddles EDGE_INDEX_THRESHOLD (32) closely, then
/// climbs by ~4-16x so each step's projection is informed by a real measurement.
const LADDER: [usize; 13] = [
    32,
    64,
    96,
    128,
    129,
    160,
    256,
    1024,
    4096,
    65_536,
    1_000_000,
    10_000_000,
    100_000_000,
];
/// Default ceiling. The big rungs are opt-in via LAZILY_PUBSUB_MAX_WIDTH.
const DEFAULT_MAX_WIDTH: usize = 65_536;
/// Subscriber-notifications per rung, so narrow rows do enough work to time.
/// Wide rungs fall back to a single publish, which by then takes seconds.
const NOTIFICATION_BUDGET: usize = 400_000;
/// Refuse to build if the projection would leave less than this free. A wrong
/// guess at 100M takes the machine down, so this aborts instead.
const DEFAULT_MEM_FLOOR_GIB: f64 = 16.0;
/// Churn soak.
const CHURN_CYCLES: usize = 200_000;
const CHURN_LIVE_WIDTH: usize = 64;

struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % bound as u64) as usize
    }
}

fn meminfo_kib(key: &str) -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    meminfo.lines().find_map(|line| {
        line.strip_prefix(key)?
            .trim_start_matches(':')
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
        })
        .unwrap_or(0)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(default)
}

struct Rung {
    build_ns_each: f64,
    notify_ns_each: f64,
    /// Cost of a publish nobody reads, per publish (not per subscriber).
    ///
    /// Once the graph is dirty, marking early-outs per node, so repeated
    /// unread publishes isolate the O(N) work `set_cell` does regardless of
    /// whether anyone is listening — currently a full clone of the dependent
    /// list. If this column grows with width, that clone is real.
    unread_publish_ns: f64,
    bytes_each: f64,
}

/// Build a width-N topic, publish into it, and report per-subscriber costs.
fn sweep_width(width: usize) -> Rung {
    let rss_before = rss_kib();
    let ctx = Context::new();
    let topic = ctx.cell(0u64);

    // Build is the interesting half: every subscriber registers an edge on the
    // topic's dependent list, which is where an O(N) dedup would make this O(N^2).
    let build_start = Instant::now();
    let subscriber_a: Vec<_> = (0..width)
        .map(|i| {
            let slot = ctx.computed(move |ctx| ctx.get_cell(&topic) + i as u64);
            ctx.get(&slot);
            slot
        })
        .collect();
    let build_ns_each = build_start.elapsed().as_nanos() as f64 / width as f64;
    let rss_after_build = rss_kib();

    let publishes = (NOTIFICATION_BUDGET / width).max(1);
    let notify_start = Instant::now();
    for publish in 1..=publishes {
        ctx.set_cell(&topic, publish as u64);
        for slot in &subscriber_a {
            std::hint::black_box(ctx.get(slot));
        }
    }
    let notify_ns_each = notify_start.elapsed().as_nanos() as f64 / (publishes * width) as f64;

    // Unread publishes: the graph is already dirty from here on, so per-node
    // marking early-outs and what remains is the fixed O(N) work set_cell does
    // whether or not anyone is listening.
    const UNREAD_PUBLISHES: usize = 200;
    ctx.set_cell(&topic, 0);
    let unread_start = Instant::now();
    for publish in 1..=UNREAD_PUBLISHES {
        ctx.set_cell(&topic, (publish + 1_000_000) as u64);
    }
    let unread_publish_ns = unread_start.elapsed().as_nanos() as f64 / UNREAD_PUBLISHES as f64;

    Rung {
        build_ns_each,
        notify_ns_each,
        unread_publish_ns,
        bytes_each: (rss_after_build.saturating_sub(rss_before)) as f64 * 1024.0 / width as f64,
    }
}

fn churn_soak() {
    let ctx = Context::new();
    let topic = ctx.cell(0u64);
    let mut subscriber_a: Vec<_> = (0..CHURN_LIVE_WIDTH)
        .map(|i| {
            let slot = ctx.computed(move |ctx| ctx.get_cell(&topic) + i as u64);
            ctx.get(&slot);
            slot
        })
        .collect();
    ctx.set_cell(&topic, 1);
    for slot in &subscriber_a {
        std::hint::black_box(ctx.get(slot));
    }
    let rss_baseline = rss_kib();

    let mut rng = Rng(0x5EED);
    let start = Instant::now();
    for cycle in 0..CHURN_CYCLES {
        let victim = rng.below(subscriber_a.len());
        subscriber_a.swap_remove(victim);
        let salt = cycle as u64;
        let slot = ctx.computed(move |ctx| ctx.get_cell(&topic) + salt);
        ctx.get(&slot);
        subscriber_a.push(slot);
        if cycle % 64 == 0 {
            ctx.set_cell(&topic, cycle as u64);
            for slot in &subscriber_a {
                std::hint::black_box(ctx.get(slot));
            }
        }
    }
    let elapsed = start.elapsed();
    let rss_after = rss_kib();

    // A stale index entry — an edge naming a recycled id — surfaces as a missed
    // update, not a crash.
    let sentinel = 9_999_999u64;
    ctx.set_cell(&topic, sentinel);
    for slot in &subscriber_a {
        let observed = ctx.get(slot);
        assert!(
            observed >= sentinel,
            "subscriber missed a publish after churn: {observed} < {sentinel}"
        );
    }

    println!(
        "\nchurn soak: {CHURN_CYCLES} cycles at width {CHURN_LIVE_WIDTH} in {:.2}s ({:.0} ns/cycle)",
        elapsed.as_secs_f64(),
        elapsed.as_nanos() as f64 / CHURN_CYCLES as f64,
    );
    println!(
        "rss {:.1} -> {:.1} MiB ({:+.2} MiB, {:.2} bytes/cycle); all {} survivors saw the final publish",
        rss_baseline as f64 / 1024.0,
        rss_after as f64 / 1024.0,
        (rss_after as f64 - rss_baseline as f64) / 1024.0,
        (rss_after.saturating_sub(rss_baseline)) as f64 * 1024.0 / CHURN_CYCLES as f64,
        subscriber_a.len(),
    );
}

fn main() {
    let max_width = env_usize("LAZILY_PUBSUB_MAX_WIDTH", DEFAULT_MAX_WIDTH);
    let mem_floor_gib = std::env::var("LAZILY_PUBSUB_MEM_FLOOR_GIB")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_MEM_FLOOR_GIB);

    let total_gib = meminfo_kib("MemTotal").unwrap_or(0) as f64 / 1024.0 / 1024.0;
    println!(
        "fan-out ladder to width {max_width} (LAZILY_PUBSUB_MAX_WIDTH), \
         {total_gib:.0} GiB total, floor {mem_floor_gib:.0} GiB"
    );
    println!("EDGE_INDEX_THRESHOLD is 32; per-subscriber cost should stay flat across it\n");
    println!(
        "{:>12}{:>14}{:>14}{:>18}{:>12}{:>12}",
        "width", "build ns/sub", "notify ns/sub", "unread pub ns", "bytes/sub", "est total"
    );

    sweep_width(8); // warm up: don't charge the first rung for lazy init

    let mut bytes_each_last: Option<f64> = None;
    for width in LADDER {
        if width > max_width {
            println!(
                "\nstopped at the LAZILY_PUBSUB_MAX_WIDTH ceiling; \
                 rerun with LAZILY_PUBSUB_MAX_WIDTH={width} to continue"
            );
            break;
        }

        // Project this rung from the last measured per-subscriber cost, and
        // refuse rather than OOM. The first rungs are too small to matter.
        if let Some(bytes_each) = bytes_each_last {
            let projected_gib = bytes_each * width as f64 / 1024.0 / 1024.0 / 1024.0;
            let available_gib = meminfo_kib("MemAvailable").unwrap_or(0) as f64 / 1024.0 / 1024.0;
            if projected_gib > available_gib - mem_floor_gib {
                println!(
                    "\nstopping before width {width}: projected {projected_gib:.1} GiB \
                     from the last rung's {bytes_each:.0} B/subscriber, \
                     but only {available_gib:.1} GiB available with a {mem_floor_gib:.0} GiB floor"
                );
                break;
            }
        }

        let rung = sweep_width(width);
        let total_mib = rung.bytes_each * width as f64 / 1024.0 / 1024.0;
        println!(
            "{:>12}{:>14.1}{:>14.1}{:>18.0}{:>12.0}{:>10.0} MiB",
            width,
            rung.build_ns_each,
            rung.notify_ns_each,
            rung.unread_publish_ns,
            rung.bytes_each,
            total_mib
        );
        bytes_each_last = Some(rung.bytes_each.max(1.0));
    }

    churn_soak();
}
