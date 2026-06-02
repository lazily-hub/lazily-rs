use std::cell::Cell as LocalCell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use lazily::{CellHandle, Context, SlotHandle, ThreadSafeContext};

const FAN_OUT_WIDTHS: [usize; 2] = [32, 256];
const MEMO_CHAIN_DEPTH: usize = 32;
const BATCH_STORM_CELLS: usize = 64;
const THREAD_WORKERS: [usize; 5] = [1, 2, 4, 8, 16];
const CONTENTION_ITERS_PER_WORKER: usize = 128;
const CONTENTION_BATCH_CELLS_PER_WORKER: usize = 4;
const SET_CELL_INVALIDATION_FAN_OUT: usize = 512;

#[derive(Clone, Copy)]
enum ThreadSafeContentionCase {
    SameSlotWriteRead,
    IndependentSlots,
    ReadMostlyWaiters,
    BatchedWriteBursts,
}

const THREAD_SAFE_CONTENTION_CASES: [ThreadSafeContentionCase; 4] = [
    ThreadSafeContentionCase::SameSlotWriteRead,
    ThreadSafeContentionCase::IndependentSlots,
    ThreadSafeContentionCase::ReadMostlyWaiters,
    ThreadSafeContentionCase::BatchedWriteBursts,
];

#[derive(Clone, Copy)]
enum ThreadSafeSetCellInvalidationCase {
    SameSlotContention,
    IndependentSlotContention,
    BatchedWriteBursts,
}

const THREAD_SAFE_SET_CELL_INVALIDATION_CASES: [ThreadSafeSetCellInvalidationCase; 3] = [
    ThreadSafeSetCellInvalidationCase::SameSlotContention,
    ThreadSafeSetCellInvalidationCase::IndependentSlotContention,
    ThreadSafeSetCellInvalidationCase::BatchedWriteBursts,
];

impl ThreadSafeContentionCase {
    fn as_str(self) -> &'static str {
        match self {
            ThreadSafeContentionCase::SameSlotWriteRead => "same_slot_write_read",
            ThreadSafeContentionCase::IndependentSlots => "independent_slots",
            ThreadSafeContentionCase::ReadMostlyWaiters => "read_mostly_waiters",
            ThreadSafeContentionCase::BatchedWriteBursts => "batched_write_bursts",
        }
    }
}

impl ThreadSafeSetCellInvalidationCase {
    fn as_str(self) -> &'static str {
        match self {
            ThreadSafeSetCellInvalidationCase::SameSlotContention => "same_slot_contention",
            ThreadSafeSetCellInvalidationCase::IndependentSlotContention => {
                "independent_slot_contention"
            }
            ThreadSafeSetCellInvalidationCase::BatchedWriteBursts => "batched_write_bursts",
        }
    }
}

fn setup_context_fan_out(width: usize) -> (Context, CellHandle<usize>, Vec<SlotHandle<usize>>) {
    let ctx = Context::new();
    let root = ctx.cell(0usize);
    let slots = (0..width)
        .map(|offset| ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(offset)))
        .collect::<Vec<_>>();

    for slot in &slots {
        black_box(ctx.get(slot));
    }

    (ctx, root, slots)
}

fn setup_thread_safe_fan_out(
    width: usize,
) -> (ThreadSafeContext, CellHandle<usize>, Vec<SlotHandle<usize>>) {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(0usize);
    let slots = (0..width)
        .map(|offset| ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(offset)))
        .collect::<Vec<_>>();

    for slot in &slots {
        black_box(ctx.get(slot));
    }

    (ctx, root, slots)
}

fn setup_context_memo_chain(depth: usize) -> (Context, CellHandle<usize>, SlotHandle<usize>) {
    let ctx = Context::new();
    let root = ctx.cell(0usize);
    let mut tail = ctx.memo(move |ctx| ctx.get_cell(&root) % 2);

    for _ in 0..depth {
        let previous = tail;
        tail = ctx.computed(move |ctx| ctx.get(&previous).wrapping_add(1));
    }

    black_box(ctx.get(&tail));
    (ctx, root, tail)
}

fn setup_thread_safe_memo_chain(
    depth: usize,
) -> (ThreadSafeContext, CellHandle<usize>, SlotHandle<usize>) {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(0usize);
    let mut tail = ctx.memo(move |ctx| ctx.get_cell(&root) % 2);

    for _ in 0..depth {
        let previous = tail;
        tail = ctx.computed(move |ctx| ctx.get(&previous).wrapping_add(1));
    }

    black_box(ctx.get(&tail));
    (ctx, root, tail)
}

