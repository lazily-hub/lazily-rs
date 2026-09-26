#![cfg(feature = "durable-jetstream")]

use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_nats::jetstream;
use futures_util::StreamExt;
use lazily::{
    CodecVersion, DurableCommit, DurableCommitOutcome, DurableEffectIntent, DurableOwnerId,
    DurableOwnerMode, DurablePosition, DurableProjectionUpdate, DurableStateMutation,
    EffectIdentity, FenceToken, InboxIdentity, JetStreamDeadLetterSink, JetStreamEnvelope,
    JetStreamIngress, JetStreamIngressConfig, JetStreamOutboxRelay, JetStreamPublisher,
    PostgresDurableHost, PostgresDurableUnitOfWork, SchemaVersion, VersionedBytes,
};
use postgres::{Client, NoTls};

macro_rules! pg {
    ($expression:expr) => {
        tokio::task::block_in_place(|| $expression)
    };
}

fn payload(text: &str) -> VersionedBytes {
    VersionedBytes::new(
        SchemaVersion::new(1).unwrap(),
        CodecVersion::new(1).unwrap(),
        text.as_bytes(),
    )
}

fn database_url() -> String {
    env::var("LAZILY_POSTGRES_URL")
        .expect("LAZILY_POSTGRES_URL is set by scripts/test-durable-jetstream.sh or CI")
}

fn nats_url() -> String {
    env::var("LAZILY_NATS_URL")
        .expect("LAZILY_NATS_URL is set by scripts/test-durable-jetstream.sh or CI")
}

fn reset_database() -> PostgresDurableHost {
    let url = database_url();
    let mut client = Client::connect(&url, NoTls).expect("connect to test Postgres");
    client
        .batch_execute(
            "DROP TABLE IF EXISTS lazily_durable_ingress_disposition;
             DROP TABLE IF EXISTS lazily_durable_timer;
             DROP TABLE IF EXISTS lazily_durable_outbox;
             DROP TABLE IF EXISTS lazily_durable_owner;",
        )
        .expect("reset durable tables");
    let host = PostgresDurableHost::connect(&url).expect("connect durable host");
    host.migrate().expect("migrate durable host");
    host
}

fn work(owner_id: &DurableOwnerId, envelope: &JetStreamEnvelope) -> PostgresDurableUnitOfWork {
    PostgresDurableUnitOfWork {
        projection: Some(DurableProjectionUpdate {
            version: 1,
            payload: payload("projection"),
            fingerprint: b"projection-v1".to_vec(),
        }),
        timers: Vec::new(),
        commit: DurableCommit {
            owner_id: owner_id.clone(),
            expected_position: DurablePosition::new(0),
            fence: FenceToken::new(1),
            inbox_identity: InboxIdentity::new(envelope.message_id.clone()).unwrap(),
            ingress_fingerprint: envelope.payload.clone(),
            state: DurableStateMutation::AppendEvents(vec![envelope.versioned_bytes().unwrap()]),
            effects: vec![DurableEffectIntent {
                identity: EffectIdentity::new(format!("effect.{}", envelope.message_id)).unwrap(),
                payload: payload("relay-payload"),
            }],
            receipts: Vec::new(),
        },
    }
}

fn unique_namespace() -> (String, String) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let suffix = format!("{}{}", std::process::id(), nanos);
    (format!("LZJS{suffix}"), format!("lzjs.{suffix}"))
}

