//! Canonical lightweight durable-client corpus (`#lzdurablefamily`).

mod common;

use std::collections::VecDeque;

use common::{Expect, FixtureJson};
use lazily::{
    AdvisoryProjectionOrder, CompatibleNatsTransport, DurableClient, DurableClientReceipt,
    DurableDeliveryClassification, DurableEnvelope, DurableProjectionObservation,
    ProjectionDelivery, classify_durable_delivery,
};
use serde_json::Value;

const SPEC_DIR: common::SpecDir = common::SpecDir("durable-client");

fn fixture_path() -> std::path::PathBuf {
    SPEC_DIR.join("envelope_v1.json")
}

fn load_fixture() -> Value {
    let raw = common::spec_read_to_string(fixture_path()).expect("read durable client fixture");
    serde_json::from_str(&raw).expect("parse durable client fixture")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_owned())
        .collect()
}

fn numbers(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_u64().expect("unsigned integer"))
        .collect()
}

#[derive(Debug, Default)]
struct MemoryNats {
    published: Vec<(String, String, Vec<u8>)>,
    received: VecDeque<Vec<u8>>,
}

impl CompatibleNatsTransport for MemoryNats {
    type Error = String;

    fn publish(
        &mut self,
        subject: &str,
        message_id: &str,
        payload: &[u8],
    ) -> Result<(), Self::Error> {
        self.published
            .push((subject.to_owned(), message_id.to_owned(), payload.to_vec()));
        Ok(())
    }

    fn try_receive(&mut self, _subject: &str) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(self.received.pop_front())
    }
}

#[test]
fn envelope_vectors_fail_closed_and_round_trip_over_injected_transport() {
    let fixture = load_fixture();
    assert!(!fixture["owner_authority"].fixture_flag("owner_authority"));
    for (index, row) in fixture["envelope_vectors"]
        .as_array()
        .expect("envelope vectors")
        .iter()
        .enumerate()
    {
        let envelope: DurableEnvelope =
            serde_json::from_value(row["envelope"].clone()).expect("typed envelope");
        let validation = envelope.validate();
        let (accepted, reason) = if validation.is_ok() {
            (true, "accepted")
        } else if envelope.protocol_version != DurableEnvelope::PROTOCOL_VERSION {
            (false, "unsupported_protocol_version")
        } else if envelope.message_id.is_empty() {
            (false, "invalid_message_id")
        } else if envelope.schema_version == 0 {
            (false, "invalid_schema_version")
        } else {
            (false, "invalid_codec_version")
        };
        let expected = Expect::new(
            fixture_path().display().to_string(),
            format!("envelope_vectors[{index}].expected"),
            &row["expected"],
        );
        expected.assert_key("accepted", accepted);
        expected.assert_key("reason", reason);
        expected.assert_key("payload_decoded", validation.is_ok());

        if validation.is_ok() {
            let mut client = DurableClient::new("durable.commands", MemoryNats::default())
                .expect("valid client");
            client.publish(&envelope).expect("publish envelope");
            let transport = client.into_transport();
            let (_, message_id, bytes) = transport.published.into_iter().next().expect("publish");
            assert_eq!(message_id, envelope.message_id);
            let mut receiving = MemoryNats::default();
            receiving.received.push_back(bytes);
            let mut client =
                DurableClient::new("durable.commands", receiving).expect("valid client");
            assert_eq!(client.try_receive().expect("receive"), Some(envelope));
        }
    }
}

