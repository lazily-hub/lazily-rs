#![cfg(feature = "serde")]

//! Replay `lazily-spec/conformance/receipts/causal_receipts.json` through the
//! library's own receipt decode path and [`ReceiptProjection`] (`#lzwiremodel2`).
//!
//! The fixture's `wire` is decoded as a [`ReceiptMessage`] (the generated wire
//! type), every receipt is folded into a [`ReceiptProjection`] at the fixture's
//! `current_generation`, and each `assertions` key is compared against what the
//! PROJECTION reports — never against a count this runner kept for itself. The
//! generation is an input to the fold, so its key is asserted against the
//! generation the library itself enforced (the `expected` it reports on a stale
//! receipt, and the generation of every receipt it recorded).

mod common;

use std::collections::BTreeSet;

use common::Expect;
use lazily::{
    CausalReceipt, ReceiptApplyStatus, ReceiptMessage, ReceiptOutcome, ReceiptProjection,
};
use serde_json::Value;

const SPEC: common::SpecDir = common::SpecDir("receipts");
const FIXTURE: &str = "causal_receipts.json";

/// Top-level keys this runner knows how to interpret. An unknown key fails the
/// run rather than being silently ignored (`#lzassertunknownkeys`).
const TOP_LEVEL_KEYS: [&str; 6] = [
    "description",
    "protocol_version",
    "kind",
    "model",
    "assertions",
    "wire",
];

fn load(name: &str) -> Option<Value> {
    if !SPEC.is_dir() {
        return None;
    }
    let path = SPEC.join(name);
    let raw = common::spec_read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Some(serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display())))
}

fn outcome_wire(outcome: ReceiptOutcome) -> Value {
    serde_json::to_value(outcome).expect("ReceiptOutcome serializes")
}

#[test]
fn causal_receipts_conformance() {
    let Some(fixture) = load(FIXTURE) else {
        eprintln!("SKIP: lazily-spec sibling missing");
        return;
    };
    let id = format!("{SPEC}/{FIXTURE}");

    let top = fixture.as_object().expect("fixture is a JSON object");
    for key in top.keys() {
        assert!(
            TOP_LEVEL_KEYS.contains(&key.as_str()),
            "{id}: unknown top-level key `{key}` — this runner does not know how to replay it"
        );
    }
    assert_eq!(fixture["protocol_version"], 1, "{id}: protocol_version");
    assert_eq!(fixture["kind"], "Receipt", "{id}: kind");
    assert_eq!(fixture["model"], "CausalReceipt", "{id}: model");

    let exp = Expect::new(id.clone(), "assertions", &fixture["assertions"]);

    // Decode through the generated wire type. serde ignores unknown fields, so
    // the re-encoded message must equal the fixture's wire byte-for-value: a
    // field the type does not carry, or a renamed variant, fails here.
    let wire = &fixture["wire"];
    let message: ReceiptMessage =
        serde_json::from_value(wire.clone()).expect("decode ReceiptMessage");
    assert_eq!(
        serde_json::to_value(&message).expect("encode ReceiptMessage"),
        *wire,
        "{id}: wire does not round-trip through ReceiptMessage"
    );
    let ReceiptMessage::CausalReceipts(batch) = message;
    let receipts: Vec<CausalReceipt> = batch.receipts;

    // `current_generation` DRIVES the fold; it is asserted below against what
    // the projection reports it enforced.
    let generation = exp
        .get("current_generation")
        .as_u64()
        .expect("assertions.current_generation is a u64");

    let mut projection = ReceiptProjection::new();
    let statuses: Vec<ReceiptApplyStatus> = receipts
        .iter()
        .map(|r| projection.observe(Some(generation), r.clone()))
        .collect();

    // Every receipt the batch carried is known to the projection, recorded or
    // stale; a receipt it never took is not counted.
    for r in &receipts {
        assert!(
            projection.contains_receipt(&r.receipt_id),
            "{id}: projection does not know receipt {}",
            r.receipt_id
        );
    }
    let known = receipts
        .iter()
        .filter(|r| projection.contains_receipt(&r.receipt_id))
        .count();
    exp.assert_key("receipt_count", known as u64);

    // The causation the batch folds into, as the projection indexes it.
    let causations: BTreeSet<&str> = receipts
        .iter()
        .map(|r| r.causation_id.as_str())
        .filter(|c| projection.latest_for(c).is_some())
        .collect();
    assert_eq!(
        causations.len(),
        1,
        "{id}: expected one causation in the projection, got {causations:?}"
    );
    let causation = *causations.iter().next().expect("one causation");
    exp.assert_key("causation_id", causation);

    // The generation the library ENFORCED: the `expected` it reports on every
    // stale receipt, plus the generation of every receipt it recorded.
    let mut enforced: BTreeSet<u64> = BTreeSet::new();
    for (r, status) in receipts.iter().zip(&statuses) {
        match status {
            ReceiptApplyStatus::StaleGeneration { expected, actual } => {
                assert_eq!(
                    *actual, r.generation,
                    "{id}: stale status misreports generation"
                );
                enforced.insert(*expected);
            }
            ReceiptApplyStatus::Recorded => {
                enforced.insert(r.generation);
            }
            other => panic!("{id}: receipt {} folded as {other:?}", r.receipt_id),
        }
    }
    assert_eq!(
        enforced.len(),
        1,
        "{id}: projection enforced more than one generation: {enforced:?}"
    );
    exp.assert_key(
        "current_generation",
        *enforced.iter().next().expect("one generation"),
    );

    let terminal = projection
        .terminal_for(causation)
        .unwrap_or_else(|| panic!("{id}: no terminal receipt for {causation}"));
    assert!(
        terminal.outcome.is_terminal(),
        "{id}: terminal_for is non-terminal"
    );
    exp.assert_key("terminal_outcome", outcome_wire(terminal.outcome));

    let stale: Vec<Value> = projection
        .stale_receipt_ids()
        .map(|s| Value::String(s.clone()))
        .collect();
    // A stale receipt must not have leaked into the recorded views.
    for s in projection.stale_receipt_ids() {
        assert_ne!(
            projection.latest_for(causation).map(|r| &r.receipt_id),
            Some(s),
            "{id}: stale receipt {s} became the recorded latest"
        );
        assert_ne!(
            &terminal.receipt_id, s,
            "{id}: stale receipt {s} became the recorded terminal"
        );
    }
    exp.assert_key("stale_receipt_ids", stale);

    // Non-terminal outcomes the projection RECORDED, in fold order, as
    // classified by the library's own `is_terminal`.
    let nonterminal: Vec<Value> = receipts
        .iter()
        .zip(&statuses)
        .filter(|(r, s)| **s == ReceiptApplyStatus::Recorded && !r.outcome.is_terminal())
        .map(|(r, _)| outcome_wire(r.outcome))
        .collect();
    exp.assert_key("nonterminal_outcomes", nonterminal);

    exp.finish();
}
