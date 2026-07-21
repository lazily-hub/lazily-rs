//! Integration tests for the eager `Signal` primitive.
//!
//! A `Signal` is an always-set derived value that recomputes eagerly on
//! invalidation, transitioning `v1 -> v2` with no observable intermediate
//! unset state, and propagating glitch-free through derived signal graphs.

use std::cell::RefCell;
use std::rc::Rc;

use lazily::Context;

#[test]
fn signal_is_materialized_eagerly_on_creation() {
    let ctx = Context::new();
    let n = ctx.cell(2i32);
    // No read happens before the assertion — the signal must already hold its
    // computed value purely from creation.
    let doubled = ctx.signal(move |ctx| n.get(ctx) * 2);
    assert_eq!(doubled.get(&ctx), 4);
}

#[test]
fn signal_recomputes_eagerly_without_a_read() {
    let ctx = Context::new();
    let n = ctx.cell(1i32);
    let computes = Rc::new(RefCell::new(0usize));
    let computes_inner = computes.clone();
    let sig = ctx.signal(move |ctx| {
        *computes_inner.borrow_mut() += 1;
        n.get(ctx) + 10
    });

    // One eager compute at creation.
    assert_eq!(*computes.borrow(), 1);

    // Changing the input recomputes eagerly: no `get` is needed to drive it.
    n.set(&ctx, 5);
    assert_eq!(*computes.borrow(), 2);
    assert_eq!(sig.get(&ctx), 15);
}

#[test]
fn signal_value_goes_v1_to_v2_with_no_intermediate_unset() {
    let ctx = Context::new();
    let n = ctx.cell(1i32);
    let sig = ctx.signal(move |ctx| n.get(ctx) * 100);

    // Record every value the signal takes, observed through a dependent effect.
    let seen = Rc::new(RefCell::new(Vec::<i32>::new()));
    let seen_inner = seen.clone();
    let _watch = ctx.effect(move |ctx| {
        seen_inner.borrow_mut().push(sig.get(ctx));
    });

    n.set(&ctx, 2);
    n.set(&ctx, 3);

    // v1 (100) -> v2 (200) -> v3 (300); never an unset / stale-skip value.
    assert_eq!(*seen.borrow(), vec![100, 200, 300]);
    // Immediately after a set the signal already reflects the new value.
    assert_eq!(sig.get(&ctx), 300);
}

#[test]
fn signal_memo_guard_skips_equal_recomputation() {
    let ctx = Context::new();
    let n = ctx.cell(4i32);
    // Value depends only on parity, so flipping between two evens is a no-op.
    let parity = ctx.signal(move |ctx| n.get(ctx) % 2);

    let downstream_runs = Rc::new(RefCell::new(0usize));
    let downstream_inner = downstream_runs.clone();
    let _watch = ctx.effect(move |ctx| {
        let _ = parity.get(ctx);
        *downstream_inner.borrow_mut() += 1;
    });

    // Initial effect run.
    assert_eq!(*downstream_runs.borrow(), 1);

    // 4 -> 6: parity stays 0, downstream must not rerun (memo guard).
    n.set(&ctx, 6);
    assert_eq!(*downstream_runs.borrow(), 1);
    assert_eq!(parity.get(&ctx), 0);

    // 6 -> 7: parity flips to 1, downstream reruns once.
    n.set(&ctx, 7);
    assert_eq!(*downstream_runs.borrow(), 2);
    assert_eq!(parity.get(&ctx), 1);
}

#[test]
fn chained_signals_propagate_eagerly() {
    let ctx = Context::new();
    let n = ctx.cell(1i32);
    let a = ctx.signal(move |ctx| n.get(ctx) + 1);
    let b = ctx.signal(move |ctx| a.get(ctx) * 10);

    assert_eq!(a.get(&ctx), 2);
    assert_eq!(b.get(&ctx), 20);

    n.set(&ctx, 5);
    // Both updated eagerly with no intervening read.
    assert_eq!(a.get(&ctx), 6);
    assert_eq!(b.get(&ctx), 60);
}

#[test]
fn diamond_signal_graph_is_glitch_free() {
    let ctx = Context::new();
    let n = ctx.cell(1i32);
    let a = ctx.signal(move |ctx| n.get(ctx) + 1); // n=1 -> 2 ; n=5 -> 6
    let c = ctx.signal(move |ctx| a.get(ctx) * 2); // -> 4 ; -> 12
    let d = ctx.signal(move |ctx| a.get(ctx) + c.get(ctx)); // -> 6 ; -> 18

    // Observe every value `d` takes. A glitch would surface the inconsistent
    // intermediate `new_a + old_c = 6 + 4 = 10`.
    let seen = Rc::new(RefCell::new(Vec::<i32>::new()));
    let seen_inner = seen.clone();
    let _watch = ctx.effect(move |ctx| {
        seen_inner.borrow_mut().push(d.get(ctx));
    });

    n.set(&ctx, 5);

    assert_eq!(d.get(&ctx), 18);
    let seen = seen.borrow();
    assert!(
        !seen.contains(&10),
        "observed glitch intermediate value: {seen:?}"
    );
    assert_eq!(*seen, vec![6, 18]);
}

