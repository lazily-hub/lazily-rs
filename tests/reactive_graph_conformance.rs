//! Cross-language conformance for the reactive-graph disposal/teardown plane
//! (`#lzspecedgeindex`) — see `lazily-spec/conformance/reactive-graph/*.json`.
//!
//! These fixtures shipped with no binding executing them. This runner is the
//! first executor, and it is parameterised over the *execution model* so the
//! corpus can reach every context `lazily` ships rather than only the basic one.
//! A disposal contract that structurally cannot reach two of three execution
//! models is the same defect as a suite that tests nothing, one layer down:
//! `#lzspecedgeindex` is about per-node graph memory, and the thread-safe and
//! async paths are exactly where a leak is hardest to notice and hardest to
//! reproduce.
//!
//! ## What is asserted
//!
//! Every assertion kind in the corpus: `value`, `read`, `error`
//! (`read_after_dispose`), `readable`, `observed_by`, `observed_count`,
//! `cleanup_order` (effect entries only — derived slots run no cleanup
//! callback), `scope_owned_count`, `dependents_of`, and `dependencies_of`. An
//! unrecognised assertion key panics rather than being skipped.
//!
//! ## Fixture shape
//!
//! Every fixture declares a top-level `shape`, `steps` or `scenarios`, and the
//! runner dispatches on it. `scenarios` exists because a claim like
//! `observationally_equal` is a *relation between two op streams*, which a
//! single `steps` array cannot express; each scenario is replayed in its own
//! context and the resulting observations compared.
//!
//! ## Positive assertion (`#lzspecconf`)
//!
//! An absence guard is not enough — a runner that skips everything must fail.
//! For each model this asserts (a) the fixture set on disk matches `FIXTURES`
//! exactly, (b) every fixture was replayed, and (c) a non-zero number of ops and
//! assertions actually executed, per fixture and in total.

#[path = "reactive_graph/engine.rs"]
mod engine;
#[path = "reactive_graph/model.rs"]
mod model;
#[path = "reactive_graph/models.rs"]
mod models;

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;

use engine::{Report, arr, replay};
use model::GraphModel;

const SPEC_DIR: &str = "../lazily-spec/conformance/reactive-graph";

/// The canonical fixture set. Asserted against the directory listing so a
/// fixture added or renamed upstream fails loudly instead of going unrun.
const FIXTURES: [&str; 11] = [
    "churn_returns_to_baseline.json",
    "cross_scope_teardown_hazard.json",
    "disarm_disposes_nothing.json",
    "disposal_does_not_run_surviving_effects.json",
    "dispose_detaches_edges_both_directions.json",
    "read_after_dispose_is_an_error.json",
    "recycled_id_inherits_nothing.json",
    "scope_teardown_equals_fold_of_disposals.json",
    "scoping_bounds_teardown_not_visibility.json",
    "teardown_runs_members_in_reverse_creation_order.json",
    "transitive_invalidation_reaches_depth.json",
];

/// Fixture assertions an execution model does not satisfy today, as
/// `<model>/<fixture>[<scenario>]#<step>:<key>`.
///
/// Each entry is a finding against the implementation, not a relaxation of the
/// fixture: the runner asserts this list matches the observed set exactly, so a
/// new divergence fails the build and a fixed one fails it until the entry is
/// removed.
const KNOWN_DIVERGENCES: &[&str] = &[
    // Empty, and it must stay empty unless a real divergence is found. The one
    // entry this ledger ever held was a fixture defect — `scope_teardown_...`
    // asserted `dependents_of` one step before the read that registers the edge
    // it counts — reported upstream and fixed in lazily-spec f9f93d5.
];

