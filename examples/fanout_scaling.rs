//! Edge-registration scaling by fanout width (#lzspecedgeindex).
//!
//! `cargo run --release --example fanout_scaling`
//!
//! Dedup on dependency-edge registration is a linear scan while a node's degree
//! is small and promotes to a hash index above `EDGE_INDEX_THRESHOLD`. This
//! measures the shape that threshold exists for.
//!
//! Both cases build the same number of nodes and the same number of edges, and
//! perform the same number of writes. The only difference is how those edges are
//! distributed:
//!
//! - `wide` — one cell with `n` dependents, the degree that must not go
//!   quadratic.
//! - `narrow` — `n` cells with one dependent each, the low-degree common case.
//!
//! With an unconditional linear scan, `wide` grows ~4x per doubling of `n` while
//! `narrow` stays flat.

use std::time::Instant;

use lazily::Context;

const SIZES: [usize; 4] = [500, 1000, 2000, 4000];
const WRITES: usize = 10;
const RUNS: usize = 5;

fn median(mut sample_a: Vec<f64>) -> f64 {
    sample_a.sort_by(|a, b| a.partial_cmp(b).expect("no NaN timings"));
    sample_a[sample_a.len() / 2]
}

/// One cell, `n` dependents reading it.
fn wide(n: usize) -> f64 {
    let ctx = Context::new();
    let src = ctx.cell(0_usize);
    let dep_a: Vec<_> = (0..n)
        .map(|i| {
            let slot = ctx.computed(move |ctx| ctx.get(&src) + i);
            ctx.get(&slot);
            slot
        })
        .collect();
    let start = Instant::now();
    for write in 1..=WRITES {
        ctx.set(&src, write);
        for slot in &dep_a {
            std::hint::black_box(ctx.get(slot));
        }
    }
    start.elapsed().as_secs_f64() * 1e3
}

/// `n` cells, one dependent each.
fn narrow(n: usize) -> f64 {
    let ctx = Context::new();
    let pair_a: Vec<_> = (0..n)
        .map(|i| {
            let src = ctx.cell(0_usize);
            let slot = ctx.computed(move |ctx| ctx.get(&src) + i);
            ctx.get(&slot);
            (src, slot)
        })
        .collect();
    let start = Instant::now();
    for write in 1..=WRITES {
        for (src, slot) in &pair_a {
            ctx.set(src, write);
            std::hint::black_box(ctx.get(slot));
        }
    }
    start.elapsed().as_secs_f64() * 1e3
}

fn main() {
    // warm up so the first size is not charged for lazy init
    wide(500);
    narrow(500);

    println!("{:<8}{:>12}{:>12}", "n", "wide (ms)", "narrow (ms)");
    for n in SIZES {
        let wide_ms = median((0..RUNS).map(|_| wide(n)).collect());
        let narrow_ms = median((0..RUNS).map(|_| narrow(n)).collect());
        println!("{n:<8}{wide_ms:>12.2}{narrow_ms:>12.2}");
    }
}
