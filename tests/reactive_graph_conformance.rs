//! Cross-language conformance for the reactive-graph disposal/teardown plane
//! (`#lzspecedgeindex`) — see `lazily-spec/conformance/reactive-graph/*.json`.
//!
//! These fixtures shipped with no binding executing them. This runner is the
//! first executor: it replays each fixture's op stream against `Context`,
//! `Context::dispose_{slot,cell,effect}`, and `Context::scope()` /
//! `TeardownScope::disarm()`.
//!
//! ## What is asserted
//!
//! Every assertion kind in the corpus: `value`, `read`, `error`
//! (`read_after_dispose`), `readable`, `observed_by`, `observed_count`,
//! `cleanup_order` (effect entries only — derived slots run no cleanup callback
//! in rs), `scope_owned_count`, and — via `Context::dependent_count` /
//! `Context::dependency_count` — `dependents_of` and `dependencies_of`. An
//! unrecognised assertion key panics rather than being skipped.
//!
//! ## Divergences found
//!
//! See `KNOWN_DIVERGENCES`. In short: rs detaches both edge directions on
//! disposal but never dirties the surviving dependents, so a live reader that
//! still names a disposed node keeps serving its pre-disposal value forever —
//! it has no dependencies left, so not even a later publish revives it. Direct
//! reads of a disposed node do error, as specified.
//!
//! ## Fixture shape
//!
//! Every fixture declares a top-level `shape`, `steps` or `scenarios`, and the
//! runner dispatches on it. `scenarios` exists because a claim like
//! `observationally_equal` is a *relation between two op streams*, which a
//! single `steps` array cannot express; each scenario is replayed in its own
//! `Context` and the resulting observations compared. `cleanup_order` is
//! cumulative across a scenario rather than per-step — the individual-disposal
//! scenario spreads three disposals over three steps and pins the whole order
//! on the last one.
//!
//! ## Positive assertion (`#lzspecconf`)
//!
//! An absence guard is not enough — a runner that skips everything must fail.
//! This file asserts (a) the fixture set on disk matches `FIXTURES` exactly,
//! (b) every fixture was replayed, and (c) a non-zero number of ops actually
//! executed, per fixture and in total.

use std::cell::Cell as StdCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;

use lazily::{CellHandle, Context, EffectHandle, SlotHandle, TeardownScope};
use serde_json::Value;

const SPEC_DIR: &str = "../lazily-spec/conformance/reactive-graph";

/// The canonical fixture set. Asserted against the directory listing so a
/// fixture added or renamed upstream fails loudly instead of going unrun.
const FIXTURES: [&str; 8] = [
    "churn_returns_to_baseline.json",
    "cross_scope_teardown_hazard.json",
    "disarm_disposes_nothing.json",
    "dispose_detaches_edges_both_directions.json",
    "read_after_dispose_is_an_error.json",
    "recycled_id_inherits_nothing.json",
    "scope_teardown_equals_fold_of_disposals.json",
    "scoping_bounds_teardown_not_visibility.json",
];

/// Fixture assertions rs does not satisfy today, as `<fixture>[<scenario>]#<step>:<key>`.
/// Each entry is a finding against the implementation, not a relaxation of the
/// fixture: the runner asserts this list matches the observed set exactly, so
/// fixing rs fails the test until the entry is removed.
const KNOWN_DIVERGENCES: &[&str] = &[
    // (i) rs does not invalidate a disposed node's dependents, so a live reader
    // that still names a disposed node keeps serving the value it computed
    // before the disposal instead of erroring on its next recompute. The
    // fixtures call this out explicitly: "it MUST NOT return the value it
    // computed before the disposal", and "a binding that returns 3 here ... is
    // non-conforming". `dispose_slot`/`dispose_cell` detach both edge
    // directions but never mark the surviving dependents dirty.
    "cross_scope_teardown_hazard.json#7:error",
    "cross_scope_teardown_hazard.json#13:error",
    "read_after_dispose_is_an_error.json#6:error",
    // (iii) fixture defect: the assertion is ordered before the pull that
    // registers the edge it counts. `outside` is created reading `topic` but is
    // not read until the *next* step, and in a lazy binding an unpulled slot has
    // registered no dependency yet — so `topic` has 1 dependent here, not 2.
    // The corpus states this rule itself, in churn_returns_to_baseline.json's
    // `why_read_each` note ("a lazy binding that never pulls the slot registers
    // no dependency"), so this fixture contradicts its own sibling. Moving the
    // `read outside` step ahead of the `read b` step would make both binding
    // classes agree; not edited here.
    "scope_teardown_equals_fold_of_disposals.json[0]#6:dependents_of.topic",
    "scope_teardown_equals_fold_of_disposals.json[1]#5:dependents_of.topic",
];

