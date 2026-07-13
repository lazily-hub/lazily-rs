//! Phase 5 spike tests for the `Inbox` / `Outbox` role facades (`#relaycell`).

use lazily::{BoundDim, Context, Inbox, IngressOutcome, KeepLatest, Outbox, Overflow, Sum};

/// An Outbox<KeepLatest> is the state-broadcast case: a slow egress conflates to
/// the latest value; the producer feels nothing (Conflate never blocks).
#[test]
fn outbox_state_conflates_to_latest() {
    let ctx = Context::new();
    let out: Outbox<i64, KeepLatest> = Outbox::new(&ctx, 2).unwrap();
    for v in [1i64, 2, 3, 4, 5] {
        assert_ne!(out.send(&ctx, v), IngressOutcome::Blocked);
    }
    // The egress drains the latest coalesced state.
    assert_eq!(out.drain(&ctx), Some(5));
}

/// An Outbox with `Block` overflow backpressures the local producer via is_full.
#[test]
fn outbox_block_backpressures_producer() {
    let ctx = Context::new();
    let out: Outbox<i64, Sum> =
        Outbox::with_overflow(&ctx, BoundDim::Count, 2, Overflow::Block).unwrap();
    assert_eq!(out.send(&ctx, 1), IngressOutcome::Accepted);
    assert_eq!(out.send(&ctx, 2), IngressOutcome::Conflated);
    assert_eq!(out.send(&ctx, 3), IngressOutcome::Blocked); // producer backpressured
    assert!(out.is_full().get(&ctx));
    assert_eq!(out.drain(&ctx), Some(3));
    assert_eq!(out.send(&ctx, 9), IngressOutcome::Accepted); // re-opened
}

/// An Inbox meters the remote via credits: when exhausted it is not `ready`, so
/// the transport stops delivering (remote throttles); consuming replenishes.
#[test]
fn inbox_credit_meters_the_remote() {
    let ctx = Context::new();
    let mut inbox: Inbox<i64, Sum> = Inbox::new(&ctx, u64::MAX, 2).unwrap();
    assert!(inbox.ready());
    assert_eq!(inbox.credits(), 2);

    inbox.receive(&ctx, 5); // credit 2 -> 1
    assert!(inbox.ready());
    inbox.receive(&ctx, 3); // credit 1 -> 0
    assert!(!inbox.ready()); // remote must stop delivering

    // App consumes the coalesced window and replenishes 2 credits.
    let window = inbox.consume(&ctx, 2);
    assert_eq!(window, Some(8)); // 5 + 3
    assert!(inbox.ready());
    assert_eq!(inbox.credits(), 2);
}

/// End-to-end link: Outbox -> (transport) -> Inbox converges to the sent state.
/// KeepLatest is associative, so the converged inbox state is transport-independent.
#[test]
fn outbox_to_inbox_link_converges() {
    let ctx = Context::new();
    let out: Outbox<i64, KeepLatest> = Outbox::new(&ctx, u64::MAX).unwrap();
    let mut inbox: Inbox<i64, KeepLatest> = Inbox::new(&ctx, u64::MAX, 100).unwrap();

    // Producer sends a burst; transport drains the outbox window and delivers it.
    for v in [10i64, 20, 30] {
        out.send(&ctx, v);
    }
    if let Some(delivered) = out.drain(&ctx) {
        inbox.receive(&ctx, delivered);
    }
    assert_eq!(inbox.consume(&ctx, 1), Some(30)); // latest state crossed the link
}
