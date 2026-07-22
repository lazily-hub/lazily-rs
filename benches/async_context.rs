#![cfg(all(feature = "async", feature = "thread-safe"))]

use std::hint::black_box;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use lazily::{AsyncContext, Context, ThreadSafeContext};
use tokio::runtime::Runtime;

const ASYNC_CONTENTION_WORKERS: [usize; 3] = [1, 4, 16];
const ASYNC_CONTENTION_ITERS: usize = 64;
const ASYNC_CANCELLATION_BURST: usize = 32;
const ASYNC_BATCH_CELLS: usize = 32;

fn rt() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}

fn bench_async_cached_resolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_cached_resolve");

    group.bench_function("sync_get", |b| {
        let rt = rt();
        let (ctx, slot) = rt.block_on(async {
            let ctx = AsyncContext::new();
            let cell = ctx.source(21usize);
            let slot = ctx.computed_async(move |ctx| {
                let v = ctx.get(&cell);
                async move { v * 2 }
            });
            let _ = ctx.get_async(&slot).await;
            (ctx, slot)
        });
        b.iter(|| black_box(ctx.get(&slot)));
    });

    group.bench_function("async_context", |b| {
        let rt = rt();
        b.iter(|| {
            rt.block_on(async {
                let ctx = AsyncContext::new();
                let cell = ctx.source(21usize);
                let slot = ctx.computed_async(move |ctx| {
                    let v = ctx.get(&cell);
                    async move { v * 2 }
                });
                let _ = ctx.get_async(&slot).await;
                black_box(ctx.get_async(&slot).await)
            })
        });
    });

    group.bench_function("sync_context_baseline", |b| {
        b.iter(|| {
            let ctx = Context::new();
            let root = ctx.source(21usize);
            let doubled = ctx.computed(move |ctx| ctx.get(&root) * 2);
            black_box(ctx.get(&doubled));
        });
    });

    group.bench_function("thread_safe_context_baseline", |b| {
        b.iter(|| {
            let ctx = ThreadSafeContext::new();
            let root = ctx.source(21usize);
            let doubled = ctx.computed(move |ctx| ctx.get(&root) * 2);
            black_box(ctx.get(&doubled));
        });
    });

    group.finish();
}