fn setup_context_batch_storm(
    cells_len: usize,
) -> (Context, Vec<CellHandle<usize>>, Rc<LocalCell<usize>>) {
    let ctx = Context::new();
    let cells = (0..cells_len).map(|idx| ctx.cell(idx)).collect::<Vec<_>>();
    let sink = Rc::new(LocalCell::new(0usize));
    let effect_cells = cells.clone();
    let effect_sink = Rc::clone(&sink);

    let _effect = ctx.effect(move |ctx| {
        let total = effect_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        effect_sink.set(total);
    });

    (ctx, cells, sink)
}

fn setup_thread_safe_batch_storm(
    cells_len: usize,
) -> (ThreadSafeContext, Vec<CellHandle<usize>>, Arc<AtomicUsize>) {
    let ctx = ThreadSafeContext::new();
    let cells = (0..cells_len).map(|idx| ctx.cell(idx)).collect::<Vec<_>>();
    let sink = Arc::new(AtomicUsize::new(0));
    let effect_cells = cells.clone();
    let effect_sink = Arc::clone(&sink);

    let _effect = ctx.effect(move |ctx| {
        let total = effect_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        effect_sink.store(total, Ordering::Relaxed);
    });

    (ctx, cells, sink)
}

fn run_thread_safe_contention(case: ThreadSafeContentionCase, workers: usize) -> usize {
    match case {
        ThreadSafeContentionCase::SameSlotWriteRead => {
            run_thread_safe_same_slot_contention(workers)
        }
        ThreadSafeContentionCase::IndependentSlots => {
            run_thread_safe_independent_slot_contention(workers)
        }
        ThreadSafeContentionCase::ReadMostlyWaiters => {
            run_thread_safe_read_mostly_contention(workers)
        }
        ThreadSafeContentionCase::BatchedWriteBursts => {
            run_thread_safe_batched_write_bursts(workers)
        }
    }
}

fn run_thread_safe_set_cell_invalidation_contention(
    case: ThreadSafeSetCellInvalidationCase,
    workers: usize,
) -> usize {
    match case {
        ThreadSafeSetCellInvalidationCase::SameSlotContention => {
            run_thread_safe_same_slot_set_cell_invalidation(workers)
        }
        ThreadSafeSetCellInvalidationCase::IndependentSlotContention => {
            run_thread_safe_independent_slot_set_cell_invalidation(workers)
        }
        ThreadSafeSetCellInvalidationCase::BatchedWriteBursts => {
            run_thread_safe_batched_set_cell_invalidation(workers)
        }
    }
}

fn run_thread_safe_same_slot_contention(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(1usize);
    let value = ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1));
    black_box(ctx.get(&value));

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&root, black_box(next));
                    sum = sum.wrapping_add(worker_ctx.get(&value));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("contention worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_same_slot_set_cell_invalidation(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(1usize);
    let value = ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1));
    black_box(ctx.get(&value));

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&root, black_box(next));
                    sum = sum.wrapping_add(next);
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    let total = threads
        .into_iter()
        .map(|worker| worker.join().expect("invalidation worker should finish"))
        .fold(0usize, usize::wrapping_add);
    total.wrapping_add(ctx.get_cell(&root))
}

