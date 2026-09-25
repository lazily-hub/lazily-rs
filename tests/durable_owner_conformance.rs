//! Canonical backend-neutral durable-owner contracts (`#lzdurablespec`).

mod common;

use common::Expect;
use lazily::{
    CodecVersion, DurableCommit, DurableCommitOutcome, DurableContractError, DurableEffectIntent,
    DurableEffectOutcome, DurableOwnerCore, DurableOwnerId, DurableOwnerMode, DurablePosition,
    DurableProjectionFingerprint, DurableReceiptIntent, DurableReceiptOutcome,
    DurableStateMutation, EffectIdentity, FenceToken, InboxIdentity, ReceiptIdentity, ReplayValue,
    SchemaVersion, VersionedBytes,
};
use serde_json::{Value, json};

const SPEC_DIR: common::SpecDir = common::SpecDir("durable-owner");

fn load_fixture(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw = common::spec_read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    serde_json::from_str(&raw).unwrap_or_else(|error| panic!("failed to parse {path}: {error}"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("`{key}` must be a string in {value}"))
}

fn number(value: &Value, key: &str) -> u64 {
    value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("`{key}` must be an unsigned integer in {value}"))
}

fn schema(value: u64) -> SchemaVersion {
    SchemaVersion::new(u32::try_from(value).expect("schema version fits u32"))
        .expect("schema version is positive")
}

fn codec(value: u64) -> CodecVersion {
    CodecVersion::new(u32::try_from(value).expect("codec version fits u32"))
        .expect("codec version is positive")
}

fn payload(value: &Value) -> VersionedBytes {
    VersionedBytes::new(
        schema(number(value, "schema_version")),
        codec(number(value, "codec_version")),
        text(value, "bytes").as_bytes(),
    )
}

fn effect(value: &Value) -> DurableEffectIntent {
    DurableEffectIntent {
        identity: EffectIdentity::new(text(value, "id")).expect("non-empty effect id"),
        payload: payload(&value["payload"]),
    }
}

fn receipt(value: &Value) -> DurableReceiptIntent {
    DurableReceiptIntent {
        identity: ReceiptIdentity::new(text(value, "id")).expect("non-empty receipt id"),
        effect_identity: EffectIdentity::new(text(value, "effect_id"))
            .expect("non-empty effect id"),
        outcome: match text(value, "outcome") {
            "applied" => DurableEffectOutcome::Applied,
            "rejected" => DurableEffectOutcome::Rejected,
            other => panic!("unknown durable receipt outcome `{other}`"),
        },
        payload: payload(&value["payload"]),
    }
}

fn commit(value: &Value) -> DurableCommit {
    let state = &value["state"];
    let state = if let Some(events) = state.get("append_events") {
        DurableStateMutation::AppendEvents(
            events
                .as_array()
                .expect("append_events must be an array")
                .iter()
                .map(payload)
                .collect(),
        )
    } else if let Some(snapshot) = state.get("replace_snapshot") {
        DurableStateMutation::ReplaceSnapshot(payload(snapshot))
    } else {
        panic!("unknown durable state mutation {state}");
    };
    DurableCommit {
        owner_id: DurableOwnerId::new(text(value, "owner_id")).expect("non-empty owner id"),
        expected_position: DurablePosition::new(number(value, "expected_position")),
        fence: FenceToken::new(number(value, "fence")),
        inbox_identity: InboxIdentity::new(text(value, "inbox_id")).expect("non-empty inbox id"),
        ingress_fingerprint: text(value, "ingress_fingerprint").as_bytes().to_vec(),
        state,
        effects: value["effects"]
            .as_array()
            .expect("effects must be an array")
            .iter()
            .map(effect)
            .collect(),
        receipts: value["receipts"]
            .as_array()
            .expect("receipts must be an array")
            .iter()
            .map(receipt)
            .collect(),
    }
}

fn payload_json(value: &VersionedBytes) -> Value {
    json!({
        "schema_version": value.schema_version.get(),
        "codec_version": value.codec_version.get(),
        "bytes": String::from_utf8(value.bytes.clone()).expect("fixture payloads are UTF-8")
    })
}

