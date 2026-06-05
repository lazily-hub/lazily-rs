#![cfg(feature = "async")]

use lazily::{AsyncCellHandle, AsyncContext, AsyncEffectHandle, AsyncSlotHandle};
use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[tokio::test]
async fn async_context_public_api_cell_round_trip() {
    let ctx = AsyncContext::new();
    let cell: AsyncCellHandle<i32> = ctx.cell(42);
    assert_eq!(ctx.get_cell(&cell), 42);
    ctx.set_cell(&cell, 99);
    assert_eq!(ctx.get_cell(&cell), 99);
}

#[tokio::test]
async fn async_context_computed_async_resolves() {
    let ctx = AsyncContext::new();
    let slot: AsyncSlotHandle<i32> = ctx.computed_async(|_ctx| async { 7 });
    let val = ctx.get_async(&slot).await;
    assert_eq!(val, 7);
}

#[tokio::test]
async fn async_context_memo_async_deduplicates_equal() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(5i32);
    let invocations = Arc::new(AtomicU64::new(0));
    let inv_clone = invocations.clone();
    let slot = ctx.memo_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        let c = inv_clone.clone();
        async move {
            c.fetch_add(1, Ordering::Relaxed);
            v % 2
        }
    });
    assert_eq!(ctx.get_async(&slot).await, 1);
    ctx.set_cell(&cell, 7);
    assert_eq!(ctx.get_async(&slot).await, 1);
    assert_eq!(invocations.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn async_context_batch_defers_and_applies() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(1i32);
    let slot = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v + 100 }
    });
    assert_eq!(ctx.get_async(&slot).await, 101);
    ctx.batch(|ctx| {
        ctx.set_cell(&cell, 2);
        ctx.set_cell(&cell, 3);
    });
    assert_eq!(ctx.get_async(&slot).await, 103);
}

#[tokio::test]
async fn async_context_effect_async_lifecycle() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(0i32);
    let observations = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observations.clone();
    let handle: AsyncEffectHandle = ctx.effect_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        let o = obs_clone.clone();
        async move {
            o.lock().unwrap().push(v);
            None::<fn()>
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(*observations.lock().unwrap(), vec![0]);
    ctx.set_cell(&cell, 1);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(*observations.lock().unwrap(), vec![0, 1]);
    ctx.dispose_async_effect(&handle);
    ctx.set_cell(&cell, 2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(*observations.lock().unwrap(), vec![0, 1]);
}

#[tokio::test]
async fn async_context_effect_async_cleanup_before_rerun() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(10i32);
    let cleanup_ran = Arc::new(AtomicBool::new(false));
    let cleanup_clone = cleanup_ran.clone();
    ctx.effect_async(move |ctx| {
        let _v = ctx.get_cell(&cell);
        let c = cleanup_clone.clone();
        async move {
            Some(move || {
                c.store(true, Ordering::Relaxed);
            })
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!cleanup_ran.load(Ordering::Relaxed));
    ctx.set_cell(&cell, 20);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(cleanup_ran.load(Ordering::Relaxed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_context_concurrent_reads_dedup() {
    let ctx = Arc::new(AsyncContext::new());
    let compute_count = Arc::new(AtomicU64::new(0));
    let count_clone = compute_count.clone();
    let slot = ctx.computed_async(move |_| {
        let c = count_clone.clone();
        async move {
            c.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            42i32
        }
    });
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let h1 = tokio::spawn(async move { ctx1.get_async(&slot).await });
    let h2 = tokio::spawn(async move { ctx2.get_async(&slot).await });
    let (v1, v2) = tokio::join!(h1, h2);
    assert_eq!(v1.unwrap(), 42);
    assert_eq!(v2.unwrap(), 42);
    assert_eq!(compute_count.load(Ordering::Relaxed), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_context_invalidation_aborts_in_flight_across_tasks() {
    let ctx = Arc::new(AsyncContext::new());
    let cell = ctx.cell(1i32);
    let compute_count = Arc::new(AtomicU64::new(0));
    let count_clone = compute_count.clone();
    let slot = ctx.computed_async(move |ctx| {
        let _v = ctx.get_cell(&cell);
        let c = count_clone.clone();
        async move {
            c.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            99i32
        }
    });
    let ctx_reader = ctx.clone();
    let reader = tokio::spawn(async move { ctx_reader.get_async(&slot).await });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    ctx.set_cell(&cell, 2);
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let val = ctx.get_async(&slot).await;
    assert_eq!(val, 99);
    let _ = reader.await;
}

#[tokio::test]
async fn async_context_chain_propagation() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(1i32);
    let a = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v * 10 }
    });
    let b = ctx.computed_async(move |ctx| {
        let ah = a;
        async move {
            let v = ctx.get_async(&ah).await;
            v + 5
        }
    });
    let c = ctx.computed_async(move |ctx| {
        let bh = b;
        async move {
            let v = ctx.get_async(&bh).await;
            v * 2
        }
    });
    assert_eq!(ctx.get_async(&c).await, 30);
    ctx.set_cell(&cell, 2);
    assert_eq!(ctx.get_async(&c).await, 50);
}

#[tokio::test]
async fn async_context_memo_blocks_downstream_on_equal() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(2i32);
    let inner_invocations = Arc::new(AtomicU64::new(0));
    let inner_clone = inner_invocations.clone();
    let memo_slot = ctx.memo_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        let c = inner_clone.clone();
        async move {
            c.fetch_add(1, Ordering::Relaxed);
            v.abs()
        }
    });
    let outer_invocations = Arc::new(AtomicU64::new(0));
    let outer_clone = outer_invocations.clone();
    let derived = ctx.computed_async(move |ctx| {
        let mh = memo_slot;
        let c = outer_clone.clone();
        async move {
            let v = ctx.get_async(&mh).await;
            c.fetch_add(1, Ordering::Relaxed);
            v + 1
        }
    });
    assert_eq!(ctx.get_async(&derived).await, 3);
    ctx.set_cell(&cell, -2);
    assert_eq!(ctx.get_async(&derived).await, 3);
    assert_eq!(inner_invocations.load(Ordering::Relaxed), 2);
    assert_eq!(
        outer_invocations.load(Ordering::Relaxed),
        2,
        "async memo does not suppress downstream propagation"
    );
}

