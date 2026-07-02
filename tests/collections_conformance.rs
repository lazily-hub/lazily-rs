//! Cross-language conformance tests for the keyed cell collections layer
//! (`lazily-spec/cell-model.md#keyed-cell-collections`), which is **required of
//! every binding** — see the Binding Conformance Matrix in
//! `lazily-spec/protocol.md`.
//!
//! Unlike the IPC fixtures these are **compute** fixtures: lazily-rs loads the
//! `initial` state, replays each `step`'s `op`, and asserts the `expected`
//! observable effects (resulting `order`, `values`, `membership`, which reader
//! classes — `value` / `membership` / `order` — invalidate, and that an atomic
//! move keeps the entry's cell `handle_stable`). The reconciliation fixture is
//! declarative: diff `prior` → `target` and assert the emitted minimal op set
//! plus that stable entries are not invalidated by a sibling reorder.

use std::collections::{HashMap, HashSet};
use std::fs;

use lazily::{CellHandle, CellMap, Context, DiffOp, apply_to_map, reconcile};
use serde_json::Value;

const SPEC_DIR: &str = "../lazily-spec/conformance/collections";

/// Cell value type used across all collection fixtures (JSON integers).
type V = i64;

fn load_fixture(name: &str) -> Value {
    let path = format!("{SPEC_DIR}/{name}");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn build_initial(ctx: &Context, initial: &Value) -> CellMap<String, V> {
    let map: CellMap<String, V> = CellMap::new(ctx);
    let order = initial
        .get("order")
        .and_then(|v| v.as_array())
        .expect("initial.order");
    let values = initial
        .get("values")
        .and_then(|v| v.as_object())
        .expect("initial.values");
    for k in order {
        let key = k.as_str().expect("order key").to_string();
        let val = values
            .get(&key)
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| panic!("missing initial value for {key}"));
        map.entry(ctx, key, val);
    }
    map
}

fn value_reader(
    ctx: &Context,
    map: &CellMap<String, V>,
    key: &str,
) -> lazily::SlotHandle<Option<V>> {
    let map = map.clone();
    let key = key.to_string();
    ctx.computed(move |ctx| map.get(ctx, &key))
}

/// Apply a fixture `op` to the live collection. Returns the removed key, if any,
/// so the caller can exclude it from the survivor value-reader checks.
fn apply_op(ctx: &Context, map: &CellMap<String, V>, op: &Value) -> Option<String> {
    let ty = op.get("type").and_then(|v| v.as_str()).expect("op.type");
    match ty {
        "set_value" => {
            let key = op.get("key").and_then(|v| v.as_str()).unwrap().to_string();
            let val = op.get("value").and_then(|v| v.as_i64()).unwrap();
            map.set(ctx, key, val);
            None
        }
        "insert" => {
            let key = op.get("key").and_then(|v| v.as_str()).unwrap().to_string();
            let val = op.get("value").and_then(|v| v.as_i64()).unwrap();
            map.entry(ctx, key.clone(), val);
            // `at` is optional: "end" (the default after `entry`) or a 0-based index.
            if let Some(idx) = op.get("at").and_then(|v| v.as_u64()) {
                map.move_to(ctx, &key, idx as usize);
            }
            None
        }
        "remove" => {
            let key = op.get("key").and_then(|v| v.as_str()).unwrap().to_string();
            map.remove(ctx, &key);
            Some(key)
        }
        "move_to" => {
            let key = op.get("key").and_then(|v| v.as_str()).unwrap().to_string();
            let idx = op.get("index").and_then(|v| v.as_u64()).unwrap() as usize;
            map.move_to(ctx, &key, idx);
            None
        }
        "move_before" => {
            let key = op.get("key").and_then(|v| v.as_str()).unwrap().to_string();
            let before = op
                .get("before")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string();
            map.move_before(ctx, &key, &before);
            None
        }
        "move_after" => {
            let key = op.get("key").and_then(|v| v.as_str()).unwrap().to_string();
            let after = op
                .get("after")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string();
            map.move_after(ctx, &key, &after);
            None
        }
        other => panic!("unknown collection op type: {other}"),
    }
}

fn assert_state(ctx: &Context, map: &CellMap<String, V>, expected: &Value) {
    if let Some(order) = expected.get("order").and_then(|v| v.as_array()) {
        let want: Vec<String> = order
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(map.keys(ctx), want, "order mismatch");
    }
    if let Some(membership) = expected.get("membership").and_then(|v| v.as_array()) {
        let want: HashSet<String> = membership
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let got: HashSet<String> = map.keys(ctx).into_iter().collect();
        assert_eq!(got, want, "membership mismatch");
    }
    if let Some(values) = expected.get("values").and_then(|v| v.as_object()) {
        for (key, val) in values {
            let want = val
                .as_i64()
                .unwrap_or_else(|| panic!("non-integer value for {key}"));
            let got = map
                .get(ctx, key)
                .unwrap_or_else(|| panic!("missing key {key} after op"));
            assert_eq!(got, want, "value mismatch for {key}");
        }
    }
}

