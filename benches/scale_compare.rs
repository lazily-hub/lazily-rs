//! #lzscalecompare — cross-library head-to-head on the identical spreadsheet
//! graph shape used by `scale.rs`: `N` input cells + `N` formula slots where
//! `formula[i] = input[i] + input[i-1]`.
//!
//! Compares `lazily` against [`leptos_reactive`] — another **lazy, pull-based
//! memo** system (Leptos 0.6's fine-grained reactivity). Both recompute a formula
//! only when it is read while dirty, so this is a same-family comparison: it
//! isolates per-node runtime overhead and the lazy-pull viewport property rather
//! than comparing a pull model against an eager push one.
//!
//! Run (matched N, default 100_000 = 200k nodes per library so leptos completes
//! in a feasible wall clock; lazily's own 1M/10M numbers live in `scale.rs` /
//! BENCHMARKS.md):
//!
//! ```text
//! cargo bench --features scale-compare --bench scale_compare
//! LAZILY_SCALE_N=250000 cargo bench --features scale-compare --bench scale_compare
//! ```
//!
//! Same four cases as `scale.rs`, run for each library:
//! - `*/build` — construct all `2N` nodes.
//! - `*/cold_full_recalc` — first read of every formula (forces every compute).
//! - `*/viewport_recalc` — edit one input, read only a bounded viewport.
//! - `*/full_recalc_invalidate_all` — touch every input, read every formula.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use lazily::{Computed, Context, Source};
use leptos_reactive::{
    Memo, ReadSignal, RuntimeId, SignalGet, SignalGetUntracked, SignalSet, WriteSignal,
    create_memo, create_runtime, create_signal,
};

fn scale_n() -> usize {
    std::env::var("LAZILY_SCALE_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000)
}

fn viewport_size(n: usize) -> usize {
    std::env::var("LAZILY_SCALE_VIEWPORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000)
        .min(n)
}

// ---------------------------------------------------------------------------
// lazily
// ---------------------------------------------------------------------------

type LazilyGraph = (Context, Vec<Source<i64>>, Vec<Computed<i64>>);

fn lazily_build(n: usize) -> LazilyGraph {
    let ctx = Context::new();
    let mut inputs: Vec<Source<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        inputs.push(ctx.source(i as i64));
    }
    let mut formulas: Vec<Computed<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        let a = inputs[i];
        let b = inputs[i.saturating_sub(1)];
        formulas.push(ctx.computed(move |ctx| ctx.get(&a) + ctx.get(&b)));
    }
    (ctx, inputs, formulas)
}

fn lazily_read_all(ctx: &Context, formulas: &[Computed<i64>]) -> i64 {
    let mut acc = 0i64;
    for f in formulas {
        acc = acc.wrapping_add(ctx.get(f));
    }
    acc
}

// ---------------------------------------------------------------------------
// leptos_reactive — wrapped so the runtime is disposed on drop (untimed by
// criterion's iter_batched teardown), keeping the per-iteration runtime from
// growing without charging dispose cost to the timed routine.
// ---------------------------------------------------------------------------

struct LeptosGraph {
    runtime: RuntimeId,
    inputs_w: Vec<WriteSignal<i64>>,
    formulas: Vec<Memo<i64>>,
    #[allow(dead_code)]
    inputs_r: Vec<ReadSignal<i64>>,
}

impl Drop for LeptosGraph {
    fn drop(&mut self) {
        self.runtime.dispose();
    }
}

fn leptos_build(n: usize) -> LeptosGraph {
    let runtime = create_runtime();
    let mut inputs_r: Vec<ReadSignal<i64>> = Vec::with_capacity(n);
    let mut inputs_w: Vec<WriteSignal<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        let (r, w) = create_signal(i as i64);
        inputs_r.push(r);
        inputs_w.push(w);
    }
    let mut formulas: Vec<Memo<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        let a = inputs_r[i];
        let b = inputs_r[i.saturating_sub(1)];
        // Tracked `.get()` INSIDE the memo so it subscribes to its inputs and is
        // invalidated when they change (untracked here would make the memo never
        // recompute — a false speedup). External reads use `get_untracked()`.
        formulas.push(create_memo(move |_| a.get() + b.get()));
    }
    LeptosGraph {
        runtime,
        inputs_w,
        formulas,
        inputs_r,
    }
}

