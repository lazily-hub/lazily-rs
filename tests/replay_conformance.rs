//! Cross-language conformance for the replay-equivalence proof (`#lzreplayrs`)
//! — see `lazily-spec/docs/replay-equivalence.md` and
//! `lazily-spec/conformance/replay/*.json`.
//!
//! Three fixtures, one obligation each: the fingerprint is bound to its log and
//! that binding is revalidated **before** any value compare; a divergence is
//! reported at the first checkpoint where the values parted; the observation
//! encoding agrees with the family on which differences are differences.
//!
//! A JSON fixture cannot carry a reactive graph, so the corpus declares its two
//! subjects in prose and [`Accumulator`] below is this binding's copy of that
//! declaration — kept to the letter, including that `observe` exposes `sum` and
//! `names` under exactly those labels.
//!
//! The third fixture never names a hex digest, only same/different pairs, which
//! is what leaves this binding free to store the canonical bytes themselves
//! rather than take on a hash crate for an optional facility (see
//! `src/replay.rs`).

mod common;

// The SANCTIONED fixture reads (`#lzsiblingrunnermasking`): `Value::as_bool` is
// banned by `clippy.toml`, so a mistyped fixture flag fails instead of
// coercing to `false` and asserting the opposite claim.
use common::FixtureJson;

use std::collections::BTreeMap;

use common::Expect;
use lazily::{
    DivergenceKind, INITIAL_SEQ, ReplayCheckpoint, ReplayEvent, ReplayFingerprint, ReplayGraph,
    ReplayHarness, ReplayLog, ReplayObservation, ReplayProofError, ReplayValue, canonical_digest,
};
use serde_json::Value;

const SPEC_DIR: common::SpecDir = common::SpecDir("replay");

fn load_fixture(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw = crate::common::spec_read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn spec_fixtures_present() -> bool {
    SPEC_DIR.join("fingerprint_log_binding.json").exists()
}

// -- the corpus's canonical subjects ------------------------------------------

/// `accumulator`, and `drifting_accumulator` when a drift is configured.
struct Accumulator {
    sum: i128,
    names: Vec<String>,
    drift_at: Option<u64>,
    drift: i128,
}

impl Accumulator {
    fn new(drift_at: Option<u64>, drift: i128) -> Self {
        Self {
            sum: 0,
            names: Vec::new(),
            drift_at,
            drift,
        }
    }
}

impl ReplayGraph for Accumulator {
    fn apply(&mut self, event: &ReplayEvent) {
        self.sum += event
            .payload
            .as_int()
            .unwrap_or_else(|| panic!("the canonical subject's payload is an integer: {event:?}"));
        self.names.push(event.name.clone());
        if self.drift_at == Some(event.seq) {
            self.sum += self.drift;
        }
    }

    fn observe(&self) -> ReplayObservation {
        ReplayObservation::new()
            .with("sum", self.sum)
            .with("names", ReplayValue::seq(self.names.iter().cloned()))
    }
}

/// The subject declared by `config.subject`, with this op's drift.
///
/// Fail-closed on an unknown subject: a corpus that adds a third one must not
/// replay silently as the accumulator and compare its `expected` block against a
/// graph the fixture never asked for.
fn subject(config: &Value, op: &Value) -> (Option<u64>, i128) {
    match config["subject"].as_str().unwrap_or_else(|| {
        panic!("replay fixture config carries no `subject`: {config}");
    }) {
        "accumulator" => (None, 0),
        "drifting_accumulator" => {
            let drift_at = config["drift_at"]
                .as_u64()
                .unwrap_or_else(|| panic!("`drifting_accumulator` needs `drift_at`: {config}"));
            let drift = i128::from(op["drift"].as_i64().unwrap_or(0));
            (Some(drift_at), drift)
        }
        other => panic!("unknown canonical replay subject `{other}`"),
    }
}

fn log_of(entries: &Value) -> ReplayLog {
    let events = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            ReplayEvent::new(
                entry["seq"].as_u64().unwrap(),
                entry["name"].as_str().unwrap(),
                ReplayValue::Int(i128::from(entry["payload"].as_i64().unwrap())),
            )
            .unwrap()
        })
        .collect();
    ReplayLog::new(events).unwrap()
}