fn run_thread_safe_independent_slot_contention(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let roots = (0..workers)
        .map(|worker| ctx.cell(worker))
        .collect::<Vec<_>>();
    let values = roots
        .iter()
        .map(|root| {
            let root = *root;
            ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1))
        })
        .collect::<Vec<_>>();
    for value in &values {
        black_box(ctx.get(value));
    }

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let root = roots[worker];
            let value = values[worker];

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&root, black_box(next));
                    sum = sum.wrapping_add(worker_ctx.get(&value));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("contention worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_independent_slot_set_cell_invalidation(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let roots = (0..workers)
        .map(|worker| ctx.cell(worker))
        .collect::<Vec<_>>();
    let values = roots
        .iter()
        .map(|root| {
            let root = *root;
            ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1))
        })
        .collect::<Vec<_>>();
    for value in &values {
        black_box(ctx.get(value));
    }

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let root = roots[worker];

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&root, black_box(next));
                    sum = sum.wrapping_add(next);
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("invalidation worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_read_mostly_contention(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(1usize);
    let value = ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1));
    black_box(ctx.get(&value));

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    if worker == 0 {
                        worker_ctx.set_cell(&root, black_box(iter));
                    }
                    sum = sum.wrapping_add(worker_ctx.get(&value));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("contention worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_batched_write_bursts(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let worker_cells = (0..workers)
        .map(|worker| {
            (0..CONTENTION_BATCH_CELLS_PER_WORKER)
                .map(|offset| {
                    ctx.cell(
                        worker
                            .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                            .wrapping_add(offset),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let all_cells = worker_cells
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<CellHandle<usize>>>();
    let total = ctx.computed(move |ctx| {
        all_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)))
    });
    black_box(ctx.get(&total));

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let cells = worker_cells[worker].clone();

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    worker_ctx.batch(|ctx| {
                        for (offset, cell) in cells.iter().enumerate() {
                            let next = worker
                                .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                                .wrapping_add(iter)
                                .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                                .wrapping_add(offset);
                            ctx.set_cell(cell, black_box(next));
                        }
                    });
                    sum = sum.wrapping_add(worker_ctx.get(&total));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("contention worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_batched_set_cell_invalidation(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let worker_cells = (0..workers)
        .map(|worker| {
            (0..CONTENTION_BATCH_CELLS_PER_WORKER)
                .map(|offset| {
                    ctx.cell(
                        worker
                            .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                            .wrapping_add(offset),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let all_cells = worker_cells
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<CellHandle<usize>>>();
    let total = ctx.computed(move |ctx| {
        all_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)))
    });
    black_box(ctx.get(&total));

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let cells = worker_cells[worker].clone();

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    worker_ctx.batch(|ctx| {
                        for (offset, cell) in cells.iter().enumerate() {
                            let next = worker
                                .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                                .wrapping_add(iter)
                                .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                                .wrapping_add(offset);
                            ctx.set_cell(cell, black_box(next));
                            sum = sum.wrapping_add(next);
                        }
                    });
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("invalidation worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn bench_cached_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("cached_reads");

    group.bench_function("context", |b| {
        let ctx = Context::new();
        let root = ctx.cell(21usize);
        let doubled = ctx.computed(move |ctx| ctx.get_cell(&root) * 2);
        black_box(ctx.get(&doubled));

        b.iter(|| black_box(ctx.get(black_box(&doubled))));
    });

    group.bench_function("thread_safe_context", |b| {
        let ctx = ThreadSafeContext::new();
        let root = ctx.cell(21usize);
        let doubled = ctx.computed(move |ctx| ctx.get_cell(&root) * 2);
        black_box(ctx.get(&doubled));

        b.iter(|| black_box(ctx.get(black_box(&doubled))));
    });

    group.finish();
}

fn bench_cold_first_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_first_get");

    group.bench_function("context", |b| {
        b.iter_batched(
            || {
                let ctx = Context::new();
                let root = ctx.cell(21usize);
                let doubled = ctx.computed(move |ctx| ctx.get_cell(&root) * 2);
                (ctx, doubled)
            },
            |(ctx, doubled)| black_box(ctx.get(black_box(&doubled))),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("thread_safe_context", |b| {
        b.iter_batched(
            || {
                let ctx = ThreadSafeContext::new();
                let root = ctx.cell(21usize);
                let doubled = ctx.computed(move |ctx| ctx.get_cell(&root) * 2);
                (ctx, doubled)
            },
            |(ctx, doubled)| black_box(ctx.get(black_box(&doubled))),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_dependency_fan_out(c: &mut Criterion) {
    let mut group = c.benchmark_group("dependency_fan_out");

    for width in FAN_OUT_WIDTHS {
        group.bench_with_input(BenchmarkId::new("context", width), &width, |b, &width| {
            b.iter_batched(
                || setup_context_fan_out(width),
                |(ctx, root, slots)| {
                    ctx.set_cell(&root, black_box(1usize));
                    let total = slots
                        .iter()
                        .fold(0usize, |sum, slot| sum.wrapping_add(ctx.get(slot)));
                    black_box(total);
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("thread_safe_context", width),
            &width,
            |b, &width| {
                b.iter_batched(
                    || setup_thread_safe_fan_out(width),
                    |(ctx, root, slots)| {
                        ctx.set_cell(&root, black_box(1usize));
                        let total = slots
                            .iter()
                            .fold(0usize, |sum, slot| sum.wrapping_add(ctx.get(slot)));
                        black_box(total);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_set_cell_invalidation(c: &mut Criterion) {
    let mut group = c.benchmark_group("set_cell_invalidation");
    group.sample_size(10);

    group.bench_with_input(
        BenchmarkId::new("high_fan_out", SET_CELL_INVALIDATION_FAN_OUT),
        &SET_CELL_INVALIDATION_FAN_OUT,
        |b, &width| {
            b.iter_batched(
                || setup_thread_safe_fan_out(width),
                |(ctx, root, slots)| {
                    ctx.set_cell(&root, black_box(1usize));
                    black_box(slots.len());
                },
                BatchSize::SmallInput,
            );
        },
    );

    for case in THREAD_SAFE_SET_CELL_INVALIDATION_CASES {
        for workers in THREAD_WORKERS {
            group.bench_with_input(
                BenchmarkId::new(case.as_str(), workers),
                &(case, workers),
                |b, &(case, workers)| {
                    b.iter(|| {
                        black_box(run_thread_safe_set_cell_invalidation_contention(
                            black_box(case),
                            black_box(workers),
                        ))
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_memo_equality_suppression(c: &mut Criterion) {
    let mut group = c.benchmark_group("memo_equality_suppression");

    group.bench_function("context", |b| {
        b.iter_batched(
            || setup_context_memo_chain(MEMO_CHAIN_DEPTH),
            |(ctx, root, tail)| {
                ctx.set_cell(&root, black_box(2usize));
                black_box(ctx.get(black_box(&tail)));
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("thread_safe_context", |b| {
        b.iter_batched(
            || setup_thread_safe_memo_chain(MEMO_CHAIN_DEPTH),
            |(ctx, root, tail)| {
                ctx.set_cell(&root, black_box(2usize));
                black_box(ctx.get(black_box(&tail)));
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_effect_flushing(c: &mut Criterion) {
    let mut group = c.benchmark_group("effect_flushing");

    group.bench_function("context", |b| {
        let ctx = Context::new();
        let root = ctx.cell(0usize);
        let seen = Rc::new(LocalCell::new(0usize));
        let effect_seen = Rc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            effect_seen.set(effect_seen.get().wrapping_add(ctx.get_cell(&root)));
        });

        let mut next = 0usize;
        b.iter(|| {
            next = next.wrapping_add(1);
            ctx.set_cell(&root, black_box(next));
            black_box(seen.get());
        });
    });

    group.bench_function("thread_safe_context", |b| {
        let ctx = ThreadSafeContext::new();
        let root = ctx.cell(0usize);
        let seen = Arc::new(AtomicUsize::new(0));
        let effect_seen = Arc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            effect_seen.fetch_add(ctx.get_cell(&root), Ordering::Relaxed);
        });

        let mut next = 0usize;
        b.iter(|| {
            next = next.wrapping_add(1);
            ctx.set_cell(&root, black_box(next));
            black_box(seen.load(Ordering::Relaxed));
        });
    });

    group.finish();
}

fn bench_batch_storms(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_storms");

    group.bench_function(BenchmarkId::new("context", BATCH_STORM_CELLS), |b| {
        let (ctx, cells, sink) = setup_context_batch_storm(BATCH_STORM_CELLS);
        let mut base = BATCH_STORM_CELLS;

        b.iter(|| {
            base = base.wrapping_add(BATCH_STORM_CELLS);
            ctx.batch(|ctx| {
                for (offset, cell) in cells.iter().enumerate() {
                    ctx.set_cell(cell, black_box(base.wrapping_add(offset)));
                }
            });
            black_box(sink.get());
        });
    });

    group.bench_function(
        BenchmarkId::new("thread_safe_context", BATCH_STORM_CELLS),
        |b| {
            let (ctx, cells, sink) = setup_thread_safe_batch_storm(BATCH_STORM_CELLS);
            let mut base = BATCH_STORM_CELLS;

            b.iter(|| {
                base = base.wrapping_add(BATCH_STORM_CELLS);
                ctx.batch(|ctx| {
                    for (offset, cell) in cells.iter().enumerate() {
                        ctx.set_cell(cell, black_box(base.wrapping_add(offset)));
                    }
                });
                black_box(sink.load(Ordering::Relaxed));
            });
        },
    );

    group.finish();
}

fn bench_thread_safe_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe_contention");
    group.sample_size(10);

    for case in THREAD_SAFE_CONTENTION_CASES {
        for workers in THREAD_WORKERS {
            group.bench_with_input(
                BenchmarkId::new(case.as_str(), workers),
                &(case, workers),
                |b, &(case, workers)| {
                    b.iter(|| {
                        black_box(run_thread_safe_contention(
                            black_box(case),
                            black_box(workers),
                        ))
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(20);
    targets =
        bench_cached_reads,
        bench_cold_first_get,
        bench_dependency_fan_out,
        bench_set_cell_invalidation,
        bench_memo_equality_suppression,
        bench_effect_flushing,
        bench_batch_storms,
        bench_thread_safe_contention
);
criterion_main!(benches);
