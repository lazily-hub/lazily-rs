# lazily v0.16.0

Minor release over v0.15.0. Last published: crates.io `0.15.0`.
Tag `v0.16.0` points at this release commit on `main`.

## Highlights

Closes the **keyed cell collections** gap in lazily-spec's Binding Conformance
Matrix — the layer (`CellMap` / `CellTree` / keyed reconciliation) the spec now
makes a `MUST` for every binding. The implementations already shipped in earlier
releases; this release adds the canonical cross-language **conformance tests**
and a small semver-compatible public-API addition so the cell-identity contract
is assertable. No default-feature behavior changed.

## Added

- **Keyed cell collections conformance** (`tests/collections_conformance.rs`).
  Replays the canonical compute fixtures in
  `lazily-spec/conformance/collections/` — the same fixtures every binding
  replays:
  - `cellmap_independence` — value / set-membership / order reactivity
    independence (`set_value` / `insert` / `remove` / `move_to`).
  - `cellmap_atomic_move` — atomic ordered move keeps the entry's cell handle
    (node identity) and dependents, bumps the order signal once, and leaves
    value readers of the moved key untouched (`#lzcellmove`).
  - `keyed_reconciliation_lis` — LIS move-minimized reconciliation emits the
    minimal `{remove, move}` op set and does not invalidate stable entries'
    value cells on a sibling reorder (`#lzkeyrecon`).
- **`CellHandle<T>` identity derives** — `Debug`, `PartialEq`, `Eq`. Two handles
  are equal when they address the same underlying node, the observable
  cell-identity contract behind atomic moves and keyed reconciliation. Additive
  (semver-compatible); no existing trait impl removed.
- **JSON Schema compliance** (`tests/schema_compliance.rs`, `#lzspecschema`) —
  lazily-rs serde output (Snapshot / Delta / `CrdtSync`, incl. `NodeKey`)
  validates against the sibling `lazily-spec/schemas`, closing the
  binding↔schema loop.
- **CI** now runs the collections conformance test against the canonical
  lazily-spec fixtures (`.github/workflows/ci.yml`).

## Conformance

With this release lazily-rs satisfies every `MUST` row of the lazily-spec
[Binding Conformance Matrix](https://github.com/lazily-hub/lazily-spec/blob/main/protocol.md):
reactive core, keyed cell collections, flat state machine, Harel state charts,
async context, IPC, C-ABI FFI (incl. `CrdtSync = 3`), the distributed CRDT
plane, the permission boundary, and capability negotiation.

## Verification

- `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features
  -D warnings` clean.
- `cargo test --locked` (default suite) and `cargo test --locked --test
  collections_conformance` (3/3) pass.
- `make benchmark-check` green (BENCHMARKS.md tracks `0.16.0`).
- `cargo publish --dry-run` clean.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.16.0 --notes-file RELEASE_NOTES_v0.16.0.md --title "lazily v0.16.0"`.