#[test]
fn batched_writes_settle_to_a_single_consistent_value() {
    let ctx = Context::new();
    let a = ctx.cell(1i32);
    let b = ctx.cell(10i32);
    let sum = ctx.signal(move |ctx| a.get(ctx) + b.get(ctx));

    let seen = Rc::new(RefCell::new(Vec::<i32>::new()));
    let seen_inner = seen.clone();
    let _watch = ctx.effect(move |ctx| {
        seen_inner.borrow_mut().push(sum.get(ctx));
    });

    ctx.batch(|ctx| {
        a.set(ctx, 2);
        b.set(ctx, 20);
    });

    // Initial 11, then a single coalesced update to 22 — no intermediate 12/21.
    assert_eq!(*seen.borrow(), vec![11, 22]);
    assert_eq!(sum.get(&ctx), 22);
}

#[test]
fn dispose_stops_eager_recomputation() {
    let ctx = Context::new();
    let n = ctx.cell(1i32);
    let computes = Rc::new(RefCell::new(0usize));
    let computes_inner = computes.clone();
    let sig = ctx.signal(move |ctx| {
        *computes_inner.borrow_mut() += 1;
        n.get(ctx) + 1
    });

    assert!(sig.is_driven(&ctx));
    assert_eq!(*computes.borrow(), 1);

    sig.undrive(&ctx);
    assert!(!sig.is_driven(&ctx));

    // Eager puller gone: a cell change no longer triggers recomputation...
    n.set(&ctx, 9);
    assert_eq!(*computes.borrow(), 1);

    // ...but the value is still readable, recomputed lazily on demand.
    assert_eq!(sig.get(&ctx), 10);
    assert_eq!(*computes.borrow(), 2);
}

#[test]
fn equal_value_set_is_a_noop_for_signals() {
    let ctx = Context::new();
    let n = ctx.cell(3i32);
    let computes = Rc::new(RefCell::new(0usize));
    let computes_inner = computes.clone();
    let sig = ctx.signal(move |ctx| {
        *computes_inner.borrow_mut() += 1;
        n.get(ctx) * 2
    });

    assert_eq!(*computes.borrow(), 1);
    // Setting the same value never invalidates, so the signal is not recomputed.
    n.set(&ctx, 3);
    assert_eq!(*computes.borrow(), 1);
    assert_eq!(sig.get(&ctx), 6);
}

/// The same eager-Signal semantics, exercised over the shared-graph
/// [`ThreadSafeContext`]. Mirrors the single-threaded suite above so the two
/// `signal` surfaces stay behaviorally identical (#lzsignalparity).
#[cfg(feature = "thread-safe")]
mod thread_safe {
    use std::sync::Arc;
    use std::sync::Mutex;

    use lazily::ThreadSafeContext;

