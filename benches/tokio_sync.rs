#![cfg(all(feature = "tokio", feature = "thread-safe"))]

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use lazily::ThreadSafeContext;
use tokio::runtime::Runtime;

const TOKIO_SYNC_WORKERS: [usize; 3] = [1, 4, 16];
const TOKIO_SYNC_ITERS: usize = 64;
const TOKIO_SYNC_BATCH_CELLS: usize = 32;

fn rt(workers: usize) -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap()
}

fn bench_tokio_sync_cached_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_sync_cached_read");

    group.bench_function("single_task", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = ThreadSafeContext::new();
                let cell = ctx.source(21usize);
                let slot = ctx.computed(move |ctx| ctx.get(&cell) * 2);
                let _ = ctx.get(&slot);
                black_box(ctx.get(&slot))
            })
        });
    });

    group.bench_function("spawn_read", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = Arc::new(ThreadSafeContext::new());
                let cell = ctx.source(21usize);
                let slot = ctx.computed(move |ctx| ctx.get(&cell) * 2);
                let _ = ctx.get(&slot);
                let ctx_c = ctx.clone();
                tokio::spawn(async move { black_box(ctx_c.get(&slot)) })
                    .await
                    .unwrap()
            })
        });
    });

    group.finish();
}

fn bench_tokio_sync_cold_first_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_sync_cold_first_get");

    group.bench_function("single_task", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = ThreadSafeContext::new();
                let cell = ctx.source(21usize);
                let slot = ctx.computed(move |ctx| ctx.get(&cell) * 2);
                black_box(ctx.get(&slot))
            })
        });
    });

    group.bench_function("spawn_compute", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = Arc::new(ThreadSafeContext::new());
                let cell = ctx.source(21usize);
                let slot = ctx.computed(move |ctx| ctx.get(&cell) * 2);
                let ctx_c = ctx.clone();
                tokio::spawn(async move { black_box(ctx_c.get(&slot)) })
                    .await
                    .unwrap()
            })
        });
    });

    group.finish();
}

fn bench_tokio_sync_invalidation(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_sync_invalidation");

    group.bench_function("single_task", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = ThreadSafeContext::new();
                let cell = ctx.source(0usize);
                let slot = ctx.computed(move |ctx| ctx.get(&cell).wrapping_add(1));
                let _ = ctx.get(&slot);
                let mut sum = 0usize;
                for i in 0..TOKIO_SYNC_ITERS {
                    ctx.set(&cell, black_box(i));
                    sum = sum.wrapping_add(ctx.get(&slot));
                }
                black_box(sum)
            })
        });
    });

    group.finish();
}

fn bench_tokio_sync_concurrent_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_sync_concurrent_contention");
    group.sample_size(10);

    for workers in TOKIO_SYNC_WORKERS {
        group.bench_with_input(
            BenchmarkId::new("same_slot_write_read", workers),
            &workers,
            |b, &workers| {
                let rt = rt(workers);
                b.iter(move || {
                    rt.block_on(async move {
                        let ctx = Arc::new(ThreadSafeContext::new());
                        let cell = ctx.source(0usize);
                        let slot = ctx.computed(move |ctx| ctx.get(&cell).wrapping_add(1));
                        let _ = ctx.get(&slot);
                        let mut handles = Vec::with_capacity(workers);
                        for w in 0..workers {
                            let ctx_c = ctx.clone();
                            let cell_c = cell;
                            let slot_c = slot;
                            handles.push(tokio::spawn(async move {
                                let mut sum = 0usize;
                                for i in 0..TOKIO_SYNC_ITERS {
                                    let next = w.wrapping_mul(TOKIO_SYNC_ITERS).wrapping_add(i);
                                    ctx_c.set(&cell_c, black_box(next));
                                    sum = sum.wrapping_add(ctx_c.get(&slot_c));
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
            BenchmarkId::new("independent_slots", workers),
            &workers,
            |b, &workers| {
                let rt = rt(workers);
                b.iter(move || {
                    rt.block_on(async move {
                        let ctx = Arc::new(ThreadSafeContext::new());
                        let mut total = 0usize;
                        let mut handles = Vec::with_capacity(workers);
                        for w in 0..workers {
                            let ctx_c = ctx.clone();
                            handles.push(tokio::spawn(async move {
                                let cell = ctx_c.source(w);
                                let slot =
                                    ctx_c.computed(move |ctx| ctx.get(&cell).wrapping_add(1));
                                let mut sum = 0usize;
                                for i in 0..TOKIO_SYNC_ITERS {
                                    ctx_c.set(&cell, black_box(i));
                                    sum = sum.wrapping_add(ctx_c.get(&slot));
                                }
                                sum
                            }));
                        }
                        for h in handles {
                            total = total.wrapping_add(h.await.unwrap());
                        }
                        black_box(total)
                    })
                });
            },
        );
    }

    group.finish();
}

fn bench_tokio_sync_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_sync_batch");

    group.bench_function("spawn_batch", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = ThreadSafeContext::new();
                let cells: Vec<_> = (0..TOKIO_SYNC_BATCH_CELLS).map(|i| ctx.source(i)).collect();
                let cells_clone = cells.clone();
                let slot = ctx.computed(move |ctx| {
                    cells_clone
                        .iter()
                        .fold(0usize, |s, c| s.wrapping_add(ctx.get(c)))
                });
                let _ = ctx.get(&slot);
                let mut total = 0usize;
                for round in 0..8usize {
                    let base = round.wrapping_mul(TOKIO_SYNC_BATCH_CELLS);
                    ctx.batch(|ctx| {
                        for (i, cell) in cells.iter().enumerate() {
                            ctx.set(cell, black_box(base.wrapping_add(i)));
                        }
                    });
                    total = total.wrapping_add(ctx.get(&slot));
                }
                black_box(total)
            })
        });
    });

    group.finish();
}

fn bench_tokio_sync_effect(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_sync_effect");

    group.bench_function("single_task", |b| {
        let rt = rt(4);
        b.iter(|| {
            rt.block_on(async {
                let ctx = ThreadSafeContext::new();
                let cell = ctx.source(0usize);
                let sink = Arc::new(AtomicUsize::new(0));
                let sink_clone = sink.clone();
                ctx.effect(move |ctx| {
                    sink_clone.store(ctx.get(&cell), Ordering::Relaxed);
                });
                std::thread::sleep(std::time::Duration::from_millis(10));
                let mut sum = 0usize;
                for i in 0..16usize {
                    ctx.set(&cell, black_box(i));
                    sum = sum.wrapping_add(sink.load(Ordering::Relaxed));
                }
                black_box(sum)
            })
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(20);
    targets =
        bench_tokio_sync_cached_read,
        bench_tokio_sync_cold_first_get,
        bench_tokio_sync_invalidation,
        bench_tokio_sync_concurrent_contention,
        bench_tokio_sync_batch,
        bench_tokio_sync_effect
);
criterion_main!(benches);
