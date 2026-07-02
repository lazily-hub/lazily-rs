# lazily v0.18.0

Minor release over v0.17.0. Last published: crates.io `0.17.0`.
Tag `v0.18.0` points at this release commit on `main`.

## Highlights

**Breaking (feature selection):** `ThreadSafeContext` + `ThreadSafeStateMachine`
are now opt-in behind a new `thread-safe` Cargo feature. This drops
`parking_lot` from the default dependency closure, symmetrically with the
existing `async` feature for `AsyncContext`. The lazily-spec conformance
contract is unchanged — both layers remain `MUST²` (platform-conditional); they
are simply feature-gated rather than always compiled in.

## Migration

If you use `ThreadSafeContext`, `ThreadSafeStateMachine`, `ReadStrategy`, or
`ThreadSafeSignalHandle`, add `thread-safe` to your feature list:

```toml
[dependencies]
lazily = { version = "0.18", features = ["thread-safe"] }
```

Single-threaded users (the reactive core on `Context`) need no change — `cargo
build` now compiles without `parking_lot` at all.

If you use `tokio_sync` tests or the `tokio_sync` example, enable both:

```toml
lazily = { version = "0.18", features = ["tokio", "thread-safe"] }
```

## Changed

### Cargo features

- **`thread-safe`** (new) — gates `ThreadSafeContext`, `ThreadSafeStateMachine`,
  `ReadStrategy`, `ThreadSafeSignalHandle`, and the `src/thread_safe.rs` module.
  Pulls `parking_lot` (optional dep).
- **`async`** — now also pulls `parking_lot` (the `AsyncContext` implementation
  uses `parking_lot::Mutex` internally, same as before — the dependency was
  always needed; it is now correctly declared as an optional dependency rather
  than a hard one).
- **`parking_lot`** — moved from a hard dependency to `optional = true`. Only
  pulled by `thread-safe` or `async`.
- **`loom`** — now implies `thread-safe` (the Loom model tests target the
  thread-safe context).
- **`std_sync_mutex`** — now implies `thread-safe` (it toggles
  ThreadSafeContext's inner state mutex; no-op without the feature).

### Source

- `src/lib.rs`: `mod thread_safe` + its `pub use` are now
  `#[cfg(feature = "thread-safe")]`.
- `src/state_machine.rs`: `ThreadSafeStateMachine` (struct + 2 impls) and its
  `ThreadSafeTransitionFn` type alias are now `#[cfg(feature = "thread-safe")]`.
  `ThreadSafeTransitionFn` is shared by `AsyncStateMachine`, so it is gated on
  `any(feature = "thread-safe", feature = "async")` to stay available for the
  async path.

### Tests / benches / examples (feature-gated)

- File-level `#![cfg(feature = "thread-safe")]`:
  `tests/thread_safe_stress.rs`, `tests/thread_safe_state_machine.rs`,
  `benches/context.rs`, `benches/profile.rs`, `examples/tokio_sync.rs`,
  `examples/instrumentation_profile.rs`.
- `#![cfg(all(..., feature = "thread-safe"))]`:
  `tests/thread_safe_loom.rs` (loom + thread-safe),
  `tests/tokio_sync.rs` (tokio + thread-safe),
  `benches/tokio_sync.rs` (tokio + thread-safe),
  `benches/async_context.rs` (async + thread-safe).
- `tests/spec_compliance.rs`: 28 `thread_safe_*` test functions individually
  `#[cfg(feature = "thread-safe")]`-gated (the rest of the suite runs on the
  default feature set).
- `tests/signal.rs`: the `thread_safe` submodule is
  `#[cfg(feature = "thread-safe")]`-gated.

### Tooling

- `Makefile`: new `test-thread-safe` target; `test-tokio` now requires
  `"tokio thread-safe"`; `instrumentation-profile` adds `thread-safe`.
  `make check` runs the new target.
- `.github/workflows/ci.yml`: new "Test thread-safe feature" step; the Tokio
  step now uses `"tokio thread-safe"`.
- `scripts/update-benchmark-results.py`: bench commands now include
  `thread-safe` where they touch ThreadSafeContext benchmarks.

## Why

Before this release, every user of `lazily` — even a purely single-threaded
reactive-graph user — pulled in `parking_lot` and compiled the 3,000-line
`thread_safe.rs` module. Now the default dependency surface is `smallvec` only
(unless `default-features = false`); opt-in features pull exactly what they
need. The spec's three-layer concurrency model (single-threaded base, optional
thread-safe, optional async) now maps 1:1 to the Cargo feature surface.

## Verification

- `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features
  -D warnings` clean.
- `cargo test --locked` (default), `--features thread-safe`, `--features async`,
  `--features "distributed serde"` all pass.
- `make benchmark-check` green (BENCHMARKS.md tracks `0.18.0`).
- `cargo publish --dry-run` clean.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.18.0 --notes-file RELEASE_NOTES_v0.18.0.md --title "lazily v0.18.0"`.
