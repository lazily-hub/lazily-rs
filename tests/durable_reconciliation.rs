use std::collections::BTreeMap;

use lazily::{
    CodecVersion, CompleteHistoryEvent, CompleteHistoryOperation, CompleteHistoryProjector,
    DurableCommit, DurableOwnerId, DurablePosition, DurableStateMutation, FenceToken,
    InboxIdentity, ProjectionCheckpoint, ProjectionHealth, ProjectionReplayError,
    ReconciliationReport, SchemaVersion, VersionedBytes,
};

fn event(
    position: u64,
    entity_id: &str,
    operation: CompleteHistoryOperation,
) -> CompleteHistoryEvent {
    CompleteHistoryEvent {
        position: DurablePosition::new(position),
        entity_id: entity_id.to_owned(),
        operation,
    }
}

fn payload(value: &str) -> VersionedBytes {
    VersionedBytes::new(
        SchemaVersion::new(1).unwrap(),
        CodecVersion::new(1).unwrap(),
        value.as_bytes(),
    )
}

fn commit(owner_id: &DurableOwnerId) -> DurableCommit {
    DurableCommit {
        owner_id: owner_id.clone(),
        expected_position: DurablePosition::new(4),
        fence: FenceToken::new(3),
        inbox_identity: InboxIdentity::new("reconcile-tick").unwrap(),
        ingress_fingerprint: b"reconcile-tick/4".to_vec(),
        state: DurableStateMutation::AppendEvents(vec![payload("reconciliation-scheduled")]),
        effects: Vec::new(),
        receipts: Vec::new(),
    }
}

#[test]
fn full_replay_and_checkpoint_resume_are_equivalent() {
    let history = vec![
        event(1, "a", CompleteHistoryOperation::Create(b"one".to_vec())),
        event(2, "b", CompleteHistoryOperation::Create(b"two".to_vec())),
        event(3, "a", CompleteHistoryOperation::Amend(b"three".to_vec())),
        event(4, "b", CompleteHistoryOperation::Retract),
    ];
    let full = CompleteHistoryProjector::rebuild(&history).unwrap();
    let checkpoint = CompleteHistoryProjector::rebuild(&history[..2]).unwrap();
    let resumed = CompleteHistoryProjector::resume(checkpoint, &history[2..]).unwrap();
    assert_eq!(resumed, full);
    assert_eq!(full.projection_version, 4);
    assert_eq!(
        full.entries,
        BTreeMap::from([("a".into(), b"three".to_vec())])
    );
}

#[test]
fn partial_and_final_retractions_are_exact() {
    let partial = CompleteHistoryProjector::rebuild(&[
        event(1, "a", CompleteHistoryOperation::Create(b"one".to_vec())),
        event(2, "b", CompleteHistoryOperation::Create(b"two".to_vec())),
        event(3, "a", CompleteHistoryOperation::Retract),
    ])
    .unwrap();
    assert_eq!(partial.entries.len(), 1);
    assert_eq!(partial.entries["b"], b"two");

    let final_state = CompleteHistoryProjector::resume(
        partial,
        &[event(4, "b", CompleteHistoryOperation::Retract)],
    )
    .unwrap();
    assert!(final_state.entries.is_empty());
}

#[test]
fn gaps_and_invalid_order_fail_closed() {
    assert!(matches!(
        CompleteHistoryProjector::rebuild(&[event(
            2,
            "a",
            CompleteHistoryOperation::Create(b"one".to_vec())
        )]),
        Err(ProjectionReplayError::PositionGap { .. })
    ));
    assert_eq!(
        CompleteHistoryProjector::rebuild(&[event(1, "a", CompleteHistoryOperation::Retract)]),
        Err(ProjectionReplayError::MissingEntity("a".into()))
    );
}

#[test]
fn dry_run_reports_health_and_never_grants_authority() {
    let expected = CompleteHistoryProjector::rebuild(&[event(
        1,
        "a",
        CompleteHistoryOperation::Create(b"authoritative".to_vec()),
    )])
    .unwrap();
    let observed = ProjectionCheckpoint::default();
    let report = ReconciliationReport::dry_run(expected, observed);
    assert_eq!(report.health, ProjectionHealth::Lagging);
    assert_eq!(report.lag, 1);
    assert!(report.drift);
    assert!(!report.read_authority.may_authorize_transition());
}

#[test]
fn repair_is_a_stable_idempotent_transactional_outbox_intent() {
    let owner_id = DurableOwnerId::new("catalog").unwrap();
    let expected = CompleteHistoryProjector::rebuild(&[event(
        1,
        "a",
        CompleteHistoryOperation::Create(b"one".to_vec()),
    )])
    .unwrap();
    let report = ReconciliationReport::dry_run(expected, ProjectionCheckpoint::default());
    let mut commit = commit(&owner_id);
    assert!(
        report
            .schedule_reconciliation(
                &owner_id,
                &mut commit,
                SchemaVersion::new(1).unwrap(),
                CodecVersion::new(1).unwrap(),
            )
            .unwrap()
    );
    assert!(
        !report
            .schedule_reconciliation(
                &owner_id,
                &mut commit,
                SchemaVersion::new(1).unwrap(),
                CodecVersion::new(1).unwrap(),
            )
            .unwrap()
    );
    assert_eq!(commit.effects.len(), 1);
    assert_eq!(commit.effects[0].identity.as_str(), "reconcile/catalog/1");
}
