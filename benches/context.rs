#![cfg(feature = "thread-safe")]

use std::cell::Cell as LocalCell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use lazily::{Context, FormulaCell, SourceCell, ThreadSafeContext};

const FAN_OUT_WIDTHS: [usize; 2] = [32, 256];
const MEMO_CHAIN_DEPTH: usize = 32;
const BATCH_STORM_CELLS: usize = 64;
const THREAD_WORKERS: [usize; 5] = [1, 2, 4, 8, 16];
const CONTENTION_ITERS_PER_WORKER: usize = 128;
const CONTENTION_BATCH_CELLS_PER_WORKER: usize = 4;
const SET_CELL_INVALIDATION_FAN_OUT: usize = 512;
const GRAPH_PROPAGATION_WIDTH: usize = 32;
const GRAPH_PROPAGATION_ITERS_PER_WORKER: usize = 64;

#[derive(Clone, Copy)]
enum ThreadSafeContentionCase {
    SameSlotWriteRead,
    IndependentSlots,
    ReadMostlyWaiters,
    BatchedWriteBursts,
}

#[derive(Clone, Copy)]
enum ThreadSafeEffectContentionCase {
    QueueCoalescing,
    CleanupExecution,
    BatchFlush,
}

#[derive(Clone, Copy)]
enum ThreadSafeGraphPropagationCase {
    EagerFanOutValidation,
    LazyFanOutDirtyEpochs,
    LazyFanInDirtyEpochs,
    BatchedFanInFlush,
}

const THREAD_SAFE_CONTENTION_CASES: [ThreadSafeContentionCase; 4] = [
    ThreadSafeContentionCase::SameSlotWriteRead,
    ThreadSafeContentionCase::IndependentSlots,
    ThreadSafeContentionCase::ReadMostlyWaiters,
    ThreadSafeContentionCase::BatchedWriteBursts,
];

const THREAD_SAFE_EFFECT_CONTENTION_CASES: [ThreadSafeEffectContentionCase; 3] = [
    ThreadSafeEffectContentionCase::QueueCoalescing,
    ThreadSafeEffectContentionCase::CleanupExecution,
    ThreadSafeEffectContentionCase::BatchFlush,
];

const EFFECT_THREAD_WORKERS: [usize; 2] = [8, 16];

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