fn leptos_read_all(formulas: &[Memo<i64>]) -> i64 {
    let mut acc = 0i64;
    for f in formulas {
        acc = acc.wrapping_add(f.get_untracked());
    }
    acc
}

// ---------------------------------------------------------------------------

fn bench_scale_compare(c: &mut Criterion) {
    let n = scale_n();
    let viewport = viewport_size(n);
    let mid = n / 2;
    let lo = mid.saturating_sub(viewport / 2);
    let hi = (lo + viewport).min(n);

    let mut group = c.benchmark_group("scale_compare");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(8));
    group.warm_up_time(Duration::from_secs(1));
    group.throughput(criterion::Throughput::Elements(n as u64));

    // ---- build ----
    group.bench_function("lazily/build", |b| {
        b.iter_batched(
            || (),
            |()| black_box(lazily_build(n)),
            BatchSize::PerIteration,
        );
    });
    group.bench_function("leptos/build", |b| {
        b.iter_batched(
            || (),
            |()| black_box(leptos_build(n)),
            BatchSize::PerIteration,
        );
    });

    // ---- cold_full_recalc ----
    group.bench_function("lazily/cold_full_recalc", |b| {
        b.iter_batched(
            || lazily_build(n),
            |(ctx, _inputs, formulas)| black_box(lazily_read_all(&ctx, &formulas)),
            BatchSize::PerIteration,
        );
    });
    group.bench_function("leptos/cold_full_recalc", |b| {
        b.iter_batched(
            || leptos_build(n),
            |g| black_box(leptos_read_all(&g.formulas)),
            BatchSize::PerIteration,
        );
    });

    // ---- viewport_recalc (persistent warmed graph) ----
    {
        let (ctx, inputs, formulas) = lazily_build(n);
        black_box(lazily_read_all(&ctx, &formulas));
        let tick = std::cell::Cell::new(0i64);
        group.bench_function("lazily/viewport_recalc", |b| {
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
    {
        let g = leptos_build(n);
        black_box(leptos_read_all(&g.formulas));
        let tick = std::cell::Cell::new(0i64);
        group.bench_function("leptos/viewport_recalc", |b| {
            b.iter(|| {
                tick.set(tick.get() + 1);
                g.inputs_w[mid].set(tick.get());
                let mut acc = 0i64;
                for f in &g.formulas[lo..hi] {
                    acc = acc.wrapping_add(f.get_untracked());
                }
                black_box(acc);
            });
        });
    }

    // ---- full_recalc_invalidate_all (persistent warmed graph) ----
    {
        let (ctx, inputs, formulas) = lazily_build(n);
        black_box(lazily_read_all(&ctx, &formulas));
        let tick = std::cell::Cell::new(0i64);
        group.bench_function("lazily/full_recalc_invalidate_all", |b| {
            b.iter(|| {
                tick.set(tick.get() + 1);
                let base = tick.get();
                for (i, cell) in inputs.iter().enumerate() {
                    cell.set(&ctx, base + i as i64);
                }
                black_box(lazily_read_all(&ctx, &formulas));
            });
        });
    }
    {
        let g = leptos_build(n);
        black_box(leptos_read_all(&g.formulas));
        let tick = std::cell::Cell::new(0i64);
        group.bench_function("leptos/full_recalc_invalidate_all", |b| {
            b.iter(|| {
                tick.set(tick.get() + 1);
                let base = tick.get();
                for (i, w) in g.inputs_w.iter().enumerate() {
                    w.set(base + i as i64);
                }
                black_box(leptos_read_all(&g.formulas));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_scale_compare);
criterion_main!(benches);
