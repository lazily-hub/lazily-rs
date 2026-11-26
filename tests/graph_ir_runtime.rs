//! Load a graph IR document, lower it onto a context, then edit it.
//!
//! Fixtures are copies of `lazily-spec` `examples/graph-ir/`. This crate's CI
//! clones spec `main`, which does not have the schema yet, so the test does
//! not read that checkout.

use std::collections::BTreeSet;

use lazily::graph_ir::{Edge, EdgeKind, Expr, GraphEdit, GraphIr, GraphRuntime, Node, Op, Policy};
use serde_json::{Value, json};

const COUNTER: &str = include_str!("fixtures/graph_ir/counter.json");
const COUNTER_EDIT: &str = include_str!("fixtures/graph_ir/counter-edit.json");
const DYNAMIC: &str = include_str!("fixtures/graph_ir/dynamic.json");
const POLICY: &str = include_str!("fixtures/graph_ir/policy.json");
const STRUCTURES: &str = include_str!("fixtures/graph_ir/structures.json");
const CYCLE: &str = include_str!("fixtures/graph_ir/cycle.json");

const COUNTER_SOURCE: &str = "\
graph counter ir 1
source count = 1
computed doubled eager = mul(ref(count), lit(2))
effect log = eval(ref(doubled))
";

#[test]
fn counter_lowers_then_an_edit_replaces_the_graph() {
    let doc = GraphIr::from_json_str(COUNTER).expect("counter document");
    assert_eq!(doc.id(), Some("counter"));
    assert!(matches!(
        doc.node("doubled"),
        Some(Node::Computed(node)) if node.eager && matches!(node.expr, Expr::Call { op: Op::Mul, .. })
    ));
    assert_eq!(
        doc.static_edges(),
        vec![read("doubled", "count"), read("log", "doubled")]
    );
    assert_eq!(doc.emit_source(), COUNTER_SOURCE);

    let again = GraphIr::from_json(&doc.to_json()).expect("round trip");
    assert_eq!(again.emit_source(), COUNTER_SOURCE);

    let mut rt = GraphRuntime::lower(doc).expect("lower");
    assert_eq!(must_get(&rt, "doubled"), json!(2));
    assert_eq!(rt.effect_trace().len(), 1);
    assert_eq!(rt.effect_trace()[0].id, "log");
    assert_eq!(rt.effect_trace()[0].result, json!(2));
    assert_eq!(rt.observed_reads("doubled"), names(&["count"]));
    assert_eq!(rt.observed_reads("log"), names(&["doubled"]));
    assert_eq!(rt.is_eager("doubled"), Ok(true));
    assert_eq!(rt.source_policy("count"), Ok(Policy::KeepLatest));

    let before_write = rt.emit_source();
    rt.write_source("count", json!(4)).expect("write count");
    assert_eq!(must_get(&rt, "doubled"), json!(8));
    assert_eq!(rt.effect_trace().len(), 2);
    assert_eq!(rt.effect_trace()[1].result, json!(8));
    assert_eq!(rt.emit_source(), before_write);

    rt.apply(&edit(COUNTER_EDIT)).expect("counter edit");
    assert_eq!(must_get(&rt, "doubled"), json!(14));
    assert_eq!(must_get(&rt, "shown"), json!("n:"));
    assert!(!rt.contains("log"));
    assert_eq!(rt.is_eager("doubled"), Ok(false));
    let trace_len = rt.effect_trace().len();
    rt.write_source("count", json!(5))
        .expect("write count again");
    assert_eq!(must_get(&rt, "doubled"), json!(15));
    assert_eq!(rt.effect_trace().len(), trace_len);
    let emitted = rt.emit_source();
    assert_eq!(
        emitted,
        "\
graph counter ir 1
source count = 1
computed doubled = add(ref(count), lit(10))
source label = \"n\"
computed shown = concat(ref(label), lit(\":\"))
"
    );
    assert!(!emitted.contains("mul("));
    assert!(!emitted.contains("effect log"));
}

#[test]
fn dynamic_if_observes_only_the_taken_branch() {
    let rt = runtime(DYNAMIC);
    assert_eq!(
        rt.static_edges(),
        vec![
            read("picked", "flag"),
            read("picked", "left"),
            read("picked", "right"),
        ]
    );
    assert_eq!(must_get(&rt, "picked"), json!("R"));
    assert_eq!(rt.observed_reads("picked"), names(&["flag", "right"]));
    rt.write_source("flag", json!(true)).expect("flip flag");
    assert_eq!(must_get(&rt, "picked"), json!("L"));
    assert_eq!(rt.observed_reads("picked"), names(&["flag", "left"]));
}

#[test]
fn sum_policy_folds_on_merge_and_skips_an_equal_write() {
    let rt = runtime(POLICY);
    assert!(rt.emit_source().contains("policy sum"));
    assert!(rt.emit_source().contains("merge(total, lit(1))"));
    assert_eq!(must_get(&rt, "total"), json!(1));
    assert_eq!(rt.effect_trace()[0].id, "bump");
    assert_eq!(rt.effect_trace()[0].result, json!([0, 1]));
    assert_eq!(rt.source_policy("total"), Ok(Policy::Sum));
    rt.write_source("tick", json!(1)).expect("tick");
    assert_eq!(must_get(&rt, "total"), json!(2));
    assert_eq!(rt.effect_trace().len(), 2);
    rt.write_source("tick", json!(1)).expect("equal tick");
    assert_eq!(rt.effect_trace().len(), 2);
    assert_eq!(must_get(&rt, "total"), json!(2));
}

#[test]
fn structures_read_fields_length_and_objects() {
    let rt = runtime(STRUCTURES);
    assert_eq!(must_get(&rt, "name"), json!("ada"));
    assert_eq!(must_get(&rt, "n"), json!(2));
    assert_eq!(must_get(&rt, "tag"), json!("ada#"));
    assert_eq!(must_get(&rt, "width"), json!(3));
    assert_eq!(must_get(&rt, "pair"), json!(["ada", 2]));
    assert_eq!(must_get(&rt, "wrapped"), json!({"name": "ada"}));
}

#[test]
fn cycle_is_schema_shaped_and_rejected_on_load() {
    let error = GraphIr::from_json_str(CYCLE).expect_err("cycle");
    assert!(error.contains("cycle in read edges"), "{error}");
    match GraphRuntime::load_json(CYCLE) {
        Ok(_) => panic!("cycle runtime lowered"),
        Err(error) => assert!(error.contains("cycle in read edges"), "{error}"),
    }
}

#[test]
fn snapshot_envelope_is_not_a_graph_document() {
    let error =
        GraphIr::from_json_str(r#"{"Snapshot":{"epoch":0,"nodes":[],"edges":[],"roots":[]}}"#)
            .expect_err("snapshot");
    assert!(error.contains("graph"), "{error}");
}

#[test]
fn null_init_and_ordered_boolean_policies() {
    let rt = runtime(
        r#"{
          "kind": "graph", "ir": "1",
          "nodes": [
            {"id": "empty", "kind": "source", "init": null},
            {"id": "hi", "kind": "source", "init": 3, "policy": "max"},
            {"id": "lo", "kind": "source", "init": 3, "policy": "min"},
            {"id": "both", "kind": "source", "init": true, "policy": "and"},
            {"id": "either", "kind": "source", "init": false, "policy": "or"},
            {"id": "fold", "kind": "effect", "body": {"do": "seq", "steps": [
              {"do": "merge", "target": "hi", "expr": {"lit": 1}},
              {"do": "merge", "target": "hi", "expr": {"lit": 5}},
              {"do": "merge", "target": "hi", "expr": {"lit": 5}},
              {"do": "merge", "target": "lo", "expr": {"lit": 1}},
              {"do": "merge", "target": "both", "expr": {"lit": false}},
              {"do": "merge", "target": "either", "expr": {"lit": true}}
            ]}}
          ]
        }"#,
    );
    assert_eq!(must_get(&rt, "empty"), Value::Null);
    assert_eq!(must_get(&rt, "hi"), json!(5));
    assert_eq!(must_get(&rt, "lo"), json!(1));
    assert_eq!(must_get(&rt, "both"), json!(false));
    assert_eq!(must_get(&rt, "either"), json!(true));
    assert_eq!(rt.source_policy("hi"), Ok(Policy::Max));
    assert_eq!(rt.source_policy("lo"), Ok(Policy::Min));
    assert_eq!(rt.source_policy("both"), Ok(Policy::And));
    assert_eq!(rt.source_policy("either"), Ok(Policy::Or));
}

#[test]
fn and_short_circuits_and_div_by_zero_faults_on_get() {
    let rt = runtime(
        r#"{
          "kind": "graph", "ir": "1",
          "nodes": [
            {"id": "flag", "kind": "source", "init": false},
            {"id": "n", "kind": "source", "init": 1},
            {"id": "bad", "kind": "computed", "expr": {"op": "div", "args": [{"ref": "n"}, {"lit": 0}]}},
            {"id": "gate", "kind": "computed", "expr": {"op": "and", "args": [{"ref": "flag"}, {"ref": "bad"}]}}
          ]
        }"#,
    );
    assert_eq!(must_get(&rt, "gate"), json!(false));
    assert_eq!(rt.observed_reads("gate"), names(&["flag"]));
    let error = rt.get("bad").expect_err("div zero");
    assert!(error.contains("division by zero"), "{error}");
}

#[test]
fn remove_of_a_live_dependency_keeps_the_previous_graph() {
    let mut rt = runtime(
        r#"{
          "kind": "graph", "ir": "1",
          "nodes": [
            {"id": "a", "kind": "source", "init": 1},
            {"id": "b", "kind": "computed", "expr": {"ref": "a"}}
          ]
        }"#,
    );
    let error = rt
        .apply(&edit(
            r#"{"kind":"edit","ir":"1","ops":[{"op":"remove","id":"a"}]}"#,
        ))
        .expect_err("remove a");
    assert!(error.contains("edit op 0"), "{error}");
    assert!(rt.contains("a") && rt.contains("b"));
    assert_eq!(must_get(&rt, "b"), json!(1));
}

