#![cfg(feature = "thread-safe")]

use lazily::{ReadStrategy, ThreadSafeContext};
use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread;

const READERS: usize = 4;
const READ_ITERS: usize = 256;
const WRITE_ITERS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReadContentionOutcome {
    final_cell: usize,
    final_total: usize,
}

fn expected_total(choose_left: bool, left: usize, right: usize, bias: usize) -> usize {
    let base = if choose_left { left } else { right };
    base + bias
}

fn run_read_contention(strategy: ReadStrategy) -> ReadContentionOutcome {
    let ctx = ThreadSafeContext::with_read_strategy(strategy);
    assert_eq!(ctx.read_strategy(), strategy);

    let input = ctx.source(0usize);
    let doubled = ctx.computed(move |ctx| ctx.get(&input).wrapping_mul(2));
    assert_eq!(ctx.get(&doubled), 0);

    let barrier = Arc::new(Barrier::new(READERS + 2));
    let min_seen = Arc::new(AtomicUsize::new(usize::MAX));
    let max_seen = Arc::new(AtomicUsize::new(0));
    let mut readers = Vec::with_capacity(READERS);
    for _ in 0..READERS {
        let ctx = ctx.clone();
        let barrier = Arc::clone(&barrier);
        let min_seen = Arc::clone(&min_seen);
        let max_seen = Arc::clone(&max_seen);
        readers.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..READ_ITERS {
                let value = ctx.get(&doubled);
                assert_eq!(value % 2, 0, "doubled value must stay even");
                assert!(value <= WRITE_ITERS * 2);
                min_seen.fetch_min(value, Ordering::SeqCst);
                max_seen.fetch_max(value, Ordering::SeqCst);
                thread::yield_now();
            }
        }));
    }

    let writer_ctx = ctx.clone();
    let writer_barrier = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        writer_barrier.wait();
        for value in 1..=WRITE_ITERS {
            if value % 8 == 0 {
                writer_ctx.batch(|ctx| {
                    ctx.set(&input, value);
                    ctx.clear(&doubled);
                    if value % 24 == 0 {
                        ctx.clear_cell_dependents(&input);
                    }
                });
            } else {
                writer_ctx.set(&input, value);
            }
            let observed = writer_ctx.get(&doubled);
            assert_eq!(
                observed % 2,
                0,
                "strategy={strategy:?} value={value} observed={observed}"
            );
            assert!(
                observed <= WRITE_ITERS * 2,
                "strategy={strategy:?} value={value} observed={observed}"
            );
            thread::yield_now();
        }
    });

    barrier.wait();
    writer.join().expect("writer should finish");
    for reader in readers {
        reader.join().expect("reader should finish");
    }
    assert_ne!(
        min_seen.load(Ordering::SeqCst),
        usize::MAX,
        "readers should observe at least one cached value"
    );
    assert!(max_seen.load(Ordering::SeqCst) <= WRITE_ITERS * 2);

    ctx.batch(|ctx| {
        ctx.set(&input, 256);
        ctx.clear(&doubled);
    });

    ReadContentionOutcome {
        final_cell: ctx.get(&input),
        final_total: ctx.get(&doubled),
    }
}

#[test]
fn read_strategy_parity_under_contention() {
    let low = run_read_contention(ReadStrategy::LowConcurrency);
    let high = run_read_contention(ReadStrategy::HighConcurrency);

    assert_eq!(
        low,
        ReadContentionOutcome {
            final_cell: 256,
            final_total: 512
        }
    );
    assert_eq!(low, high);
}