const THREAD_SAFE_GRAPH_PROPAGATION_CASES: [ThreadSafeGraphPropagationCase; 4] = [
    ThreadSafeGraphPropagationCase::EagerFanOutValidation,
    ThreadSafeGraphPropagationCase::LazyFanOutDirtyEpochs,
    ThreadSafeGraphPropagationCase::LazyFanInDirtyEpochs,
    ThreadSafeGraphPropagationCase::BatchedFanInFlush,
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

impl ThreadSafeEffectContentionCase {
    fn as_str(self) -> &'static str {
        match self {
            ThreadSafeEffectContentionCase::QueueCoalescing => "queue_coalescing",
            ThreadSafeEffectContentionCase::CleanupExecution => "cleanup_execution",
            ThreadSafeEffectContentionCase::BatchFlush => "batch_flush",
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

impl ThreadSafeGraphPropagationCase {
    fn as_str(self) -> &'static str {
        match self {
            ThreadSafeGraphPropagationCase::EagerFanOutValidation => "fan_out_eager_validation",
            ThreadSafeGraphPropagationCase::LazyFanOutDirtyEpochs => "fan_out_lazy_dirty_epochs",
            ThreadSafeGraphPropagationCase::LazyFanInDirtyEpochs => "fan_in_lazy_dirty_epochs",
            ThreadSafeGraphPropagationCase::BatchedFanInFlush => "fan_in_batched_flush",
        }
    }
}

fn setup_context_fan_out(width: usize) -> (Context, SourceCell<usize>, Vec<FormulaCell<usize>>) {
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
) -> (
    ThreadSafeContext,
    SourceCell<usize>,
    Vec<FormulaCell<usize>>,
) {
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

fn setup_context_memo_chain(depth: usize) -> (Context, SourceCell<usize>, FormulaCell<usize>) {
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
) -> (ThreadSafeContext, SourceCell<usize>, FormulaCell<usize>) {
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
) -> (Context, Vec<SourceCell<usize>>, Rc<LocalCell<usize>>) {
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
) -> (ThreadSafeContext, Vec<SourceCell<usize>>, Arc<AtomicUsize>) {
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

fn run_thread_safe_effect_contention(
    case: ThreadSafeEffectContentionCase,
    workers: usize,
) -> usize {
    let ctx = ThreadSafeContext::new();
    match case {
        ThreadSafeEffectContentionCase::QueueCoalescing => {
            run_thread_safe_effect_queue_coalescing(&ctx, workers)
        }
        ThreadSafeEffectContentionCase::CleanupExecution => {
            run_thread_safe_effect_cleanup_execution(&ctx, workers)
        }
        ThreadSafeEffectContentionCase::BatchFlush => {
            run_thread_safe_effect_batch_flush(&ctx, workers)
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

fn run_thread_safe_graph_propagation(
    case: ThreadSafeGraphPropagationCase,
    workers: usize,
) -> usize {
    match case {
        ThreadSafeGraphPropagationCase::EagerFanOutValidation => {
            run_thread_safe_fan_out_eager_validation(workers)
        }
        ThreadSafeGraphPropagationCase::LazyFanOutDirtyEpochs => {
            run_thread_safe_fan_out_lazy_dirty_epochs(workers)
        }
        ThreadSafeGraphPropagationCase::LazyFanInDirtyEpochs => {
            run_thread_safe_fan_in_lazy_dirty_epochs(workers)
        }
        ThreadSafeGraphPropagationCase::BatchedFanInFlush => {
            run_thread_safe_fan_in_batched_flush(workers)
        }
    }
}

fn run_thread_safe_fan_out_eager_validation(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(0usize);
    let slots = (0..GRAPH_PROPAGATION_WIDTH)
        .map(|offset| ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(offset)))
        .collect::<Vec<_>>();
    let effect_slots = slots.clone();
    let sink = Arc::new(AtomicUsize::new(0));
    let sink_for_effect = Arc::clone(&sink);
    let _effect = ctx.effect(move |ctx| {
        let total = effect_slots
            .iter()
            .fold(0usize, |sum, slot| sum.wrapping_add(ctx.get(slot)));
        sink_for_effect.store(total, Ordering::Relaxed);
    });

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let sink = Arc::clone(&sink);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;
                for iter in 0..GRAPH_PROPAGATION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(GRAPH_PROPAGATION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&root, black_box(next));
                    sum = sum.wrapping_add(sink.load(Ordering::Relaxed));
                }
                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("fan-out worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_fan_out_lazy_dirty_epochs(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(0usize);
    let slots = (0..GRAPH_PROPAGATION_WIDTH)
        .map(|offset| ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(offset)))
        .collect::<Vec<_>>();
    for slot in &slots {
        black_box(ctx.get(slot));
    }

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;
                for iter in 0..GRAPH_PROPAGATION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(GRAPH_PROPAGATION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&root, black_box(next));
                    sum = sum.wrapping_add(next);
                }
                sum
            })
        })
        .collect::<Vec<_>>();

    let written = threads
        .into_iter()
        .map(|worker| worker.join().expect("fan-out worker should finish"))
        .fold(0usize, usize::wrapping_add);
    slots
        .iter()
        .fold(written, |sum, slot| sum.wrapping_add(ctx.get(slot)))
}

fn run_thread_safe_fan_in_lazy_dirty_epochs(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let mut roots = Vec::with_capacity(workers * CONTENTION_BATCH_CELLS_PER_WORKER);
    for worker in 0..workers {
        for offset in 0..CONTENTION_BATCH_CELLS_PER_WORKER {
            roots.push(
                ctx.cell(
                    worker
                        .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                        .wrapping_add(offset),
                ),
            );
        }
    }
    let branches = roots
        .iter()
        .map(|root| {
            let root = *root;
            ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(1))
        })
        .collect::<Vec<_>>();
    let total_branches = branches.clone();
    let total = ctx.computed(move |ctx| {
        total_branches
            .iter()
            .fold(0usize, |sum, slot| sum.wrapping_add(ctx.get(slot)))
    });
    black_box(ctx.get(&total));

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let worker_roots = roots[worker * CONTENTION_BATCH_CELLS_PER_WORKER
                ..(worker + 1) * CONTENTION_BATCH_CELLS_PER_WORKER]
                .to_vec();

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;
                for iter in 0..GRAPH_PROPAGATION_ITERS_PER_WORKER {
                    for (offset, root) in worker_roots.iter().enumerate() {
                        let next = worker
                            .wrapping_mul(GRAPH_PROPAGATION_ITERS_PER_WORKER)
                            .wrapping_add(iter)
                            .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                            .wrapping_add(offset);
                        worker_ctx.set_cell(root, black_box(next));
                        sum = sum.wrapping_add(next);
                    }
                }
                sum
            })
        })
        .collect::<Vec<_>>();

    let written = threads
        .into_iter()
        .map(|worker| worker.join().expect("fan-in worker should finish"))
        .fold(0usize, usize::wrapping_add);
    written.wrapping_add(ctx.get(&total))
}

