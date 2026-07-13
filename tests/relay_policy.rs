//! Phase 6 spike tests for the extra reactive policies (`#relaycell`).

use lazily::{
    Context, ExpiryPolicy, KeyedRelay, Max, Overflow, PriorityStorage, RatePolicy, Sum,
    WindowPolicy,
};

/// RatePolicy paces egress (token bucket): drains are permitted only while tokens
/// remain; ticks refill up to capacity.
#[test]
fn rate_policy_token_bucket() {
    let mut rate = RatePolicy::new(3, 2);
    assert!(rate.try_egress()); // 3 -> 2
    assert!(rate.try_egress()); // 2 -> 1
    assert!(rate.try_egress()); // 1 -> 0
    assert!(!rate.try_egress()); // empty: paced/backpressured
    rate.tick(); // +2 -> 2
    assert_eq!(rate.tokens(), 2);
    assert!(rate.try_egress());
    rate.tick();
    rate.tick(); // saturates at capacity 3
    assert_eq!(rate.tokens(), 3);
}

/// WindowPolicy flushes when the window fills OR on an interval tick — a window
/// is a flush group, so the converged state is unchanged.
#[test]
fn window_policy_flushes_on_fill_or_tick() {
    let mut w = WindowPolicy::new(3);
    assert!(!w.on_ingress());
    assert!(!w.on_ingress());
    assert!(w.on_ingress()); // window full -> flush
    assert!(!w.on_ingress());
    assert!(w.tick()); // interval elapsed with pending -> flush
    assert!(!w.tick()); // nothing pending -> no flush
}

/// A WindowPolicy over a Sum relay: whatever the flush grouping, the converged
/// egress is the flat fold (flushGroupingIrrelevant).
#[test]
fn window_grouping_preserves_converged_sum() {
    let ctx = Context::new();
    let ops = [1i64, 2, 3, 4, 5, 6, 7];
    let flat: i64 = ops.iter().sum();

    let mut converged = 0i64;
    let relay: lazily::RelayCell<i64, Sum> = lazily::RelayCell::new(
        &ctx,
        lazily::BackpressurePolicy::new(
            &ctx,
            lazily::BoundDim::Count,
            u64::MAX,
            1,
            Overflow::Conflate,
        ),
    )
    .unwrap();
    let mut w = WindowPolicy::new(3);
    for &op in &ops {
        relay.ingress(&ctx, op);
        if w.on_ingress()
            && let Some(win) = relay.drain(&ctx)
        {
            converged += win;
        }
    }
    if let Some(win) = relay.drain(&ctx) {
        converged += win;
    }
    assert_eq!(converged, flat);
}

/// ExpiryPolicy drops elements older than the TTL against the logical clock.
#[test]
fn expiry_policy_drops_aged() {
    let mut e = ExpiryPolicy::new(10);
    e.advance(5);
    assert!(e.is_live(0)); // age 5 <= ttl 10
    e.advance(8); // now = 13
    assert!(!e.is_live(0)); // age 13 > 10
    assert!(e.is_live(5)); // age 8 <= 10

    let batch = vec![(0u64, "cold"), (5, "warm"), (13, "hot")];
    assert_eq!(e.retain_live(batch), vec!["warm", "hot"]);
}

/// PriorityStorage pops highest priority first, FIFO within equal priority.
#[test]
fn priority_storage_orders_by_priority() {
    let mut pq: PriorityStorage<&str> = PriorityStorage::new();
    pq.push(1, "low-a");
    pq.push(3, "high");
    pq.push(1, "low-b");
    pq.push(2, "mid");
    assert_eq!(pq.len(), 4);
    assert_eq!(pq.pop(), Some("high"));
    assert_eq!(pq.pop(), Some("mid"));
    assert_eq!(pq.pop(), Some("low-a")); // FIFO within priority 1
    assert_eq!(pq.pop(), Some("low-b"));
    assert_eq!(pq.pop(), None);
    assert!(pq.is_empty());
}

/// KeyedRelay shards by key; each key's converged state equals a single relay for
/// that key (commutative merge across shards).
#[test]
fn keyed_relay_shards_by_key() {
    let ctx = Context::new();
    let mut keyed: KeyedRelay<&str, i64, Sum> = KeyedRelay::new(u64::MAX, Overflow::Conflate);
    for (k, v) in [("a", 1), ("b", 10), ("a", 2), ("b", 20), ("a", 3)] {
        keyed.ingress(&ctx, k, v);
    }
    assert_eq!(keyed.drain(&ctx, &"a"), Some(6)); // 1+2+3
    assert_eq!(keyed.drain(&ctx, &"b"), Some(30)); // 10+20
    assert_eq!(keyed.drain(&ctx, &"missing"), None);

    // Idempotent policy across shards behaves too.
    let mut km: KeyedRelay<&str, i64, Max> = KeyedRelay::new(u64::MAX, Overflow::Conflate);
    km.ingress(&ctx, "x", 5);
    km.ingress(&ctx, "x", 9);
    km.ingress(&ctx, "x", 2);
    assert_eq!(km.drain(&ctx, &"x"), Some(9));
}