fn image_json(core: &DurableOwnerCore) -> Value {
    let image = core.image();
    let history = image
        .history
        .iter()
        .map(|record| {
            json!({
                "position": record.position.get(),
                "payload": payload_json(&record.payload)
            })
        })
        .collect::<Vec<_>>();
    let snapshot = image.snapshot.as_ref().map_or(Value::Null, |record| {
        json!({
            "position": record.position.get(),
            "payload": payload_json(&record.payload)
        })
    });
    let inbox = image
        .inbox
        .values()
        .map(|record| {
            json!({
                "id": record.identity.as_str(),
                "ingress_fingerprint": String::from_utf8(record.ingress_fingerprint.clone())
                    .expect("fixture fingerprints are UTF-8"),
                "committed_through": record.committed_through.get()
            })
        })
        .collect::<Vec<_>>();
    let outbox = image
        .outbox
        .values()
        .map(|effect| {
            json!({
                "id": effect.identity.as_str(),
                "accepted_at": effect.accepted_at.get(),
                "payload": payload_json(&effect.payload)
            })
        })
        .collect::<Vec<_>>();
    let receipts = image
        .receipts
        .values()
        .map(|receipt| {
            json!({
                "id": receipt.identity.as_str(),
                "effect_id": receipt.effect_identity.as_str(),
                "outcome": match receipt.outcome {
                    DurableEffectOutcome::Applied => "applied",
                    DurableEffectOutcome::Rejected => "rejected",
                },
                "recorded_at": receipt.recorded_at.get(),
                "fence": receipt.fence.get(),
                "payload": payload_json(&receipt.payload)
            })
        })
        .collect::<Vec<_>>();
    let pending = core
        .pending_effects()
        .iter()
        .map(|effect| effect.identity.as_str())
        .collect::<Vec<_>>();
    json!({
        "position": image.position.get(),
        "fence": image.fence.get(),
        "history": history,
        "snapshot": snapshot,
        "inbox": inbox,
        "outbox": outbox,
        "receipts": receipts,
        "pending_effect_ids": pending
    })
}

fn commit_result(result: Result<DurableCommitOutcome, DurableContractError>) -> Value {
    match result {
        Ok(DurableCommitOutcome::Committed { through }) => {
            json!({"outcome": "committed", "through": through.get(), "ack": "safe"})
        }
        Ok(DurableCommitOutcome::Duplicate { through }) => {
            json!({"outcome": "duplicate", "through": through.get(), "ack": "safe"})
        }
        Err(DurableContractError::PositionConflict { .. }) => {
            json!({"outcome": "position_conflict", "ack": "withhold"})
        }
        Err(DurableContractError::StaleFence { .. }) => {
            json!({"outcome": "stale_fence", "ack": "withhold"})
        }
        Err(DurableContractError::InboxIdentityConflict(_)) => {
            json!({"outcome": "inbox_identity_conflict", "ack": "withhold"})
        }
        Err(DurableContractError::EffectIdentityConflict(_)) => {
            json!({"outcome": "effect_identity_conflict", "ack": "withhold"})
        }
        Err(error) => panic!("unexpected durable commit result: {error:?}"),
    }
}

fn receipt_result(result: Result<DurableReceiptOutcome, DurableContractError>) -> Value {
    match result {
        Ok(DurableReceiptOutcome::Recorded) => json!({"outcome": "receipt_recorded"}),
        Ok(DurableReceiptOutcome::Duplicate) => json!({"outcome": "receipt_duplicate"}),
        Err(DurableContractError::ReceiptIdentityConflict(_)) => {
            json!({"outcome": "receipt_identity_conflict"})
        }
        Err(DurableContractError::EffectAlreadyReceipted(_)) => {
            json!({"outcome": "effect_already_receipted"})
        }
        Err(DurableContractError::StaleFence { .. }) => {
            json!({"outcome": "stale_fence", "ack": "withhold"})
        }
        Err(error) => panic!("unexpected durable receipt result: {error:?}"),
    }
}

fn replay_operations(name: &str) {
    let path = format!("{SPEC_DIR}/{name}");
    let fixture = load_fixture(name);
    for (_, scenario_id, scenario) in common::scenarios(&path, &fixture) {
        let scenario = scenario.value();
        let mode = match text(scenario, "mode") {
            "event_history" => DurableOwnerMode::EventHistory,
            "snapshot" => DurableOwnerMode::Snapshot,
            other => panic!("unknown durable owner mode `{other}`"),
        };
        let mut core = DurableOwnerCore::new(
            DurableOwnerId::new(text(scenario, "owner_id")).expect("non-empty owner id"),
            mode,
            FenceToken::new(number(scenario, "fence")),
        );
        let steps = scenario["steps"]
            .as_array()
            .expect("steps must be an array");
        let mut executed = 0;
        for (index, step) in steps.iter().enumerate() {
            let op = &step["op"];
            let actual_result = match text(op, "type") {
                "crash_before_commit" => {
                    let _validated = commit(&op["commit"]);
                    json!({"outcome": "aborted", "ack": "withhold"})
                }
                "commit" => commit_result(core.commit(commit(&op["commit"]))),
                "crash_recover" => {
                    core = DurableOwnerCore::recover(core.into_image())
                        .expect("the committed durable image reconstructs");
                    json!({"outcome": "recovered"})
                }
                "advance_fence" => {
                    core.advance_fence(FenceToken::new(number(op, "fence")))
                        .expect("fixture advances the fence");
                    json!({"outcome": "fence_advanced"})
                }
                "record_receipt" => receipt_result(core.record_receipt(
                    FenceToken::new(number(op, "fence")),
                    receipt(&op["receipt"]),
                )),
                other => panic!("unknown durable owner operation `{other}`"),
            };
            let expect = Expect::new(
                &path,
                format!("{scenario_id}.steps[{index}].expect"),
                &step["expect"],
            );
            expect.assert_key("result", actual_result);
            expect.assert_key("image", image_json(&core));
            executed += 1;
        }
        assert_eq!(executed, steps.len(), "{path}: every loaded step executes");
    }
}

