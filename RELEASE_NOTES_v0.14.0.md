# lazily v0.14.0

Minor release over v0.13.1. Last published: crates.io `0.13.1`.
Tag `v0.14.0` points at this release commit on `main`.

## Highlights

Adds a native full-Harel/SCXML **state chart** behind a new opt-in `statechart`
feature — the reactive counterpart of the Lean `LazilyFormal.StateChart` model
and the `lazily-spec/docs/state-charts.md` chapter. The core reactive primitives
(`Context` / `ThreadSafeContext` / `AsyncContext`, slots, cells, effects,
signals, the flat `StateMachine`) are unchanged; no default-feature surface
changed.

## Added

- **`StateChart` + `ChartDef` (opt-in `statechart` feature).** A reactive full
  Harel state chart whose active configuration lives in a
  `CellHandle<BTreeSet<String>>`, so any slot/signal/effect reading
  `configuration`, `active_leaves`, or `matches` is invalidated on a real
  transition (a no-op self-transition is suppressed by the cell's `PartialEq`
  guard). Implemented subset: compound states, orthogonal (parallel) regions,
  shallow + deep history (record-on-exit / restore-on-enter), entry/exit/
  transition actions (exit innermost-first → transition → entry outermost-first),
  named guards (fail-closed), and external + internal transitions. Per the
  spec's implementation-status note, `run` actions and `{"expr": …}` context
  guards are rejected explicitly.
- **Deterministic `send` by construction.** `send` mirrors the Lean
  `LazilyFormal.StateChart.send` total function — enabled-transition walk-up,
  LCA exit/enter sets, history record-on-exit / restore-on-enter, dedup + apply
  — so a given `(chart, configuration, history, guards, event)` yields a unique
  result. That confluence guarantee is what the cross-language conformance
  fixtures exercise.
- **`from_json` parsing gated to `serde_json`.** The reactive engine is
  dependency-free; the `statechart` feature adds only `serde_json` for
  `ChartDef::from_json` (the declarative chart form). The `cdylib`, FFI, IPC,
  and WebRTC surfaces are unaffected.
- **Cross-language conformance.** `tests/statechart_conformance.rs` replays the
  shared `lazily-spec/conformance/statechart/` fixtures (`flat_cycle`,
  `hierarchical_player`, `guarded_door`, `parallel_regions`, `history_shallow`,
  `history_deep`, `entry_exit_actions`) — the same contract every binding
  replays.

## Docs

- README now references `lazily-spec` (wire protocol + conformance) and
  `lazily-formal` (Lean 4 formal model).

## Verification

- `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features
  -- -D warnings` clean.
- Default-feature `cargo build` is warning-clean (statechart parsing helpers are
  feature-gated; the `Kind`/`HistoryKind` variants take a targeted dead_code
  allow that is inert under `--all-features`).
- `cargo test --locked` (default suite), `--features statechart --test
  statechart_conformance` (7/7), and the full `--features statechart` suite
  pass.
- `make benchmark-check` green (BENCHMARKS.md tracks `0.14.0`).
- `cargo publish --dry-run` clean: 99 files packaged, verifies.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.14.0 --notes-file RELEASE_NOTES_v0.14.0.md --title "lazily v0.14.0"`.
