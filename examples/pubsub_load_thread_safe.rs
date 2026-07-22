//! Fan-out width ladder over `ThreadSafeContext` (#lzspecedgeindex).
//!
//! `cargo run --release --features thread-safe --example pubsub_load_thread_safe`
//!
//! `ThreadSafeContext` carries its own edge lists, independent of `Context`, so
//! the single-threaded fix does not reach it. This is the same shape as
//! `pubsub_load`, run against the lock-backed graph a concurrent broadcast hub
//! would actually use: one topic cell, N subscriber slots, publish and read
//! through.
//!
//! Build cost per subscriber is the tell. Registration dedups against the
//! topic's dependent list, so an unconditional linear scan makes building a
//! width-N topic O(N^2) — the column climbs with width instead of staying flat.

use std::time::Instant;

use lazily::ThreadSafeContext;

const LADDER: [usize; 9] = [32, 64, 128, 256, 1024, 4096, 16_384, 65_536, 262_144];
const NOTIFICATION_BUDGET: usize = 200_000;

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

fn sweep_width(width: usize) -> (f64, f64, f64) {
    let rss_before = rss_kib();
    let ctx = ThreadSafeContext::new();
    let topic = ctx.source(0u64);

    let build_start = Instant::now();
    let subscriber_a: Vec<_> = (0..width)
        .map(|i| {
            let slot = ctx.computed(move |ctx| ctx.get(&topic) + i as u64);
            ctx.get(&slot);
            slot
        })
        .collect();
    let build_ns_each = build_start.elapsed().as_nanos() as f64 / width as f64;
    let rss_after_build = rss_kib();

    let publishes = (NOTIFICATION_BUDGET / width).max(1);
    let notify_start = Instant::now();
    for publish in 1..=publishes {
        ctx.set(&topic, publish as u64);
        for slot in &subscriber_a {
            std::hint::black_box(ctx.get(slot));
        }
    }
    let notify_ns_each = notify_start.elapsed().as_nanos() as f64 / (publishes * width) as f64;

    (
        build_ns_each,
        notify_ns_each,
        (rss_after_build.saturating_sub(rss_before)) as f64 * 1024.0 / width as f64,
    )
}

fn main() {
    println!("ThreadSafeContext fan-out ladder\n");
    println!(
        "{:>10}{:>16}{:>16}{:>14}",
        "width", "build ns/sub", "notify ns/sub", "bytes/sub"
    );
    sweep_width(64); // warm up
    for width in LADDER {
        let (build_ns, notify_ns, bytes) = sweep_width(width);
        println!("{width:>10}{build_ns:>16.1}{notify_ns:>16.1}{bytes:>14.0}");
    }
}
