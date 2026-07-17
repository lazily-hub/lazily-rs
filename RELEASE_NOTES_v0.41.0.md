# lazily-rs v0.41.0 — reactive-core perf + memory (`#lzsmallany`, `#lzbatchborrow` follow-ups)

Performance + memory release for the single-threaded `Context`, `ThreadSafeContext`,
and `AsyncContext` reactive cores. Ports the inline-value-storage idea introduced by
the sibling bindings back into the reference, plus a sweep of alloc/borrow removals.

## Performance (`src/context.rs`, `src/thread_safe.rs`, `src/async_context.rs`)

- **SmallAny inline value storage** (`#lzsmallany`). Computed values and cell writes
  are no longer unconditionally boxed into `Rc<dyn Any>` / `Arc<dyn Any>`. Scalar and
  small values (≤ inline cap) are stored inline on the node; only oversized values
  spill to the heap. Removes one allocation per recompute and per `set_cell`. Closes
  the ~3× `cold_full_recalc` lead the lazily-cpp `SmallAny` had opened over the
  reference.
- **Async single-locked invalidation frontier.** `AsyncContext`'s recursive,
  re-lock-per-dependent invalidation is replaced by a single-locked frontier walk
  that collects effect schedules under one lock and flushes once — mirroring the
  `ThreadSafeContext` invalidation plan and `Context::mark_frontier_locked`.
- **Reusable DFS scratch.** `Context::mark_frontier_locked` /
  `clear_frontier_locked` no longer allocate fresh `effects` / `stack` / `force_stack`
  Vecs per invalidation; the two stacks are folded into one reusable `Vec<(SlotId,bool)>`
  on `ContextInner`.
- **Single-borrow stale-dependency removal** in the single-threaded `Context`
  (`remove_stale_dependencies_locked`), mirroring the thread-safe variant.
- **`Vec<bool>` scheduled-effect bitset** for the single-threaded `Context` (the
  thread-safe variant already had it), replacing `HashSet<SlotId>` on the effect path.
- **Read-guard cached read** in `ThreadSafeContext`: hold the `slot_fast_paths` read
  guard across `read_fresh` instead of cloning + dropping an `Arc` per cached read.
- **Dropped redundant `is_slot_node` borrow** in `refresh_slot`'s dependency walk.

## Benchmarks (BENCHMARKS.md)

| case | before | after |
|---|---|---|
| scale / cold_full_recalc | 135.3 ms | 53.2 ms (−61%) |
| scale / full_recalc_invalidate_all | 85.4 ms | 49.7 ms (−42%) |
| scale / viewport_recalc | 5.04 µs | 3.03 µs (−40%) |
| scale / build | 157.9 ms | 109.4 ms (−31%) |
| effect_flushing / context | 91.0 ns | 29.6 ns (−67%) |
| memo_equality / context | 2.85 µs | 1.35 µs (−53%) |
| batch_storms / context / 64 | 3.38 µs | 1.70 µs (−50%) |
| dependency_fan_out / context / 256 | 50.9 µs | 40.5 µs (−21%) |
| cached_reads / context | 4.57 ns | 3.55 ns (−22%) |
| cached_reads / thread_safe_context | 64.2 ns | 56.8 ns (−12%) |

Deterministic lock-site budgets unchanged (`make benchmark-check` green).

## Verification

fmt / clippy / build (all features) / every feature test / both Lean formal models
(`lazily-spec` + `lazily-formal`) / loom green. `cargo publish --dry-run` clean.

## Out of scope (deferred)

- `Node` enum size tax (`Box<SlotNode>` was net-negative on the balanced cell/slot
  scale graph; struct-of-arrays is the real fix).
- Async reuse-notifier / sync-resolve in-place poll (conflicts with window-2
  reresolve semantics).