    #[test]
    fn signal_is_materialized_eagerly_on_creation() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(2i32);
        let doubled = ctx.signal(move |ctx| ctx.get_cell(&n) * 2);
        assert_eq!(doubled.get(&ctx), 4);
    }

    #[test]
    fn signal_recomputes_eagerly_without_a_read() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(1i32);
        let computes = Arc::new(Mutex::new(0usize));
        let computes_inner = computes.clone();
        let sig = ctx.signal(move |ctx| {
            *computes_inner.lock().unwrap() += 1;
            ctx.get_cell(&n) + 10
        });

        assert_eq!(*computes.lock().unwrap(), 1);

        ctx.set_cell(&n, 5);
        assert_eq!(*computes.lock().unwrap(), 2);
        assert_eq!(sig.get(&ctx), 15);
    }

    #[test]
    fn signal_value_goes_v1_to_v2_with_no_intermediate_unset() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(1i32);
        let sig = ctx.signal(move |ctx| ctx.get_cell(&n) * 100);

        let seen = Arc::new(Mutex::new(Vec::<i32>::new()));
        let seen_inner = seen.clone();
        let _watch = ctx.effect(move |ctx| {
            seen_inner.lock().unwrap().push(sig.get(ctx));
        });

        ctx.set_cell(&n, 2);
        ctx.set_cell(&n, 3);

        assert_eq!(*seen.lock().unwrap(), vec![100, 200, 300]);
        assert_eq!(sig.get(&ctx), 300);
    }

    #[test]
    fn signal_memo_guard_skips_equal_recomputation() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(4i32);
        let parity = ctx.signal(move |ctx| ctx.get_cell(&n) % 2);

        let downstream_runs = Arc::new(Mutex::new(0usize));
        let downstream_inner = downstream_runs.clone();
        let _watch = ctx.effect(move |ctx| {
            let _ = parity.get(ctx);
            *downstream_inner.lock().unwrap() += 1;
        });

        assert_eq!(*downstream_runs.lock().unwrap(), 1);

        // 4 -> 6: parity stays 0, downstream must not rerun (memo guard).
        ctx.set_cell(&n, 6);
        assert_eq!(*downstream_runs.lock().unwrap(), 1);
        assert_eq!(parity.get(&ctx), 0);

        // 6 -> 7: parity flips to 1, downstream reruns once.
        ctx.set_cell(&n, 7);
        assert_eq!(*downstream_runs.lock().unwrap(), 2);
        assert_eq!(parity.get(&ctx), 1);
    }

    #[test]
    fn chained_signals_propagate_eagerly() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(1i32);
        let a = ctx.signal(move |ctx| ctx.get_cell(&n) + 1);
        let b = ctx.signal(move |ctx| a.get(ctx) * 10);

        assert_eq!(a.get(&ctx), 2);
        assert_eq!(b.get(&ctx), 20);

        ctx.set_cell(&n, 5);
        assert_eq!(a.get(&ctx), 6);
        assert_eq!(b.get(&ctx), 60);
    }

    #[test]
    fn diamond_signal_graph_is_glitch_free() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(1i32);
        let a = ctx.signal(move |ctx| ctx.get_cell(&n) + 1);
        let c = ctx.signal(move |ctx| a.get(ctx) * 2);
        let d = ctx.signal(move |ctx| a.get(ctx) + c.get(ctx));

        let seen = Arc::new(Mutex::new(Vec::<i32>::new()));
        let seen_inner = seen.clone();
        let _watch = ctx.effect(move |ctx| {
            seen_inner.lock().unwrap().push(d.get(ctx));
        });

        ctx.set_cell(&n, 5);

        assert_eq!(d.get(&ctx), 18);
        let seen = seen.lock().unwrap();
        assert!(
            !seen.contains(&10),
            "observed glitch intermediate value: {seen:?}"
        );
        assert_eq!(*seen, vec![6, 18]);
    }

    #[test]
    fn batched_writes_settle_to_a_single_consistent_value() {
        let ctx = ThreadSafeContext::new();
        let a = ctx.cell(1i32);
        let b = ctx.cell(10i32);
        let sum = ctx.signal(move |ctx| ctx.get_cell(&a) + ctx.get_cell(&b));

        let seen = Arc::new(Mutex::new(Vec::<i32>::new()));
        let seen_inner = seen.clone();
        let _watch = ctx.effect(move |ctx| {
            seen_inner.lock().unwrap().push(sum.get(ctx));
        });

        ctx.batch(|ctx| {
            ctx.set_cell(&a, 2);
            ctx.set_cell(&b, 20);
        });

        assert_eq!(*seen.lock().unwrap(), vec![11, 22]);
        assert_eq!(sum.get(&ctx), 22);
    }

    #[test]
    fn dispose_stops_eager_recomputation() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(1i32);
        let computes = Arc::new(Mutex::new(0usize));
        let computes_inner = computes.clone();
        let sig = ctx.signal(move |ctx| {
            *computes_inner.lock().unwrap() += 1;
            ctx.get_cell(&n) + 1
        });

        assert!(sig.is_active(&ctx));
        assert_eq!(*computes.lock().unwrap(), 1);

        sig.dispose(&ctx);
        assert!(!sig.is_active(&ctx));

        // Eager puller gone: a cell change no longer triggers recomputation...
        ctx.set_cell(&n, 9);
        assert_eq!(*computes.lock().unwrap(), 1);

        // ...but the value is still readable, recomputed lazily on demand.
        assert_eq!(sig.get(&ctx), 10);
        assert_eq!(*computes.lock().unwrap(), 2);
    }

    #[test]
    fn equal_value_set_is_a_noop_for_signals() {
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(3i32);
        let computes = Arc::new(Mutex::new(0usize));
        let computes_inner = computes.clone();
        let sig = ctx.signal(move |ctx| {
            *computes_inner.lock().unwrap() += 1;
            ctx.get_cell(&n) * 2
        });

        assert_eq!(*computes.lock().unwrap(), 1);
        ctx.set_cell(&n, 3);
        assert_eq!(*computes.lock().unwrap(), 1);
        assert_eq!(sig.get(&ctx), 6);
    }

    #[test]
    fn signal_handle_is_send_and_shared_across_threads() {
        // The shared-graph signal must be usable from another OS thread, which
        // is the entire point of the ThreadSafeContext variant.
        let ctx = ThreadSafeContext::new();
        let n = ctx.cell(2i32);
        let sig = ctx.signal(move |ctx| ctx.get_cell(&n) * 3);
        assert_eq!(sig.get(&ctx), 6);

        let ctx2 = ctx.clone();
        let observed = std::thread::spawn(move || sig.get(&ctx2)).join().unwrap();
        assert_eq!(observed, 6);

        ctx.set_cell(&n, 4);
        let ctx3 = ctx.clone();
        let observed = std::thread::spawn(move || sig.get(&ctx3)).join().unwrap();
        assert_eq!(observed, 12);
    }
}
