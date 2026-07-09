# lazily v0.22.1

Patch release over v0.22.0. No API changes. High-load invalidation-throughput
improvement on `ThreadSafeContext` (`#lzfrontierarc`).

## Highlights

**Perf: cache the fast-path `Arc` across the invalidation frontier pass.**
`ThreadSafeContext`'s lock-free invalidation fast path
(`try_mark_slot_frontier_fast`) previously re-fetched each slot's
`ThreadSafeSlotFastPath` from the `slot_fast_paths` `RwLock` once in the BFS
discovery phase and again in the marking phase — two shared-read acquisitions
per frontier slot. The marking phase now reuses the `Arc` observed during the
BFS, halving the `slot_fast_paths` read acquisitions.

Under 16-way contention the `slot_fast_paths` reader-count atomic dominates
(cache line bouncing across cores), so halving the reads yields a measured,
statistically-significant throughput improvement on the high-load cases —
without touching the deterministic state-mutex acquisition budgets and without
any change to the microbenchmark hot paths.

### Controlled A/B evidence

Same-session `--save-baseline before_opt` A/B (criterion statistical
comparison, 16 workers):

| case | change | p-value |
|---|---|---|
| `thread_safe_graph_propagation/fan_out_lazy_dirty_epochs/16` | **−46.8%** | p=0.00 |
| `thread_safe_graph_propagation/fan_in_lazy_dirty_epochs/16` | **−22.6%** | p=0.00 |
| `set_cell_invalidation/independent_slot_contention/16` | **−17.3%** | p=0.00 |
| `thread_safe_contention/independent_slots/16` | −5.3% | p=0.37 (not significant) |

The microbenchmark cases (`cached_reads`, `cold_first_get`) correctly show no
change — they do not touch the invalidation frontier.

### Safety

The cached `Arc` is provably identical to a re-fetched one: `slot_fast_paths`
entries are write-once for a given slot id (set at slot creation, never cleared
or replaced), because `ThreadSafeContext` never frees a slot id (only
`dispose_effect` pushes to `free_ids`, and effects do not index
`slot_fast_paths`). The invariant is documented at the cache site and covered
by two new Loom model tests in `tests/thread_safe_loom.rs`
(`invalidation_frontier_cached_arc_*`).

### Why the deterministic budgets are unchanged

The project's regression budgets use deterministic state-mutex lock-acquisition
counts. `slot_fast_paths` is a separate `RwLock` that was not (and is not)
instrumented by the profile wrapper, so its acquisition counts do not appear in
the budget table. The improvement is therefore captured by the controlled
wall-clock A/B above, recorded as a watch-item row in `BENCHMARKS.md`, rather
than by a budget-line change.
