# lazily v0.17.0

Minor release over v0.16.0. Last published: crates.io `0.16.0`.
Tag `v0.17.0` points at this release commit on `main`.

## Highlights

Closes the **full lazily-spec + lazily-formal compliance** picture. v0.16.0
declared lazily-rs conformant to every `MUST` row of the lazily-spec Binding
Conformance Matrix; v0.17.0 makes that declaration *auditable*:

- the remaining canonical collections conformance fixtures are now replayed
  (SemTree / SeqCrdt / StableId / TextCrdt), and
- the **lazily-formal** Lean model is now part of the test suite — both
  `make check` (local) and CI build it on every run.

Every primitive lazily-formal models (Slot/Cell/Signal/Effect, CellMap/
CellFamily, CellTree, keyed reconciliation, async slot state) now has a
matching lazily-rs implementation *and* a canonical fixture replaying against
the spec.

## Added

### Canonical collections conformance — 4 new fixture replays

- **`tests/collections_conformance.rs`**
  - `conformance_semtree_incremental` — replays
    `collections/semtree_incremental.json`: ancestor-chain-only recompute,
    sibling isolation, memo-equality suppression guard, removal update.
  - `conformance_stableid_alignment` — replays
    `collections/stableid_alignment.json`: in-band anchors, content-derived
    keys (reflow-stable, edit-sensitive), word-LCS similarity alignment
    (`Same` / `Edited` / `Inserted`), `assign_stable_keys` identity flow.
  - `conformance_textcrdt_convergence` — replays
    `collections/textcrdt_convergence.json`: Fugue/RGA character CRDT — local
    insert/delete, concurrent same-point inserts converge deterministically,
    concurrent insert+delete both apply, commutative + idempotent merge,
    stable-leaf GC, concurrent-delete convergence.
- **`tests/seqcrdt_conformance.rs`** (new file, feature-gated) — replays
  `collections/seqcrdt_convergence.json`: move-aware sequence CRDT — single-LWW
  move (no duplication), concurrent-insert same-gap convergence (peer
  tiebreak), concurrent move converges to later stamp, concurrent move +
  value-edit independence, tombstone convergence + commutative merge.

### lazily-formal in the test suite + CI

- `make check` already ran `test-lazily-formal` since v0.16.0; CI now matches.
  The `lean` job builds **both** Lean packages: the lazily-spec IPC/state-machine
  model (`#lzleanmodel`) and the full lazily-formal model (`#lzformal`) — the
  flat FSM kernel, the full Harel state chart, the reactive graph kernel
  (Slot/Cell/Signal/Effect), the keyed collection, the ordered tree, the LIS
  reconciliation, and the async slot state machine.
- New `make test-seqcrdt-conformance` target (runs the SeqCrdt conformance
  fixture under `--features distributed`).

### Public API (additive, semver-compatible)

- `SeqCrdt` now derives `Clone` and exposes `peer()` + `fork(peer)`. Deep-copy
  of a replica's state (entries, stamps, positions) under a new owning peer is
  required to replay the canonical SeqCrdt convergence fixtures (clone/fork
  steps). Matches the existing `Clone` derive on `TextCrdt`.

## Documentation

- `README.md` — lazily-formal description now lists the full 8-module scope
  (flat FSM, Harel chart, reactive graph, collections, tree, reconciliation,
  async slot state) instead of the v0.16.0 2-module summary.
- `AGENTS.md` — Commands section now documents `test-lazily-formal` (was
  missing), `test-seqcrdt-conformance`, and the corrected `make check`
  description (both Lean models + every feature test).

## Conformance

With this release lazily-rs satisfies every `MUST` row of the lazily-spec
[Binding Conformance Matrix](https://github.com/lazily-hub/lazily-spec/blob/main/protocol.md)
**and** every primitive modeled in
[`lazily-formal`](https://github.com/lazily-hub/lazily-formal) has a matching
lazily-rs implementation that the canonical fixtures exercise:

| lazily-formal module | lazily-rs source | Conformance fixture |
|---|---|---|
| `StateMachine` | `src/state_machine.rs` | `tests/state_machine.rs` |
| `StateChart` | `src/statechart.rs` | `tests/statechart_conformance.rs` |
| `Reactive` (Slot/Cell/Signal/Effect) | `src/{slot,cell,signal,effect}.rs` | `tests/{spec_compliance,signal}.rs` |
| `Collection` (CellMap/CellFamily) | `src/cell_family.rs` | `tests/collections_conformance.rs` (cellmap_*) |
| `Tree` (CellTree) | `src/cell_tree.rs` | `tests/collections_conformance.rs` (semtree_incremental) |
| `Reconciliation` | `src/reconcile.rs` | `tests/collections_conformance.rs` (keyed_reconciliation_lis) |
| `AsyncSlotState` | `src/async_context.rs` | `tests/async_state_machine.rs` |

The `SeqCrdt` and `TextCrdt` modules (cell-model.md's move-aware sequence order
and free-text CRDT) are conformance-replayed via `seqcrdt_convergence.json` and
`textcrdt_convergence.json` respectively.

## Verification

- `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features
  -D warnings` clean.
- `cargo test --locked` (default suite), `cargo test --locked --features
  distributed` (SeqCrdt conformance), and `cargo test --locked --test
  collections_conformance` (6/6) pass.
- `make test-lazily-formal` green (lazily-formal `lake build` clean).
- `make benchmark-check` green (BENCHMARKS.md tracks `0.17.0`).
- `cargo publish --dry-run` clean.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.17.0 --notes-file RELEASE_NOTES_v0.17.0.md --title "lazily v0.17.0"`.
