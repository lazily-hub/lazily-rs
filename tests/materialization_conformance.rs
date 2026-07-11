//! Cross-language conformance tests for the `ReactiveFamily` materialization
//! mode (`#lzmatmode`), driven by the canonical fixtures in
//! `lazily-spec/conformance/materialization/`. These exercise the laws proved in
//! `lazily-formal`'s `Materialization` module against the Rust `ReactiveFamily`
//! vehicle:
//!
//! - `observational_transparency.json` — eager and lazy return identical values
//!   for every key (`observe_canonical` / `eager_lazy_observationally_equivalent`);
//!   eager materializes all up front, lazy only the read keys; default is eager.
//! - `deferral_not_deallocation.json` — the present set only *grows* and is
//!   unchanged by a re-read (`materialize_present_monotone`); the lazy present
//!   set is a subset of the eager one (`lazy_present_subset_eager`).
//! - `entry_kind_orthogonal_to_mode.json` — input **cell** entries are
//!   materialized in every mode; derived **slot** entries defer under lazy
//!   (`cell_entries_materialized_in_every_mode` / `slot_entries_deferred_under_lazy`).

use std::collections::HashSet;
use std::fs;

use lazily::{CellHandle, Context, EntryKind, MaterializationMode, ReactiveFamily, SlotHandle};
use serde_json::Value;

const SPEC_DIR: &str = "../lazily-spec/conformance/materialization";

type V = i64;

fn spec_fixtures_present() -> bool {
    std::path::Path::new(SPEC_DIR).exists()
}

fn load_fixture(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn str_array(v: &Value, path: &str) -> Vec<String> {
    v.get(path)
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("missing array {path}"))
        .iter()
        .map(|k| k.as_str().expect("array of strings").to_string())
        .collect()
}

fn as_set(keys: &[String]) -> HashSet<String> {
    keys.iter().cloned().collect()
}

/// Parse a `spec.val` object of `key -> canonical value` into ordered keys and a
/// lookup closure input.
fn parse_val_spec(fixture: &Value) -> Vec<(String, V)> {
    let obj = fixture
        .get("spec")
        .and_then(|s| s.get("val"))
        .and_then(|v| v.as_object())
        .expect("spec.val object");
    obj.iter()
        .map(|(k, v)| (k.clone(), v.as_i64().expect("integer val")))
        .collect()
}

/// Assert the shared invariants both `spec.val` fixtures declare: default mode
/// eager, observationally-transparent reads, eager materializes all, and the
/// lazy present set after the read sequence equals the (deduped) read keys.
fn check_val_fixture(name: &str) -> Value {
    let fixture = load_fixture(name);
    let entries = parse_val_spec(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    let expected = fixture.get("expected").expect("expected");

    // default_mode_eager
    assert_eq!(
        expected.get("default_mode").and_then(|v| v.as_str()),
        Some("eager")
    );
    assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);

    let ctx = Context::new();
    let lookup = {
        let entries = entries.clone();
        move |k: &String| -> V {
            entries
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| *v)
                .unwrap_or_else(|| panic!("no spec val for key {k}"))
        }
    };

    let eager: ReactiveFamily<String, V, SlotHandle<V>> =
        ReactiveFamily::eager(&ctx, keys.clone(), lookup.clone());
    let lazy: ReactiveFamily<String, V, SlotHandle<V>> =
        ReactiveFamily::lazy(&ctx, keys.clone(), lookup);

    // eager_materializes_all
    assert_eq!(eager.present_count(), keys.len());
    assert_eq!(
        as_set(&eager.present_keys()),
        as_set(&str_array(expected, "eager_present"))
    );
    // Lazy defers every derived slot: nothing present at build.
    assert_eq!(lazy.present_count(), 0);

    // observe_canonical / eager_lazy_observationally_equivalent
    let observe = expected
        .get("observe")
        .and_then(|v| v.as_object())
        .expect("expected.observe");
    for (k, want) in observe {
        let want = want.as_i64().expect("observe int");
        assert_eq!(eager.observe(&ctx, k.clone()), want, "eager observe {k}");
        assert_eq!(lazy.observe(&ctx, k.clone()), want, "lazy observe {k}");
    }

    fixture
}

