//! View-document conformance. Replays `lazily-spec/conformance/view`.
//!
//! The area is absent from the published spec tag this crate's CI clones, so a
//! missing directory skips. When the directory is present every fixture in it
//! is listed and replayed.

#![cfg(feature = "view")]

mod common;

use std::fs;

use common::Expect;
use lazily::{Context, View};
use serde_json::Value;

const SPEC_DIR: common::SpecDir = common::SpecDir("view");

const FIXTURES: &[&str] = &[
    "batch_send.json",
    "counter.json",
    "field_change.json",
    "malformed_rejected.json",
];

fn load_fixture(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw = crate::common::spec_read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn observe(view: &View, ctx: &Context, block: &Expect) {
    block.assert_key("signals", view.signals_json(ctx));
    block.assert_key("expressions", view.expressions_json(ctx));
    block.assert_key("layout", view.layout_json());
    block.assert_key("trace", Value::Array(Vec::new()));
    block.assert_key("chart", view.chart_json(ctx));
}

fn run_fixture(name: &str) {
    let fixture = load_fixture(name);
    let ctx = Context::new();
    let view = View::install(&ctx, fixture.get("view").expect("view"))
        .unwrap_or_else(|e| panic!("{name}: install: {e}"));
    let _subscription = view.subscription();

    let fx = Expect::new(format!("{SPEC_DIR}/{name}"), "<fixture>", &fixture);
    fx.excuse_key("kind", "corpus routing tag");
    fx.excuse_key("view", "the view document under test; an input");
    fx.excuse_key("steps", "the event sequence replayed below");
    observe(&view, &ctx, &fx);

    let steps = fx.raw()["steps"].as_array().expect("steps");
    for (i, step) in steps.iter().enumerate() {
        let step = fx.nested(format!("steps[{i}]"), step);
        step.excuse_key("node", "the node the event is dispatched to; an input");
        step.excuse_key("event", "the semantic event; an input");
        step.excuse_key("value", "the event payload; an input");
        let node = step.raw()["node"].as_str().expect("node");
        let event = step.raw()["event"].as_str().expect("event");
        let trace = view
            .dispatch_json(&ctx, node, event, &step.raw()["value"])
            .unwrap_or_else(|e| panic!("{name} step {i}: {e}"));
        step.assert_key("signals", view.signals_json(&ctx));
        step.assert_key("expressions", view.expressions_json(&ctx));
        step.assert_key("trace", trace);
        step.assert_key("chart", view.chart_json(&ctx));
    }
}

fn run_malformed() {
    let fixture = load_fixture("malformed_rejected.json");
    let fx = Expect::new(
        format!("{SPEC_DIR}/malformed_rejected.json"),
        "<fixture>",
        &fixture,
    );
    fx.excuse_key("kind", "corpus routing tag");
    fx.excuse_key(
        "cases",
        "each case is an input document install must reject",
    );
    let cases = fx.raw()["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "malformed corpus must contain cases");
    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let ctx = Context::new();
        let err = View::install(&ctx, &case["view"]);
        assert!(err.is_err(), "{name} must be rejected");
    }
}

#[test]
fn view_conformance() {
    if !SPEC_DIR.is_dir() {
        eprintln!("skipping: {SPEC_DIR} absent - run with the lazily-spec sibling");
        return;
    }
    let mut found: Vec<String> = fs::read_dir(SPEC_DIR.path())
        .unwrap_or_else(|e| panic!("read {}: {e}", SPEC_DIR.path().display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("read entry: {e}"))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".json"))
        .collect();
    found.sort();
    assert_eq!(found, FIXTURES, "view fixture set changed");
    run_fixture("counter.json");
    run_fixture("field_change.json");
    run_fixture("batch_send.json");
    run_malformed();
}
