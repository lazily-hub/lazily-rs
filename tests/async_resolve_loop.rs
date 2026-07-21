//! Deterministic regression coverage for the `#k03k` `get_async` resolve-loop
//! windows in `src/async_context.rs`.
//!
//! Loom cannot model this code path — `AsyncContext` is built on tokio's async
//! executor and `tokio::sync::watch`, while Loom only shims synchronous
//! `loom::sync` primitives (which is why `thread_safe.rs` has a Loom model and
//! this module does not). Instead we drive the two race windows directly:
//!
//!   * **Window 1** — the slot transitions `Computing/Empty -> Resolved` between
//!     the lock-free fast-path `get()` check and the re-lock, so `get_async`
//!     must return through the `Resolved`-after-re-lock arm. The gap is purely
//!     synchronous (no `.await`), so cooperative scheduling cannot force it; we
//!     use the `instrumentation`-gated one-shot seam to resolve the slot inside
//!     the gap deterministically and assert the race arm was taken.
//!   * **Window 2** — the notifier's `watch` senders all drop without a final
//!     `Resolved` send when an in-flight compute is superseded; `get_async` must
//!     re-resolve rather than panic. This is already exercised by
//!     `async_stress.rs::get_async_waiter_cancellation_and_stale_completion_keep_latest`
//!     (oneshot-gated supersede); the test below pins the same guarantee with a
//!     focused, named assertion.

#![cfg(all(feature = "async", feature = "instrumentation"))]

use lazily::AsyncContext;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, oneshot};

/// Window 1: resolve the slot inside `get_async`'s fast-path/re-lock gap and
/// assert the reader returns through the `Resolved`-after-re-lock arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window1_resolved_between_fastpath_and_relock() {
    let ctx = Arc::new(AsyncContext::new());
    let slot = ctx.computed_async(|_| async { 42i32 });

    // One-shot seam: when reader A parks in the window-1 gap, resolve the slot
    // from a separate reader (the hook has already disarmed itself, so reader B
    // takes the normal compute path), then wait until the value is published so
    // reader A re-locks straight into the `Resolved` arm.
    let ctx_hook = ctx.clone();
    ctx.__install_window1_hook(Arc::new(move || {
        let ctx_hook = ctx_hook.clone();
        Box::pin(async move {
            let resolver = tokio::spawn({
                let ctx_hook = ctx_hook.clone();
                async move { ctx_hook.get_async(&slot).await }
            });
            assert_eq!(resolver.await.unwrap(), 42, "resolver should compute slot");

            let deadline = Instant::now() + Duration::from_secs(2);
            while ctx_hook.get(&slot).is_none() {
                assert!(Instant::now() < deadline, "slot never resolved in gap");
                tokio::task::yield_now().await;
            }
        })
    }));

    let value = ctx.get_async(&slot).await;
    assert_eq!(value, 42);
    assert_eq!(
        ctx.__window1_resolved_hits(),
        1,
        "get_async must return through the window-1 Resolved-after-re-lock arm"
    );
}

/// Window 2: a superseded compute drops its notifier without a final `Resolved`
/// send; the waiting `get_async` must re-resolve to the latest value instead of
/// panicking on the dropped `watch` sender.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window2_superseded_notifier_drop_reresolves_to_latest() {
    let ctx = Arc::new(AsyncContext::new());
    let cell = ctx.cell(1i32);

    // Gate the first compute so it is still in flight when we supersede it.
    let (release_first, first_gate) = oneshot::channel::<()>();
    let first_gate = Arc::new(Mutex::new(Some(first_gate)));

    let slot = ctx.computed_async({
        let first_gate = first_gate.clone();
        move |ctx| {
            let observed = ctx.get(&cell);
            let first_gate = first_gate.clone();
            async move {
                // Only the first (observed == 1) compute parks on the gate; the
                // superseding compute (observed == 2) returns immediately.
                if observed == 1 {
                    let gate = first_gate.lock().await.take();
                    if let Some(gate) = gate {
                        let _ = gate.await;
                    }
                }
                observed * 10
            }
        }
    });

    // Reader parks while the first compute is gated in flight.
    let reader = tokio::spawn({
        let ctx = ctx.clone();
        async move { ctx.get_async(&slot).await }
    });
    tokio::task::yield_now().await;

    // Supersede: a new revision drops the first compute's notifier without a
    // final `Resolved` send, forcing the reader through window 2.
    ctx.set(&cell, 2);
    let _ = release_first.send(());

    assert_eq!(
        reader.await.unwrap(),
        20,
        "reader must re-resolve to the latest superseding value, not panic"
    );
    assert_eq!(ctx.get(&slot), Some(20));
}