#[test]
fn ordering_dedup_receipt_and_projection_fingerprints_replay() {
    let fixture = load_fixture();

    let transport_order = &fixture["ordering_vectors"][0];
    assert_eq!(
        strings(&transport_order["observed_message_ids"]),
        strings(&transport_order["expected_delivery_order"])
    );
    assert!(!transport_order["owner_order_inferred"].fixture_flag("owner_order_inferred"));

    let order = &fixture["projection_ordering_vectors"][0];
    let mut buffer = AdvisoryProjectionOrder::default();
    let mut classifications = Vec::new();
    let mut applied = Vec::new();
    for position in numbers(&order["observed_source_positions"]) {
        let observation = buffer.observe(position);
        classifications.push(match observation.delivery {
            ProjectionDelivery::Buffered => "buffered",
            ProjectionDelivery::Applied => "applied",
            ProjectionDelivery::Duplicate => "duplicate",
        });
        applied.extend(observation.applied_positions);
    }
    assert_eq!(applied, numbers(&order["expected_applied_positions"]));
    assert_eq!(
        classifications,
        strings(&order["expected_delivery_classification"])
    );
    assert!(!order["broker_order_authoritative"].fixture_flag("broker_order_authoritative"));
    assert!(!order["may_authorize_transition"].fixture_flag("may_authorize_transition"));
    assert!(!buffer.may_authorize_transition());

    let dedup = &fixture["dedup_vectors"][0];
    let deliveries: Vec<DurableEnvelope> = dedup["deliveries"]
        .as_array()
        .expect("deliveries")
        .iter()
        .map(|value| serde_json::from_value(value.clone()).expect("delivery envelope"))
        .collect();
    let actual = [
        classify_durable_delivery(None, &deliveries[0]),
        classify_durable_delivery(Some(&deliveries[0]), &deliveries[1]),
        classify_durable_delivery(Some(&deliveries[0]), &deliveries[2]),
    ];
    assert_eq!(
        actual,
        [
            DurableDeliveryClassification::First,
            DurableDeliveryClassification::Duplicate,
            DurableDeliveryClassification::Conflict,
        ]
    );
    assert_eq!(
        strings(&dedup["expected_classification"]),
        ["first", "duplicate", "conflict"]
    );

    let receipt_row = &fixture["receipt_vectors"][0];
    let receipt: DurableClientReceipt =
        serde_json::from_value(receipt_row["receipt"].clone()).expect("typed receipt");
    let expected_receipt: DurableClientReceipt =
        serde_json::from_value(receipt_row["expected_round_trip"].clone())
            .expect("expected typed receipt");
    assert_eq!(receipt, expected_receipt);
    receipt.validate().expect("valid terminal receipt");
    assert_eq!(
        DurableClientReceipt::from_wire(&serde_json::to_vec(&receipt).expect("receipt wire"))
            .expect("observe receipt"),
        receipt
    );
    assert_eq!(
        receipt.transport_ack_equivalent(),
        receipt_row["transport_ack_equivalent"].fixture_flag("transport_ack_equivalent")
    );

    for (index, row) in fixture["projection_fingerprint_vectors"]
        .as_array()
        .expect("fingerprint vectors")
        .iter()
        .enumerate()
    {
        let left: DurableProjectionObservation =
            serde_json::from_value(row["left"].clone()).expect("left observation");
        let right: DurableProjectionObservation =
            serde_json::from_value(row["right"].clone()).expect("right observation");
        assert!(!left.may_authorize_transition);
        assert!(!right.may_authorize_transition);
        left.validate().expect("valid left observation");
        right.validate().expect("valid right observation");
        assert_eq!(
            DurableProjectionObservation::from_wire(
                &serde_json::to_vec(&left).expect("projection wire")
            )
            .expect("observe projection"),
            left
        );
        let expected = Expect::new(
            fixture_path().display().to_string(),
            format!("projection_fingerprint_vectors[{index}].expected"),
            &row["expected"],
        );
        expected.assert_key("same_source", left.source_position == right.source_position);
        expected.assert_key("same_fingerprint", left.fingerprint == right.fingerprint);
        expected.assert_key("same_completeness", left.completeness == right.completeness);
        expected.assert_key("equivalent", left.equivalent_to(&right));
    }
}

#[test]
fn typed_observation_paths_reject_unknown_protocol_identity_and_authority() {
    let mut transport = MemoryNats::default();
    transport.received.push_back(
        br#"{"protocol_version":2,"message_id":"m","schema_version":1,"codec_version":1,"payload":[0,1,2,3]}"#
            .to_vec(),
    );
    let mut client = DurableClient::new("durable.commands", transport).expect("client");
    assert!(
        client
            .try_receive()
            .expect_err("unknown envelope protocol")
            .to_string()
            .contains("unsupported protocol")
    );

    let invalid_receipt = br#"{"protocol_version":1,"receipt_id":"","message_id":"m","outcome":"committed","owner_position":1}"#;
    assert!(DurableClientReceipt::from_wire(invalid_receipt).is_err());

    let invalid_projection = br#"{"projection_id":"orders","source_position":1,"fingerprint":"aabb","completeness":"complete_history","may_authorize_transition":true}"#;
    assert!(DurableProjectionObservation::from_wire(invalid_projection).is_err());
}