fn history_core(owner: &str, fence: u64, records: &[Value]) -> DurableOwnerCore {
    let owner_id = DurableOwnerId::new(owner).expect("non-empty owner id");
    let mut core = DurableOwnerCore::new(
        owner_id.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(fence),
    );
    for (index, record) in records.iter().enumerate() {
        let expected = u64::try_from(index).expect("fixture index fits u64");
        assert_eq!(number(record, "position"), expected + 1);
        core.commit(DurableCommit {
            owner_id: owner_id.clone(),
            expected_position: DurablePosition::new(expected),
            fence: FenceToken::new(fence),
            inbox_identity: InboxIdentity::new(format!("fingerprint/{index}"))
                .expect("generated inbox id is non-empty"),
            ingress_fingerprint: format!("fingerprint/{index}").into_bytes(),
            state: DurableStateMutation::AppendEvents(vec![payload(&record["payload"])]),
            effects: Vec::new(),
            receipts: Vec::new(),
        })
        .expect("canonical history commits");
    }
    core
}

fn relation<T: PartialEq>(left: &T, right: &T) -> &'static str {
    if left == right { "equal" } else { "different" }
}

#[test]
fn operation_fixtures_replay_against_the_reference_core() {
    replay_operations("atomic_crash_boundary.json");
    replay_operations("ordered_replay.json");
    replay_operations("inbox_outbox_deduplication.json");
}

#[test]
fn projection_fingerprint_fixture_separates_content_from_history() {
    let name = "projection_fingerprint.json";
    let path = format!("{SPEC_DIR}/{name}");
    let fixture = load_fixture(name);
    let mut equal_relations = 0;
    let mut different_relations = 0;
    for (_, scenario_id, scenario) in common::scenarios(&path, &fixture) {
        let scenario = scenario.value();
        let case = &scenario["fingerprint_case"];
        let left_history = case["left_history"].as_array().expect("left history array");
        let right_history = case["right_history"]
            .as_array()
            .expect("right history array");
        let left_core = history_core(
            text(scenario, "owner_id"),
            number(scenario, "fence"),
            left_history,
        );
        let right_core = history_core(
            text(scenario, "owner_id"),
            number(scenario, "fence"),
            right_history,
        );
        let left_projection = payload(&case["left_projection"]);
        let right_projection = payload(&case["right_projection"]);
        let left_complete = left_core
            .complete_history_fingerprint(
                left_projection.schema_version,
                left_projection.codec_version,
                ReplayValue::Bytes(left_projection.bytes.clone()),
            )
            .expect("left history fingerprint");
        let right_complete = right_core
            .complete_history_fingerprint(
                right_projection.schema_version,
                right_projection.codec_version,
                ReplayValue::Bytes(right_projection.bytes.clone()),
            )
            .expect("right history fingerprint");
        let projection_relation = relation(
            &(
                left_complete.schema_version,
                left_complete.codec_version,
                &left_complete.projection_digest,
            ),
            &(
                right_complete.schema_version,
                right_complete.codec_version,
                &right_complete.projection_digest,
            ),
        );
        let history_relation =
            relation(&left_complete.source_digest, &right_complete.source_digest);

        let left_latest = payload(&case["left_latest"]);
        let right_latest = payload(&case["right_latest"]);
        let left_latest = DurableProjectionFingerprint::latest_durable_projection(
            DurableOwnerId::new(text(scenario, "owner_id")).unwrap(),
            DurablePosition::new(u64::try_from(left_history.len()).unwrap()),
            &left_latest,
            ReplayValue::Bytes(left_latest.bytes.clone()),
        )
        .expect("left latest fingerprint");
        let right_latest = DurableProjectionFingerprint::latest_durable_projection(
            DurableOwnerId::new(text(scenario, "owner_id")).unwrap(),
            DurablePosition::new(u64::try_from(right_history.len()).unwrap()),
            &right_latest,
            ReplayValue::Bytes(right_latest.bytes.clone()),
        )
        .expect("right latest fingerprint");
        let latest_relation = relation(&left_latest, &right_latest);

        let expect = Expect::new(
            &path,
            format!("{scenario_id}.fingerprint_case.expect"),
            &case["expect"],
        );
        expect.assert_key("projection_fingerprint_relation", projection_relation);
        expect.assert_key("history_fingerprint_relation", history_relation);
        expect.assert_key("latest_fingerprint_relation", latest_relation);
        expect.assert_key("history_capability", "complete_history");
        expect.assert_key("latest_capability", "latest_state_only");
        for relation in [projection_relation, history_relation, latest_relation] {
            match relation {
                "equal" => equal_relations += 1,
                "different" => different_relations += 1,
                other => panic!("unknown relation `{other}`"),
            }
        }
    }
    assert!(equal_relations > 0, "fixture exercises an equal control");
    assert!(
        different_relations > 0,
        "fixture exercises a different control"
    );
}