#[test]
fn forward_ref_inside_one_edit_applies_nothing() {
    let mut rt = runtime(r#"{"kind":"graph","ir":"1","nodes":[]}"#);
    let error = rt
        .apply(&edit(
            r#"{
              "kind": "edit", "ir": "1",
              "ops": [
                {"op": "add", "node": {"id": "a", "kind": "computed", "expr": {"ref": "b"}}},
                {"op": "add", "node": {"id": "b", "kind": "source", "init": 1}}
              ]
            }"#,
        ))
        .expect_err("forward ref");
    assert!(error.contains("edit op 0"), "{error}");
    assert!(!rt.contains("a") && !rt.contains("b"));
    assert_eq!(rt.emit_source(), "graph ir 1\n");
}

#[test]
fn replace_source_resets_the_live_value_to_the_new_init() {
    let mut rt = runtime(
        r#"{"kind":"graph","ir":"1","id":"g","nodes":[
          {"id":"count","kind":"source","init":1}
        ]}"#,
    );
    rt.write_source("count", json!(9)).expect("host write");
    assert_eq!(must_get(&rt, "count"), json!(9));
    assert_eq!(rt.emit_source(), "graph g ir 1\nsource count = 1\n");
    rt.apply(&edit(
        r#"{"kind":"edit","ir":"1","ops":[
          {"op":"replace","node":{"id":"count","kind":"source","init":0}}
        ]}"#,
    ))
    .expect("replace source");
    assert_eq!(must_get(&rt, "count"), json!(0));
    assert_eq!(rt.emit_source(), "graph g ir 1\nsource count = 0\n");
}