fn run_thread_safe_fan_in_batched_flush(workers: usize) -> usize {
    let ctx = ThreadSafeContext::new();
    let worker_cells = effect_worker_cells(&ctx, workers);
    let all_cells = worker_cells
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<SourceCell<usize>>>();
    let branches = all_cells
        .iter()
        .map(|cell| {
            let cell = *cell;
            ctx.computed(move |ctx| ctx.get_cell(&cell).wrapping_add(1))
        })
        .collect::<Vec<_>>();
    let total_branches = branches.clone();
    let total = ctx.computed(move |ctx| {
        total_branches
            .iter()
            .fold(0usize, |sum, slot| sum.wrapping_add(ctx.get(slot)))
    });
    let sink = Arc::new(AtomicUsize::new(0));
    let sink_for_effect = Arc::clone(&sink);
    let _effect = ctx.effect(move |ctx| {
        sink_for_effect.store(ctx.get(&total), Ordering::Relaxed);
    });

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let cells = worker_cells[worker].clone();
            let sink = Arc::clone(&sink);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;
                for iter in 0..GRAPH_PROPAGATION_ITERS_PER_WORKER {
                    worker_ctx.batch(|ctx| {
                        for (offset, cell) in cells.iter().enumerate() {
                            let next = worker
                                .wrapping_mul(GRAPH_PROPAGATION_ITERS_PER_WORKER)
                                .wrapping_add(iter)
                                .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                                .wrapping_add(offset);
                            ctx.set_cell(cell, black_box(next));
                        }
                    });
                    sum = sum.wrapping_add(sink.load(Ordering::Relaxed));
                }
                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("fan-in worker should finish"))
        .fold(0usize, usize::wrapping_add)
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
        .collect::<Vec<SourceCell<usize>>>();
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

fn effect_worker_cells(ctx: &ThreadSafeContext, workers: usize) -> Vec<Vec<SourceCell<usize>>> {
    (0..workers)
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
        .collect::<Vec<_>>()
}

fn run_thread_safe_effect_queue_coalescing(ctx: &ThreadSafeContext, workers: usize) -> usize {
    let worker_cells = effect_worker_cells(ctx, workers);
    let all_cells = worker_cells
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<SourceCell<usize>>>();
    let effect_runs = Arc::new(AtomicUsize::new(0));
    let sink = Arc::new(AtomicUsize::new(0));
    let effect_runs_for_effect = Arc::clone(&effect_runs);
    let sink_for_effect = Arc::clone(&sink);
    let _effect = ctx.effect(move |ctx| {
        effect_runs_for_effect.fetch_add(1, Ordering::Relaxed);
        let total = all_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        sink_for_effect.store(total, Ordering::Relaxed);
    });

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let cells = worker_cells[worker].clone();
            let sink = Arc::clone(&sink);
            let effect_runs = Arc::clone(&effect_runs);

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
                    sum = sum
                        .wrapping_add(sink.load(Ordering::Relaxed))
                        .wrapping_add(effect_runs.load(Ordering::Relaxed));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("effect worker should finish"))
        .fold(0usize, usize::wrapping_add)
}

fn run_thread_safe_effect_cleanup_execution(ctx: &ThreadSafeContext, workers: usize) -> usize {
    let cells = (0..workers)
        .map(|worker| ctx.cell(worker))
        .collect::<Vec<_>>();
    let cleanup_runs = Arc::new(AtomicUsize::new(0));
    let sink = Arc::new(AtomicUsize::new(0));
    let effect_cells = cells.clone();
    let cleanup_runs_for_effect = Arc::clone(&cleanup_runs);
    let sink_for_effect = Arc::clone(&sink);
    let effect = ctx.effect(move |ctx| {
        let total = effect_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        sink_for_effect.store(total, Ordering::Relaxed);
        let cleanup_runs = Arc::clone(&cleanup_runs_for_effect);
        move || {
            cleanup_runs.fetch_add(1, Ordering::Relaxed);
        }
    });

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let cell = cells[worker];
            let cleanup_runs = Arc::clone(&cleanup_runs);
            let sink = Arc::clone(&sink);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    let next = worker
                        .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                        .wrapping_add(iter);
                    worker_ctx.set_cell(&cell, black_box(next));
                    sum = sum
                        .wrapping_add(sink.load(Ordering::Relaxed))
                        .wrapping_add(cleanup_runs.load(Ordering::Relaxed));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    let total = threads
        .into_iter()
        .map(|worker| worker.join().expect("effect cleanup worker should finish"))
        .fold(0usize, usize::wrapping_add);
    ctx.dispose_effect(&effect);
    total.wrapping_add(cleanup_runs.load(Ordering::Relaxed))
}

