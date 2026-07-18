# lazily-rs v0.44.0 — delta-CRDT sync (`#lzspecdeltacrdt`)

Completes the last Phase 3 item: implements the `DeltaSinceRequest` wire type
and `delta_reply` method for incremental CRDT anti-entropy, so peers exchange
only the cell states that changed rather than the full converged state per
round.

## New API

- **`DeltaSinceRequest`** — a lightweight control frame carrying the requester's
  per-peer stamp frontier. The receiver responds with a `CrdtSync` whose `ops`
  carry only the states past that frontier.
- **`CrdtPlaneRuntime::delta_reply(&request)`** — ships only the missing ops
  (the delta), not the full converged state.
- Added to `IpcMessage` enum (FFI kind 6) and `is_control()`.
- The join is the same semilattice (`apply_delta` ≡ `merge`), so deltas are
  safe to resend and apply in any order.

## Phase 3 completion

This completes all eight Phase 3 items of `#lzperfaudit`. See the plan
(`tasks/agent-doc/plans/lazily-perf-memory-audit.md`) for the full status.
