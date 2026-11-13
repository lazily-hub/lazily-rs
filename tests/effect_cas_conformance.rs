//! Canonical generic effect-CAS corpus replay (`#lzeffectcas`).

mod common;

use std::collections::BTreeMap;

use lazily::{
    EffectAttemptIdentity, EffectCasApplied, EffectCasClaim, EffectCasCore, EffectCasFailure,
    EffectCasReconnect, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan, EffectTerminal,
};
use serde_json::{Value, json};

const SPEC_DIR: common::SpecDir = common::SpecDir("egress");
const FIXTURE: &str = "effect_cas.json";

type Core = EffectCasCore<String, u64, String, String, String>;

fn load() -> Value {
    let path = format!("{SPEC_DIR}/{FIXTURE}");
    let raw = common::spec_read_to_string(path).expect("read effect-CAS fixture");
    serde_json::from_str(&raw).expect("parse effect-CAS fixture")
}

fn upsert_value(outcome: EffectCasUpsert) -> Value {
    match outcome {
        EffectCasUpsert::Accepted => json!({ "upsert": "accepted" }),
        EffectCasUpsert::Unchanged => json!({ "upsert": "unchanged" }),
        EffectCasUpsert::AlreadyApplied { applied_through } => {
            json!({ "upsert": "already_applied", "applied_through": applied_through })
        }
        EffectCasUpsert::StaleEpoch { current } => {
            json!({ "upsert": "stale_epoch", "current": current })
        }
        EffectCasUpsert::EpochConflict => json!({ "upsert": "epoch_conflict" }),
    }
}

fn applied_value(outcome: EffectCasApplied) -> Value {
    match outcome {
        EffectCasApplied::StepApplied { next_step } => {
            json!({ "applied": "step_applied", "next_step": next_step })
        }
        EffectCasApplied::Completed { applied_through } => {
            json!({ "applied": "completed", "applied_through": applied_through })
        }
        EffectCasApplied::Superseded => json!({ "applied": "superseded" }),
        EffectCasApplied::Duplicate => json!({ "applied": "duplicate" }),
        EffectCasApplied::UnknownAttempt => json!({ "applied": "unknown_attempt" }),
        EffectCasApplied::StaleGeneration { current } => {
            json!({ "applied": "stale_generation", "current": current })
        }
    }
}

fn failure_value(outcome: EffectCasFailure) -> Value {
    match outcome {
        EffectCasFailure::Superseded => json!({ "failure": "superseded" }),
        EffectCasFailure::Retrying { retry_at } => {
            json!({ "failure": "retrying", "retry_at": retry_at })
        }
        EffectCasFailure::Rejected => json!({ "failure": "rejected" }),
        EffectCasFailure::UnknownAttempt => json!({ "failure": "unknown_attempt" }),
        EffectCasFailure::StaleGeneration { current } => {
            json!({ "failure": "stale_generation", "current": current })
        }
    }
}

fn reconnect_value(outcome: EffectCasReconnect) -> Value {
    match outcome {
        EffectCasReconnect::Advanced {
            generation,
            requeued,
            superseded,
        } => json!({
            "reconnect": "advanced",
            "generation": generation,
            "requeued": requeued,
            "superseded": superseded,
        }),
        EffectCasReconnect::Unchanged { generation } => {
            json!({ "reconnect": "unchanged", "generation": generation })
        }
        EffectCasReconnect::StaleGeneration { current } => {
            json!({ "reconnect": "stale_generation", "current": current })
        }
    }
}

fn claim_value(
    outcome: EffectCasClaim<String, u64, String, String>,
) -> (Value, Option<EffectAttemptIdentity>) {
    match outcome {
        EffectCasClaim::Claimed(attempt) => (
            json!({
                "claim": "claimed",
                "identity": attempt.identity.effect.as_str(),
                "step": attempt.identity.step,
                "attempt": attempt.identity.attempt,
                "expected": attempt.expected,
                "payload": attempt.payload,
            }),
            Some(attempt.identity),
        ),
        EffectCasClaim::Empty => (json!({ "claim": "empty" }), None),
        EffectCasClaim::Busy => (json!({ "claim": "busy" }), None),
        EffectCasClaim::Waiting { retry_at } => {
            (json!({ "claim": "waiting", "retry_at": retry_at }), None)
        }
        EffectCasClaim::StaleGeneration { current } => (
            json!({ "claim": "stale_generation", "current": current }),
            None,
        ),
    }
}

