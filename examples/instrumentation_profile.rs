use std::cell::Cell as LocalCell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use lazily::{Context, InstrumentationSnapshot, ThreadSafeContext};

const FAN_OUT_WIDTH: usize = 32;
const BATCH_STORM_CELLS: usize = 64;
const CONTENTION_ITERS_PER_WORKER: usize = 16;
const CONTENTION_WORKERS: [usize; 5] = [1, 2, 4, 8, 16];

fn main() {
    println!(
        "profile,node_allocations,slot_recomputes,duplicate_speculative_recomputes,\
dependency_edges_added,dependency_edges_removed,effect_queue_pushes,\
max_effect_queue_depth,lock_acquisitions,lock_wait_nanos,lock_hold_nanos"
    );

    emit("context_memo_effect", context_memo_effect());
    emit("context_fan_out_32", context_fan_out());
    emit("context_batch_storm_64", context_batch_storm());
    emit("thread_safe_first_get_2", thread_safe_first_get());

    for workers in CONTENTION_WORKERS {
        emit(
            &format!("thread_safe_contention_{workers}"),
            thread_safe_contention(workers),
        );
    }
}

fn emit(profile: &str, snapshot: InstrumentationSnapshot) {
    println!(
        "{},{},{},{},{},{},{},{},{},{},{}",
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
        snapshot.lock_hold_nanos
    );
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

fn thread_safe_contention(workers: usize) -> InstrumentationSnapshot {
    let ctx = ThreadSafeContext::new();
    ctx.reset_instrumentation();

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

    let total = threads
        .into_iter()
        .map(|worker| worker.join().expect("contention worker should finish"))
        .fold(0usize, usize::wrapping_add);
    black_box(total);

    let snapshot = ctx.instrumentation_snapshot();
    assert!(snapshot.lock_acquisitions > 0);
    assert_eq!(snapshot.duplicate_speculative_recomputes, 0);
    snapshot
}
