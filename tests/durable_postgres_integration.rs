#![cfg(feature = "durable-postgres")]

use std::env;

use lazily::{
    CodecVersion, DurableCommit, DurableCommitOutcome, DurableEffectIntent, DurableEffectOutcome,
    DurableOwnerId, DurableOwnerMode, DurablePosition, DurableProjectionUpdate,
    DurableReceiptIntent, DurableStateMutation, DurableTimerChange, DurableTimerRecord,
    EffectIdentity, FenceToken, InboxIdentity, PostgresDurableError, PostgresDurableHost,
    PostgresDurableUnitOfWork, PostgresRetryPolicy, ReceiptIdentity, SchemaVersion, TimerIdentity,
    VersionedBytes,
};
use postgres::{Client, NoTls};

fn database_url() -> String {
    env::var("LAZILY_POSTGRES_URL")
        .expect("LAZILY_POSTGRES_URL is set by scripts/test-durable-postgres.sh or CI")
}

fn reset_database() -> PostgresDurableHost {
    let url = database_url();
    let mut client = Client::connect(&url, NoTls).expect("connect to test Postgres");
    client
        .batch_execute(
            "DROP FUNCTION IF EXISTS lazily_fail_first_serializable() CASCADE;
             DROP SEQUENCE IF EXISTS lazily_retry_once;
             DROP TABLE IF EXISTS lazily_durable_ingress_disposition;
             DROP TABLE IF EXISTS lazily_durable_timer;
             DROP TABLE IF EXISTS lazily_durable_outbox;
             DROP TABLE IF EXISTS lazily_durable_owner;",
        )
        .expect("reset durable tables");
    let host = PostgresDurableHost::connect(&url).expect("connect durable host");
    host.migrate().expect("migrate durable host");
    host
}

fn payload(text: &str) -> VersionedBytes {
    VersionedBytes::new(
        SchemaVersion::new(1).unwrap(),
        CodecVersion::new(1).unwrap(),
        text.as_bytes(),
    )
}

fn commit(
    owner_id: &DurableOwnerId,
    inbox_id: &str,
    expected_position: u64,
    event: &str,
    effect_id: &str,
) -> DurableCommit {
    DurableCommit {
        owner_id: owner_id.clone(),
        expected_position: DurablePosition::new(expected_position),
        fence: FenceToken::new(1),
        inbox_identity: InboxIdentity::new(inbox_id).unwrap(),
        ingress_fingerprint: format!("fingerprint:{inbox_id}").into_bytes(),
        state: DurableStateMutation::AppendEvents(vec![payload(event)]),
        effects: vec![DurableEffectIntent {
            identity: EffectIdentity::new(effect_id).unwrap(),
            payload: payload(&format!("publish:{effect_id}")),
        }],
        receipts: Vec::new(),
    }
}

fn work(commit: DurableCommit, projection_version: u64) -> PostgresDurableUnitOfWork {
    PostgresDurableUnitOfWork {
        projection: Some(DurableProjectionUpdate {
            version: projection_version,
            payload: payload(&format!("projection:{projection_version}")),
            fingerprint: format!("projection-fingerprint:{projection_version}").into_bytes(),
        }),
        timers: vec![DurableTimerChange::Upsert(DurableTimerRecord {
            identity: TimerIdentity::new("sample.timer").unwrap(),
            deadline_epoch_millis: 1_000 + i64::try_from(projection_version).unwrap(),
            attempt: 0,
            payload: payload("timer-payload"),
        })],
        commit,
    }
}

#[test]
fn transaction_is_atomic_and_crash_redelivery_is_idempotent() {
    let host = reset_database();
    let owner_id = DurableOwnerId::new("sample-owner").unwrap();
    assert!(
        host.create_owner(
            owner_id.clone(),
            DurableOwnerMode::EventHistory,
            FenceToken::new(1),
        )
        .unwrap()
    );

    let first = work(commit(&owner_id, "inbox-1", 0, "event-1", "effect-1"), 1);
    assert_eq!(
        host.commit_unit_of_work(first.clone()).unwrap(),
        DurableCommitOutcome::Committed {
            through: DurablePosition::new(1)
        }
    );

    // Crash after database commit but before transport ACK: a fresh host sees
    // the same inbox identity and returns the original position without writes.
    let recovered = PostgresDurableHost::connect(&database_url()).unwrap();
    assert_eq!(
        recovered.commit_unit_of_work(first).unwrap(),
        DurableCommitOutcome::Duplicate {
            through: DurablePosition::new(1)
        }
    );

    let before = recovered.load_owner(&owner_id).unwrap().unwrap();
    let projection_before = recovered.load_projection(&owner_id).unwrap().unwrap();
    let timers_before = recovered.load_timers(&owner_id).unwrap();

    // A stale CAS plus new inbox/effect identities fails after the row lock and
    // rolls back the entire state/projection/outbox/timer boundary.
    let stale = work(commit(&owner_id, "inbox-2", 0, "event-2", "effect-2"), 2);
    assert!(matches!(
        recovered.commit_unit_of_work(stale),
        Err(PostgresDurableError::Contract(_))
    ));
    assert_eq!(recovered.load_owner(&owner_id).unwrap().unwrap(), before);
    assert_eq!(
        recovered.load_projection(&owner_id).unwrap().unwrap(),
        projection_before
    );
    assert_eq!(recovered.load_timers(&owner_id).unwrap(), timers_before);

    let image = recovered.load_owner(&owner_id).unwrap().unwrap();
    assert_eq!(image.history.len(), 1);
    assert_eq!(image.history[0].payload.bytes, b"event-1");
    assert_eq!(image.inbox.len(), 1);
    assert_eq!(image.outbox.len(), 1);
}