#[test]
fn observational_transparency() {
    if !spec_fixtures_present() {
        eprintln!("skipping: {SPEC_DIR} not present");
        return;
    }
    let fixture = check_val_fixture("observational_transparency.json");
    let expected = fixture.get("expected").unwrap();

    // Replay the lazy read sequence on a fresh family; the lazy present set is
    // exactly the read keys (lazy_defers_slots).
    let entries = parse_val_spec(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    let ctx = Context::new();
    let lazy: ReactiveFamily<String, V, SlotHandle<V>> =
        ReactiveFamily::lazy(&ctx, keys, move |k: &String| {
            entries.iter().find(|(key, _)| key == k).unwrap().1
        });
    for k in str_array(&fixture, "reads") {
        lazy.observe(&ctx, k);
    }
    assert_eq!(
        as_set(&lazy.present_keys()),
        as_set(&str_array(expected, "lazy_present_after_reads"))
    );
}

#[test]
fn deferral_not_deallocation() {
    if !spec_fixtures_present() {
        eprintln!("skipping: {SPEC_DIR} not present");
        return;
    }
    let fixture = check_val_fixture("deferral_not_deallocation.json");
    let expected = fixture.get("expected").unwrap();

    let entries = parse_val_spec(&fixture);
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    let ctx = Context::new();
    let lazy: ReactiveFamily<String, V, SlotHandle<V>> =
        ReactiveFamily::lazy(&ctx, keys, move |k: &String| {
            entries.iter().find(|(key, _)| key == k).unwrap().1
        });

    // present_after_each_read: cumulative present-set size, monotone and
    // unchanged by a re-read (materialize_present_monotone).
    let want_sizes: Vec<usize> = expected
        .get("present_after_each_read")
        .and_then(|v| v.as_array())
        .expect("present_after_each_read")
        .iter()
        .map(|n| n.as_u64().expect("size") as usize)
        .collect();
    let mut got_sizes = Vec::new();
    for k in str_array(&fixture, "reads") {
        lazy.observe(&ctx, k);
        got_sizes.push(lazy.present_count());
    }
    assert_eq!(got_sizes, want_sizes, "cumulative present-set sizes");

    // lazy_present_after_reads is a subset of eager_present (lazy_present_subset_eager).
    let lazy_present = as_set(&lazy.present_keys());
    assert_eq!(
        lazy_present,
        as_set(&str_array(expected, "lazy_present_after_reads"))
    );
    let eager_present = as_set(&str_array(expected, "eager_present"));
    assert!(
        lazy_present.is_subset(&eager_present),
        "lazy present set must be a subset of eager present set"
    );
}

#[test]
fn entry_kind_orthogonal_to_mode() {
    if !spec_fixtures_present() {
        eprintln!("skipping: {SPEC_DIR} not present");
        return;
    }
    let fixture = load_fixture("entry_kind_orthogonal_to_mode.json");
    let expected = fixture.get("expected").unwrap();
    assert_eq!(
        expected.get("default_mode").and_then(|v| v.as_str()),
        Some("eager")
    );

    let spec_entries = fixture
        .get("spec")
        .and_then(|s| s.get("entries"))
        .and_then(|v| v.as_object())
        .expect("spec.entries");

    // Split the family's declared entries by kind: input cells vs derived slots.
    // A single `ReactiveFamily<K,V,H>` fixes one handle kind, so a mixed-kind
    // fixture is modelled by a cell family over the cell entries and a slot
    // family over the slot entries — sharing one logical key space.
    let mut cell_keys: Vec<String> = Vec::new();
    let mut slot_keys: Vec<String> = Vec::new();
    let mut vals: Vec<(String, V)> = Vec::new();
    for (key, entry) in spec_entries {
        let kind = entry.get("kind").and_then(|v| v.as_str()).expect("kind");
        let val = entry.get("val").and_then(|v| v.as_i64()).expect("val");
        vals.push((key.clone(), val));
        match kind {
            "cell" => cell_keys.push(key.clone()),
            "slot" => slot_keys.push(key.clone()),
            other => panic!("unknown entry kind {other}"),
        }
    }
    let lookup = {
        let vals = vals.clone();
        move |k: &String| vals.iter().find(|(key, _)| key == k).unwrap().1
    };

    let ctx = Context::new();

    // Eager build: every entry present (cells + slots).
    let eager_cells: ReactiveFamily<String, V, CellHandle<V>> =
        ReactiveFamily::eager(&ctx, cell_keys.clone(), lookup.clone());
    let eager_slots: ReactiveFamily<String, V, SlotHandle<V>> =
        ReactiveFamily::eager(&ctx, slot_keys.clone(), lookup.clone());
    assert_eq!(eager_cells.entry_kind(), EntryKind::Cell);
    assert_eq!(eager_slots.entry_kind(), EntryKind::Slot);
    let mut eager_present = as_set(&eager_cells.present_keys());
    eager_present.extend(eager_slots.present_keys());
    assert_eq!(eager_present, as_set(&str_array(expected, "eager_present")));

    // Lazy build: cells present at build, slots deferred.
    let lazy_cells: ReactiveFamily<String, V, CellHandle<V>> =
        ReactiveFamily::lazy(&ctx, cell_keys.clone(), lookup.clone());
    let lazy_slots: ReactiveFamily<String, V, SlotHandle<V>> =
        ReactiveFamily::lazy(&ctx, slot_keys.clone(), lookup.clone());
    let present_at_build = as_set(&lazy_cells.present_keys());
    assert!(
        lazy_slots.present_keys().is_empty(),
        "slots deferred at build"
    );
    assert_eq!(
        present_at_build,
        as_set(&str_array(expected, "lazy_present_at_build"))
    );

    // Reads (slot pulls) grow only the slot present set.
    for k in str_array(&fixture, "reads") {
        if slot_keys.contains(&k) {
            lazy_slots.observe(&ctx, k);
        } else {
            lazy_cells.observe(&ctx, k);
        }
    }
    let mut lazy_after = as_set(&lazy_cells.present_keys());
    lazy_after.extend(lazy_slots.present_keys());
    assert_eq!(
        lazy_after,
        as_set(&str_array(expected, "lazy_present_after_reads"))
    );

    // Observational transparency across kinds.
    let observe = expected.get("observe").and_then(|v| v.as_object()).unwrap();
    for (k, want) in observe {
        let want = want.as_i64().unwrap();
        if cell_keys.contains(k) {
            assert_eq!(eager_cells.observe(&ctx, k.clone()), want);
            assert_eq!(lazy_cells.observe(&ctx, k.clone()), want);
        } else {
            assert_eq!(eager_slots.observe(&ctx, k.clone()), want);
            assert_eq!(lazy_slots.observe(&ctx, k.clone()), want);
        }
    }
}
