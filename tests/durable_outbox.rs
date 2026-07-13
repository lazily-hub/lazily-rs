#![cfg(feature = "ipc")]

use lazily::{Delta, DurableOutbox, InMemoryOutbox, IpcMessage};
use serde_json::Value;

fn fixture() -> Option<Value> {
    let text = std::fs::read_to_string(
        "../lazily-spec/conformance/reliable-sync/outbox_store_protocol.json",
    )
    .ok()?;
    Some(serde_json::from_str(&text).expect("outbox-store fixture JSON"))
}

fn frame(epoch: u64) -> IpcMessage {
    IpcMessage::Delta(Delta::new(epoch.saturating_sub(1), epoch, vec![]))
}

#[test]
fn generic_outbox_retains_orders_prunes_and_keeps_cursor_monotone() {
    let mut outbox = InMemoryOutbox::default();
    outbox.append(3, frame(3));
    outbox.append(1, frame(1));
    outbox.append(2, frame(2));
    assert_eq!(outbox.retained_epochs(), vec![1, 2, 3]);
    assert_eq!(
        outbox
            .replay_from(1)
            .into_iter()
            .map(|(e, _)| e)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    outbox.ack_through(2);
    outbox.ack_through(1);
    assert_eq!(outbox.acked_through(), 2);
    assert_eq!(outbox.retained_epochs(), vec![3]);
}

#[test]
fn generic_outbox_replays_canonical_store_fixture() {
    let Some(fixture) = fixture() else {
        eprintln!("skipping: lazily-spec outbox-store fixture is not present as a sibling");
        return;
    };
    assert_eq!(fixture["model"], "OutboxStore");
    for scenario in fixture["scenarios"].as_array().unwrap() {
        let mut outbox = InMemoryOutbox::default();
        for epoch in scenario["put_epochs"].as_array().unwrap() {
            let epoch = epoch.as_u64().unwrap();
            outbox.append(epoch, frame(epoch));
        }
        let expected = &scenario["expect"];
        if let Some(cursor) = scenario["scan_after"].as_u64() {
            let epochs = outbox
                .replay_from(cursor)
                .into_iter()
                .map(|(epoch, _)| epoch)
                .collect::<Vec<_>>();
            assert_eq!(
                epochs,
                expected["epochs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(Value::as_u64)
                    .collect::<Option<Vec<_>>>()
                    .unwrap()
            );
        }
        if let Some(acks) = scenario["ack_through"].as_array() {
            for ack in acks {
                outbox.ack_through(ack.as_u64().unwrap());
            }
        }
        if let Some(cursor) = expected["cursor"]
            .as_u64()
            .or_else(|| expected["loaded_cursor"].as_u64())
        {
            assert_eq!(outbox.acked_through(), cursor);
        }
        if let Some(retained) = expected["retained"].as_array() {
            assert_eq!(
                outbox.retained_epochs(),
                retained
                    .iter()
                    .map(Value::as_u64)
                    .collect::<Option<Vec<_>>>()
                    .unwrap()
            );
        }
        let replay = expected
            .get("replay_from_zero")
            .or_else(|| expected.get("replay"));
        if let Some(replay) = replay.and_then(Value::as_array) {
            assert_eq!(
                outbox
                    .replay_from(0)
                    .into_iter()
                    .map(|(epoch, _)| epoch)
                    .collect::<Vec<_>>(),
                replay
                    .iter()
                    .map(Value::as_u64)
                    .collect::<Option<Vec<_>>>()
                    .unwrap(),
            );
        }
    }
}

#[cfg(feature = "durable-sqlite")]
#[test]
fn sqlite_outbox_recovers_cursor_and_unacked_suffix_after_reopen() {
    use lazily::SqliteOutbox;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("outbox.db");
    {
        let mut outbox = SqliteOutbox::open(&path, "doc").unwrap();
        outbox.append(1, frame(1));
        outbox.append(2, frame(2));
        outbox.append(3, frame(3));
        outbox.ack_through(1);
    }
    let outbox = SqliteOutbox::open(&path, "doc").unwrap();
    assert_eq!(outbox.acked_through(), 1);
    assert_eq!(outbox.retained_epochs(), vec![2, 3]);
    assert_eq!(
        outbox
            .replay_from(0)
            .into_iter()
            .map(|(e, _)| e)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
}
