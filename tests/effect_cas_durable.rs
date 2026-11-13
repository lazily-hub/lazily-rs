//! Durable hydration of the effect-CAS core (`#lzeffectcasdurable`).
//!
//! The spec requires crash recovery to hydrate "the retained intent,
//! generation, cursor, logical retry time, and receipts before the runtime
//! becomes live", then reconnect at a newer generation and replay the same
//! effect identity. These tests pin that `durable_state` / `hydrate` are a
//! lossless pair, that a hydrated core is behaviourally identical to the one
//! that wrote the image, and that two contenders sharing one durable image
//! resolve through the core's `Superseded` contract.

use lazily::{
    EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasDurableState, EffectCasFailure, EffectCasHydrateError, EffectCasReconnect,
    EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan, EffectTerminal,
};
use proptest::prelude::*;

type Core = EffectCasCore<String, u64, u64, u64, String>;
type Image = EffectCasDurableState<String, u64, u64, u64, String>;

fn intent(key: &str, epoch: u64, expected: u64, steps: Vec<u64>) -> EffectIntent<String, u64, u64> {
    EffectIntent {
        identity: EffectIdentity::new(format!("{key}#{epoch}")).unwrap(),
        epoch,
        key: key.to_owned(),
        expected,
        plan: EffectPlan::new(steps).unwrap(),
    }
}

fn claimed(core: &mut Core, key: &str, generation: u64) -> EffectAttempt<String, u64, u64, u64> {
    match core.claim(&key.to_owned(), generation, 0).1 {
        EffectCasClaim::Claimed(attempt) => attempt,
        other => panic!("expected a claim, got {other:?}"),
    }
}

/// A process boundary: the only thing that survives is the durable image.
fn restart(core: &Core) -> Core {
    Core::hydrate(core.durable_state()).expect("a written image hydrates")
}

#[test]
fn hydrate_is_lossless_for_every_retained_field() {
    let mut core = Core::new(1);
    let doc = "doc".to_owned();
    // Partial two-step plan: step 0 receipted, step 1 in flight.
    core.upsert_desired(intent("doc", 1, 10, vec![100, 200]));
    let step0 = claimed(&mut core, "doc", 1);
    core.record_applied(&doc, &step0.identity, 1000, 11);
    let step1 = claimed(&mut core, "doc", 1);
    // A newer desired epoch waits behind the in-flight attempt.
    core.upsert_desired(intent("doc", 2, 11, vec![300]));
    // An independent key sits in logical retry.
    core.upsert_desired(intent("other", 1, 0, vec![1]));
    let retrying = claimed(&mut core, "other", 1);
    core.record_retryable(&"other".to_owned(), &retrying.identity, "busy".into(), 42);

    let restored = restart(&core);
    assert_eq!(restored.durable_state(), core.durable_state());
    assert_eq!(restored.snapshot(), core.snapshot());
    assert_eq!(restored.generation(), 1);

    // The restored core accepts the exact attempt the writer handed out.
    let mut restored = restored;
    assert_eq!(
        restored.record_applied(&doc, &step1.identity, 2000, 12).1,
        EffectCasApplied::Superseded,
        "a newer desired epoch stops the old plan after hydration too"
    );
    assert_eq!(
        restored.claim(&"other".to_owned(), 1, 41).1,
        EffectCasClaim::Waiting { retry_at: 42 },
        "the logical retry time survives hydration"
    );
}

#[test]
fn crash_replay_reconnects_and_resumes_the_same_identity_and_cursor() {
    let mut writer = Core::new(1);
    let doc = "doc".to_owned();
    writer.upsert_desired(intent("doc", 7, 10, vec![100, 200, 300]));
    let step0 = claimed(&mut writer, "doc", 1);
    writer.record_applied(&doc, &step0.identity, 1000, 11);
    let crashed = claimed(&mut writer, "doc", 1);
    assert_eq!(crashed.identity.step, 1);

    // The writer crashes mid-step; a successor hydrates its image.
    let mut successor = restart(&writer);
    assert_eq!(
        successor.reconnect(2).1,
        EffectCasReconnect::Advanced {
            generation: 2,
            requeued: 1,
            superseded: 0,
        }
    );
    let resumed = claimed(&mut successor, "doc", 2);
    assert_eq!(
        resumed.identity.effect, crashed.identity.effect,
        "no new identity"
    );
    assert_eq!(resumed.identity.epoch, 7);
    assert_eq!(
        resumed.identity.step, 1,
        "the receipted step is not repeated"
    );
    assert_eq!(resumed.identity.attempt, crashed.identity.attempt + 1);
    assert_eq!(resumed.expected, 11, "the cursor expects the prior receipt");
    assert_eq!(resumed.prior_receipts.len(), 1);
    assert_eq!(resumed.prior_receipts[0].receipt, 1000);

    // The crashed actor's late receipt is fenced and changes nothing.
    let before = successor.durable_state();
    assert_eq!(
        successor
            .record_applied(&doc, &crashed.identity, 2000, 12)
            .1,
        EffectCasApplied::StaleGeneration { current: 2 }
    );
    assert_eq!(successor.durable_state(), before);
}