#[tokio::test]
async fn async_context_dynamic_dependency_switch() {
    let ctx = AsyncContext::new();
    let cell_a = ctx.cell(10i32);
    let cell_b = ctx.cell(20i32);
    let flag = ctx.cell(true);
    let slot = ctx.computed_async(move |ctx| {
        let f = ctx.get_cell(&flag);
        let v = if f {
            ctx.get_cell(&cell_a)
        } else {
            ctx.get_cell(&cell_b)
        };
        async move { v }
    });
    assert_eq!(ctx.get_async(&slot).await, 10);
    ctx.set_cell(&flag, false);
    assert_eq!(ctx.get_async(&slot).await, 20);
    ctx.set_cell(&cell_a, 99);
    let val = ctx.get_async(&slot).await;
    assert_eq!(val, 20, "cell_a change should not propagate after switch");
}

#[tokio::test]
async fn async_context_cell_noop_set_no_invalidation() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(5i32);
    let compute_count = Arc::new(AtomicU64::new(0));
    let count_clone = compute_count.clone();
    let slot = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        let c = count_clone.clone();
        async move {
            c.fetch_add(1, Ordering::Relaxed);
            v * 2
        }
    });
    assert_eq!(ctx.get_async(&slot).await, 10);
    ctx.set_cell(&cell, 5);
    assert_eq!(ctx.get_async(&slot).await, 10);
    assert_eq!(compute_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn async_context_batch_multiple_cells() {
    let ctx = AsyncContext::new();
    let cell_x = ctx.cell(1i32);
    let cell_y = ctx.cell(10i32);
    let slot = ctx.computed_async(move |ctx| {
        let x = ctx.get_cell(&cell_x);
        let y = ctx.get_cell(&cell_y);
        async move { x + y }
    });
    assert_eq!(ctx.get_async(&slot).await, 11);
    ctx.batch(|ctx| {
        ctx.set_cell(&cell_x, 5);
        ctx.set_cell(&cell_y, 50);
    });
    assert_eq!(ctx.get_async(&slot).await, 55);
}

#[tokio::test]
async fn async_context_dispose_effect_prevents_rerun() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(1i32);
    let run_count = Arc::new(AtomicU64::new(0));
    let count_clone = run_count.clone();
    let effect = ctx.effect_async(move |ctx| {
        let _v = ctx.get_cell(&cell);
        let c = count_clone.clone();
        async move {
            c.fetch_add(1, Ordering::Relaxed);
            None::<fn()>
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let after_create = run_count.load(Ordering::Relaxed);
    assert!(after_create >= 1);
    ctx.dispose_async_effect(&effect);
    for _ in 0..3 {
        ctx.set_cell(&cell, 2);
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        run_count.load(Ordering::Relaxed),
        after_create,
        "no reruns after dispose"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_context_shared_across_tokio_tasks() {
    let ctx = Arc::new(AsyncContext::new());
    let cell = ctx.cell(0i32);
    let slot = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v * 3 }
    });
    let _ = ctx.get_async(&slot).await;
    let ctx1 = ctx.clone();
    let reader = tokio::spawn(async move { ctx1.get_async(&slot).await });
    assert_eq!(reader.await.unwrap(), 0);
    ctx.set_cell(&cell, 7);
    let ctx2 = ctx.clone();
    let reader2 = tokio::spawn(async move { ctx2.get_async(&slot).await });
    assert_eq!(reader2.await.unwrap(), 21);
}

#[tokio::test]
async fn async_context_handles_are_copy() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(1i32);
    let cell_copy = cell;
    let slot = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell_copy);
        async move { v + 1 }
    });
    let slot_copy = slot;
    assert_eq!(ctx.get_async(&slot_copy).await, 2);
    let effect = ctx.effect_async(move |ctx| {
        let _v = ctx.get_cell(&cell);
        async { None::<fn()> }
    });
    let _effect_copy = effect;
    ctx.dispose_async_effect(&effect);
}

