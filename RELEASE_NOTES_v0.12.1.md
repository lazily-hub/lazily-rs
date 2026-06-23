# lazily v0.12.1

Patch release over v0.12.0. Last published: crates.io `0.12.0`.
Tag `v0.12.1` points at this release commit on `main`.

## Highlights

Adds finite-state-machine wrappers for the existing single-threaded,
thread-safe, and async reactive contexts. These wrappers let downstream systems
represent accepted transitions as typed events while still exposing the current
state through lazily cells/signals/effects.

## Added

- **`StateMachine<S, E>`** wraps a `Context` cell plus a pure transition
  function. `send(event)` accepts transitions that return `Some(next_state)` and
  rejects guarded transitions that return `None`.
- **`ThreadSafeStateMachine<S, E>`** mirrors `StateMachine` for
  `ThreadSafeContext`, requiring `Send + Sync + 'static` state/events and
  transition functions so the machine can be shared across threads.
- **`AsyncStateMachine<S, E>`** mirrors the same API over `AsyncContext` behind
  the existing async support.

## Verification

- `cargo test state_machine`
- `cargo publish --dry-run`

## Publish checklist

1. `cargo publish` (dry-run verified clean from a clean worktree).
2. `gh release create v0.12.1 --notes-file RELEASE_NOTES_v0.12.1.md --title "lazily v0.12.1"`.
