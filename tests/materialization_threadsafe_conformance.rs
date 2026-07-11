//! Thread-safe `ReactiveFamily` materialization conformance (`#lzmatmode`,
//! thread-safe flavor). Replays the canonical fixtures in
//! `lazily-spec/conformance/materialization/` through
//! [`ThreadSafeReactiveFamily`], proving the `Send + Sync` flavor obeys the same
//! materialization laws as the single-threaded family — plus **confluence** (the
//! order-independence proved in `lazily-formal`'s `Materialization` module:
//! `materialize_present_comm` / `materialize_observe_comm`), the property that
//! justifies mutex-serialized concurrent materialization.
#![cfg(feature = "thread-safe")]

use std::collections::HashSet;
use std::fs;

use lazily::{MaterializationMode, SlotHandle, ThreadSafeContext, ThreadSafeReactiveFamily};
use serde_json::Value;

const SPEC_DIR: &str = "../lazily-spec/conformance/materialization";
type V = i64;

fn present() -> bool {
    std::path::Path::new(SPEC_DIR).exists()
}

fn load(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}")))
        .unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn val_entries(fixture: &Value) -> Vec<(String, V)> {
    fixture
        .get("spec")
        .and_then(|s| s.get("val"))
        .and_then(|v| v.as_object())
        .expect("spec.val")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_i64().expect("int val")))
        .collect()
}

fn str_array(v: &Value, path: &str) -> Vec<String> {
    v.get(path)
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("missing array {path}"))
        .iter()
        .map(|k| k.as_str().expect("string").to_string())
        .collect()
}

fn as_set(keys: &[String]) -> HashSet<String> {
    keys.iter().cloned().collect()
}

fn slot_family(
    ctx: &ThreadSafeContext,
    mode: MaterializationMode,
    keys: Vec<String>,
    entries: Vec<(String, V)>,
) -> ThreadSafeReactiveFamily<String, V, SlotHandle<V>> {
    let lookup = move |k: &String| -> V {
        entries
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("no val for {k}"))
    };
    match mode {
        MaterializationMode::Eager => ThreadSafeReactiveFamily::eager(ctx, keys, lookup),
        MaterializationMode::Lazy => ThreadSafeReactiveFamily::lazy(ctx, keys, lookup),
    }
}

/// The shared `spec.val` laws, replayed through the thread-safe family: default
/// eager, eager materializes all, lazy defers all, observationally-transparent
/// reads under either mode.
fn check_val_fixture(name: &str) -> Value {
    let fixture = load(name);
    let entries = val_entries(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    let expected = fixture.get("expected").expect("expected");

    assert_eq!(
        expected.get("default_mode").and_then(|v| v.as_str()),
        Some("eager")
    );
    assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);

    let ctx = ThreadSafeContext::new();
    let eager = slot_family(&ctx, MaterializationMode::Eager, keys.clone(), entries.clone());
    let lazy = slot_family(&ctx, MaterializationMode::Lazy, keys.clone(), entries);

    assert_eq!(eager.present_count(), keys.len());
    assert_eq!(
        as_set(&eager.present_keys()),
        as_set(&str_array(expected, "eager_present"))
    );
    assert_eq!(lazy.present_count(), 0);

    for (k, want) in expected.get("observe").and_then(|v| v.as_object()).unwrap() {
        let want = want.as_i64().unwrap();
        assert_eq!(eager.observe(&ctx, k.clone()), want, "eager observe {k}");
        assert_eq!(lazy.observe(&ctx, k.clone()), want, "lazy observe {k}");
    }
    fixture
}

#[test]
fn observational_transparency_thread_safe() {
    if !present() {
        eprintln!("skipping: {SPEC_DIR} absent");
        return;
    }
    let fixture = check_val_fixture("observational_transparency.json");
    let expected = fixture.get("expected").unwrap();
    let entries = val_entries(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();

    let ctx = ThreadSafeContext::new();
    let lazy = slot_family(&ctx, MaterializationMode::Lazy, keys, entries);
    for k in str_array(&fixture, "reads") {
        lazy.observe(&ctx, k);
    }
    assert_eq!(
        as_set(&lazy.present_keys()),
        as_set(&str_array(expected, "lazy_present_after_reads"))
    );
}

#[test]
fn deferral_not_deallocation_thread_safe() {
    if !present() {
        eprintln!("skipping: {SPEC_DIR} absent");
        return;
    }
    let fixture = check_val_fixture("deferral_not_deallocation.json");
    let expected = fixture.get("expected").unwrap();
    let entries = val_entries(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();

    let ctx = ThreadSafeContext::new();
    let lazy = slot_family(&ctx, MaterializationMode::Lazy, keys, entries);
    let want_sizes: Vec<usize> = expected
        .get("present_after_each_read")
        .and_then(|v| v.as_array())
        .expect("present_after_each_read")
        .iter()
        .map(|n| n.as_u64().unwrap() as usize)
        .collect();
    let mut got = Vec::new();
    for k in str_array(&fixture, "reads") {
        lazy.observe(&ctx, k);
        got.push(lazy.present_count());
    }
    assert_eq!(got, want_sizes);
    let lazy_present = as_set(&lazy.present_keys());
    assert!(lazy_present.is_subset(&as_set(&str_array(expected, "eager_present"))));
}

/// **Confluence** (`materialize_present_comm` / `materialize_observe_comm`): two
/// lazy families over the same spec, read in *opposite* key orders, reach the
/// same present set and identical observed values — the order-independence that
/// makes the `Arc<Mutex>`-serialized family safe under any interleaving.
#[test]
fn materialization_confluent_under_reordering() {
    if !present() {
        eprintln!("skipping: {SPEC_DIR} absent");
        return;
    }
    let fixture = load("observational_transparency.json");
    let entries = val_entries(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();

    let ctx_fwd = ThreadSafeContext::new();
    let fwd = slot_family(&ctx_fwd, MaterializationMode::Lazy, keys.clone(), entries.clone());
    let ctx_rev = ThreadSafeContext::new();
    let rev = slot_family(&ctx_rev, MaterializationMode::Lazy, keys.clone(), entries);

    let mut fwd_vals = Vec::new();
    for k in keys.iter() {
        fwd_vals.push((k.clone(), fwd.observe(&ctx_fwd, k.clone())));
    }
    let mut rev_vals = Vec::new();
    for k in keys.iter().rev() {
        rev_vals.push((k.clone(), rev.observe(&ctx_rev, k.clone())));
    }

    // Same observed value per key regardless of the order it was materialized.
    for (k, v) in &fwd_vals {
        let rv = rev_vals.iter().find(|(rk, _)| rk == k).map(|(_, v)| *v).unwrap();
        assert_eq!(*v, rv, "observe {k} order-independent");
    }
    // Same present set regardless of materialization order.
    assert_eq!(as_set(&fwd.present_keys()), as_set(&rev.present_keys()));
}
