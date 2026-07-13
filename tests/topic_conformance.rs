//! Canonical TopicCell broadcast/cursor/retention conformance (#lztopiccell).

use lazily::{
    Context, TopicCell, TopicDurability, TopicSnapshot, TopicSubscribeOutcome,
    TopicSubscriptionSnapshot,
};
use std::collections::HashMap;

#[test]
fn broadcast_delivery_and_cursor_isolation() {
    let ctx = Context::new();
    let topic = TopicCell::<String>::new(&ctx);
    let alpha = "alpha".to_owned();
    let beta = "beta".to_owned();

    assert_eq!(
        topic.subscribe(&ctx, alpha.clone(), TopicDurability::Durable),
        TopicSubscribeOutcome::Created
    );
    assert_eq!(
        topic.subscribe(&ctx, beta.clone(), TopicDurability::Durable),
        TopicSubscribeOutcome::Created
    );
    assert_eq!(topic.publish(&ctx, "a".into()), 0);
    assert_eq!(topic.read_stream(&ctx, &alpha), ["a"]);
    assert_eq!(topic.read_stream(&ctx, &beta), ["a"]);

    assert_eq!(topic.advance(&ctx, &alpha).as_deref(), Some("a"));
    assert!(topic.read_stream(&ctx, &alpha).is_empty());
    assert_eq!(topic.read_stream(&ctx, &beta), ["a"]);

    topic.publish(&ctx, "b".into());
    assert_eq!(topic.read_stream(&ctx, &alpha), ["b"]);
    assert_eq!(topic.read_stream(&ctx, &beta), ["a", "b"]);
    assert_eq!(topic.advance(&ctx, &beta).as_deref(), Some("a"));
    assert_eq!(topic.read_stream(&ctx, &alpha), ["b"]);
    assert_eq!(topic.read_stream(&ctx, &beta), ["b"]);
}

#[test]
fn durable_restart_replay_and_slowest_cursor_gc() {
    let ctx = Context::new();
    let fast = "fast".to_owned();
    let slow = "slow".to_owned();
    let topic = TopicCell::<String>::from_snapshot(
        &ctx,
        TopicSnapshot {
            base_offset: 0,
            elements: ["a", "b", "c"].map(str::to_owned).into(),
            subscriptions: HashMap::from([
                (
                    fast.clone(),
                    TopicSubscriptionSnapshot {
                        cursor: 3,
                        durability: TopicDurability::Durable,
                        connected: true,
                    },
                ),
                (
                    slow.clone(),
                    TopicSubscriptionSnapshot {
                        cursor: 0,
                        durability: TopicDurability::Durable,
                        connected: true,
                    },
                ),
            ]),
        },
    );

    assert!(topic.disconnect(&ctx, &slow));
    topic.publish(&ctx, "d".into());
    let restored = TopicCell::from_snapshot(&ctx, topic.snapshot());
    assert_eq!(restored.subscription(&slow).unwrap().cursor, 0);
    assert_eq!(
        restored.reconnect(&ctx, slow.clone()),
        TopicSubscribeOutcome::Reconnected
    );
    assert_eq!(restored.read_stream(&ctx, &slow), ["a", "b", "c", "d"]);
    assert_eq!(restored.gc(), 0);
    assert_eq!(restored.advance(&ctx, &slow).as_deref(), Some("a"));
    assert_eq!(restored.advance(&ctx, &slow).as_deref(), Some("b"));
    assert_eq!(restored.gc(), 2);
    assert_eq!(restored.base_offset(), 2);
    assert_eq!(restored.elements(), ["c", "d"]);
    assert_eq!(restored.read_stream(&ctx, &fast), ["d"]);
    assert_eq!(restored.read_stream(&ctx, &slow), ["c", "d"]);
}

