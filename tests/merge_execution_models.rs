//! `apply_merge` on the thread-safe and async execution models (`#lzmergefeed`,
//! design §9.1).
//!
//! `merge` — the algebra under RelayCell, whose transports include `CrossThread`
//! — used to live on `Context` alone. These tests pin the ported merge write on
//! `ThreadSafeContext` and `AsyncContext`: it folds under the policy, reduces to
//! a no-op when the fold does not change the value (`PartialEq` store guard),
//! and — the structural invariant §7 cares about — a `Source` never acquires a
//! dependency edge, no matter how it is written.

#[cfg(feature = "thread-safe")]
#[test]
fn threadsafe_apply_merge_folds_under_sum_without_an_edge() {
    use lazily::{Sum, ThreadSafeContext};

    let ctx = ThreadSafeContext::new();
    let acc = ctx.source(0i64);

    ctx.apply_merge::<i64, Sum>(&acc, 1);
    ctx.apply_merge::<i64, Sum>(&acc, 2);
    ctx.apply_merge::<i64, Sum>(&acc, 3);

    assert_eq!(ctx.get(&acc), 6, "1 + 2 + 3 under Sum");
    // A written source stays edge-free in both directions (§9.2.3).
    assert_eq!(ctx.dependency_count(&acc), 0);
    assert_eq!(ctx.dependent_count(&acc), 0);
}

#[cfg(feature = "thread-safe")]
#[test]
fn threadsafe_apply_merge_identity_fold_is_a_no_op() {
    use lazily::{Max, ThreadSafeContext};

    let ctx = ThreadSafeContext::new();
    let acc = ctx.source(5i64);
    let observed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = observed.clone();
    // A memo over acc so we can see whether a merge invalidated downstream.
    let mirror = ctx.computed(move |c| {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        c.get(&acc)
    });
    assert_eq!(ctx.get(&mirror), 5);
    let before = observed.load(std::sync::atomic::Ordering::SeqCst);

    // Max(5, 3) == 5: the store guard skips the write, so nothing downstream
    // recomputes.
    ctx.apply_merge::<i64, Max>(&acc, 3);
    assert_eq!(ctx.get(&mirror), 5);
    assert_eq!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        before,
        "an identity fold must not invalidate dependents"
    );

    // Max(5, 9) == 9 changes the value and does propagate.
    ctx.apply_merge::<i64, Max>(&acc, 9);
    assert_eq!(ctx.get(&mirror), 9);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_apply_merge_folds_under_sum_without_an_edge() {
    use lazily::{AsyncContext, Sum};

    let ctx = AsyncContext::new();
    let acc = ctx.source(0i64);

    // Merge is synchronous even here — cells are the synchronous input layer.
    ctx.apply_merge::<i64, Sum>(&acc, 1);
    ctx.apply_merge::<i64, Sum>(&acc, 2);
    ctx.apply_merge::<i64, Sum>(&acc, 3);

    assert_eq!(ctx.get(&acc), 6, "1 + 2 + 3 under Sum");
    assert_eq!(ctx.dependency_count(&acc), 0);
    assert_eq!(ctx.dependent_count(&acc), 0);
}