/// Two processes contend for one key through a shared durable image. Each
/// transition is load → hydrate → operate → store, the way a backend
/// transaction serializes them. The incumbent's receipt for an older epoch is
/// `Superseded`; only the newer epoch can complete.
#[test]
fn two_contenders_sharing_a_durable_image_resolve_through_superseded() {
    let doc = "doc".to_owned();
    let mut store: Image = Core::new(1).durable_state();
    let mut transact = |op: &mut dyn FnMut(&mut Core)| {
        let mut core = Core::hydrate(store.clone()).unwrap();
        op(&mut core);
        store = core.durable_state();
    };

    // Contender A desires epoch 1 and claims it.
    let mut a_attempt = None;
    transact(&mut |core| {
        core.upsert_desired(intent("doc", 1, 10, vec![100]));
        a_attempt = Some(claimed(core, "doc", 1));
    });
    let a_attempt = a_attempt.unwrap();

    // Contender B cannot claim while A is in flight; it desires a newer epoch.
    transact(&mut |core| {
        assert_eq!(core.claim(&doc, 1, 0).1, EffectCasClaim::Busy);
        assert_eq!(
            core.upsert_desired(intent("doc", 2, 10, vec![200])).1,
            EffectCasUpsert::Accepted
        );
    });

    // A's adapter applied, but the shared image already holds B's newer epoch.
    transact(&mut |core| {
        assert_eq!(
            core.record_applied(&doc, &a_attempt.identity, 1000, 11).1,
            EffectCasApplied::Superseded
        );
        assert!(matches!(
            core.state(&doc).unwrap().terminal,
            Some(EffectTerminal::Superseded { epoch: 1, .. })
        ));
        assert_eq!(core.state(&doc).unwrap().applied_through, None);
    });

    // Epoch 2 completes once; the superseded epoch can never be re-desired.
    transact(&mut |core| {
        let b_attempt = claimed(core, "doc", 1);
        assert_eq!(b_attempt.identity.epoch, 2);
        assert_eq!(
            core.record_outcome(
                &doc,
                &b_attempt.identity,
                EffectApply::Applied {
                    receipt: 2000,
                    authority: 12,
                },
                0,
            )
            .1,
            lazily::EffectCasOutcome::Applied(EffectCasApplied::Completed { applied_through: 2 })
        );
        assert_eq!(
            core.upsert_desired(intent("doc", 1, 10, vec![100])).1,
            EffectCasUpsert::AlreadyApplied { applied_through: 2 }
        );
    });
}

#[test]
fn compare_mismatch_supersedes_across_a_restart_without_advancing_the_frontier() {
    let doc = "doc".to_owned();
    let mut core = Core::new(3);
    core.upsert_desired(intent("doc", 1, 10, vec![100]));
    let attempt = claimed(&mut core, "doc", 3);
    let mut core = restart(&core);
    assert_eq!(
        core.record_superseded(&doc, &attempt.identity, 99).1,
        EffectCasFailure::Superseded
    );
    let core = restart(&core);
    let state = core.state(&doc).unwrap();
    assert_eq!(state.applied_through, None);
    assert!(state.inflight.is_none());
    assert!(matches!(
        state.terminal,
        Some(EffectTerminal::Superseded {
            observed: Some(99),
            ..
        })
    ));
}

#[test]
fn hydrate_refuses_corrupt_images() {
    let mut core = Core::new(1);
    core.upsert_desired(intent("doc", 1, 10, vec![100, 200]));
    claimed(&mut core, "doc", 1);
    let image = core.durable_state();

    let mut cursor = image.clone();
    cursor.keys[0].inflight.as_mut().unwrap().retained.next_step = 2;
    assert_eq!(
        Core::hydrate(cursor).unwrap_err(),
        EffectCasHydrateError::CursorOutOfRange
    );

    let mut attempt = image.clone();
    attempt.keys[0].inflight.as_mut().unwrap().attempt.expected = 11;
    assert_eq!(
        Core::hydrate(attempt).unwrap_err(),
        EffectCasHydrateError::AttemptMismatch
    );

    let mut future = image.clone();
    future.generation = 0;
    assert_eq!(
        Core::hydrate(future).unwrap_err(),
        EffectCasHydrateError::FutureGeneration
    );

    let mut key = image.clone();
    key.keys[0].key = "elsewhere".into();
    assert_eq!(
        Core::hydrate(key).unwrap_err(),
        EffectCasHydrateError::KeyMismatch
    );

    let mut duplicate = image.clone();
    duplicate.keys.push(duplicate.keys[0].clone());
    assert_eq!(
        Core::hydrate(duplicate).unwrap_err(),
        EffectCasHydrateError::DuplicateKey
    );
}

