use std::hint::black_box;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lazily::{Context, InstrumentationSnapshot, ThreadSafeContext};

fn consume_snapshot(snapshot: InstrumentationSnapshot) -> u64 {
    snapshot
        .node_allocations
        .wrapping_add(snapshot.slot_recomputes)
        .wrapping_add(snapshot.duplicate_speculative_recomputes)
        .wrapping_add(snapshot.dependency_edges_added)
        .wrapping_add(snapshot.dependency_edges_removed)
        .wrapping_add(snapshot.effect_queue_pushes)
        .wrapping_add(snapshot.max_effect_queue_depth)
        .wrapping_add(snapshot.lock_acquisitions)
        .wrapping_add(snapshot.lock_wait_nanos)
        .wrapping_add(snapshot.lock_hold_nanos)
}

fn context_profile_snapshot() -> InstrumentationSnapshot {
    let ctx = Context::new();
    let root = ctx.cell(0usize);
    let parity = ctx.memo(move |ctx| ctx.get_cell(&root) % 2);
    let label = ctx.computed(move |ctx| ctx.get(&parity).wrapping_add(1));
    let _effect = ctx.effect(move |ctx| {
        black_box(ctx.get(&label));
    });

    ctx.reset_instrumentation();
    ctx.set_cell(&root, 2);
    black_box(ctx.get(&label));

    ctx.instrumentation_snapshot()
}

fn thread_safe_profile_snapshot() -> InstrumentationSnapshot {
    let ctx = ThreadSafeContext::new();
    let root = ctx.cell(40usize);
    let answer = ctx.computed(move |ctx| {
        thread::sleep(Duration::from_micros(200));
        ctx.get_cell(&root).wrapping_add(2)
    });
    let start = Arc::new(Barrier::new(2));

    ctx.reset_instrumentation();

    let workers = (0..2)
        .map(|_| {
            let ctx = ctx.clone();
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                black_box(ctx.get(&answer))
            })
        })
        .collect::<Vec<_>>();

    for worker in workers {
        worker.join().expect("profile worker should finish");
    }

    ctx.instrumentation_snapshot()
}

fn bench_profile_instrumentation(c: &mut Criterion) {
    let mut group = c.benchmark_group("profile_instrumentation");
    group.sample_size(10);

    group.bench_function("context_snapshot", |b| {
        b.iter_batched(
            || (),
            |()| black_box(consume_snapshot(context_profile_snapshot())),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("thread_safe_snapshot", |b| {
        b.iter_batched(
            || (),
            |()| black_box(consume_snapshot(thread_safe_profile_snapshot())),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_profile_instrumentation);
criterion_main!(benches);
