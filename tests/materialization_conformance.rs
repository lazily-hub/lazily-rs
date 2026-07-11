//! Cross-language conformance tests for **materialization mode** (`#lzmatmode`)
//! — the eager-default / lazy-opt-in axis fixed by `lazily-spec/cell-model.md`
//! § "Materialization mode" and proved in `lazily-formal`'s `Materialization`
//! module.
//!
//! These are **compute** fixtures: lazily-rs reads `spec.val` (each derived
//! key's canonical value), builds the keyed family under *both* modes, replays
//! the `reads` sequence against the lazy build, and asserts the observable
//! consequences:
//!
//! - `observe` — every key returns its canonical value under *either* mode
//!   (`observe_canonical` / `eager_lazy_observationally_equivalent`).
//! - `eager_present` — the eager build materializes every key up front
//!   (`eager_materializes_all`).
//! - `lazy_present_after_reads` — the lazy build materializes only read keys
//!   (`lazy_defers_slots`), and that set is a subset of `eager_present`
//!   (`lazy_present_subset_eager`).
//! - `present_after_each_read` — the lazy present set grows monotonically and is
//!   unchanged by re-reads (`materialize_present_monotone`).
//! - `default_mode` — the default materialization mode is eager
//!   (`default_mode_eager`).

use std::collections::HashMap;
use std::fs;

use lazily::{Context, MaterializationMode, MaterializedFamily};
use serde_json::Value;

const SPEC_DIR: &str = "../lazily-spec/conformance/materialization";

fn load_fixture(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn spec_fixtures_present() -> bool {
    std::path::Path::new(&format!("{SPEC_DIR}/observational_transparency.json")).exists()
}

/// Parse `spec.val` into a `key -> value` map (string keys, integer values).
fn parse_val(spec: &Value) -> HashMap<String, i64> {
    spec.get("val")
        .and_then(|v| v.as_object())
        .expect("spec.val object")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_i64().expect("integer val")))
        .collect()
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|e| e.as_str().expect("string").to_string())
        .collect()
}

/// A family deriving each key's value from the fixture `val` map. This is the
/// binding-side stand-in for the formal `Spec.val`: a pure per-key value, so the
/// only thing under test is *when* the node is allocated, never what it computes.
fn make_family(
    ctx: &Context,
    mode: MaterializationMode,
    val: &HashMap<String, i64>,
) -> MaterializedFamily<String, i64> {
    let table = val.clone();
    let keys: Vec<String> = val.keys().cloned().collect();
    MaterializedFamily::new(ctx, mode, keys, move |_ctx, k: &String| {
        *table.get(k).expect("key in spec.val")
    })
}

fn run_fixture(name: &str) {
    let fx = load_fixture(name);
    let spec = fx.get("spec").expect("spec");
    let val = parse_val(spec);
    let expected = fx.get("expected").expect("expected");
    let ctx = Context::new();

    // default_mode_eager — the default mode is eager.
    if let Some(dm) = expected.get("default_mode").and_then(|v| v.as_str()) {
        assert_eq!(dm, "eager", "{name}: fixture pins eager default");
        assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);
    }

    // Build both modes over the same spec.
    let eager = make_family(&ctx, MaterializationMode::Eager, &val);
    let lazy = make_family(&ctx, MaterializationMode::Lazy, &val);

    // eager_materializes_all — eager allocated every key at build.
    let eager_present = str_vec(expected.get("eager_present").expect("eager_present"));
    assert_eq!(
        eager.materialized_count(),
        eager_present.len(),
        "{name}: eager present-set size"
    );
    for k in &eager_present {
        assert!(eager.is_materialized(k), "{name}: eager present {k}");
    }
    // lazy starts empty (nothing read yet -> nothing allocated).
    assert_eq!(lazy.materialized_count(), 0, "{name}: lazy starts deferred");

    // Replay the reads against the lazy build, checking observe transparency and
    // monotone growth.
    let reads = str_vec(fx.get("reads").expect("reads"));
    let per_read = expected.get("present_after_each_read").map(|v| {
        v.as_array()
            .expect("array")
            .iter()
            .map(|n| n.as_u64().expect("u64") as usize)
            .collect::<Vec<_>>()
    });
    let mut last_count = 0usize;
    for (i, k) in reads.iter().enumerate() {
        let got = lazy.observe(&ctx, k);
        let want = *val.get(k).expect("read key in val");
        assert_eq!(got, want, "{name}: lazy observe {k}");
        // eager_lazy_observationally_equivalent — same value under either mode.
        assert_eq!(
            eager.observe(&ctx, k),
            got,
            "{name}: eager==lazy observe {k}"
        );
        // materialize_present_monotone — present set never shrinks.
        let count = lazy.materialized_count();
        assert!(
            count >= last_count,
            "{name}: present set monotone at read {i}"
        );
        last_count = count;
        if let Some(per) = &per_read {
            assert_eq!(count, per[i], "{name}: present_after_each_read[{i}]");
        }
    }

    // lazy_present_after_reads — final lazy present set (checked *before* any
    // further reads), a subset of eager's (lazy_present_subset_eager).
    let lazy_present = str_vec(
        expected
            .get("lazy_present_after_reads")
            .expect("lazy_present_after_reads"),
    );
    for k in &val.keys().cloned().collect::<Vec<_>>() {
        let present = lazy.is_materialized(k);
        assert_eq!(
            present,
            lazy_present.contains(k),
            "{name}: lazy present({k}) matches fixture"
        );
        if present {
            assert!(
                eager.is_materialized(k),
                "{name}: lazy present ⊆ eager present for {k}"
            );
        }
    }

    // observe — every key returns its canonical value under either mode. A fresh
    // lazy build proves observe is mode-independent for keys never read above
    // (materialize_preserves_observe: materializing them now yields the same
    // value the eager build has held since construction).
    let fresh_lazy = make_family(&ctx, MaterializationMode::Lazy, &val);
    let observe = expected
        .get("observe")
        .and_then(|v| v.as_object())
        .expect("observe map");
    for (k, want) in observe {
        let want = want.as_i64().expect("integer");
        assert_eq!(eager.observe(&ctx, k), want, "{name}: eager observe {k}");
        assert_eq!(
            fresh_lazy.observe(&ctx, k),
            want,
            "{name}: lazy observe {k}"
        );
    }
}

#[test]
fn observational_transparency() {
    if !spec_fixtures_present() {
        eprintln!("skipping: lazily-spec conformance fixtures not present");
        return;
    }
    run_fixture("observational_transparency.json");
}

#[test]
fn deferral_not_deallocation() {
    if !spec_fixtures_present() {
        eprintln!("skipping: lazily-spec conformance fixtures not present");
        return;
    }
    run_fixture("deferral_not_deallocation.json");
}