#[test]
fn relay_claim_retries_publish_before_receipt_with_stable_identity() {
    let host = reset_database();
    let owner_id = DurableOwnerId::new("relay-owner").unwrap();
    host.create_owner(
        owner_id.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(1),
    )
    .unwrap();
    host.commit_unit_of_work(work(
        commit(&owner_id, "inbox-relay", 0, "event", "effect-stable"),
        1,
    ))
    .unwrap();

    let first = host.claim_outbox("worker-a", 0, 100, 10).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].effect.identity.as_str(), "effect-stable");
    assert!(
        host.claim_outbox("worker-b", 50, 150, 10)
            .unwrap()
            .is_empty()
    );

    // Simulate publish success followed by process loss before receipt storage.
    let retry = host.claim_outbox("worker-b", 101, 200, 10).unwrap();
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].effect.identity, first[0].effect.identity);

    let receipt = DurableReceiptIntent {
        identity: ReceiptIdentity::new("receipt-stable").unwrap(),
        effect_identity: retry[0].effect.identity.clone(),
        outcome: DurableEffectOutcome::Applied,
        payload: payload("publication-ack"),
    };
    host.record_publication_receipt(&owner_id, FenceToken::new(1), receipt.clone())
        .unwrap();
    assert!(
        host.claim_outbox("worker-c", 201, 300, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        host.record_publication_receipt(&owner_id, FenceToken::new(1), receipt)
            .unwrap(),
        lazily::DurableReceiptOutcome::Duplicate
    );
}

#[test]
fn serialization_failure_retries_the_complete_transaction() {
    let host = reset_database().with_retry_policy(PostgresRetryPolicy { max_attempts: 3 });
    let owner_id = DurableOwnerId::new("retry-owner").unwrap();
    host.create_owner(
        owner_id.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(1),
    )
    .unwrap();

    // A sequence is intentionally nontransactional: the first UPDATE raises a
    // real SQLSTATE 40001 and rolls back, while the retry observes nextval=2.
    let mut client = Client::connect(&database_url(), NoTls).unwrap();
    client
        .batch_execute(
            "CREATE SEQUENCE lazily_retry_once START 1;
             CREATE FUNCTION lazily_fail_first_serializable() RETURNS trigger AS $$
             BEGIN
                 IF nextval('lazily_retry_once') = 1 THEN
                     RAISE EXCEPTION 'deterministic serialization retry' USING ERRCODE = '40001';
                 END IF;
                 RETURN NEW;
             END;
             $$ LANGUAGE plpgsql;
             CREATE TRIGGER lazily_retry_once_trigger
             BEFORE UPDATE ON lazily_durable_owner
             FOR EACH ROW EXECUTE FUNCTION lazily_fail_first_serializable();",
        )
        .unwrap();

    let outcome = host
        .commit_unit_of_work(work(
            commit(&owner_id, "inbox-retry", 0, "event", "effect-retry"),
            1,
        ))
        .unwrap();
    assert_eq!(
        outcome,
        DurableCommitOutcome::Committed {
            through: DurablePosition::new(1)
        }
    );
    assert_eq!(
        host.load_owner(&owner_id).unwrap().unwrap().history.len(),
        1
    );
}

#[test]
fn full_event_history_rebuild_survives_multiple_commits() {
    let host = reset_database();
    let owner_id = DurableOwnerId::new("history-owner").unwrap();
    host.create_owner(
        owner_id.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(1),
    )
    .unwrap();
    host.commit_unit_of_work(work(
        commit(&owner_id, "inbox-a", 0, "create", "effect-a"),
        1,
    ))
    .unwrap();
    host.commit_unit_of_work(work(
        commit(&owner_id, "inbox-b", 1, "amend", "effect-b"),
        2,
    ))
    .unwrap();

    let rebuilt = PostgresDurableHost::connect(&database_url())
        .unwrap()
        .load_owner(&owner_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        rebuilt
            .history
            .iter()
            .map(|record| record.position.get())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(rebuilt.history[0].payload.bytes, b"create");
    assert_eq!(rebuilt.history[1].payload.bytes, b"amend");
}