// -- panic-as-error plumbing -------------------------------------------------

thread_local! {
    /// Set when a *nested* read (inside a compute or effect callback) hit a
    /// disposed node. The compute itself must not unwind: `Context` pushes and
    /// pops its tracking frame without an RAII guard, so unwinding out of a
    /// compute would strand a frame on the thread-local stack and corrupt every
    /// later read. Catching inside the callback keeps the stack balanced.
    static POISON: StdCell<bool> = const { StdCell::new(false) };
}

/// Run `f`, converting a panic into `Err(())` with the panic message suppressed.
fn quiet<R>(f: impl FnOnce() -> R) -> Result<R, ()> {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let out = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(prev);
    out.map_err(|_| ())
}

#[derive(Clone, Copy)]
enum NodeRef {
    Cell(CellHandle<i64>),
    Slot(SlotHandle<i64>),
    Effect(EffectHandle),
}

/// Read a node's value, reporting a disposed node as `Err` rather than a panic.
fn read_node(ctx: &Context, node: NodeRef) -> Result<i64, ()> {
    match node {
        NodeRef::Cell(h) => quiet(|| ctx.get_cell(&h)),
        NodeRef::Slot(h) => quiet(|| ctx.get(&h)),
        NodeRef::Effect(_) => Err(()),
    }
}

/// Read performed from inside a compute/effect callback: never unwinds, and
/// records the failure so the top-level read can surface `read_after_dispose`.
fn tracked_read(ctx: &Context, node: NodeRef) -> i64 {
    match read_node(ctx, node) {
        Ok(v) => v,
        Err(()) => {
            POISON.with(|p| p.set(true));
            0
        }
    }
}

fn dispose(ctx: &Context, node: NodeRef) {
    match node {
        NodeRef::Cell(h) => ctx.dispose_cell(&h),
        NodeRef::Slot(h) => ctx.dispose_slot(&h),
        NodeRef::Effect(h) => ctx.dispose_effect(&h),
    }
}

type Log = Rc<std::cell::RefCell<Vec<String>>>;

/// Everything a scenario leaves behind that the `observationally_equal` claim
/// is allowed to compare.
#[derive(Default, PartialEq, Eq, Debug)]
struct Observation {
    cleanup_order: Vec<String>,
    readable: BTreeMap<String, bool>,
    reads: BTreeMap<String, i64>,
    after_publish_observed: Vec<String>,
    after_publish_reads: BTreeMap<String, i64>,
    degrees: BTreeMap<String, usize>,
}

/// A fixture assertion rs does not currently satisfy.
#[derive(Debug)]
struct Divergence {
    step: usize,
    key: String,
    detail: String,
}