fn run_batch_effect_disposal_stress(strategy: ReadStrategy) -> usize {
    let ctx = ThreadSafeContext::with_read_strategy(strategy);
    let choose_left = ctx.source(false);
    let left = ctx.source(1usize);
    let right = ctx.source(10usize);
    let bias = ctx.source(100usize);
    let selected = ctx.computed(move |ctx| {
        expected_total(
            ctx.get(&choose_left),
            ctx.get(&left),
            ctx.get(&right),
            ctx.get(&bias),
        )
    });

    let observations = Arc::new(Mutex::new(Vec::new()));
    let cleanups = Arc::new(AtomicUsize::new(0));
    let effect = ctx.effect({
        let observations = Arc::clone(&observations);
        let cleanups = Arc::clone(&cleanups);
        move |ctx| {
            let value = ctx.get(&selected);
            observations.lock().unwrap().push(value);
            let cleanups = Arc::clone(&cleanups);
            move || {
                cleanups.fetch_add(1, Ordering::SeqCst);
            }
        }
    });

    assert_eq!(ctx.get(&selected), 110);
    assert_eq!(*observations.lock().unwrap(), vec![110]);

    let barrier = Arc::new(Barrier::new(4));
    let progress = Arc::new(AtomicUsize::new(0));
    let disposed = Arc::new(AtomicBool::new(false));

    let batcher = {
        let ctx = ctx.clone();
        let barrier = Arc::clone(&barrier);
        let progress = Arc::clone(&progress);
        thread::spawn(move || {
            barrier.wait();
            for step in 1..=64usize {
                let choose = step % 2 == 0;
                let left_value = step;
                let right_value = step * 10;
                let bias_value = 1_000 + step;
                ctx.batch(|ctx| {
                    ctx.set(&choose_left, choose);
                    ctx.set(&left, left_value);
                    ctx.set(&right, right_value);
                    ctx.set(&bias, bias_value);
                    if step % 5 == 0 {
                        ctx.clear(&selected);
                    }
                    if step % 7 == 0 {
                        ctx.clear_cell_dependents(&right);
                    }
                });
                assert_eq!(
                    ctx.get(&selected),
                    expected_total(choose, left_value, right_value, bias_value)
                );
                progress.store(step, Ordering::SeqCst);
                thread::yield_now();
            }
        })
    };

    let disposer = {
        let ctx = ctx.clone();
        let barrier = Arc::clone(&barrier);
        let progress = Arc::clone(&progress);
        let disposed = Arc::clone(&disposed);
        thread::spawn(move || {
            barrier.wait();
            while progress.load(Ordering::SeqCst) < 8 {
                thread::yield_now();
            }
            ctx.dispose_effect(&effect);
            disposed.store(true, Ordering::SeqCst);
        })
    };

    let reader = {
        let ctx = ctx.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            for _ in 0..128 {
                let value = ctx.get(&selected);
                assert!(value >= 100);
                assert!(value < 10_000);
                thread::yield_now();
            }
        })
    };

    barrier.wait();
    batcher.join().expect("batcher should finish");
    disposer.join().expect("disposer should finish");
    reader.join().expect("reader should finish");

    assert!(disposed.load(Ordering::SeqCst));
    assert!(!ctx.is_effect_active(&effect));
    assert!(cleanups.load(Ordering::SeqCst) >= 1);

    let observations_after_dispose = observations.lock().unwrap().len();
    ctx.batch(|ctx| {
        ctx.set(&choose_left, true);
        ctx.set(&left, 7);
        ctx.set(&right, 70);
        ctx.set(&bias, 100);
        ctx.clear(&selected);
    });
    assert_eq!(ctx.get(&selected), 107);
    ctx.set(&left, 8);
    assert_eq!(ctx.get(&selected), 108);
    assert_eq!(
        observations.lock().unwrap().len(),
        observations_after_dispose,
        "disposed effect should not observe writes after all race participants finish"
    );

    ctx.get(&selected)
}

#[test]
fn batch_effect_disposal_interaction_survives_contention() {
    assert_eq!(
        run_batch_effect_disposal_stress(ReadStrategy::LowConcurrency),
        108
    );
    assert_eq!(
        run_batch_effect_disposal_stress(ReadStrategy::HighConcurrency),
        108
    );
}
