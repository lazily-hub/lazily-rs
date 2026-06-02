use std::cell::Cell as LocalCell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use lazily::{
    CellHandle, Context, InstrumentationSnapshot, THREAD_SAFE_LOCK_SITE_COUNT, ThreadSafeContext,
    ThreadSafeLockSiteSnapshot,
};

const FAN_OUT_WIDTH: usize = 32;
const SET_CELL_INVALIDATION_FAN_OUT: usize = 512;
const BATCH_STORM_CELLS: usize = 64;
const CONTENTION_ITERS_PER_WORKER: usize = 16;
const CONTENTION_WORKERS: [usize; 5] = [1, 2, 4, 8, 16];
const EFFECT_CONTENTION_WORKERS: [usize; 2] = [8, 16];
const CONTENTION_BATCH_CELLS_PER_WORKER: usize = 4;

#[derive(Clone, Copy)]
enum ThreadSafeContentionCase {
    SameSlotWriteRead,
    IndependentSlots,
    ReadMostlyWaiters,
    BatchedWriteBursts,
}

#[derive(Clone, Copy)]
enum ThreadSafeSetCellInvalidationCase {
    SameSlotContention,
    IndependentSlotContention,
    BatchedWriteBursts,
}

#[derive(Clone, Copy)]
enum ThreadSafeEffectContentionCase {
    QueueCoalescing,
    CleanupExecution,
    BatchFlush,
}

const THREAD_SAFE_CONTENTION_CASES: [ThreadSafeContentionCase; 4] = [
    ThreadSafeContentionCase::SameSlotWriteRead,
    ThreadSafeContentionCase::IndependentSlots,
    ThreadSafeContentionCase::ReadMostlyWaiters,
    ThreadSafeContentionCase::BatchedWriteBursts,
];

const THREAD_SAFE_SET_CELL_INVALIDATION_CASES: [ThreadSafeSetCellInvalidationCase; 3] = [
    ThreadSafeSetCellInvalidationCase::SameSlotContention,
    ThreadSafeSetCellInvalidationCase::IndependentSlotContention,
    ThreadSafeSetCellInvalidationCase::BatchedWriteBursts,
];

const THREAD_SAFE_EFFECT_CONTENTION_CASES: [ThreadSafeEffectContentionCase; 3] = [
    ThreadSafeEffectContentionCase::QueueCoalescing,
    ThreadSafeEffectContentionCase::CleanupExecution,
    ThreadSafeEffectContentionCase::BatchFlush,
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

impl ThreadSafeEffectContentionCase {
    fn as_str(self) -> &'static str {
        match self {
            ThreadSafeEffectContentionCase::QueueCoalescing => "queue_coalescing",
            ThreadSafeEffectContentionCase::CleanupExecution => "cleanup_execution",
            ThreadSafeEffectContentionCase::BatchFlush => "batch_flush",
        }
    }
}

fn main() {
    println!(
        "profile,node_allocations,slot_recomputes,duplicate_speculative_recomputes,\
dependency_edges_added,dependency_edges_removed,effect_queue_pushes,\
max_effect_queue_depth,lock_acquisitions,lock_wait_nanos,lock_hold_nanos,lock_attribution"
    );

    emit("context_memo_effect", context_memo_effect());
    emit("context_fan_out_32", context_fan_out());
    emit("context_batch_storm_64", context_batch_storm());
    emit("thread_safe_first_get_2", thread_safe_first_get());
    emit(
        "thread_safe_set_cell_invalidation_high_fan_out_512",
        thread_safe_set_cell_invalidation_high_fan_out(),
    );

    for case in THREAD_SAFE_SET_CELL_INVALIDATION_CASES {
        for workers in CONTENTION_WORKERS {
            emit(
                &format!(
                    "thread_safe_set_cell_invalidation_{}_{workers}",
                    case.as_str()
                ),
                thread_safe_set_cell_invalidation(case, workers),
            );
        }
    }

    for case in THREAD_SAFE_CONTENTION_CASES {
        for workers in CONTENTION_WORKERS {
            emit(
                &format!("thread_safe_contention_{}_{workers}", case.as_str()),
                thread_safe_contention(case, workers),
            );
        }
    }

    for case in THREAD_SAFE_EFFECT_CONTENTION_CASES {
        for workers in EFFECT_CONTENTION_WORKERS {
            emit(
                &format!("thread_safe_effect_contention_{}_{workers}", case.as_str()),
                thread_safe_effect_contention(case, workers),
            );
        }
    }
}

