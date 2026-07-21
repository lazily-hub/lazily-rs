//! #lzscalebench — rigorous, criterion-backed scale benchmark for large graphs.
//!
//! Models a spreadsheet-shaped graph: `N` input cells plus `N` formula slots,
//! where `formula[i] = input[i] + input[i-1]` (local fan-in, like a column of
//! `=A_i + A_{i-1}`). With the default `N = 1_000_000` that is ~2M reactive
//! nodes. Four criterion benchmarks cover the spreadsheet lifecycle:
//!
//! - `build` — construct all `2N` nodes.
//! - `cold_full_recalc` — first read of every formula (forces every compute).
//! - `viewport_recalc` — edit one input, then read only a bounded viewport; the
//!   lazy-pull win (off-viewport formulas stay dirty and never recompute).
//! - `full_recalc_invalidate_all` — touch every input, then read every formula.
//!
//! Gated behind the `scale-bench` feature so it is skipped by a plain
//! `cargo bench`. It IS included when the feature is enabled (the benchmark
//! generator passes `scale-bench`), producing the `scale` group in BENCHMARKS.md.
//! Run on demand or at a different size:
//!
//! ```text
//! cargo bench --features scale-bench --bench scale
//! LAZILY_SCALE_N=2000000 cargo bench --features scale-bench --bench scale
//! ```

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lazily::{Context, FormulaCell, SourceCell};

type Graph = (Context, Vec<SourceCell<i64>>, Vec<FormulaCell<i64>>);

fn scale_n() -> usize {
    std::env::var("LAZILY_SCALE_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000)
}

fn viewport_size(n: usize) -> usize {
    std::env::var("LAZILY_SCALE_VIEWPORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000)
        .min(n)
}

/// Build the spreadsheet-shaped graph (cells not yet computed).
fn build_graph(n: usize) -> Graph {
    let ctx = Context::new();
    let mut inputs: Vec<SourceCell<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        inputs.push(ctx.cell(i as i64));
    }
    let mut formulas: Vec<FormulaCell<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        let a = inputs[i];
        let b = inputs[i.saturating_sub(1)];
        formulas.push(ctx.computed(move |ctx| ctx.get_cell(&a) + ctx.get_cell(&b)));
    }
    (ctx, inputs, formulas)
}

/// Read every formula once, returning a folded accumulator.
fn read_all(ctx: &Context, formulas: &[FormulaCell<i64>]) -> i64 {
    let mut acc = 0i64;
    for f in formulas {
        acc = acc.wrapping_add(ctx.get(f));
    }
    acc
}

fn bench_scale(c: &mut Criterion) {
    let n = scale_n();
    let viewport = viewport_size(n);

    let mut group = c.benchmark_group("scale");
    // A 1M-node iteration is expensive; statistical rigor with a feasible wall
    // clock means a small sample with a bounded measurement window.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(8));
    group.warm_up_time(Duration::from_secs(1));
    group.throughput(criterion::Throughput::Elements(n as u64));

    // build: construct all 2N nodes from scratch.
    group.bench_function("build", |b| {
        b.iter_batched(
            || (),
            |()| {
                let graph = build_graph(n);
                black_box(graph);
            },
            BatchSize::PerIteration,
        );
    });

    // cold_full_recalc: first read of every formula forces every compute.
    group.bench_function("cold_full_recalc", |b| {
        b.iter_batched(
            || build_graph(n),
            |(ctx, _inputs, formulas)| {
                black_box(read_all(&ctx, &formulas));
            },
            BatchSize::PerIteration,
        );
    });

    // viewport_recalc: build + warm ONCE (the cheap routine would otherwise make
    // criterion rebuild the whole sheet thousands of times). Each iteration edits
    // one input and reads only a viewport-sized window — off-viewport formulas
    // stay dirty and never recompute. The edit value toggles so the PartialEq
    // cell guard does not suppress the invalidation.
    let mid = n / 2;
    let lo = mid.saturating_sub(viewport / 2);
    let hi = (lo + viewport).min(n);
    {
        let (ctx, inputs, formulas) = build_graph(n);
        black_box(read_all(&ctx, &formulas));
        let tick = std::cell::Cell::new(0i64);
        group.bench_function("viewport_recalc", |b| {
            b.iter(|| {
                tick.set(tick.get() + 1);
                inputs[mid].set(&ctx, tick.get());
                let mut acc = 0i64;
                for f in &formulas[lo..hi] {
                    acc = acc.wrapping_add(ctx.get(f));
                }
                black_box(acc);
            });
        });
    }

    // full_recalc_invalidate_all: build + warm ONCE, then each iteration touches
    // every input and reads every formula — a worst-case full-sheet edit. The
    // routine is expensive enough to time directly on a persistent graph.
    {
        let (ctx, inputs, formulas) = build_graph(n);
        black_box(read_all(&ctx, &formulas));
        let tick = std::cell::Cell::new(0i64);
        group.bench_function("full_recalc_invalidate_all", |b| {
            b.iter(|| {
                tick.set(tick.get() + 1);
                let base = tick.get();
                for (i, cell) in inputs.iter().enumerate() {
                    cell.set(&ctx, base + i as i64);
                }
                black_box(read_all(&ctx, &formulas));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_scale);
criterion_main!(benches);