struct Report {
    failures: Vec<Divergence>,
    ops: usize,
    checks: usize,
    observation: Observation,
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

fn strs(v: &Value) -> Vec<String> {
    arr(v)
        .iter()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

/// Replay one op stream. `tail` is the `scenarios` fixture's `expected` block,
/// evaluated against the final world state when present.
fn replay(ctx: &Context, fixture: &str, steps: &[Value], tail: Option<&Value>) -> Report {
    let mut nodes: HashMap<String, NodeRef> = HashMap::new();
    // Handles are kept forever so `dispose_stale_handle` can dispose through an
    // id that has since been recycled.
    let mut stale: HashMap<String, NodeRef> = HashMap::new();
    let mut scopes: HashMap<String, TeardownScope<'_>> = HashMap::new();
    let mut poisoned: BTreeSet<String> = BTreeSet::new();
    let runs: Log = Log::default();
    let cleanups: Log = Log::default();
    let mut ops = 0usize;
    let mut checks = 0usize;
    let mut failures: Vec<Divergence> = Vec::new();
    let mut step_idx;

    macro_rules! reads_of {
        ($op:expr) => {
            strs(&$op["reads"])
                .into_iter()
                .map(|r| {
                    *nodes
                        .get(&r)
                        .unwrap_or_else(|| panic!("{fixture}: op reads unknown node {r}"))
                })
                .collect::<Vec<NodeRef>>()
        };
    }

    // A subscriber created by `fanout`/`churn`. Modelled as an effect, not a
    // derived slot: the fixtures assert `observed_count` on a publish, and in a
    // lazy binding only an eager reader observes a publish without being pulled.
    macro_rules! subscriber_fn {
        ($name:expr, $reads:expr) => {{
            let deps: Vec<NodeRef> = $reads;
            let name: String = $name;
            let run_log = runs.clone();
            move |c: &Context| {
                for r in &deps {
                    tracked_read(c, *r);
                }
                run_log.borrow_mut().push(name.clone());
            }
        }};
    }

    macro_rules! compute_fn {
        ($reads:expr, $offset:expr) => {{
            let reads: Vec<NodeRef> = $reads;
            let offset: i64 = $offset;
            move |c: &Context| -> i64 {
                let mut acc = offset;
                for r in &reads {
                    acc += tracked_read(c, *r);
                }
                acc
            }
        }};
    }

    // Top-level read: an `Err` here is the fixture's `read_after_dispose`.
    macro_rules! read_id {
        ($id:expr) => {{
            let id: &str = $id;
            if poisoned.contains(id) {
                Err(())
            } else {
                let node = *nodes
                    .get(id)
                    .unwrap_or_else(|| panic!("{fixture}: read of unknown node {id}"));
                POISON.with(|p| p.set(false));
                match read_node(ctx, node) {
                    Err(()) => {
                        poisoned.insert(id.to_owned());
                        Err(())
                    }
                    Ok(v) => {
                        if POISON.with(|p| p.get()) {
                            // A live reader that still names a disposed
                            // dependency errors on its next recompute, and stays
                            // broken until it is itself rebuilt.
                            poisoned.insert(id.to_owned());
                            Err(())
                        } else {
                            Ok(v)
                        }
                    }
                }
            }
        }};
    }

    // Record rather than panic: a fixture assertion that rs does not satisfy is
    // a finding, and the whole corpus should be reported in one run. The set of
    // recorded divergences is asserted against `KNOWN_DIVERGENCES` by the
    // caller, so a divergence can neither be forgotten nor silently appear.
    macro_rules! check {
        ($key:expr, $got:expr, $want:expr) => {{
            checks += 1;
            let (got, want) = ($got, $want);
            if got != want {
                failures.push(Divergence {
                    step: step_idx,
                    key: $key.to_string(),
                    detail: format!("got {got:?}, want {want:?}"),
                });
            }
        }};
    }

    macro_rules! degree {
        ($id:expr, $method:ident) => {{
            let id: &str = $id;
            match nodes
                .get(id)
                .unwrap_or_else(|| panic!("{fixture}: degree of unknown node {id}"))
            {
                NodeRef::Cell(h) => ctx.$method(h),
                NodeRef::Slot(h) => ctx.$method(h),
                NodeRef::Effect(h) => ctx.$method(h),
            }
        }};
    }

    for (i, step) in steps.iter().enumerate() {
        step_idx = i;
        let op = &step["op"];
        let kind = op["type"]
            .as_str()
            .unwrap_or_else(|| panic!("{fixture}: step has no op type"));
        let runs_before = runs.borrow().len();
        let cleanups_before = cleanups.borrow().len();
        let mut op_error = false;
        let mut op_value: Option<i64> = None;
        ops += 1;

        match kind {
            "cell" => {
                let id = op["id"].as_str().unwrap().to_owned();
                let h = match op["scope"].as_str() {
                    Some(s) => scopes[s].cell(op["value"].as_i64().unwrap()),
                    None => ctx.cell(op["value"].as_i64().unwrap()),
                };
                nodes.insert(id.clone(), NodeRef::Cell(h));
                stale.insert(id.clone(), NodeRef::Cell(h));
                poisoned.remove(&id);
            }
            "computed" => {
                let id = op["id"].as_str().unwrap().to_owned();
                let f = compute_fn!(reads_of!(op), op["offset"].as_i64().unwrap_or(0));
                let h = match op["scope"].as_str() {
                    Some(s) => scopes[s].computed(f),
                    None => ctx.computed(f),
                };
                nodes.insert(id.clone(), NodeRef::Slot(h));
                stale.insert(id.clone(), NodeRef::Slot(h));
                poisoned.remove(&id);
            }
            "effect" => {
                let id = op["id"].as_str().unwrap().to_owned();
                let deps: Vec<NodeRef> = reads_of!(op);
                let name = id.clone();
                let run_log = runs.clone();
                let cleanup_log = cleanups.clone();
                let f = move |c: &Context| {
                    for r in &deps {
                        tracked_read(c, *r);
                    }
                    run_log.borrow_mut().push(name.clone());
                    let cl = cleanup_log.clone();
                    let n = name.clone();
                    move || cl.borrow_mut().push(n)
                };
                let h = match op["scope"].as_str() {
                    Some(s) => scopes[s].effect(f),
                    None => ctx.effect(f),
                };
                nodes.insert(id.clone(), NodeRef::Effect(h));
                stale.insert(id.clone(), NodeRef::Effect(h));
                poisoned.remove(&id);
            }
            "read" => match read_id!(op["id"].as_str().unwrap()) {
                Ok(v) => op_value = Some(v),
                Err(()) => op_error = true,
            },
            "set_cell" => {
                let id = op["id"].as_str().unwrap();
                match nodes[id] {
                    NodeRef::Cell(h) => ctx.set_cell(&h, op["value"].as_i64().unwrap()),
                    _ => panic!("{fixture}: set_cell on non-cell {id}"),
                }
            }
            "dispose" => {
                // The entry stays in the map: a disposed id remains readable-as-
                // an-error, and disposing it again must be a no-op.
                dispose(ctx, nodes[op["id"].as_str().unwrap()]);
            }
            "fanout" => {
                let prefix = op["id_prefix"].as_str().unwrap();
                let count = op["count"].as_u64().unwrap();
                let read_each = op["read_each"].as_bool().unwrap_or(false);
                let base: Vec<NodeRef> = reads_of!(op);
                let _ = read_each; // an effect subscriber pulls on creation
                for i in 0..count {
                    let id = format!("{prefix}_{i}");
                    let h = ctx.effect(subscriber_fn!(id.clone(), base.clone()));
                    nodes.insert(id.clone(), NodeRef::Effect(h));
                    stale.insert(id, NodeRef::Effect(h));
                }
            }
            "dispose_fanout" => {
                let prefix = op["id_prefix"].as_str().unwrap();
                for i in 0..op["count"].as_u64().unwrap() {
                    let id = format!("{prefix}_{i}");
                    if let Some(n) = nodes.get(&id) {
                        dispose(ctx, *n);
                    }
                }
            }
            "churn" => {
                let source = *nodes.get(op["source"].as_str().unwrap()).unwrap();
                let prefix = op["id_prefix"].as_str().unwrap();
                let width = op["live_width"].as_u64().unwrap();
                let cycles = op["cycles"].as_u64().unwrap();
                let _ = op["read_each"].as_bool().unwrap_or(false);
                match op["mode"].as_str().unwrap() {
                    // Hold `live_width` subscribers; each cycle disposes one and
                    // creates its replacement, so the live count is invariant.
                    "dispose_then_create" => {
                        for c in 0..cycles {
                            let id = format!("{prefix}_{}", c % width);
                            if let Some(n) = nodes.get(&id) {
                                dispose(ctx, *n);
                            }
                            let h = ctx.effect(subscriber_fn!(id.clone(), vec![source]));
                            nodes.insert(id, NodeRef::Effect(h));
                        }
                    }
                    // One teardown scope per cycle; its subscriber is gone by the
                    // end of its own cycle.
                    "scope_per_cycle" => {
                        for _ in 0..cycles {
                            let sc = ctx.scope();
                            sc.effect(subscriber_fn!(format!("{prefix}_scoped"), vec![source]));
                            drop(sc);
                        }
                    }
                    other => panic!("{fixture}: unknown churn mode {other}"),
                }
            }
            "begin_scope" => {
                scopes.insert(op["scope"].as_str().unwrap().to_owned(), ctx.scope());
            }
            "end_scope" => {
                let name = op["scope"].as_str().unwrap().to_owned();
                let sc = scopes.remove(&name).unwrap();
                op_error = quiet(move || drop(sc)).is_err();
            }
            "disarm" => {
                let name = op["scope"].as_str().unwrap();
                scopes.remove(name).unwrap().disarm();
                // A disarmed scope owns nothing and is gone; re-open an empty one
                // under the same name so a later `end_scope` is a no-op.
                scopes.insert(name.to_owned(), ctx.scope());
            }
            "dispose_stale_handle" => {
                let of = op["handle_of"].as_str().unwrap();
                let h = *stale
                    .get(of)
                    .unwrap_or_else(|| panic!("{fixture}: no recorded handle for {of}"));
                match (op["handle_kind"].as_str().unwrap(), h) {
                    ("cell", NodeRef::Cell(c)) => ctx.dispose_cell(&c),
                    ("slot", NodeRef::Slot(s)) => ctx.dispose_slot(&s),
                    ("effect", NodeRef::Effect(e)) => ctx.dispose_effect(&e),
                    (k, _) => panic!("{fixture}: handle_kind {k} does not match recorded handle"),
                }
            }
            other => panic!("{fixture}: unknown op {other}"),
        }

        let observed: Vec<String> = runs.borrow()[runs_before..].to_vec();
        // `cleanup_order` is cumulative, not per-step: the individual-disposal
        // scenario spreads three disposals over three steps and pins the whole
        // order on the last one.
        let _ = cleanups_before;
        let cleaned: Vec<String> = cleanups.borrow().clone();

        let Some(expect) = step.get("expect") else {
            continue;
        };
        let Some(expect) = expect.as_object() else {
            continue;
        };

        for (key, want) in expect {
            match key.as_str() {
                "note" => {}
                "dependents_of" => {
                    for (id, v) in want.as_object().unwrap() {
                        check!(
                            format!("dependents_of.{id}"),
                            degree!(id.as_str(), dependent_count),
                            v.as_u64().unwrap() as usize
                        );
                    }
                }
                "dependencies_of" => {
                    for (id, v) in want.as_object().unwrap() {
                        check!(
                            format!("dependencies_of.{id}"),
                            degree!(id.as_str(), dependency_count),
                            v.as_u64().unwrap() as usize
                        );
                    }
                }
                "error" => match want.as_str() {
                    Some("read_after_dispose") => check!("error", op_error, true),
                    None => check!("error", op_error, false),
                    Some(other) => panic!("{fixture}: unknown expected error {other}"),
                },
                "value" => {
                    if expect.get("error").and_then(Value::as_str).is_none() {
                        check!("value", op_value, want.as_i64());
                    }
                }
                "read" => {
                    for (id, v) in want.as_object().unwrap() {
                        check!(
                            format!("read.{id}"),
                            read_id!(id.as_str()),
                            Ok(v.as_i64().unwrap())
                        );
                    }
                }
                "readable" => {
                    for (id, v) in want.as_object().unwrap() {
                        let alive = match nodes.get(id.as_str()) {
                            None => false,
                            Some(NodeRef::Effect(h)) => ctx.is_effect_active(h),
                            Some(_) => read_id!(id.as_str()).is_ok(),
                        };
                        check!(format!("readable.{id}"), alive, v.as_bool().unwrap());
                    }
                }
                "observed_by" => check!("observed_by", observed.clone(), strs(want)),
                "observed_count" => {
                    check!(
                        "observed_count",
                        observed.len() as u64,
                        want.as_u64().unwrap()
                    )
                }
                "cleanup_order" => {
                    // Only effects run a cleanup callback in rs; derived slots
                    // have none, so the expected order is projected onto its
                    // effect entries.
                    let want: Vec<String> = strs(want)
                        .into_iter()
                        .filter(|id| matches!(stale.get(id), Some(NodeRef::Effect(_))))
                        .collect();
                    check!("cleanup_order", cleaned.clone(), want);
                }
                "scope_owned_count" => {
                    for (name, v) in want.as_object().unwrap() {
                        check!(
                            format!("scope_owned_count.{name}"),
                            scopes[name.as_str()].len() as u64,
                            v.as_u64().unwrap()
                        );
                    }
                }
                other => panic!("{fixture}: unknown expectation {other}"),
            }
        }
    }

    // -- `scenarios`-shaped tail ------------------------------------------
    let mut observation = Observation {
        cleanup_order: cleanups.borrow().clone(),
        ..Observation::default()
    };
    if let Some(tail) = tail {
        step_idx = usize::MAX; // the `expected` tail is not a numbered step
        let fin = &tail["final_state"];
        for (id, v) in fin["dependents_of"].as_object().into_iter().flatten() {
            check!(
                format!("final.dependents_of.{id}"),
                degree!(id.as_str(), dependent_count),
                v.as_u64().unwrap() as usize
            );
            observation
                .degrees
                .insert(id.clone(), degree!(id.as_str(), dependent_count));
        }
        for (id, v) in fin["readable"].as_object().into_iter().flatten() {
            let alive = match nodes.get(id.as_str()) {
                None => false,
                Some(NodeRef::Effect(h)) => ctx.is_effect_active(h),
                Some(_) => read_id!(id.as_str()).is_ok(),
            };
            check!(format!("final.readable.{id}"), alive, v.as_bool().unwrap());
            observation.readable.insert(id.clone(), alive);
        }
        for (id, v) in fin["read"].as_object().into_iter().flatten() {
            let got = read_id!(id.as_str());
            check!(format!("final.read.{id}"), got, Ok(v.as_i64().unwrap()));
            observation
                .reads
                .insert(id.clone(), got.unwrap_or_default());
        }

        let pub_ = &tail["after_publish"];
        if let Some(pop) = pub_.get("op") {
            let id = pop["id"].as_str().unwrap();
            let before = runs.borrow().len();
            match nodes[id] {
                NodeRef::Cell(h) => ctx.set_cell(&h, pop["value"].as_i64().unwrap()),
                _ => panic!("{fixture}: after_publish set_cell on non-cell"),
            }
            observation.after_publish_observed = runs.borrow()[before..].to_vec();
            check!(
                "after_publish.observed_by",
                observation.after_publish_observed.clone(),
                strs(&pub_["observed_by"])
            );
            for (rid, v) in pub_["read"].as_object().into_iter().flatten() {
                let got = read_id!(rid.as_str());
                check!(
                    format!("after_publish.read.{rid}"),
                    got,
                    Ok(v.as_i64().unwrap())
                );
                observation
                    .after_publish_reads
                    .insert(rid.clone(), got.unwrap_or_default());
            }
            for (id, v) in pub_["dependents_of"].as_object().into_iter().flatten() {
                check!(
                    format!("after_publish.dependents_of.{id}"),
                    degree!(id.as_str(), dependent_count),
                    v.as_u64().unwrap() as usize
                );
            }
        }
    }

    Report {
        failures,
        ops,
        checks,
        observation,
    }
}

fn load(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn present() -> bool {
    std::path::Path::new(SPEC_DIR).is_dir()
}

#[test]
fn reactive_graph_conformance() {
    if !present() {
        eprintln!(
            "SKIP reactive_graph_conformance: {SPEC_DIR} not found — clone lazily-spec as a \
             sibling to run the #lzspecedgeindex disposal/teardown fixtures"
        );
        return;
    }

    // The fixture set on disk must be exactly the one this runner knows about,
    // so an upstream addition cannot arrive unexecuted.
    let on_disk: BTreeSet<String> = fs::read_dir(SPEC_DIR)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    let known: BTreeSet<String> = FIXTURES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        on_disk, known,
        "reactive-graph fixture set drifted; every fixture must be replayed by this runner"
    );

    let mut replayed = 0usize;
    let mut total_ops = 0usize;
    let mut total_checks = 0usize;
    let mut divergences = 0usize;
    let mut observed_divergences: BTreeSet<String> = BTreeSet::new();

    for name in FIXTURES {
        let fx = load(name);
        // Dispatch on the fixture's declared `shape` (lazily-spec 60f62aa), not
        // on its filename: a filename special case goes stale silently the
        // moment a second scenarios-shaped fixture is added. An unrecognised
        // shape is a hard error rather than a fallback to `steps`.
        let reports: Vec<Report> = match fx["shape"].as_str() {
            Some("steps") => {
                let ctx = Context::new();
                vec![replay(&ctx, name, arr(&fx["steps"]), None)]
            }
            Some("scenarios") => fx["scenarios"]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: shape is `scenarios` but no scenarios array"))
                .iter()
                .map(|s| {
                    let ctx = Context::new();
                    replay(&ctx, name, arr(&s["steps"]), Some(&fx["expected"]))
                })
                .collect(),
            Some(other) => panic!("{name}: unknown fixture shape {other}"),
            None => panic!("{name}: fixture declares no `shape`"),
        };

        // `observationally_equal`: the named scenarios must agree on every
        // observable, not merely each satisfy `expected` independently.
        if let Some(pair) = fx["expected"]["observationally_equal"].as_array() {
            let names: Vec<&str> = fx["scenarios"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap())
                .collect();
            let idx: Vec<usize> = pair
                .iter()
                .map(|p| {
                    names
                        .iter()
                        .position(|n| *n == p.as_str().unwrap())
                        .unwrap_or_else(|| panic!("{name}: unknown scenario {p}"))
                })
                .collect();
            for w in idx.windows(2) {
                assert_eq!(
                    reports[w[0]].observation, reports[w[1]].observation,
                    "{name}: scenarios are not observationally equal"
                );
            }
            total_checks += 1;
        }

        let ops: usize = reports.iter().map(|r| r.ops).sum();
        let checks: usize = reports.iter().map(|r| r.checks).sum();
        assert!(ops > 0, "{name}: replayed zero ops");
        assert!(checks > 0, "{name}: replayed zero assertions");

        // Divergence ledger: the recorded set must equal the documented one, so
        // a new divergence fails the build and a fixed one forces the entry to
        // be deleted. Neither direction can pass unnoticed.
        let mut got: BTreeSet<String> = BTreeSet::new();
        for (si, r) in reports.iter().enumerate() {
            for f in &r.failures {
                let step = if f.step == usize::MAX {
                    "expected".to_owned()
                } else {
                    f.step.to_string()
                };
                let scenario = if reports.len() > 1 {
                    format!("[{si}]")
                } else {
                    String::new()
                };
                eprintln!(
                    "  DIVERGENCE {name}{scenario}#{step}:{} — {}",
                    f.key, f.detail
                );
                got.insert(format!("{name}{scenario}#{step}:{}", f.key));
            }
        }
        divergences += got.len();
        observed_divergences.extend(got);

        eprintln!("reactive-graph {name}: {ops} ops, {checks} assertions");
        total_ops += ops;
        total_checks += checks;
        replayed += 1;
    }

    eprintln!(
        "reactive-graph conformance: {replayed} fixtures, {total_ops} ops, \
         {total_checks} assertions, {divergences} documented divergences"
    );

    // Divergence ledger: the observed set must equal the documented one, so a
    // new divergence fails the build and a fixed one forces its entry to be
    // deleted. Neither direction can pass unnoticed.
    let documented: BTreeSet<String> = KNOWN_DIVERGENCES.iter().map(|d| (*d).to_owned()).collect();
    assert_eq!(
        observed_divergences, documented,
        "reactive-graph divergence ledger is stale — update KNOWN_DIVERGENCES \
         (left = observed, right = documented)"
    );

    // Positive assertion: the runner must have actually executed the corpus.
    assert_eq!(
        replayed,
        FIXTURES.len(),
        "reactive-graph runner did not replay every fixture"
    );
    assert!(total_ops > 0, "reactive-graph runner executed zero ops");
    assert!(
        total_checks > 0,
        "reactive-graph runner executed zero assertions"
    );
}