#[test]
fn ephemeral_lifecycle_starts_at_tail_and_never_holds_gc() {
    let ctx = Context::new();
    let topic = TopicCell::<String>::new(&ctx);
    let ephemeral = "ephemeral".to_owned();
    topic.publish(&ctx, "old".into());
    topic.subscribe(&ctx, ephemeral.clone(), TopicDurability::Ephemeral);
    assert!(topic.read_stream(&ctx, &ephemeral).is_empty());
    topic.publish(&ctx, "live".into());
    assert_eq!(topic.advance(&ctx, &ephemeral).as_deref(), Some("live"));
    assert!(topic.disconnect(&ctx, &ephemeral));
    assert!(topic.subscription(&ephemeral).is_none());

    topic.publish(&ctx, "missed".into());
    topic.subscribe(&ctx, ephemeral.clone(), TopicDurability::Ephemeral);
    assert!(topic.read_stream(&ctx, &ephemeral).is_empty());
    assert_eq!(topic.gc(), 3);
    assert_eq!(topic.base_offset(), 3);
    assert!(topic.elements().is_empty());
}

#[test]
fn per_subscriber_reader_invalidation_is_independent() {
    let ctx = Context::new();
    let topic = TopicCell::<i32>::new(&ctx);
    let alpha = "alpha".to_owned();
    let beta = "beta".to_owned();
    topic.subscribe(&ctx, alpha.clone(), TopicDurability::Durable);
    topic.subscribe(&ctx, beta.clone(), TopicDurability::Durable);
    topic.publish(&ctx, 1);

    let alpha_reader = topic.reader_handle(&alpha).unwrap();
    let beta_reader = topic.reader_handle(&beta).unwrap();
    assert_eq!(ctx.get(&alpha_reader), vec![1]);
    assert_eq!(ctx.get(&beta_reader), vec![1]);
    assert!(ctx.is_set(&alpha_reader));
    assert!(ctx.is_set(&beta_reader));

    assert_eq!(topic.advance(&ctx, &alpha), Some(1));
    assert!(!ctx.is_set(&alpha_reader));
    assert!(ctx.is_set(&beta_reader));
    assert_eq!(ctx.get(&beta_reader), vec![1]);

    topic.publish(&ctx, 2);
    assert!(!ctx.is_set(&alpha_reader));
    assert!(!ctx.is_set(&beta_reader));
}

#[test]
fn tail_and_offline_advance_are_noops() {
    let ctx = Context::new();
    let topic = TopicCell::<String>::new(&ctx);
    let worker = "worker".to_owned();

    topic.subscribe(&ctx, worker.clone(), TopicDurability::Durable);
    topic.publish(&ctx, "a".into());
    assert_eq!(topic.advance(&ctx, &worker).as_deref(), Some("a"));
    assert_eq!(topic.advance(&ctx, &worker), None);
    assert_eq!(topic.subscription(&worker).unwrap().cursor, 1);

    assert!(topic.disconnect(&ctx, &worker));
    topic.publish(&ctx, "b".into());
    assert!(topic.read_stream(&ctx, &worker).is_empty());
    assert_eq!(topic.advance(&ctx, &worker), None);
    assert_eq!(topic.subscription(&worker).unwrap().cursor, 1);

    assert_eq!(
        topic.reconnect(&ctx, worker.clone()),
        TopicSubscribeOutcome::Reconnected
    );
    assert_eq!(topic.read_stream(&ctx, &worker), ["b"]);
    assert_eq!(topic.gc(), 1);
    assert_eq!(topic.base_offset(), 1);
    assert_eq!(topic.subscription(&worker).unwrap().cursor, 1);
}

#[test]
#[should_panic(expected = "disconnected ephemeral TopicCell subscriptions must be removed")]
fn snapshot_rejects_disconnected_ephemeral_subscription() {
    let ctx = Context::new();
    let _ = TopicCell::<String>::from_snapshot(
        &ctx,
        TopicSnapshot {
            base_offset: 0,
            elements: Vec::new(),
            subscriptions: HashMap::from([(
                "viewer".to_owned(),
                TopicSubscriptionSnapshot {
                    cursor: 0,
                    durability: TopicDurability::Ephemeral,
                    connected: false,
                },
            )]),
        },
    );
}