fn run_thread_safe_effect_batch_flush(ctx: &ThreadSafeContext, workers: usize) -> usize {
    let worker_cells = effect_worker_cells(ctx, workers);
    let all_cells = worker_cells
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<SourceCell<usize>>>();
    let total = ctx.computed(move |ctx| {
        all_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)))
    });
    let sink = Arc::new(AtomicUsize::new(0));
    let sink_for_effect = Arc::clone(&sink);
    let _effect = ctx.effect(move |ctx| {
        sink_for_effect.store(ctx.get(&total), Ordering::Relaxed);
    });

    let barrier = Arc::new(Barrier::new(workers));
    let threads = (0..workers)
        .map(|worker| {
            let worker_ctx = ctx.clone();
            let worker_barrier = Arc::clone(&barrier);
            let cells = worker_cells[worker].clone();
            let sink = Arc::clone(&sink);

            thread::spawn(move || {
                worker_barrier.wait();
                let mut sum = 0usize;

                for iter in 0..CONTENTION_ITERS_PER_WORKER {
                    worker_ctx.batch(|ctx| {
                        ctx.batch(|ctx| {
                            for (offset, cell) in cells.iter().enumerate() {
                                if offset % 2 == 0 {
                                    let next = worker
                                        .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                                        .wrapping_add(iter)
                                        .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                                        .wrapping_add(offset);
                                    ctx.set_cell(cell, black_box(next));
                                }
                            }
                        });
                        for (offset, cell) in cells.iter().enumerate() {
                            if offset % 2 == 1 {
                                let next = worker
                                    .wrapping_mul(CONTENTION_ITERS_PER_WORKER)
                                    .wrapping_add(iter)
                                    .wrapping_mul(CONTENTION_BATCH_CELLS_PER_WORKER)
                                    .wrapping_add(offset);
                                ctx.set_cell(cell, black_box(next));
                            }
                        }
                    });
                    sum = sum.wrapping_add(sink.load(Ordering::Relaxed));
                }

                sum
            })
        })
        .collect::<Vec<_>>();

    threads
        .into_iter()
        .map(|worker| worker.join().expect("effect batch worker should finish"))
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
        .collect::<Vec<SourceCell<usize>>>();
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