#[test]
fn when_set_writes_and_a_skipped_when_records_null() {
    let rt = runtime(
        r#"{
          "kind": "graph", "ir": "1",
          "nodes": [
            {"id": "on", "kind": "source", "init": true},
            {"id": "value", "kind": "source", "init": 0},
            {"id": "apply", "kind": "effect", "body": {
              "do": "when",
              "cond": {"ref": "on"},
              "then": {"do": "set", "target": "value", "expr": {"lit": 7}}
            }}
          ]
        }"#,
    );
    assert_eq!(must_get(&rt, "value"), json!(7));
    assert_eq!(rt.effect_trace()[0].result, json!(7));

    let rt = runtime(
        r#"{
          "kind": "graph", "ir": "1",
          "nodes": [
            {"id": "on", "kind": "source", "init": false},
            {"id": "value", "kind": "source", "init": 0},
            {"id": "apply", "kind": "effect", "body": {
              "do": "when",
              "cond": {"ref": "on"},
              "then": {"do": "set", "target": "value", "expr": {"lit": 7}}
            }}
          ]
        }"#,
    );
    assert_eq!(must_get(&rt, "value"), json!(0));
    assert_eq!(rt.effect_trace()[0].result, Value::Null);
}

#[test]
fn empty_graph_accepts_an_add() {
    let mut rt = runtime(r#"{"kind":"graph","ir":"1","nodes":[]}"#);
    rt.apply(&edit(
        r#"{"kind":"edit","ir":"1","ops":[
          {"op":"add","node":{"id":"x","kind":"source","init":1}}
        ]}"#,
    ))
    .expect("add");
    assert_eq!(must_get(&rt, "x"), json!(1));
    assert_eq!(rt.emit_source(), "graph ir 1\nsource x = 1\n");
}

fn runtime(text: &str) -> GraphRuntime {
    GraphRuntime::load_json(text).unwrap_or_else(|error| panic!("{error}"))
}

fn edit(text: &str) -> GraphEdit {
    GraphEdit::from_json_str(text).unwrap_or_else(|error| panic!("{error}"))
}

fn must_get(rt: &GraphRuntime, id: &str) -> Value {
    rt.get(id)
        .unwrap_or_else(|error| panic!("get {id}: {error}"))
}

fn names(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

fn read(from: &str, to: &str) -> Edge {
    Edge {
        from: from.to_string(),
        kind: EdgeKind::Read,
        to: to.to_string(),
    }
}