/// Verify the `invalidates` reader-class independence contract: among survivors,
/// exactly the listed value readers invalidate; the membership reader matches
/// `membership`; the order reader matches `order`.
fn assert_invalidation(
    ctx: &Context,
    value_readers: &HashMap<String, lazily::SlotHandle<Option<V>>>,
    membership_reader: &lazily::SlotHandle<usize>,
    order_reader: &lazily::SlotHandle<Vec<String>>,
    invalidates: &Value,
    survivors: &HashSet<String>,
) {
    let value_inv: HashSet<String> = invalidates
        .get("value")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();

    for key in survivors {
        let Some(reader) = value_readers.get(key) else {
            continue;
        };
        let cached = ctx.is_set(reader);
        if value_inv.contains(key) {
            assert!(
                !cached,
                "value reader for `{key}` should have been invalidated"
            );
        } else {
            assert!(
                cached,
                "value reader for `{key}` should have stayed cached (unrelated change)"
            );
        }
    }

    let mem_inv = invalidates
        .get("membership")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mem_cached = ctx.is_set(membership_reader);
    if mem_inv {
        assert!(
            !mem_cached,
            "membership reader should have been invalidated"
        );
    } else {
        assert!(
            mem_cached,
            "membership reader should have stayed cached (set identity unchanged)"
        );
    }

    let ord_inv = invalidates
        .get("order")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ord_cached = ctx.is_set(order_reader);
    if ord_inv {
        assert!(!ord_cached, "order reader should have been invalidated");
    } else {
        assert!(
            ord_cached,
            "order reader should have stayed cached (order unchanged)"
        );
    }
}

/// Verify the `handle_stable` contract: a key marked stable keeps the SAME cell
/// handle (node identity) across the op — the atomic-move guarantee rather than
/// a remove + re-mint.
fn assert_handle_stable(
    map: &CellMap<String, V>,
    expected: &Value,
    handle_before: &HashMap<String, Option<CellHandle<V>>>,
) {
    let Some(hs) = expected.get("handle_stable").and_then(|v| v.as_object()) else {
        return;
    };
    for (key, want) in hs {
        if !want.as_bool().unwrap_or(false) {
            continue;
        }
        let before = handle_before
            .get(key)
            .unwrap_or_else(|| panic!("no handle captured for `{key}` before op"));
        let after = map.handle(key);
        assert_eq!(
            after, *before,
            "handle_stable{{{key}}} violated: cell handle (node identity) changed across op"
        );
        // A stable handle is observable too: it still reads a value (the cell lives on).
        assert!(
            after.is_some(),
            "handle_stable{{{key}}} violated: handle missing after op"
        );
    }
}

/// Replay a step-based collection fixture (`initial` + `steps`).
fn run_steps_fixture(name: &str) {
    let fixture = load_fixture(name);
    let ctx = Context::new();
    let map = build_initial(&ctx, fixture.get("initial").expect("initial"));

    let steps = fixture
        .get("steps")
        .and_then(|v| v.as_array())
        .expect("steps");

    for (i, step) in steps.iter().enumerate() {
        let op = step.get("op").expect("op");
        let expected = step.get("expected").expect("expected");

        // Build + prime value/membership/order readers from the CURRENT key set
        // so each step's invalidation is measured in isolation.
        let current_keys = map.keys(&ctx);
        let mut value_readers: HashMap<String, lazily::SlotHandle<Option<V>>> = HashMap::new();
        for key in &current_keys {
            value_readers.insert(key.clone(), value_reader(&ctx, &map, key));
        }
        let membership_reader = {
            let map = map.clone();
            ctx.computed(move |ctx| map.len(ctx))
        };
        let order_reader = {
            let map = map.clone();
            ctx.computed(move |ctx| map.keys(ctx))
        };
        for reader in value_readers.values() {
            ctx.get(reader);
        }
        ctx.get(&membership_reader);
        ctx.get(&order_reader);

        // Snapshot handles (node identities) before the op for handle_stable.
        let handle_before: HashMap<String, Option<CellHandle<V>>> = current_keys
            .iter()
            .map(|k| (k.clone(), map.handle(k)))
            .collect();

        apply_op(&ctx, &map, op);

        let survivors: HashSet<String> = map.keys(&ctx).into_iter().collect();

        if let Some(invalidates) = step.get("invalidates") {
            assert_invalidation(
                &ctx,
                &value_readers,
                &membership_reader,
                &order_reader,
                invalidates,
                &survivors,
            );
        }
        assert_handle_stable(&map, expected, &handle_before);
        assert_state(&ctx, &map, expected);

        // Readers are rebuilt next iteration; touch the reactive values so the
        // graph settles before the following step's prime.
        let _ = i;
    }
}