/// The sum the declared subject ends on, computed WITHOUT the harness.
///
/// This is the independent side of the `record` cross-check: the fixture's
/// `final_sum` is compared against a straight fold, and the fingerprint's `sum`
/// digest is then compared against the digest of that same value. Without it a
/// harness that observed some other value entirely would still satisfy every
/// other key in the block.
fn final_sum(drift_at: Option<u64>, drift: i128, log: &ReplayLog) -> i128 {
    let mut graph = Accumulator::new(drift_at, drift);
    for event in log.events() {
        graph.apply(event);
    }
    graph.sum
}

fn checkpoint_seqs(fingerprint: &ReplayFingerprint) -> Vec<i64> {
    fingerprint
        .checkpoints()
        .iter()
        .map(ReplayCheckpoint::seq)
        .collect()
}

// -- obligations 1 and 2 ------------------------------------------------------

// No per-fixture step COUNT anywhere below (`#lzcorpusfloorguard`). A
// hard-coded floor drifts: `#lzreplayframing` grew
// `replay/canonical_encoding_equality.json` from 11 steps to 14 and eight of
// nine bindings were still pinned at 11, so the three new rows sat inside the
// slack and would have reported green WITHOUT EXECUTING. What replaces the
// number is exact and can never drift — every step LOADED is counted as
// EXECUTED, and an unrecognised `op.type` panics instead of falling through.
// The one thing a floor did buy, noticing the corpus SHRINK, is now guarded at
// the single place a shrink can happen: lazily-spec's `corpus-counts.json`
// pinned by `scripts/check-corpus-floors.mjs`.
fn drive_harness_fixture(name: &str) {
    let fx = load_fixture(name);
    assert_eq!(fx["kind"], "Replay");
    assert_eq!(fx["model"], "ReplayHarness");
    let path = format!("{SPEC_DIR}/{name}");
    let config = &fx["config"];
    let logs: BTreeMap<String, ReplayLog> = config["logs"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), log_of(value)))
        .collect();
    let mut fingerprints: BTreeMap<String, ReplayFingerprint> = BTreeMap::new();
    let steps = fx["steps"].as_array().unwrap();
    let mut executed = 0usize;

    for (index, step) in steps.iter().enumerate() {
        let op = &step["op"];
        let kind = op["type"]
            .as_str()
            .unwrap_or_else(|| panic!("replay step op carries no `type`: {step}"));
        let where_ = format!("{path} step {index} ({kind})");
        let exp = Expect::new(
            path.clone(),
            format!("steps[{index}].expected"),
            &step["expected"],
        );

        if kind == "log_digest_equal" {
            let left = &logs[op["left"].as_str().unwrap()];
            let right = &logs[op["right"].as_str().unwrap()];
            assert_eq!(
                step.fixture_flag_at("returns"),
                left.digest() == right.digest(),
                "{where_}: returns"
            );
            executed += 1;
            continue;
        }

        let (drift_at, drift) = subject(config, op);
        let stride = usize::try_from(
            op["stride"]
                .as_u64()
                .or_else(|| config["stride"].as_u64())
                .unwrap_or(1),
        )
        .unwrap();
        let harness =
            ReplayHarness::with_stride(move || Accumulator::new(drift_at, drift), stride).unwrap();
        let log = &logs[op["log"].as_str().unwrap()];

        match kind {
            "record" => {
                let fingerprint = harness.record(log).unwrap();
                exp.assert_key_at("outcome", "recorded", &where_);
                exp.assert_key_at("checkpoint_seqs", checkpoint_seqs(&fingerprint), &where_);
                exp.assert_key_at(
                    "stride",
                    i64::try_from(fingerprint.stride()).unwrap(),
                    &where_,
                );
                let sum = final_sum(drift_at, drift, log);
                exp.assert_key_at("final_sum", i64::try_from(sum).unwrap(), &where_);
                // The fingerprint must have observed the value the subject ends
                // on, not merely SOME value: this is the one place the digest and
                // the declared state meet. Without it the fixture would accept a
                // harness that fingerprinted something else entirely.
                assert_eq!(
                    fingerprint.final_checkpoint().get("sum"),
                    canonical_digest(&ReplayValue::Int(sum)).ok().as_ref(),
                    "{where_}: the recorded `sum` digest is not the subject's final sum"
                );
                fingerprints.insert(op["into"].as_str().unwrap().to_owned(), fingerprint);
            }
            "prove" => {
                let replays = usize::try_from(op["replays"].as_u64().unwrap()).unwrap();
                harness.prove(log, replays).unwrap_or_else(|e| {
                    panic!("{where_}: prove failed: {e}");
                });
                exp.assert_key_at("outcome", "ok", &where_);
                exp.assert_key_at("divergences", 0, &where_);
            }
            "verify" => {
                let fingerprint = &fingerprints[op["fingerprint"].as_str().unwrap()];
                match harness.verify(log, fingerprint) {
                    Ok(_) => {
                        exp.assert_key_at("outcome", "ok", &where_);
                        exp.assert_key_at("divergences", 0, &where_);
                    }
                    Err(ReplayProofError::LogMismatch { .. }) => {
                        exp.assert_key_at("outcome", "log_mismatch", &where_);
                        exp.assert_key_at("divergences", 0, &where_);
                    }
                    Err(ReplayProofError::StrideMismatch { .. }) => {
                        exp.assert_key_at("outcome", "stride_mismatch", &where_);
                        exp.assert_key_at("divergences", 0, &where_);
                    }
                    Err(error @ ReplayProofError::Divergence(_)) => {
                        let first = error.first_divergence().unwrap();
                        exp.assert_key_at("outcome", "divergent", &where_);
                        exp.assert_key_at("first_divergent_seq", first.seq, &where_);
                        exp.assert_key_at("first_divergent_label", first.label.as_str(), &where_);
                        exp.assert_key_at("first_divergent_kind", first.kind.as_str(), &where_);
                    }
                    Err(other) => panic!("{where_}: unexpected proof error: {other}"),
                }
            }
            "check" => {
                let fingerprint = &fingerprints[op["fingerprint"].as_str().unwrap()];
                match harness.check(log, fingerprint) {
                    Ok(divergences) => {
                        exp.assert_key_at("outcome", "ok", &where_);
                        exp.assert_key_at("divergences", divergences.len(), &where_);
                    }
                    // The reporting form collects value divergences instead of
                    // failing, but still refuses a stale fingerprint: an
                    // unanswerable question is not a report.
                    Err(ReplayProofError::LogMismatch { .. }) => {
                        exp.assert_key_at("outcome", "log_mismatch", &where_);
                        exp.assert_key_at("divergences", 0, &where_);
                    }
                    Err(ReplayProofError::StrideMismatch { .. }) => {
                        exp.assert_key_at("outcome", "stride_mismatch", &where_);
                        exp.assert_key_at("divergences", 0, &where_);
                    }
                    Err(other) => panic!("{where_}: unexpected proof error: {other}"),
                }
            }
            other => panic!("unknown canonical replay operation `{other}`"),
        }
        executed += 1;
    }

    // The constant-free replacement for the deleted floor: a step the loop
    // walked past is a step the corpus does not actually check.
    assert_eq!(
        executed,
        steps.len(),
        "{path}: loaded {} steps but executed {executed}",
        steps.len()
    );
}

