#![cfg(feature = "async")]

use lazily::AsyncContext;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

async fn recv_compute_start(starts: &mut mpsc::UnboundedReceiver<i32>, label: &str) -> i32 {
    tokio::time::timeout(Duration::from_secs(2), starts.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
        .unwrap_or_else(|| panic!("compute start channel closed while waiting for {label}"))
}

async fn wait_until(label: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if predicate() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn event_index(events: &[String], needle: &str) -> usize {
    events
        .iter()
        .position(|event| event == needle)
        .unwrap_or_else(|| panic!("missing event {needle}; events={events:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_async_waiter_cancellation_and_stale_completion_keep_latest() {
    let ctx = Arc::new(AsyncContext::new());
    let cell = ctx.cell(1i32);
    let gates = Arc::new(Mutex::new(VecDeque::<oneshot::Receiver<()>>::new()));
    let (starts_tx, mut starts_rx) = mpsc::unbounded_channel();

    let slot = ctx.computed_async({
        let gates = gates.clone();
        move |ctx| {
            let observed = ctx.get_cell(&cell);
            let gate = gates.lock().unwrap().pop_front();
            let starts_tx = starts_tx.clone();
            async move {
                let _ = starts_tx.send(observed);
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                observed * 10
            }
        }
    });

    let (release_first, first_gate) = oneshot::channel();
    let (release_second, second_gate) = oneshot::channel();
    {
        let mut gates = gates.lock().unwrap();
        gates.push_back(first_gate);
        gates.push_back(second_gate);
    }

    let primary = tokio::spawn({
        let ctx = ctx.clone();
        async move { ctx.get_async(&slot).await }
    });
    assert_eq!(
        recv_compute_start(&mut starts_rx, "initial compute").await,
        1
    );

    let canceled_waiter = tokio::spawn({
        let ctx = ctx.clone();
        async move { ctx.get_async(&slot).await }
    });
    tokio::task::yield_now().await;
    canceled_waiter.abort();
    assert!(canceled_waiter.await.unwrap_err().is_cancelled());

    ctx.set_cell(&cell, 2);
    let _ = release_first.send(());
    assert_eq!(
        recv_compute_start(&mut starts_rx, "replacement compute").await,
        2
    );

    let latest_reader = tokio::spawn({
        let ctx = ctx.clone();
        async move { ctx.get_async(&slot).await }
    });
    let _ = release_second.send(());

    assert_eq!(primary.await.unwrap(), 20);
    assert_eq!(latest_reader.await.unwrap(), 20);
    assert_eq!(ctx.get(&slot), Some(20));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dependency_tracking_across_awaits_replaces_dynamic_edges() {
    let ctx = AsyncContext::new();
    let use_left = ctx.cell(true);
    let left = ctx.cell(2i32);
    let right = ctx.cell(10i32);
    let outer_runs = Arc::new(AtomicU64::new(0));

    let left_slot = ctx.computed_async(move |ctx| {
        let value = ctx.get_cell(&left);
        async move {
            tokio::task::yield_now().await;
            value * 10
        }
    });
    let right_slot = ctx.computed_async(move |ctx| {
        let value = ctx.get_cell(&right);
        async move {
            tokio::task::yield_now().await;
            value * 10
        }
    });

    let outer = ctx.computed_async({
        let outer_runs = outer_runs.clone();
        move |ctx| {
            let selected = if ctx.get_cell(&use_left) {
                left_slot
            } else {
                right_slot
            };
            let outer_runs = outer_runs.clone();
            async move {
                tokio::task::yield_now().await;
                let value = ctx.get_async(&selected).await;
                outer_runs.fetch_add(1, Ordering::Relaxed);
                value + 1
            }
        }
    });

    assert_eq!(ctx.get_async(&outer).await, 21);
    ctx.set_cell(&left, 3);
    assert_eq!(ctx.get_async(&outer).await, 31);

    ctx.set_cell(&use_left, false);
    assert_eq!(ctx.get_async(&outer).await, 101);
    let runs_after_switch = outer_runs.load(Ordering::Relaxed);

    ctx.set_cell(&left, 4);
    assert_eq!(ctx.get(&outer), Some(101));
    assert_eq!(outer_runs.load(Ordering::Relaxed), runs_after_switch);

    ctx.set_cell(&right, 11);
    assert_eq!(ctx.get_async(&outer).await, 111);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn effect_cleanup_runs_before_each_replacement_body() {
    let ctx = AsyncContext::new();
    let cell = ctx.cell(0i32);
    let events = Arc::new(Mutex::new(Vec::<String>::new()));

    ctx.effect_async({
        let events = events.clone();
        move |ctx| {
            let observed = ctx.get_cell(&cell);
            let events = events.clone();
            async move {
                events.lock().unwrap().push(format!("run:{observed}"));
                Some(move || {
                    events.lock().unwrap().push(format!("cleanup:{observed}"));
                })
            }
        }
    });

    wait_until("initial async effect run", || {
        events.lock().unwrap().iter().any(|event| event == "run:0")
    })
    .await;

    for value in 1..=6 {
        ctx.set_cell(&cell, value);
        wait_until(&format!("async effect run {value}"), || {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event == &format!("run:{value}"))
        })
        .await;
    }

    let events = events.lock().unwrap().clone();
    for value in 0..6 {
        let cleanup = event_index(&events, &format!("cleanup:{value}"));
        let next_run = event_index(&events, &format!("run:{}", value + 1));
        assert!(
            cleanup < next_run,
            "cleanup for {value} must precede next run; events={events:?}"
        );
    }
}

// A cyclic async dependency graph must not hang `invalidate_frontier_async`.
//
// `AsyncComputeContext::get_async` registers the dependency edge SYNCHRONOUSLY,
// in the non-async prelude, before the returned future is ever awaited. That
// decouples edge registration from resolution: a compute can declare a
// dependency it never awaits, so `A -> B -> A` is constructible without either
// compute diverging. The invalidation walk then has a genuine cycle to traverse.
//
// The walk runs entirely inside `self.inner.lock()`, so a non-terminating walk
// wedges the whole context rather than merely spinning a task. The repro
// therefore runs on a dedicated OS thread and is judged by a channel timeout --
// a tokio timeout cannot preempt a sync spin that holds the mutex.
#[test]
fn cyclic_async_dependency_invalidation_terminates() {
    use std::sync::OnceLock;
    use std::sync::mpsc as std_mpsc;

    let (done_tx, done_rx) = std_mpsc::channel::<()>();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let ctx = AsyncContext::new();
            let cell = ctx.cell(1i32);

            // `b`'s handle is not known when `a` is created; publish it through a
            // OnceLock so `a`'s compute can close over it.
            static B_HANDLE: OnceLock<lazily::AsyncSlotHandle<i32>> = OnceLock::new();

            let a = ctx.computed_async(move |cx| {
                let v = cx.get_cell(&cell);
                if let Some(b) = B_HANDLE.get() {
                    // Register the edge a -> b without awaiting it. The edge is
                    // recorded by `get_async` itself; dropping the future avoids
                    // the recursive resolve that would otherwise deadlock.
                    drop(cx.get_async(b));
                }
                async move { v + 1 }
            });

            let b = ctx.computed_async(move |cx| {
                drop(cx.get_async(&a));
                async move { 0i32 }
            });
            assert!(B_HANDLE.set(b).is_ok(), "B_HANDLE set once");

            // Resolve both so their computes run and both edges are registered.
            let _ = ctx.get_async(&b).await;
            let _ = ctx.get_async(&a).await;

            // Force `a` to recompute so it observes B_HANDLE and registers a -> b.
            ctx.set_cell(&cell, 2);
            let _ = ctx.get_async(&a).await;

            // Both directions are now present: the walk has a real cycle.
            assert_eq!(ctx.dependent_count(&a), 1, "b must depend on a");
            assert_eq!(ctx.dependent_count(&b), 1, "a must depend on b");

            // Pre-fix this never returns -- it spins forever holding inner.lock().
            ctx.set_cell(&cell, 3);
        });
        let _ = done_tx.send(());
    });

    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .is_ok(),
        "cyclic async invalidation did not terminate within 10s: the frontier \
         walk pushed dependents unconditionally with no visited set"
    );
}