fn run_step(
    core: &mut Core,
    attempts: &mut BTreeMap<String, EffectAttemptIdentity>,
    step: &Value,
) -> Value {
    let op = &step["op"];
    let key = || op["key"].as_str().expect("key").to_owned();
    let attempt = || {
        attempts
            .get(op["attempt"].as_str().expect("attempt label"))
            .expect("saved attempt")
            .clone()
    };
    match op["type"].as_str().expect("op type") {
        "upsert" => {
            let plan = op["plan"]
                .as_array()
                .expect("plan")
                .iter()
                .map(|value| value.as_str().expect("plan payload").to_owned())
                .collect();
            upsert_value(
                core.upsert_desired(EffectIntent {
                    identity: EffectIdentity::new(op["identity"].as_str().expect("identity"))
                        .expect("non-empty effect identity"),
                    epoch: op["epoch"].as_u64().expect("epoch"),
                    key: key(),
                    expected: op["expected"].as_u64().expect("expected authority"),
                    plan: EffectPlan::new(plan).expect("non-empty plan"),
                })
                .1,
            )
        }
        "claim" => {
            let (returned, claimed) = claim_value(
                core.claim(
                    &key(),
                    op["generation"].as_u64().expect("generation"),
                    op["now"].as_u64().expect("logical clock"),
                )
                .1,
            );
            if let Some(label) = step.get("save").and_then(Value::as_str) {
                attempts.insert(label.to_owned(), claimed.expect("saved claim must succeed"));
            }
            returned
        }
        "applied" => applied_value(
            core.record_applied(
                &key(),
                &attempt(),
                op["receipt"].as_str().expect("receipt").to_owned(),
                op["authority"].as_u64().expect("authority"),
            )
            .1,
        ),
        "superseded" => {
            assert_eq!(op["backend_before"], op["backend_after"]);
            failure_value(
                core.record_superseded(
                    &key(),
                    &attempt(),
                    op["observed"].as_u64().expect("observed authority"),
                )
                .1,
            )
        }
        "retryable" => failure_value(
            core.record_retryable(
                &key(),
                &attempt(),
                op["error"].as_str().expect("error").to_owned(),
                op["retry_at"].as_u64().expect("retry time"),
            )
            .1,
        ),
        "rejected" => failure_value(
            core.record_rejected(
                &key(),
                &attempt(),
                op["error"].as_str().expect("error").to_owned(),
            )
            .1,
        ),
        "reconnect" => reconnect_value(
            core.reconnect(op["generation"].as_u64().expect("generation"))
                .1,
        ),
        other => panic!("unknown effect-CAS op `{other}`"),
    }
}

fn terminal_value(terminal: &EffectTerminal<String, u64, String>) -> (&'static str, usize) {
    match terminal {
        EffectTerminal::Applied { receipts, .. } => ("applied", receipts.len()),
        EffectTerminal::Superseded { receipts, .. } => ("superseded", receipts.len()),
        EffectTerminal::Rejected { receipts, .. } => ("rejected", receipts.len()),
    }
}

fn final_value(core: &Core) -> Value {
    let snapshot = core.snapshot();
    let states = snapshot
        .keys
        .iter()
        .map(|(key, state)| {
            let (terminal, receipt_count) = state
                .terminal
                .as_ref()
                .map(terminal_value)
                .map_or((None, 0), |(kind, count)| (Some(kind), count));
            (
                key.clone(),
                json!({
                    "pending_identity": state.pending.as_ref().map(|pending| pending.intent.identity.as_str()),
                    "inflight_identity": state.inflight.as_ref().map(|inflight| inflight.identity.effect.as_str()),
                    "applied_through": state.applied_through,
                    "terminal": terminal,
                    "receipt_count": receipt_count,
                    "retry_at": state.retry.as_ref().map(|retry| retry.retry_at),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Value::Object(states)
}

#[test]
fn effect_cas_corpus_replays_exactly() {
    let fixture = load();
    assert_eq!(fixture["kind"], "EffectCas");
    assert_eq!(fixture["model"], "EffectCasCore");
    let path = format!("{SPEC_DIR}/{FIXTURE}");
    let mut scenario_count = 0;
    for (_, id, scenario) in common::scenarios(&path, &fixture) {
        let mut core = Core::new(scenario["generation"].as_u64().expect("generation"));
        let mut attempts = BTreeMap::new();
        for (index, step) in scenario["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .enumerate()
        {
            assert_eq!(
                run_step(&mut core, &mut attempts, step),
                step["returns"],
                "{id} step {index}"
            );
        }
        assert_eq!(final_value(&core), scenario["final"], "{id} final state");
        scenario_count += 1;
    }
    assert_eq!(scenario_count, 9);
}