async fn publish(publisher: &JetStreamPublisher, subject: &str, id: &str, body: &str) {
    publisher
        .publish(
            subject,
            &JetStreamEnvelope::new(id, &payload(body)).unwrap(),
        )
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_transport_crash_windows_preserve_postgres_authority() {
    let host = pg!(reset_database());
    let client = async_nats::connect(nats_url()).await.unwrap();
    let context = jetstream::new(client.clone());
    let publisher = JetStreamPublisher::new(context.clone());
    let (stream_name, root) = unique_namespace();
    let ingress_subject = format!("{root}.ingress");
    let output_subject = format!("{root}.output");
    let missing_subject = format!("missing.{root}");
    let dead_subject = format!("{root}.dead");

    context
        .get_or_create_stream(jetstream::stream::Config {
            name: stream_name.clone(),
            subjects: vec![format!("{root}.>")],
            duplicate_window: Duration::from_secs(120),
            ..Default::default()
        })
        .await
        .unwrap();

    let ingress = JetStreamIngress::bind(
        context.clone(),
        JetStreamIngressConfig {
            stream: stream_name.clone(),
            subject: ingress_subject.clone(),
            durable_consumer: format!("consumer-{}", std::process::id()),
            ack_wait: Duration::from_millis(250),
            max_deliver: 20,
            max_ack_pending: 1,
            fetch_timeout: Duration::from_millis(120),
        },
    )
    .await
    .unwrap();

    // A failed database operation sends no ACK. The same broker delivery returns.
    publish(&publisher, &ingress_subject, "commit-fails", "first").await;
    let delivery = ingress.next().await.unwrap().unwrap();
    let first_sequence = delivery.metadata().stream_sequence;
    let unavailable_owner = DurableOwnerId::new("unavailable-owner").unwrap();
    let unavailable_work = work(&unavailable_owner, delivery.envelope().unwrap());
    assert!(
        delivery
            .commit_then_ack(&host, unavailable_work)
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let delivery = ingress.next().await.unwrap().unwrap();
    assert_eq!(delivery.metadata().stream_sequence, first_sequence);
    delivery.retry(None).await.unwrap();

    // A progress ACK extends transport ownership only: Postgres remains empty.
    let delivery = ingress.next().await.unwrap().unwrap();
    delivery.progress().await.unwrap();
    assert!(
        pg!(host.load_owner(&DurableOwnerId::new("progress-owner").unwrap()))
            .unwrap()
            .is_none()
    );
    delivery.retry(None).await.unwrap();
    let delivery = ingress.next().await.unwrap().unwrap();
    delivery
        .retry(Some(Duration::from_millis(10)))
        .await
        .unwrap();
    let delivery = ingress.next().await.unwrap().unwrap();
    delivery.retry(None).await.unwrap();

    // Isolate the remainder from the deliberately retried delivery.
    let retry_owner = DurableOwnerId::new("retry-owner").unwrap();
    pg!(host.create_owner(
        retry_owner.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(1),
    ))
    .unwrap();
    let delivery = ingress.next().await.unwrap().unwrap();
    let retry_work = work(&retry_owner, delivery.envelope().unwrap());
    delivery.commit_then_ack(&host, retry_work).await.unwrap();

    // Crash after COMMIT and before ACK: redelivery is consumed through inbox
    // deduplication and the ACK follows the duplicate durable result.
    let owner_id = DurableOwnerId::new("commit-before-ack-owner").unwrap();
    pg!(host.create_owner(
        owner_id.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(1),
    ))
    .unwrap();
    publish(&publisher, &ingress_subject, "commit-before-ack", "event").await;
    let delivery = ingress.next().await.unwrap().unwrap();
    let committed = work(&owner_id, delivery.envelope().unwrap());
    assert!(matches!(
        pg!(host.commit_unit_of_work(committed)).unwrap(),
        DurableCommitOutcome::Committed { .. }
    ));
    drop(delivery);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let delivery = ingress.next().await.unwrap().unwrap();
    let duplicate_work = work(&owner_id, delivery.envelope().unwrap());
    assert!(matches!(
        delivery
            .commit_then_ack(&host, duplicate_work)
            .await
            .unwrap(),
        DurableCommitOutcome::Duplicate { .. }
    ));
    assert_eq!(
        pg!(host.load_owner(&owner_id))
            .unwrap()
            .unwrap()
            .history
            .len(),
        1
    );

    // MaxAckPending=1 prevents a second pull while the first delivery is held.
    publish(&publisher, &ingress_subject, "bounded-a", "a").await;
    publish(&publisher, &ingress_subject, "bounded-b", "b").await;
    let held = ingress.next().await.unwrap().unwrap();
    assert!(ingress.next().await.unwrap().is_none());
    held.retry(None).await.unwrap();
    let releasable = ingress.next().await.unwrap().unwrap();
    releasable.retry(None).await.unwrap();
    let releasable = ingress.next().await.unwrap().unwrap();
    releasable.retry(None).await.unwrap();

    // Drain stops new pulls but does not revoke a delivery already handed out.
    let draining = ingress.next().await.unwrap().unwrap();
    ingress.begin_drain();
    assert!(ingress.next().await.unwrap().is_none());
    draining.retry(None).await.unwrap();

    // Poison disposition is durable before TERM. A core subscription observes
    // the stable dead-letter publication, while Postgres records the bytes.
    let poison_ingress = JetStreamIngress::bind(
        context.clone(),
        JetStreamIngressConfig {
            stream: stream_name,
            subject: format!("{root}.poison"),
            durable_consumer: format!("poison-consumer-{}", std::process::id()),
            ack_wait: Duration::from_millis(250),
            max_deliver: 5,
            max_ack_pending: 1,
            fetch_timeout: Duration::from_millis(120),
        },
    )
    .await
    .unwrap();
    let mut dead_subscription = client.subscribe(dead_subject.clone()).await.unwrap();
    client
        .publish(format!("{root}.poison"), "not-json".into())
        .await
        .unwrap();
    client.flush().await.unwrap();
    let poison = poison_ingress.next().await.unwrap().unwrap();
    let poison_id = poison.metadata().delivery_id();
    assert!(poison.envelope().is_err());
    let dead_letters = JetStreamDeadLetterSink::new(publisher.clone(), dead_subject).unwrap();
    assert!(
        poison
            .persist_poison_then_term(&host, &dead_letters, 1_000)
            .await
            .unwrap()
    );
    tokio::time::timeout(Duration::from_secs(1), dead_subscription.next())
        .await
        .unwrap()
        .unwrap();
    let disposition = pg!(host.load_ingress_poison("nats-jetstream", &poison_id))
        .unwrap()
        .unwrap();
    assert_eq!(disposition.payload, b"not-json");

    // Publish failure records no receipt; a lease retry can publish elsewhere.
    let relay_claim = pg!(host.claim_outbox("relay-a", 0, 10, 10)).unwrap();
    assert!(!relay_claim.is_empty());
    let failed_relay = JetStreamOutboxRelay::new(publisher.clone(), missing_subject).unwrap();
    assert!(
        failed_relay
            .publish_claim(&host, &relay_claim[0], FenceToken::new(1))
            .await
            .is_err()
    );
    let retry_claim = pg!(host.claim_outbox("relay-b", 11, 20, 10)).unwrap();
    assert_eq!(
        retry_claim[0].effect.identity,
        relay_claim[0].effect.identity
    );

    // Publish success followed by loss before receipt: the stable Nats-Msg-Id
    // makes the retry a broker duplicate, then the Postgres receipt retires it.
    let message_id = format!(
        "{}.{}",
        retry_claim[0].owner_id.as_str(),
        retry_claim[0].effect.identity.as_str()
    );
    let envelope = JetStreamEnvelope::new(message_id, &retry_claim[0].effect.payload).unwrap();
    let first_publication = publisher.publish(&output_subject, &envelope).await.unwrap();
    assert!(!first_publication.duplicate);
    let crashed_claim = pg!(host.claim_outbox("relay-c", 21, 30, 10)).unwrap();
    let relay = JetStreamOutboxRelay::new(publisher, output_subject).unwrap();
    let outcome = relay
        .publish_claim(&host, &crashed_claim[0], FenceToken::new(1))
        .await
        .unwrap();
    assert!(outcome.publication.duplicate);
    let remaining = pg!(host.claim_outbox("relay-d", 31, 40, 10)).unwrap();
    assert!(
        remaining
            .iter()
            .all(|claim| claim.effect.identity != crashed_claim[0].effect.identity)
    );
    pg!(drop(host));
}