struct ProfileResult {
    snapshot: InstrumentationSnapshot,
    lock_profile: Option<[ThreadSafeLockSiteSnapshot; THREAD_SAFE_LOCK_SITE_COUNT]>,
}

impl From<InstrumentationSnapshot> for ProfileResult {
    fn from(snapshot: InstrumentationSnapshot) -> Self {
        Self {
            snapshot,
            lock_profile: None,
        }
    }
}

fn emit(profile: &str, result: impl Into<ProfileResult>) {
    let result = result.into();
    let snapshot = result.snapshot;
    println!(
        "{},{},{},{},{},{},{},{},{},{},{},{}",
        profile,
        snapshot.node_allocations,
        snapshot.slot_recomputes,
        snapshot.duplicate_speculative_recomputes,
        snapshot.dependency_edges_added,
        snapshot.dependency_edges_removed,
        snapshot.effect_queue_pushes,
        snapshot.max_effect_queue_depth,
        snapshot.lock_acquisitions,
        snapshot.lock_wait_nanos,
        snapshot.lock_hold_nanos,
        format_lock_attribution(result.lock_profile.as_ref())
    );
}

fn format_lock_attribution(
    lock_profile: Option<&[ThreadSafeLockSiteSnapshot; THREAD_SAFE_LOCK_SITE_COUNT]>,
) -> String {
    let Some(lock_profile) = lock_profile else {
        return String::new();
    };

    lock_profile
        .iter()
        .map(|site| {
            format!(
                "{}={}:{}:{}",
                site.site.as_str(),
                site.lock_acquisitions,
                site.lock_wait_nanos,
                site.lock_hold_nanos
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn context_memo_effect() -> InstrumentationSnapshot {
    let ctx = Context::new();
    ctx.reset_instrumentation();

    let root = ctx.cell(0usize);
    let parity = ctx.memo(move |ctx| ctx.get_cell(&root) % 2);
    let label = ctx.computed(move |ctx| ctx.get(&parity).wrapping_add(1));
    let _effect = ctx.effect(move |ctx| {
        black_box(ctx.get(&label));
    });

    ctx.set_cell(&root, 2);
    black_box(ctx.get(&label));

    ctx.instrumentation_snapshot()
}

fn context_fan_out() -> InstrumentationSnapshot {
    let ctx = Context::new();
    ctx.reset_instrumentation();

    let root = ctx.cell(0usize);
    let slots = (0..FAN_OUT_WIDTH)
        .map(|offset| ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(offset)))
        .collect::<Vec<_>>();

    for slot in &slots {
        black_box(ctx.get(slot));
    }

    ctx.set_cell(&root, 1);
    let total = slots
        .iter()
        .fold(0usize, |sum, slot| sum.wrapping_add(ctx.get(slot)));
    black_box(total);

    ctx.instrumentation_snapshot()
}

fn context_batch_storm() -> InstrumentationSnapshot {
    let ctx = Context::new();
    ctx.reset_instrumentation();

    let cells = (0..BATCH_STORM_CELLS)
        .map(|idx| ctx.cell(idx))
        .collect::<Vec<_>>();
    let sink = Rc::new(LocalCell::new(0usize));
    let effect_cells = cells.clone();
    let effect_sink = Rc::clone(&sink);

    let _effect = ctx.effect(move |ctx| {
        let total = effect_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        effect_sink.set(total);
    });

    ctx.batch(|ctx| {
        for (offset, cell) in cells.iter().enumerate() {
            ctx.set_cell(cell, BATCH_STORM_CELLS.wrapping_add(offset));
        }
    });
    black_box(sink.get());

    ctx.instrumentation_snapshot()
}

fn thread_safe_first_get() -> InstrumentationSnapshot {
    let ctx = ThreadSafeContext::new();
    ctx.reset_instrumentation();

    let root = ctx.cell(40usize);
    let answer = ctx.computed(move |ctx| {
        thread::sleep(Duration::from_micros(200));
        ctx.get_cell(&root).wrapping_add(2)
    });
    let barrier = Arc::new(Barrier::new(2));

    let workers = (0..2)
        .map(|_| {
            let ctx = ctx.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                black_box(ctx.get(&answer))
            })
        })
        .collect::<Vec<_>>();

    for worker in workers {
        assert_eq!(worker.join().expect("profile worker should finish"), 42);
    }

    ctx.instrumentation_snapshot()
}

fn thread_safe_set_cell_invalidation_high_fan_out() -> ProfileResult {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(0usize);
    let slots = (0..SET_CELL_INVALIDATION_FAN_OUT)
        .map(|offset| ctx.computed(move |ctx| ctx.get_cell(&root).wrapping_add(offset)))
        .collect::<Vec<_>>();

    for slot in &slots {
        black_box(ctx.get(slot));
    }

    ctx.reset_instrumentation();
    ctx.set_cell(&root, 1);
    black_box(slots.len());

    thread_safe_set_cell_profile_result(ctx)
}

fn thread_safe_set_cell_invalidation(
    case: ThreadSafeSetCellInvalidationCase,
    workers: usize,
) -> ProfileResult {
    match case {
        ThreadSafeSetCellInvalidationCase::SameSlotContention => {
            thread_safe_set_cell_invalidation_profile(
                workers,
                run_thread_safe_same_slot_set_cell_invalidation,
            )
        }
        ThreadSafeSetCellInvalidationCase::IndependentSlotContention => {
            thread_safe_set_cell_invalidation_profile(
                workers,
                run_thread_safe_independent_slot_set_cell_invalidation,
            )
        }
        ThreadSafeSetCellInvalidationCase::BatchedWriteBursts => {
            thread_safe_set_cell_invalidation_profile(
                workers,
                run_thread_safe_batched_set_cell_invalidation,
            )
        }
    }
}

fn thread_safe_set_cell_invalidation_profile(
    workers: usize,
    run: fn(&ThreadSafeContext, usize) -> usize,
) -> ProfileResult {
    let ctx = ThreadSafeContext::new();
    ctx.reset_instrumentation();
    let total = run(&ctx, workers);
    black_box(total);

    thread_safe_set_cell_profile_result(ctx)
}

fn thread_safe_set_cell_profile_result(ctx: ThreadSafeContext) -> ProfileResult {
    let snapshot = ctx.instrumentation_snapshot();
    let lock_profile = ctx.lock_profile_snapshot();
    assert_eq!(snapshot.duplicate_speculative_recomputes, 0);
    ProfileResult {
        snapshot,
        lock_profile: Some(lock_profile),
    }
}

fn thread_safe_contention(case: ThreadSafeContentionCase, workers: usize) -> ProfileResult {
    match case {
        ThreadSafeContentionCase::SameSlotWriteRead => {
            thread_safe_contention_profile(workers, run_thread_safe_same_slot_contention)
        }
        ThreadSafeContentionCase::IndependentSlots => {
            thread_safe_contention_profile(workers, run_thread_safe_independent_slot_contention)
        }
        ThreadSafeContentionCase::ReadMostlyWaiters => {
            thread_safe_contention_profile(workers, run_thread_safe_read_mostly_contention)
        }
        ThreadSafeContentionCase::BatchedWriteBursts => {
            thread_safe_contention_profile(workers, run_thread_safe_batched_write_bursts)
        }
    }
}

fn thread_safe_effect_contention(
    case: ThreadSafeEffectContentionCase,
    workers: usize,
) -> ProfileResult {
    match case {
        ThreadSafeEffectContentionCase::QueueCoalescing => {
            thread_safe_contention_profile(workers, run_thread_safe_effect_queue_coalescing)
        }
        ThreadSafeEffectContentionCase::CleanupExecution => {
            thread_safe_contention_profile(workers, run_thread_safe_effect_cleanup_execution)
        }
        ThreadSafeEffectContentionCase::BatchFlush => {
            thread_safe_contention_profile(workers, run_thread_safe_effect_batch_flush)
        }
    }
}

fn thread_safe_contention_profile(
    workers: usize,
    run: fn(&ThreadSafeContext, usize) -> usize,
) -> ProfileResult {
    let ctx = ThreadSafeContext::new();
    ctx.reset_instrumentation();
    let total = run(&ctx, workers);
    black_box(total);

    let snapshot = ctx.instrumentation_snapshot();
    assert!(snapshot.lock_acquisitions > 0);
    assert_eq!(snapshot.duplicate_speculative_recomputes, 0);
    ProfileResult {
        snapshot,
        lock_profile: Some(ctx.lock_profile_snapshot()),
    }
}

fn run_thread_safe_same_slot_set_cell_invalidation(
    ctx: &ThreadSafeContext,
    workers: usize,
) -> usize {
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

fn run_thread_safe_independent_slot_set_cell_invalidation(
    ctx: &ThreadSafeContext,
    workers: usize,
) -> usize {
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

fn run_thread_safe_batched_set_cell_invalidation(ctx: &ThreadSafeContext, workers: usize) -> usize {
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

fn run_thread_safe_same_slot_contention(ctx: &ThreadSafeContext, workers: usize) -> usize {
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

fn run_thread_safe_independent_slot_contention(ctx: &ThreadSafeContext, workers: usize) -> usize {
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

fn run_thread_safe_read_mostly_contention(ctx: &ThreadSafeContext, workers: usize) -> usize {
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

fn run_thread_safe_batched_write_bursts(ctx: &ThreadSafeContext, workers: usize) -> usize {
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

fn effect_worker_cells(ctx: &ThreadSafeContext, workers: usize) -> Vec<Vec<CellHandle<usize>>> {
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
        .collect::<Vec<CellHandle<usize>>>();
    let effect_runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sink = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let effect_runs_for_effect = Arc::clone(&effect_runs);
    let sink_for_effect = Arc::clone(&sink);
    let _effect = ctx.effect(move |ctx| {
        effect_runs_for_effect.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let total = all_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        sink_for_effect.store(total, std::sync::atomic::Ordering::Relaxed);
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
                        .wrapping_add(sink.load(std::sync::atomic::Ordering::Relaxed))
                        .wrapping_add(effect_runs.load(std::sync::atomic::Ordering::Relaxed));
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
    let cleanup_runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sink = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let effect_cells = cells.clone();
    let cleanup_runs_for_effect = Arc::clone(&cleanup_runs);
    let sink_for_effect = Arc::clone(&sink);
    let effect = ctx.effect(move |ctx| {
        let total = effect_cells
            .iter()
            .fold(0usize, |sum, cell| sum.wrapping_add(ctx.get_cell(cell)));
        sink_for_effect.store(total, std::sync::atomic::Ordering::Relaxed);
        let cleanup_runs = Arc::clone(&cleanup_runs_for_effect);
        move || {
            cleanup_runs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                        .wrapping_add(sink.load(std::sync::atomic::Ordering::Relaxed))
                        .wrapping_add(cleanup_runs.load(std::sync::atomic::Ordering::Relaxed));
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
    total.wrapping_add(cleanup_runs.load(std::sync::atomic::Ordering::Relaxed))
}

fn run_thread_safe_effect_batch_flush(ctx: &ThreadSafeContext, workers: usize) -> usize {
    let worker_cells = effect_worker_cells(ctx, workers);
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
    let sink = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sink_for_effect = Arc::clone(&sink);
    let _effect = ctx.effect(move |ctx| {
        sink_for_effect.store(ctx.get(&total), std::sync::atomic::Ordering::Relaxed);
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
                    sum = sum.wrapping_add(sink.load(std::sync::atomic::Ordering::Relaxed));
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