/// Replay the declarative LIS reconciliation fixture (`reconcile` block).
fn run_reconcile_fixture(name: &str) {
    let fixture = load_fixture(name);
    let reconcile_block = fixture.get("reconcile").expect("reconcile");
    let prior = keyed_pairs(reconcile_block, "prior");
    let target = keyed_pairs(reconcile_block, "target");
    let expected = fixture.get("expected").expect("expected");
    let result_order: Vec<String> = expected
        .get("result_order")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    // 1. The emitted op set is minimal and matches the fixture op-for-op.
    let ops = reconcile(&prior, &target);
    let expected_ops = expected.get("ops").and_then(|v| v.as_array()).unwrap();
    assert_eq!(
        ops.len(),
        expected_ops.len(),
        "op count must be minimal (LIS); emitted {ops:?}"
    );
    for expected_op in expected_ops {
        let ty = expected_op.get("type").and_then(|v| v.as_str()).unwrap();
        let key = expected_op
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        match ty {
            "remove" => assert!(
                ops.iter()
                    .any(|o| matches!(o, DiffOp::Remove { key: k } if k == &key)),
                "expected Remove{{{key}}} in emitted ops {ops:?}"
            ),
            "insert" => assert!(
                ops.iter()
                    .any(|o| matches!(o, DiffOp::Insert { key: k, .. } if k == &key)),
                "expected Insert{{{key}}} in emitted ops {ops:?}"
            ),
            "update" => assert!(
                ops.iter()
                    .any(|o| matches!(o, DiffOp::Update { key: k, .. } if k == &key)),
                "expected Update{{{key}}} in emitted ops {ops:?}"
            ),
            "move" => {
                // Resolve the fixture's (possibly relative) anchor to the key's
                // absolute final index, which is what DiffOp::Move carries.
                let want_to = if let Some(after) = expected_op.get("after").and_then(|v| v.as_str())
                {
                    result_order.iter().position(|k| k == after).unwrap() + 1
                } else if let Some(before) = expected_op.get("before").and_then(|v| v.as_str()) {
                    result_order.iter().position(|k| k == before).unwrap()
                } else {
                    expected_op.get("to").and_then(|v| v.as_u64()).unwrap() as usize
                };
                assert!(
                    ops.iter().any(|o| matches!(
                        o,
                        DiffOp::Move { key: k, to } if k == &key && *to == want_to
                    )),
                    "expected Move{{{key}, to={want_to}}} in emitted ops {ops:?}"
                );
            }
            other => panic!("unknown expected op type: {other}"),
        }
    }

    // 2. Driving a live CellMap from `prior` with the op set converges to
    //    result_order.
    let ctx = Context::new();
    let map: CellMap<String, V> = CellMap::new(&ctx);
    for (k, v) in &prior {
        map.entry(&ctx, k.clone(), *v);
    }
    apply_to_map(&ctx, &map, &ops);
    assert_eq!(
        map.keys(&ctx),
        result_order,
        "reconcile op set did not converge to result_order"
    );

    // 3. Stable entries' value cells are NOT invalidated by the sibling reorder.
    let stable: Vec<String> = expected
        .get("stable_keys_not_invalidated")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    if !stable.is_empty() {
        let ctx = Context::new();
        let map: CellMap<String, V> = CellMap::new(&ctx);
        for (k, v) in &prior {
            map.entry(&ctx, k.clone(), *v);
        }
        let readers: HashMap<String, lazily::SlotHandle<Option<V>>> = stable
            .iter()
            .map(|k| (k.clone(), value_reader(&ctx, &map, k)))
            .collect();
        for reader in readers.values() {
            ctx.get(reader);
        }
        map.reconcile(&ctx, &target);
        for key in &stable {
            assert!(
                ctx.is_set(&readers[key]),
                "stable entry `{key}` value cell must not be invalidated by sibling reorder"
            );
        }
    }
}

fn keyed_pairs(v: &Value, field: &str) -> Vec<(String, V)> {
    let block = v.get(field).unwrap_or_else(|| panic!("reconcile.{field}"));
    let order = block.get("order").and_then(|v| v.as_array()).unwrap();
    let values = block.get("values").and_then(|v| v.as_object()).unwrap();
    order
        .iter()
        .map(|k| {
            let key = k.as_str().unwrap().to_string();
            let val = values
                .get(&key)
                .and_then(|v| v.as_i64())
                .unwrap_or_else(|| panic!("missing {field} value for {key}"));
            (key, val)
        })
        .collect()
}

#[test]
fn conformance_cellmap_independence() {
    run_steps_fixture("cellmap_independence.json");
}

#[test]
fn conformance_cellmap_atomic_move() {
    run_steps_fixture("cellmap_atomic_move.json");
}

#[test]
fn conformance_keyed_reconciliation_lis() {
    run_reconcile_fixture("keyed_reconciliation_lis.json");
}
