//! #lzscalebench — evidence-backed scale benchmark for >=1M reactive nodes.
//!
//! Models a spreadsheet-shaped graph: `N` input cells plus `N` formula cells,
//! where `formula[i] = input[i] + input[i-1]` (local fan-in, like a column of
//! `=A_i + A_{i-1}`). With the default `N = 1_000_000` that is ~2M reactive
//! nodes. The harness reports build time + resident memory, cold full recalc,
//! warm cached reads, single-input invalidation with a bounded *viewport*
//! recompute (the lazy-pull win), and a full invalidate-everything recalc.
//!
//! This is a hand-rolled timing harness (not criterion) so one real pass runs
//! in a few seconds and prints copy-pasteable numbers. It is gated behind the
//! `scale-bench` feature so the default `cargo bench` / `make benchmark-check`
//! never runs it. Run it explicitly:
//!
//! ```text
//! cargo bench --features scale-bench --bench scale
//! LAZILY_SCALE_N=2000000 cargo bench --features scale-bench --bench scale
//! ```

use std::hint::black_box;
use std::time::Instant;

use lazily::{CellHandle, Context, SlotHandle};

/// Resident set size in bytes (Linux `/proc/self/statm`), or `None` elsewhere.
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = 4096u64; // getconf PAGE_SIZE on x86_64 Linux
    Some(resident_pages * page_size)
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn main() {
    let n: usize = std::env::var("LAZILY_SCALE_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let viewport: usize = std::env::var("LAZILY_SCALE_VIEWPORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000)
        .min(n);

    println!("# lazily scale benchmark (#lzscalebench)");
    println!(
        "N = {n} input cells + {n} formula slots = {} nodes; viewport = {viewport}",
        2 * n
    );

    let rss_start = resident_bytes();

    // -- Build ----------------------------------------------------------------
    let ctx = Context::new();
    let t = Instant::now();
    let mut inputs: Vec<CellHandle<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        inputs.push(ctx.cell(i as i64));
    }
    let mut formulas: Vec<SlotHandle<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        let a = inputs[i];
        let b = inputs[i.saturating_sub(1)];
        formulas.push(ctx.computed(move |ctx| ctx.get_cell(&a) + ctx.get_cell(&b)));
    }
    let build = t.elapsed();
    let rss_built = resident_bytes();

    // -- Cold full recalc (first read forces every formula to compute) --------
    let t = Instant::now();
    let mut acc: i64 = 0;
    for f in &formulas {
        acc = acc.wrapping_add(ctx.get(f));
    }
    black_box(acc);
    let cold = t.elapsed();

    // -- Warm cached reads (no recompute) -------------------------------------
    let t = Instant::now();
    let mut acc2: i64 = 0;
    for f in &formulas {
        acc2 = acc2.wrapping_add(ctx.get(f));
    }
    black_box(acc2);
    let warm = t.elapsed();

    // -- Single-input edit + bounded viewport recompute (the lazy-pull win) ---
    // Edit one input near the middle, then read only a viewport-sized window.
    // Off-viewport formulas stay dirty and are never recomputed.
    let mid = n / 2;
    let t = Instant::now();
    inputs[mid].set(&ctx, 123_456);
    let lo = mid.saturating_sub(viewport / 2);
    let hi = (lo + viewport).min(n);
    let mut accv: i64 = 0;
    for f in &formulas[lo..hi] {
        accv = accv.wrapping_add(ctx.get(f));
    }
    black_box(accv);
    let viewport_recalc = t.elapsed();

    // -- Full invalidate-everything recalc ------------------------------------
    // Touch every input, then read every formula: a worst-case full sheet edit.
    let t = Instant::now();
    for (i, c) in inputs.iter().enumerate() {
        c.set(&ctx, (i as i64) + 1);
    }
    let invalidate = t.elapsed();
    let t = Instant::now();
    let mut acc3: i64 = 0;
    for f in &formulas {
        acc3 = acc3.wrapping_add(ctx.get(f));
    }
    black_box(acc3);
    let full_recalc = t.elapsed();

    // -- Report ---------------------------------------------------------------
    let per = |d: std::time::Duration, count: usize| d.as_secs_f64() * 1e9 / count as f64;
    println!();
    println!("| phase | total | per-node |");
    println!("|---|---:|---:|");
    println!(
        "| build ({} nodes) | {:.3} s | {:.1} ns |",
        2 * n,
        build.as_secs_f64(),
        per(build, 2 * n)
    );
    println!(
        "| cold full recalc ({n}) | {:.3} s | {:.1} ns |",
        cold.as_secs_f64(),
        per(cold, n)
    );
    println!(
        "| warm cached reads ({n}) | {:.3} s | {:.1} ns |",
        warm.as_secs_f64(),
        per(warm, n)
    );
    println!(
        "| 1 input edit + viewport recalc ({viewport}) | {:.3} ms | {:.1} ns |",
        viewport_recalc.as_secs_f64() * 1e3,
        per(viewport_recalc, viewport.max(1))
    );
    println!(
        "| invalidate all inputs ({n}) | {:.3} s | {:.1} ns |",
        invalidate.as_secs_f64(),
        per(invalidate, n)
    );
    println!(
        "| full recalc after invalidate ({n}) | {:.3} s | {:.1} ns |",
        full_recalc.as_secs_f64(),
        per(full_recalc, n)
    );
    if let (Some(start), Some(built)) = (rss_start, rss_built) {
        let delta = built.saturating_sub(start);
        println!();
        println!(
            "memory: RSS {:.0} MiB after build (+{:.0} MiB for {} nodes => ~{:.0} B/node)",
            mib(built),
            mib(delta),
            2 * n,
            delta as f64 / (2 * n) as f64
        );
    }
}