#[tokio::test]
async fn async_context_sync_get_returns_resolved_value() {
    let ctx = AsyncContext::new();
    let slot = ctx.computed_async(|_| async { 7i32 });
    assert!(ctx.get(&slot).is_none());
    let _ = ctx.get_async(&slot).await;
    assert_eq!(ctx.get(&slot), Some(7));
}

#[tokio::test]
async fn async_context_sync_get_invalidated_returns_none() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(1i32);
    let slot = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v * 3 }
    });
    assert_eq!(ctx.get_async(&slot).await, 3);
    assert_eq!(ctx.get(&slot), Some(3));
    ctx.set_cell(&cell, 5);
    assert!(ctx.get(&slot).is_none());
    assert_eq!(ctx.get_async(&slot).await, 15);
    assert_eq!(ctx.get(&slot), Some(15));
}

#[tokio::test]
async fn async_context_sync_get_with_chain() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(2i32);
    let a = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v + 10 }
    });
    let b = ctx.computed_async(move |ctx| {
        let ah = a;
        async move { ctx.get_async(&ah).await * 2 }
    });
    assert_eq!(ctx.get_async(&b).await, 24);
    assert_eq!(ctx.get(&a), Some(12));
    assert_eq!(ctx.get(&b), Some(24));
    ctx.set_cell(&cell, 5);
    assert!(ctx.get(&a).is_none());
    assert!(ctx.get(&b).is_none());
}

#[tokio::test]
async fn async_context_sync_get_across_tokio_tasks() {
    let ctx = Arc::new(AsyncContext::new());
    let cell = ctx.cell(10i32);
    let slot = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v + 1 }
    });
    let _ = ctx.get_async(&slot).await;
    let ctx_c = ctx.clone();
    let reader = tokio::spawn(async move { ctx_c.get(&slot) });
    assert_eq!(reader.await.unwrap(), Some(11));
    ctx.set_cell(&cell, 20);
    let ctx_c = ctx.clone();
    let reader2 = tokio::spawn(async move { ctx_c.get(&slot) });
    assert_eq!(reader2.await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_context_concurrent_set_and_get_async_never_panics_k03k() {
    // Regression for #k03k. Concurrent `set_cell` (invalidation) racing
    // `get_async` (re-resolve) previously panicked on a benign race:
    //   - `unreachable!("get() already checked Resolved")` when the slot
    //     transitioned `Computing -> Resolved` between the `get()` fast-path
    //     check and the re-lock inside `get_async`; and
    //   - `get_async: notifier dropped unexpectedly` when a superseded
    //     (stale-revision) or invalidated compute dropped its `watch` senders
    //     without a final `Resolved` send.
    // Both must now re-resolve from authoritative slot state, not panic.
    let ctx = Arc::new(AsyncContext::new());
    let cell = ctx.cell(0usize);
    let slot: AsyncSlotHandle<usize> = ctx.computed_async(move |ctx| {
        let v = ctx.get_cell(&cell);
        async move { v.wrapping_add(1) }
    });
    let _ = ctx.get_async(&slot).await;

    let workers = 8usize;
    let iters = 250usize;
    let mut handles = Vec::with_capacity(workers);
    for w in 0..workers {
        let ctx_c = Arc::clone(&ctx);
        let cell_c = cell;
        let slot_c = slot;
        handles.push(tokio::spawn(async move {
            for i in 0..iters {
                ctx_c.set_cell(&cell_c, w * iters + i);
                // Always resolves to (some observed cell + 1); never panics.
                let v = ctx_c.get_async(&slot_c).await;
                assert!(v >= 1, "computed value should be cell + 1");
            }
        }));
    }
    for h in handles {
        h.await.expect("worker task panicked (race in get_async)");
    }

    // After contention settles, a fresh write resolves deterministically.
    ctx.set_cell(&cell, 4242);
    assert_eq!(ctx.get_async(&slot).await, 4243);
}
