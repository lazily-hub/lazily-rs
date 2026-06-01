#![cfg(feature = "tokio")]

use lazily::ThreadSafeContext;
use std::sync::{Arc, Mutex};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn thread_safe_context_crosses_tokio_spawn_boundaries() {
    let ctx = ThreadSafeContext::new();
    let input = ctx.cell(1usize);
    let doubled = ctx.computed(move |ctx| ctx.get_cell(&input) * 2);

    let reader_ctx = ctx.clone();
    let reader = tokio::spawn(async move {
        tokio::task::yield_now().await;
        reader_ctx.get(&doubled)
    });
    assert_eq!(reader.await.expect("reader task should finish"), 2);

    let writer_ctx = ctx.clone();
    let writer = tokio::spawn(async move {
        writer_ctx.set_cell(&input, 21);
        tokio::task::yield_now().await;
        writer_ctx.get(&doubled)
    });
    assert_eq!(writer.await.expect("writer task should finish"), 42);
    assert_eq!(ctx.get(&doubled), 42);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spawn_blocking_preserves_batch_effect_flush_order() {
    let ctx = ThreadSafeContext::new();
    let input = ctx.cell(0i32);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_effect = Arc::clone(&seen);

    let effect = ctx.effect(move |ctx| {
        seen_for_effect
            .lock()
            .expect("seen lock should not be poisoned")
            .push(ctx.get_cell(&input));
    });

    assert_eq!(*seen.lock().expect("seen lock"), vec![0]);

    let worker_ctx = ctx.clone();
    let seen_for_worker = Arc::clone(&seen);
    tokio::task::spawn_blocking(move || {
        worker_ctx.batch(|ctx| {
            ctx.set_cell(&input, 1);
            ctx.set_cell(&input, 2);
            assert_eq!(
                *seen_for_worker.lock().expect("seen lock"),
                vec![0],
                "effect should not rerun before batch exit"
            );
        });
    })
    .await
    .expect("blocking task should finish");

    assert_eq!(*seen.lock().expect("seen lock"), vec![0, 2]);
    ctx.dispose_effect(&effect);
}