fn load(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn present() -> bool {
    std::path::Path::new(SPEC_DIR).is_dir()
}

/// Replay the whole corpus against one execution model.
fn run_corpus<M: GraphModel>() {
    let model_name = M::NAME;
    if !present() {
        eprintln!(
            "SKIP reactive_graph_conformance[{model_name}]: {SPEC_DIR} not found — clone \
             lazily-spec as a sibling to run the #lzspecedgeindex disposal/teardown fixtures"
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
    let mut observed_divergences: BTreeSet<String> = BTreeSet::new();

    for name in FIXTURES {
        let fx = load(name);
        // Dispatch on the fixture's declared `shape`, not on its filename: a
        // filename special case goes stale silently the moment a second
        // scenarios-shaped fixture is added. An unrecognised shape is a hard
        // error rather than a fallback to `steps`.
        let models: Vec<M>;
        let reports: Vec<Report> = match fx["shape"].as_str() {
            Some("steps") => {
                models = vec![M::create()];
                vec![replay(&models[0], name, arr(&fx["steps"]), None)]
            }
            Some("scenarios") => {
                let scenarios = fx["scenarios"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{name}: shape is `scenarios` but no scenarios"));
                models = scenarios.iter().map(|_| M::create()).collect();
                scenarios
                    .iter()
                    .zip(&models)
                    .map(|(s, m)| replay(m, name, arr(&s["steps"]), Some(&fx["expected"])))
                    .collect()
            }
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
                    "{model_name}/{name}: scenarios are not observationally equal"
                );
            }
            total_checks += 1;
        }

        let ops: usize = reports.iter().map(|r| r.ops).sum();
        let checks: usize = reports.iter().map(|r| r.checks).sum();
        assert!(ops > 0, "{model_name}/{name}: replayed zero ops");
        assert!(checks > 0, "{model_name}/{name}: replayed zero assertions");

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
                let entry = format!("{model_name}/{name}{scenario}#{step}:{}", f.key);
                eprintln!("  DIVERGENCE {entry} — {}", f.detail);
                observed_divergences.insert(entry);
            }
        }

        eprintln!("reactive-graph[{model_name}] {name}: {ops} ops, {checks} assertions");
        total_ops += ops;
        total_checks += checks;
        replayed += 1;
    }

    eprintln!(
        "reactive-graph[{model_name}]: {replayed} fixtures, {total_ops} ops, \
         {total_checks} assertions, {} documented divergences",
        observed_divergences.len()
    );

    // Divergence ledger: the observed set must equal the documented one, so a
    // new divergence fails the build and a fixed one forces its entry to be
    // deleted. Neither direction can pass unnoticed.
    let documented: BTreeSet<String> = KNOWN_DIVERGENCES
        .iter()
        .filter(|d| d.starts_with(&format!("{model_name}/")))
        .map(|d| (*d).to_owned())
        .collect();
    assert_eq!(
        observed_divergences, documented,
        "{model_name}: divergence ledger is stale — update KNOWN_DIVERGENCES \
         (left = observed, right = documented)"
    );

    // Positive assertion: the runner must have actually executed the corpus.
    assert_eq!(
        replayed,
        FIXTURES.len(),
        "{model_name}: did not replay every fixture"
    );
    assert!(total_ops > 0, "{model_name}: executed zero ops");
    assert!(total_checks > 0, "{model_name}: executed zero assertions");
}

#[test]
fn reactive_graph_conformance_basic() {
    run_corpus::<models::BasicModel>();
}

#[cfg(feature = "thread-safe")]
#[test]
fn reactive_graph_conformance_thread_safe() {
    run_corpus::<models::ThreadSafeModel>();
}

#[cfg(feature = "async")]
#[test]
fn reactive_graph_conformance_async() {
    run_corpus::<models::AsyncModel>();
}

/// The ledger must not name a model that no longer runs, and every model that
/// runs must be reachable from a test above. Guards against a `#[cfg]` quietly
/// removing a model's coverage while its ledger entries linger.
#[test]
fn divergence_ledger_names_only_known_models() {
    let models: BTreeSet<&str> = ["Context", "ThreadSafeContext", "AsyncContext"]
        .into_iter()
        .collect();
    for entry in KNOWN_DIVERGENCES {
        let model = entry.split('/').next().unwrap();
        assert!(
            models.contains(model),
            "KNOWN_DIVERGENCES names unknown model {model}"
        );
    }
}
