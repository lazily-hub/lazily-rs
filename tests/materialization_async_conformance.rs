//! Async `ReactiveFamily` materialization conformance (`#lzmatmode`, async
//! flavor). Replays the canonical fixtures in
//! `lazily-spec/conformance/materialization/` through [`AsyncReactiveFamily`],
//! proving the async flavor obeys the same present-set materialization laws and
//! the **eventual transparency** law proved in `lazily-formal`'s
//! `AsyncMaterialization` module: a driven (resolved) async slot observes the
//! canonical value, identical under eager or lazy.
#![cfg(feature = "async")]

use std::collections::HashSet;
use std::fs;

use lazily::{AsyncContext, AsyncReactiveFamily, AsyncSlotHandle, MaterializationMode};
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
    ctx: &AsyncContext,
    mode: MaterializationMode,
    keys: Vec<String>,
    entries: Vec<(String, V)>,
) -> AsyncReactiveFamily<String, V, AsyncSlotHandle<V>> {
    let lookup = move |k: &String| -> V {
        entries
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("no val for {k}"))
    };
    match mode {
        MaterializationMode::Eager => AsyncReactiveFamily::eager(ctx, keys, lookup),
        MaterializationMode::Lazy => AsyncReactiveFamily::lazy(ctx, keys, lookup),
    }
}

/// Eventual transparency + present-set laws replayed through the async family:
/// eager materializes all, lazy defers all, and a driven slot resolves to the
/// canonical value identically under either mode.
#[tokio::test]
async fn eventual_transparency_async() {
    if !present() {
        eprintln!("skipping: {SPEC_DIR} absent");
        return;
    }
    let fixture = load("observational_transparency.json");
    let entries = val_entries(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    let expected = fixture.get("expected").unwrap();

    assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);

    let ctx_e = AsyncContext::new();
    let eager = slot_family(&ctx_e, MaterializationMode::Eager, keys.clone(), entries.clone());
    let ctx_l = AsyncContext::new();
    let lazy = slot_family(&ctx_l, MaterializationMode::Lazy, keys.clone(), entries);

    // Present-set laws (allocation axis, unchanged by async resolution).
    assert_eq!(eager.present_count(), keys.len());
    assert_eq!(
        as_set(&eager.present_keys()),
        as_set(&str_array(expected, "eager_present"))
    );
    assert_eq!(lazy.present_count(), 0);

    // Eventual transparency: drive each slot; resolved value = canonical, and the
    // eager and lazy families agree.
    for (k, want) in expected.get("observe").and_then(|v| v.as_object()).unwrap() {
        let want = want.as_i64().unwrap();
        let ve = ctx_e.get_async(&eager.get(&ctx_e, k.clone())).await;
        let vl = ctx_l.get_async(&lazy.get(&ctx_l, k.clone())).await;
        assert_eq!(ve, want, "eager async observe {k}");
        assert_eq!(vl, want, "lazy async observe {k}");
    }
}

/// The lazy present set after the fixture read sequence is exactly the read keys
/// (deferral, not de-allocation) — same as the sync/thread-safe families.
#[tokio::test]
async fn deferral_not_deallocation_async() {
    if !present() {
        eprintln!("skipping: {SPEC_DIR} absent");
        return;
    }
    let fixture = load("observational_transparency.json");
    let expected = fixture.get("expected").unwrap();
    let entries = val_entries(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();

    let ctx = AsyncContext::new();
    let lazy = slot_family(&ctx, MaterializationMode::Lazy, keys, entries);
    for k in str_array(&fixture, "reads") {
        let _ = lazy.get(&ctx, k);
    }
    assert_eq!(
        as_set(&lazy.present_keys()),
        as_set(&str_array(expected, "lazy_present_after_reads"))
    );
}