#[test]
fn canonical_fingerprint_log_binding() {
    if !spec_fixtures_present() {
        return;
    }
    drive_harness_fixture("fingerprint_log_binding.json");
}

#[test]
fn canonical_divergence_localization() {
    if !spec_fixtures_present() {
        return;
    }
    drive_harness_fixture("divergence_localization.json");
}

// -- obligation 3 -------------------------------------------------------------

fn hex_bytes(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "hex literal `{text}`");
    let (pairs, _rest) = text.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The corpus's tagged value form. Values are type-tagged because JSON cannot
/// distinguish int `1` from float `1.0`, and integers carry decimal STRINGS so a
/// value beyond 2^53 stays exact.
fn value_of(tagged: &Value) -> ReplayValue {
    let tag = tagged["t"]
        .as_str()
        .unwrap_or_else(|| panic!("canonical value carries no `t`: {tagged}"));
    match tag {
        "int" => ReplayValue::Int(tagged["v"].as_str().unwrap().parse().unwrap()),
        "str" => ReplayValue::Str(tagged["v"].as_str().unwrap().to_owned()),
        "float" => ReplayValue::Float(tagged["v"].as_str().unwrap().parse().unwrap()),
        "bool" => ReplayValue::Bool(tagged.fixture_flag_at("v")),
        "bytes" => ReplayValue::Bytes(hex_bytes(tagged["v"].as_str().unwrap())),
        "seq" => ReplayValue::Seq(
            tagged["v"]
                .as_array()
                .unwrap()
                .iter()
                .map(value_of)
                .collect(),
        ),
        "set" => ReplayValue::Set(
            tagged["v"]
                .as_array()
                .unwrap()
                .iter()
                .map(value_of)
                .collect(),
        ),
        "map" => ReplayValue::Map(
            tagged["v"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    let pair = entry.as_array().unwrap();
                    (
                        ReplayValue::Str(pair[0].as_str().unwrap().to_owned()),
                        value_of(&pair[1]),
                    )
                })
                .collect(),
        ),
        // The residual dynamic case the encoding does not define. It must fail
        // loudly rather than fall back on the host's default rendering, which
        // would report a FALSE divergence on every run.
        "opaque" => ReplayValue::Opaque("opaque".to_owned()),
        other => panic!("unknown canonical value tag `{other}`"),
    }
}

