//! `#lzspecrevisionengine` crossover benchmark: push vs revision on
//! high-fan-out write-heavy workloads.
//!
//! The revision engine trades O(dirty-cone) writes for O(1) writes at the cost
//! of O(changed-subpath) reads. This benchmark measures the crossover:
//! - **High-fan-out write**: cell → N dependent slots. Push walks all N (O(N));
//!   revision bumps a counter (O(1)).
//! - **Subsequent read**: the memo guard suppresses downstream cascade in both
//!   engines when values are equal.

#![allow(dead_code)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use lazily::Context;

const FAN_OUTS: [usize; 4] = [1, 16, 128, 1024];
const WRITES_PER_MEASUREMENT: usize = 10;

fn make_fanout_graph(
    ctx: &Context,
    fanout: usize,
) -> (lazily::SourceCell<i32>, Vec<lazily::FormulaCell<i32>>) {
    let source = ctx.cell(0);
    let slots: Vec<_> = (0..fanout)
        .map(|_| ctx.computed(move |ctx| ctx.get_cell(&source) + 1))
        .collect();
    // Prime: compute all slots once.
    for s in &slots {
        let _ = ctx.get(s);
    }
    (source, slots)
}

fn bench_write_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("revision_write_cost");

    for &fanout in &FAN_OUTS {
        group.bench_with_input(BenchmarkId::new("push", fanout), &fanout, |b, &fanout| {
            b.iter(|| {
                let ctx = Context::new();
                let (source, _) = make_fanout_graph(&ctx, fanout);
                for i in 0..WRITES_PER_MEASUREMENT as i32 {
                    ctx.set_cell(&source, i);
                }
                black_box(&ctx);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("revision", fanout),
            &fanout,
            |b, &fanout| {
                b.iter(|| {
                    let ctx = Context::with_revision_engine();
                    let (source, _) = make_fanout_graph(&ctx, fanout);
                    for i in 0..WRITES_PER_MEASUREMENT as i32 {
                        ctx.set_cell(&source, i);
                    }
                    black_box(&ctx);
                });
            },
        );
    }
    group.finish();
}

fn bench_write_then_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("revision_write_then_read");

    for &fanout in &FAN_OUTS {
        group.bench_with_input(BenchmarkId::new("push", fanout), &fanout, |b, &fanout| {
            b.iter(|| {
                let ctx = Context::new();
                let (source, slots) = make_fanout_graph(&ctx, fanout);
                ctx.set_cell(&source, 42);
                for s in &slots {
                    black_box(ctx.get(s));
                }
            });
        });

        group.bench_with_input(
            BenchmarkId::new("revision", fanout),
            &fanout,
            |b, &fanout| {
                b.iter(|| {
                    let ctx = Context::with_revision_engine();
                    let (source, slots) = make_fanout_graph(&ctx, fanout);
                    ctx.set_cell(&source, 42);
                    for s in &slots {
                        black_box(ctx.get(s));
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_write_cost, bench_write_then_read);
criterion_main!(benches);