#[cfg(feature = "serde")]
#[test]
fn durable_image_round_trips_through_json_and_refuses_invalid_identities() {
    let mut core = Core::new(1);
    let doc = "doc".to_owned();
    core.upsert_desired(intent("doc", 1, 10, vec![100, 200]));
    let step0 = claimed(&mut core, "doc", 1);
    core.record_applied(&doc, &step0.identity, 1000, 11);
    claimed(&mut core, "doc", 1);

    let json = serde_json::to_string(&core.durable_state()).unwrap();
    let image: Image = serde_json::from_str(&json).unwrap();
    assert_eq!(image, core.durable_state());
    assert_eq!(
        Core::hydrate(image).unwrap().durable_state(),
        core.durable_state()
    );

    let empty_identity = json.replace("\"doc#1\"", "\"\"");
    assert!(serde_json::from_str::<Image>(&empty_identity).is_err());
    let empty_plan = json.replace("[100,200]", "[]");
    assert!(serde_json::from_str::<Image>(&empty_plan).is_err());
}

#[derive(Debug, Clone)]
enum Op {
    Upsert { key: u8, steps: u8 },
    Claim { key: u8 },
    Apply { key: u8 },
    Supersede { key: u8 },
    Retry { key: u8, at: u8 },
    Reject { key: u8 },
    Reconnect,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..2, 1u8..4).prop_map(|(key, steps)| Op::Upsert { key, steps }),
        (0u8..2).prop_map(|key| Op::Claim { key }),
        (0u8..2).prop_map(|key| Op::Apply { key }),
        (0u8..2).prop_map(|key| Op::Supersede { key }),
        (0u8..2, 0u8..4).prop_map(|(key, at)| Op::Retry { key, at }),
        (0u8..2).prop_map(|key| Op::Reject { key }),
        Just(Op::Reconnect),
    ]
}

fn run(core: &mut Core, op: &Op, epoch: &mut u64, now: u64) -> String {
    let key_name = |key: u8| format!("k{key}");
    let inflight = |core: &Core, key: &String| core.state(key).and_then(|state| state.inflight);
    match op {
        Op::Upsert { key, steps } => {
            *epoch += 1;
            let steps = (0..u64::from(*steps)).collect();
            format!(
                "{:?}",
                core.upsert_desired(intent(&key_name(*key), *epoch, *epoch, steps))
            )
        }
        Op::Claim { key } => format!("{:?}", core.claim(&key_name(*key), core.generation(), now)),
        Op::Apply { key } => {
            let key = key_name(*key);
            match inflight(core, &key) {
                Some(attempt) => format!(
                    "{:?}",
                    core.record_applied(&key, &attempt.identity, now, now + 1)
                ),
                None => "none".into(),
            }
        }
        Op::Supersede { key } => {
            let key = key_name(*key);
            match inflight(core, &key) {
                Some(attempt) => {
                    format!("{:?}", core.record_superseded(&key, &attempt.identity, now))
                }
                None => "none".into(),
            }
        }
        Op::Retry { key, at } => {
            let key = key_name(*key);
            match inflight(core, &key) {
                Some(attempt) => format!(
                    "{:?}",
                    core.record_retryable(&key, &attempt.identity, "e".into(), u64::from(*at))
                ),
                None => "none".into(),
            }
        }
        Op::Reject { key } => {
            let key = key_name(*key);
            match inflight(core, &key) {
                Some(attempt) => {
                    format!(
                        "{:?}",
                        core.record_rejected(&key, &attempt.identity, "x".into())
                    )
                }
                None => "none".into(),
            }
        }
        Op::Reconnect => format!("{:?}", core.reconnect(core.generation() + 1)),
    }
}

proptest! {
    /// A core restarted from its durable image after every operation is
    /// indistinguishable from one that never restarted.
    #[test]
    fn restarting_after_every_operation_is_unobservable(ops in prop::collection::vec(op(), 1..60)) {
        let mut live = Core::new(1);
        let mut restarted = Core::new(1);
        let (mut live_epoch, mut restarted_epoch) = (0, 0);
        for (now, op) in ops.iter().enumerate() {
            let now = now as u64;
            let expected = run(&mut live, op, &mut live_epoch, now);
            restarted = restart(&restarted);
            let observed = run(&mut restarted, op, &mut restarted_epoch, now);
            prop_assert_eq!(&observed, &expected);
            prop_assert_eq!(restarted.durable_state(), live.durable_state());
            prop_assert_eq!(restarted.snapshot(), live.snapshot());
        }
    }
}