fn bench_async_cold_resolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_cold_resolve");

    group.bench_function("async_context", |b| {
        let rt = rt();
        b.iter(|| {
            rt.block_on(async {
                let ctx = AsyncContext::new();
                let cell = ctx.source(21usize);
                let slot = ctx.computed_async(move |ctx| {
                    let v = ctx.get(&cell);
                    async move { v * 2 }
                });
                black_box(ctx.get_async(&slot).await)
            })
        });
    });

    group.bench_function("sync_context_baseline", |b| {
        b.iter_batched(
            || {
                let ctx = Context::new();
                let root = ctx.source(21usize);
                let doubled = ctx.computed(move |ctx| ctx.get(&root) * 2);
                (ctx, doubled)
            },
            |(ctx, doubled)| black_box(ctx.get(black_box(&doubled))),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("thread_safe_context_baseline", |b| {
        b.iter_batched(
            || {
                let ctx = ThreadSafeContext::new();
                let root = ctx.source(21usize);
                let doubled = ctx.computed(move |ctx| ctx.get(&root) * 2);
                (ctx, doubled)
            },
            |(ctx, doubled)| black_box(ctx.get(black_box(&doubled))),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_async_invalidation_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_invalidation_throughput");

    group.bench_function("async_context", |b| {
        let rt = rt();
        b.iter(|| {
            rt.block_on(async {
                let ctx = AsyncContext::new();
                let cell = ctx.source(0usize);
                let slot = ctx.computed_async(move |ctx| {
                    let v = ctx.get(&cell);
                    async move { v.wrapping_add(1) }
                });
                let _ = ctx.get_async(&slot).await;
                let mut sum = 0usize;
                for i in 0..ASYNC_CONTENTION_ITERS {
                    ctx.set(&cell, black_box(i));
                    sum = sum.wrapping_add(ctx.get_async(&slot).await);
                }
                black_box(sum)
            })
        });
    });

    group.bench_function("sync_context_baseline", |b| {
        b.iter(|| {
            let ctx = Context::new();
            let root = ctx.source(0usize);
            let doubled = ctx.computed(move |ctx| ctx.get(&root).wrapping_add(1));
            black_box(ctx.get(&doubled));
            let mut sum = 0usize;
            for i in 0..ASYNC_CONTENTION_ITERS {
                ctx.set(&root, black_box(i));
                sum = sum.wrapping_add(ctx.get(&doubled));
            }
            black_box(sum)
        });
    });

    group.bench_function("thread_safe_context_baseline", |b| {
        b.iter(|| {
            let ctx = ThreadSafeContext::new();
            let root = ctx.source(0usize);
            let doubled = ctx.computed(move |ctx| ctx.get(&root).wrapping_add(1));
            black_box(ctx.get(&doubled));
            let mut sum = 0usize;
            for i in 0..ASYNC_CONTENTION_ITERS {
                ctx.set(&root, black_box(i));
                sum = sum.wrapping_add(ctx.get(&doubled));
            }
            black_box(sum)
        });
    });

    group.finish();
}

fn bench_async_cancellation_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_cancellation_throughput");
    group.sample_size(10);

    group.bench_function("async_invalidate_in_flight", |b| {
        let rt = rt();
        b.iter(|| {
            rt.block_on(async {
                for i in 0..ASYNC_CANCELLATION_BURST {
                    let ctx = Arc::new(AsyncContext::new());
                    let cell = ctx.source(i);
                    let slot = ctx.computed_async(move |ctx| {
                        let _v = ctx.get(&cell);
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            99usize
                        }
                    });
                    let ctx_clone = ctx.clone();
                    let _h = tokio::spawn(async move { ctx_clone.get_async(&slot).await });
                    ctx.set(&cell, i.wrapping_add(1));
                }
            })
        });
    });

    group.finish();
}

fn bench_async_concurrent_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_concurrent_contention");
    group.sample_size(10);

    for workers in ASYNC_CONTENTION_WORKERS {
        group.bench_with_input(
            BenchmarkId::new("async_context", workers),
            &workers,
            |b, &workers| {
                let rt = rt();
                b.iter(move || {
                    rt.block_on(async move {
                        let ctx = Arc::new(AsyncContext::new());
                        let cell = ctx.source(0usize);
                        let slot = ctx.computed_async(move |ctx| {
                            let v = ctx.get(&cell);
                            async move { v.wrapping_add(1) }
                        });
                        let _ = ctx.get_async(&slot).await;
                        let mut handles = Vec::with_capacity(workers);
                        for w in 0..workers {
                            let ctx_c = ctx.clone();
                            let cell_c = cell;
                            let slot_c = slot;
                            handles.push(tokio::spawn(async move {
                                let mut sum = 0usize;
                                for i in 0..ASYNC_CONTENTION_ITERS {
                                    let next =
                                        w.wrapping_mul(ASYNC_CONTENTION_ITERS).wrapping_add(i);
                                    ctx_c.set(&cell_c, black_box(next));
                                    sum = sum.wrapping_add(ctx_c.get_async(&slot_c).await);
                                }
                                sum
                            }));
                        }
                        let mut total = 0usize;
                        for h in handles {
                            total = total.wrapping_add(h.await.unwrap());
                        }
                        black_box(total)
                    })
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("thread_safe_context_baseline", workers),
            &workers,
            |b, &workers| {
                b.iter(move || {
                    let ctx = Arc::new(ThreadSafeContext::new());
                    let root = ctx.source(0usize);
                    let doubled = ctx.computed(move |ctx| ctx.get(&root).wrapping_add(1));
                    black_box(ctx.get(&doubled));
                    let mut handles = Vec::with_capacity(workers);
                    for w in 0..workers {
                        let ctx_c = ctx.clone();
                        let root_c = root;
                        let doubled_c = doubled;
                        handles.push(std::thread::spawn(move || {
                            let mut sum = 0usize;
                            for i in 0..ASYNC_CONTENTION_ITERS {
                                let next = w.wrapping_mul(ASYNC_CONTENTION_ITERS).wrapping_add(i);
                                ctx_c.set(&root_c, black_box(next));
                                sum = sum.wrapping_add(ctx_c.get(&doubled_c));
                            }
                            sum
                        }));
                    }
                    let total = handles
                        .into_iter()
                        .map(|h| h.join().unwrap())
                        .fold(0usize, usize::wrapping_add);
                    black_box(total)
                });
            },
        );
    }

    group.finish();
}

fn bench_async_effect_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_effect_throughput");

    group.bench_function("async_context", |b| {
        let rt = rt();
        b.iter(|| {
            rt.block_on(async {
                let ctx = AsyncContext::new();
                let cell = ctx.source(0usize);
                let sink = Arc::new(AtomicUsize::new(0));
                let sink_clone = sink.clone();
                ctx.effect_async(move |ctx| {
                    let v = ctx.get(&cell);
                    let s = sink_clone.clone();
                    async move {
                        s.store(v, Ordering::Relaxed);
                        None::<fn()>
                    }
                });
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                let mut sum = 0usize;
                for i in 0..16usize {
                    ctx.set(&cell, black_box(i));
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    sum = sum.wrapping_add(sink.load(Ordering::Relaxed));
                }
                black_box(sum)
            })
        });
    });

    group.finish();
}

fn bench_async_batch_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_batch_throughput");

    group.bench_function("async_context", |b| {
        let rt = rt();
        b.iter(|| {
            rt.block_on(async {
                let ctx = AsyncContext::new();
                let cells: Vec<_> = (0..ASYNC_BATCH_CELLS).map(|i| ctx.source(i)).collect();
                let cells_clone = cells.clone();
                let slot = ctx.computed_async(move |ctx| {
                    let sum = cells_clone
                        .iter()
                        .fold(0usize, |s, c| s.wrapping_add(ctx.get(c)));
                    async move { sum }
                });
                let _ = ctx.get_async(&slot).await;
                let mut total = 0usize;
                for round in 0..8usize {
                    let base = round.wrapping_mul(ASYNC_BATCH_CELLS);
                    ctx.batch(|ctx| {
                        for (i, cell) in cells.iter().enumerate() {
                            ctx.set(cell, black_box(base.wrapping_add(i)));
                        }
                    });
                    total = total.wrapping_add(ctx.get_async(&slot).await);
                }
                black_box(total)
            })
        });
    });

    group.bench_function("sync_context_baseline", |b| {
        b.iter(|| {
            let ctx = Context::new();
            let cells: Vec<_> = (0..ASYNC_BATCH_CELLS).map(|i| ctx.source(i)).collect();
            let cells_clone = cells.clone();
            let slot = ctx.computed(move |ctx| {
                cells_clone
                    .iter()
                    .fold(0usize, |s, c| s.wrapping_add(ctx.get(c)))
            });
            black_box(ctx.get(&slot));
            let mut total = 0usize;
            for round in 0..8usize {
                let base = round.wrapping_mul(ASYNC_BATCH_CELLS);
                ctx.batch(|ctx| {
                    for (i, cell) in cells.iter().enumerate() {
                        ctx.set(cell, black_box(base.wrapping_add(i)));
                    }
                });
                total = total.wrapping_add(ctx.get(&slot));
            }
            black_box(total)
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(20);
    targets =
        bench_async_cached_resolve,
        bench_async_cold_resolve,
        bench_async_invalidation_throughput,
        bench_async_cancellation_throughput,
        bench_async_concurrent_contention,
        bench_async_effect_throughput,
        bench_async_batch_throughput
);
criterion_main!(benches);