fn bench_thread_safe_effect_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe_effect_contention");
    group.sample_size(10);

    for case in THREAD_SAFE_EFFECT_CONTENTION_CASES {
        for workers in EFFECT_THREAD_WORKERS {
            group.bench_with_input(
                BenchmarkId::new(case.as_str(), workers),
                &(case, workers),
                |b, &(case, workers)| {
                    b.iter(|| {
                        black_box(run_thread_safe_effect_contention(
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

fn bench_thread_safe_graph_propagation(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe_graph_propagation");
    group.sample_size(10);

    for case in THREAD_SAFE_GRAPH_PROPAGATION_CASES {
        for workers in EFFECT_THREAD_WORKERS {
            group.bench_with_input(
                BenchmarkId::new(case.as_str(), workers),
                &(case, workers),
                |b, &(case, workers)| {
                    b.iter(|| {
                        black_box(run_thread_safe_graph_propagation(
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

fn bench_typed_cache_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("typed_cache_reads");

    group.bench_function("context_slot", |b| {
        let ctx = Context::new();
        let cell = ctx.cell(42usize);
        let slot = ctx.computed(move |ctx| ctx.get_cell(&cell));
        black_box(ctx.get(&slot));

        b.iter(|| black_box(ctx.get(black_box(&slot))));
    });

    group.bench_function("context_cell", |b| {
        let ctx = Context::new();
        let cell = ctx.cell(99usize);

        b.iter(|| black_box(ctx.get_cell(black_box(&cell))));
    });

    group.bench_function("thread_safe_slot", |b| {
        let ctx = ThreadSafeContext::new();
        let cell = ctx.cell(42usize);
        let slot = ctx.computed(move |ctx| ctx.get_cell(&cell));
        black_box(ctx.get(&slot));

        b.iter(|| black_box(ctx.get(black_box(&slot))));
    });

    group.bench_function("thread_safe_cell", |b| {
        let ctx = ThreadSafeContext::new();
        let cell = ctx.cell(99usize);

        b.iter(|| black_box(ctx.get_cell(black_box(&cell))));
    });

    group.bench_function("context_rc_slot", |b| {
        let ctx = Context::new();
        let cell = ctx.cell(42usize);
        let slot = ctx.computed(move |ctx| ctx.get_cell(&cell));
        black_box(ctx.get(&slot));

        b.iter(|| black_box(ctx.get_rc(black_box(&slot))));
    });

    group.bench_function("context_rc_cell", |b| {
        let ctx = Context::new();
        let cell = ctx.cell(99usize);

        b.iter(|| black_box(ctx.get_cell_rc(black_box(&cell))));
    });

    // #lzrsgetarc: the `Send + Sync` analog to `get_rc`. The pair below is the
    // honest A/B — `get_arc` is *not* a free win, it trades a `T::clone` for an
    // atomic refcount bump. For a `usize` that is a wash (or worse, since it
    // bypasses the cached-read sidecar); for a value with a heap buffer the
    // clone is an allocation plus a memcpy and the `Arc` is not.
    group.bench_function("thread_safe_arc_slot", |b| {
        let ctx = ThreadSafeContext::new();
        let cell = ctx.cell(42usize);
        let slot = ctx.computed(move |ctx| ctx.get_cell(&cell));
        black_box(ctx.get(&slot));

        b.iter(|| black_box(ctx.get_arc(black_box(&slot))));
    });

    group.bench_function("thread_safe_string_slot", |b| {
        let ctx = ThreadSafeContext::new();
        let cell = ctx.cell(64usize);
        let slot = ctx.computed(move |ctx| "lazily".repeat(ctx.get_cell(&cell)));
        black_box(ctx.get(&slot));

        b.iter(|| black_box(ctx.get(black_box(&slot))));
    });

    group.bench_function("thread_safe_arc_string_slot", |b| {
        let ctx = ThreadSafeContext::new();
        let cell = ctx.cell(64usize);
        let slot = ctx.computed(move |ctx| "lazily".repeat(ctx.get_cell(&cell)));
        black_box(ctx.get(&slot));

        b.iter(|| black_box(ctx.get_arc(black_box(&slot))));
    });

    group.finish();
}

/// Phase-0 demand-driven reader-kinds (`relaycell-backpressure-analysis.md`
/// §5): a `QueueCell` op with **zero subscribers** should collapse toward raw
/// storage cost, because no reader-kind value is derived and no effect is
/// scheduled (the merge cost law, §4.0). The A/B here documents that collapse:
/// `raw_vecdeque_push_pop` is the floor, `unsubscribed_push_pop` should sit near
/// it, and `subscribed_len_push_pop` is the reactive-tax upper bound the
/// unsubscribed path avoids.
fn bench_queue_reactive_shell_overhead(c: &mut Criterion) {
    use lazily::QueueCell;
    use std::collections::VecDeque;

    let mut group = c.benchmark_group("queue_reactive_shell_overhead");

    group.bench_function("raw_vecdeque_push_pop", |b| {
        let mut q: VecDeque<i32> = VecDeque::new();
        b.iter(|| {
            q.push_back(black_box(1));
            black_box(q.pop_front());
        });
    });

    group.bench_function("unsubscribed_push_pop", |b| {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);
        b.iter(|| {
            q.try_push(&ctx, black_box(1)).unwrap();
            black_box(q.try_pop(&ctx).unwrap());
        });
    });

    group.bench_function("subscribed_len_push_pop", |b| {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);
        let seen = Rc::new(LocalCell::new(0usize));
        let _effect = {
            let q = q.clone();
            let seen = Rc::clone(&seen);
            ctx.effect(move |ctx| {
                seen.set(q.len(ctx));
            })
        };
        b.iter(|| {
            q.try_push(&ctx, black_box(1)).unwrap();
            black_box(q.try_pop(&ctx).unwrap());
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(20);
    targets =
        bench_cached_reads,
        bench_queue_reactive_shell_overhead,
        bench_cold_first_get,
        bench_dependency_fan_out,
        bench_set_cell_invalidation,
        bench_memo_equality_suppression,
        bench_effect_flushing,
        bench_batch_storms,
        bench_thread_safe_contention,
        bench_thread_safe_effect_contention,
        bench_thread_safe_graph_propagation,
        bench_typed_cache_reads
);
criterion_main!(benches);