#[test]
fn canonical_encoding_equality_classes() {
    if !spec_fixtures_present() {
        return;
    }
    let name = "canonical_encoding_equality.json";
    let fx = load_fixture(name);
    assert_eq!(fx["kind"], "Replay");
    assert_eq!(fx["model"], "CanonicalEncoding");
    let path = format!("{SPEC_DIR}/{name}");
    let values = fx["config"]["values"].as_object().unwrap();
    let steps = fx["steps"].as_array().unwrap();
    // The step count that used to be pinned here (`#lzcorpusfloorguard`) is
    // gone — see the note on `drive_harness_fixture`. This fixture is the exact
    // one that drifted: `#lzreplayframing` took it 11 -> 14 and the floor stayed
    // at 11, so the three member-framing rows that pin the length prefix
    // (`seq_a_sbc`/`seq_as_bc`, `map_a_sb`/`map_as_b`, and the nested
    // `seq_nested_*` pair) sat in the slack. Executed-equals-loaded below covers
    // any future addition; the shrink direction lives in lazily-spec's
    // `corpus-counts.json` / `scripts/check-corpus-floors.mjs`.
    let mut executed = 0usize;
    let mut outcomes: Vec<bool> = Vec::new();

    for (index, step) in steps.iter().enumerate() {
        let op = &step["op"];
        let kind = op["type"]
            .as_str()
            .unwrap_or_else(|| panic!("encoding step op carries no `type`: {step}"));
        let where_ = format!("{path} step {index} ({kind})");
        let exp = Expect::new(
            path.clone(),
            format!("steps[{index}].expected"),
            &step["expected"],
        );

        match kind {
            "digest_equal" => {
                let left = canonical_digest(&value_of(&values[op["left"].as_str().unwrap()]));
                let right = canonical_digest(&value_of(&values[op["right"].as_str().unwrap()]));
                let equal = left.as_ref().ok() == right.as_ref().ok() && left.is_ok();
                assert_eq!(step.fixture_flag_at("returns"), equal, "{where_}: returns");
                outcomes.push(equal);
            }
            "digest_defined" => {
                let result = canonical_digest(&value_of(&values[op["value"].as_str().unwrap()]));
                assert_eq!(
                    step.fixture_flag_at("returns"),
                    result.is_ok(),
                    "{where_}: returns"
                );
                assert!(
                    matches!(result, Err(ReplayProofError::Encoding { .. })),
                    "{where_}: an undefined value must fail as an ENCODING fault, \
                     routable by type rather than by message"
                );
                exp.assert_key_at("outcome", "encoding_error", &where_);
            }
            other => panic!("unknown canonical encoding operation `{other}`"),
        }
        executed += 1;
    }

    assert_eq!(
        executed,
        steps.len(),
        "{path}: loaded {} steps but executed {executed}",
        steps.len()
    );

    // Both outcomes really occurred: a runner that only ever saw `false` would
    // pass every inequality claim with a thoroughly broken encoding.
    assert!(
        outcomes.contains(&true) && outcomes.contains(&false),
        "{path}: the equality classes must produce BOTH outcomes, got {outcomes:?}"
    );
}

// -- the harness's own obligations, beyond the corpus -------------------------

/// The corpus asserts the localization but not the label vocabulary. A missing
/// or unexpected label is a different fault than a value that differs, and a
/// driver routing on `kind` has to be able to tell them apart.
#[test]
fn divergence_kinds_are_distinct() {
    assert_eq!(DivergenceKind::Value.as_str(), "value");
    assert_eq!(DivergenceKind::Missing.as_str(), "missing");
    assert_eq!(DivergenceKind::Unexpected.as_str(), "unexpected");
    assert_eq!(INITIAL_SEQ, -1);
}
