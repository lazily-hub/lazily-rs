# lazily-rs v0.61.0 — generation-fenced stale-handle disposal

- Fence node handles by allocator generation and node kind before any graph mutation.
- Make stale and cross-kind disposal idempotent no-ops across synchronous,
  thread-safe, and asynchronous contexts.
- Preserve recycled successors, their edges, writable state, and effect cleanup
  ownership.
- Replay the canonical lazily-spec v0.43.0 stale-handle matrix with mutation
  evidence.
