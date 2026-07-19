# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.45.0`.

Environment: `rustc 1.97.0 (2d8144b78 2026-07-07)` on `x86_64-unknown-linux-gnu`.

Refresh command:

```bash
python3 scripts/update-benchmark-results.py
```

Regression workflow:

```bash
cargo bench --features instrumentation,thread-safe -- --save-baseline before
# apply the performance patch
cargo bench --features instrumentation,thread-safe -- --baseline before
python3 scripts/update-benchmark-results.py --no-run
```

Regression budgets enforced by `python3 scripts/update-benchmark-results.py --check`:

| Profile | Max lock acquisitions | Site lock budgets |
|---|---:|---|
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 700 | set_cell_invalidation<=260, dependency_edge<=16, get_refresh<=32, publish<=32 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 900 | other<=800, set_cell_invalidation<=16, dependency_edge<=64, get_refresh<=2, publish<=2 |
| thread_safe_contention_same_slot_write_read_16 | 1400 | get_refresh<=160, publish<=256, in_flight_wait<=700, set_cell_invalidation<=260 |
| thread_safe_contention_independent_slots_16 | 1100 | other<=450, get_refresh<=64, publish<=320, dependency_edge<=16, set_cell_invalidation<=300 |
| thread_safe_contention_read_mostly_waiters_16 | 256 | get_refresh<=128, publish<=64, in_flight_wait<=96 |
| thread_safe_contention_batched_write_bursts_16 | 950 | other<=800, get_refresh<=128, dependency_edge<=64, set_cell_invalidation<=16, publish<=64, in_flight_wait<=64 |
| thread_safe_effect_contention_queue_coalescing_16 | 2600 | other<=900, dependency_edge<=1600, set_cell_invalidation<=16, get_refresh<=64, publish<=0 |
| thread_safe_effect_contention_cleanup_execution_16 | 1300 | other<=450, dependency_edge<=700, set_cell_invalidation<=256, get_refresh<=0, publish<=0 |
| thread_safe_effect_contention_batch_flush_16 | 1500 | other<=1300, get_refresh<=32, dependency_edge<=96, set_cell_invalidation<=16, publish<=32 |

Budgets use deterministic lock acquisition counts instead of elapsed wait/hold time.

Synchronization strategy adoption gate:

| Strategy | Status | Required throughput evidence | Required p50/p95 latency evidence | Lock-site and safety gate |
|---|---|---|---|---|
| current_std_mutex_condvar | baseline | thread_safe_contention and thread_safe_effect_contention at 8/16 workers | p50/p95 latency for same-slot, read-mostly, batch, and effect-heavy cases | must stay within current lock-site budgets and Loom safety coverage |
| narrower_condvar_wakeups | adopted for per-slot recompute waiters | same-slot write/read and read-mostly waiter throughput at 8/16 workers | p50/p95 latency for waiter wakeup handoff and stale-completion retry | must not regress effect queue, cleanup, or batch flush budgets |
| parking_lot_style_parking | candidate only | same contention matrix measured against current_std_mutex_condvar | p50/p95 latency for parking/unparking under 8/16 workers | requires no worse lock-site budgets plus a deadlock/starvation model |
| targeted_cas | candidate only | fresh cached reads and independent-slot throughput at 8/16 workers | p50/p95 latency for revision validation fallback and publish races | requires unchanged effect/batch/disposal budgets plus Loom/Shuttle proof |

Candidates do not replace the current strategy before the same run reports throughput, p50/p95 latency, and lock-site budgets for the required 8/16-worker cases.

Required latency evidence uses Criterion sample per-iteration timing.

Watch-item A/B follow-up:

| Watch item | Baseline/current refs | Focused command | Controlled rerun result | Decision |
|---|---|---|---|---|
| cached ThreadSafeContext read latency | a8b6fc3 vs c917401 | `cargo bench --features instrumentation,thread-safe --bench context -- cached_reads/thread_safe_context` | 73.48 ns baseline vs 73.20 ns current on warm-cache repeat | no tuning; the archived 56.5 ns row did not reproduce under controlled A/B |
| effect cleanup contention at 16 workers | a8b6fc3 vs c917401 | `cargo bench --features instrumentation,thread-safe --bench context -- thread_safe_effect_contention/cleanup_execution/16` | 2.31 ms baseline vs 2.43 ms current on warm-cache repeat with overlapping CIs | keep watching; Criterion reported no statistically significant change |
| invalidation-frontier fast-path Arc cache (#lzfrontierarc) | 15d4206 vs this change (controlled --save-baseline before_opt A/B, same session) | `cargo bench --features instrumentation,thread-safe --bench context -- --baseline before_opt` | fan_out_lazy_dirty_epochs/16 -46.8% (p=0.00), fan_in_lazy_dirty_epochs/16 -22.6% (p=0.00), independent_slot_contention/16 -17.3% (p=0.00), independent_slots/16 -5.3% (p=0.37 n.s.) | adopted; the cached Arc reuses the BFS-time fast path in the marking pass, halving uninstrumented slot_fast_paths RwLock read acquisitions whose reader-count atomics dominate under 16-way contention. Deterministic state-mutex acquisition counts (the budget metric) are unchanged because slot_fast_paths is a separate uninstrumented lock; the evidence is the controlled wall-clock A/B. Microbench cases (cached_reads) correctly show no change as they do not touch the invalidation frontier. |
| Context slot clean-cache-hit fast path (#lzslotfastpath) | 8c64f33 vs this change (controlled --save-baseline before_slot A/B, same session) | `cargo bench --features instrumentation,thread-safe --bench context -- --baseline before_slot 'cached_reads|typed_cache_reads'` | typed_cache_reads/context_slot -58.9% (p=0.00), cached_reads/context -51.6% (p=0.00), typed_cache_reads/context_cell -2.1% (p=0.76 n.s.) | adopted; refresh_slot now early-returns when the slot holds a value and is neither dirty nor force-recompute, skipping the cycle-guard borrowMut + guard-drop borrowMut + dependencies Vec clone + per-dep is_slot_node borrows + clear_slot_dirty_flags borrowMut on the cache-hit path. Correctness rests on mark_slot_dirty always being called with force_recompute=true from invalidate_dependent_from_changed_value, so any upstream change sets dirty=true and bypasses the fast path. context_slot 11.8 -> 4.7 ns, now within ~1.5 ns of context_cell (3.0 ns); the previous downcast 'tax' framing was wrong (the cell also downcasts) - the real cost was refresh_slot's redundant work on clean reads. |

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 2.841 ms | 3.727 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.861 ms | 8.335 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.701 ms | 2.229 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 5.503 ms | 6.548 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 582.257 us | 677.153 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.146 ms | 1.417 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.648 ms | 2.750 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.986 ms | 4.535 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.285 ms | 1.513 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.930 ms | 3.851 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.277 ms | 1.471 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.043 ms | 4.138 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.515 ms | 2.405 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 3.808 ms | 6.639 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.027 ms | 3.176 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 4.862 ms | 5.155 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.772 ms | 1.831 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.571 ms | 3.889 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.125 ms | 3.125 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 7.863 ms | 8.575 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 971.940 us | 1.138 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.707 ms | 2.229 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 3.612 ns | 3.598 ns - 3.631 ns |
| cached_reads | thread_safe_context | 55.474 ns | 55.420 ns - 55.572 ns |
| cold_first_get | context | 89.234 ns | 80.093 ns - 98.006 ns |
| cold_first_get | thread_safe_context | 901.292 ns | 896.127 ns - 906.620 ns |
| dependency_fan_out | context / 32 | 2.217 us | 2.106 us - 2.324 us |
| dependency_fan_out | context / 256 | 18.606 us | 17.324 us - 19.920 us |
| dependency_fan_out | thread_safe_context / 32 | 18.243 us | 18.136 us - 18.363 us |
| dependency_fan_out | thread_safe_context / 256 | 141.272 us | 140.362 us - 142.272 us |
| set_cell_invalidation | high_fan_out / 512 | 88.804 us | 79.090 us - 98.882 us |
| set_cell_invalidation | same_slot_contention / 1 | 78.046 us | 76.034 us - 79.919 us |
| set_cell_invalidation | same_slot_contention / 2 | 158.541 us | 156.865 us - 160.230 us |
| set_cell_invalidation | same_slot_contention / 4 | 407.644 us | 394.790 us - 422.157 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.455 ms | 1.353 ms - 1.600 ms |
| set_cell_invalidation | same_slot_contention / 16 | 3.234 ms | 2.960 ms - 3.513 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 74.987 us | 72.828 us - 76.895 us |
| set_cell_invalidation | independent_slot_contention / 2 | 151.467 us | 149.392 us - 153.493 us |
| set_cell_invalidation | independent_slot_contention / 4 | 400.719 us | 386.127 us - 414.109 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.336 ms | 1.202 ms - 1.472 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 2.852 ms | 2.539 ms - 3.185 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 134.800 us | 133.040 us - 136.562 us |
| set_cell_invalidation | batched_write_bursts / 2 | 172.759 us | 168.259 us - 177.253 us |
| set_cell_invalidation | batched_write_bursts / 4 | 398.411 us | 381.989 us - 415.887 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.242 ms | 1.126 ms - 1.384 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.596 ms | 3.494 ms - 3.698 ms |
| memo_equality_suppression | context | 1.218 us | 1.095 us - 1.341 us |
| memo_equality_suppression | thread_safe_context | 28.565 us | 28.018 us - 29.412 us |
| effect_flushing | context | 30.434 ns | 30.398 ns - 30.476 ns |
| effect_flushing | thread_safe_context | 883.311 ns | 881.420 ns - 885.613 ns |
| batch_storms | context / 64 | 2.092 us | 2.088 us - 2.096 us |
| batch_storms | thread_safe_context / 64 | 7.173 us | 7.155 us - 7.193 us |
| thread_safe_contention | same_slot_write_read / 1 | 130.899 us | 127.901 us - 133.798 us |
| thread_safe_contention | same_slot_write_read / 2 | 357.152 us | 345.411 us - 369.045 us |
| thread_safe_contention | same_slot_write_read / 4 | 824.172 us | 767.708 us - 889.468 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.873 ms | 2.610 ms - 3.141 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.781 ms | 7.531 ms - 7.998 ms |
| thread_safe_contention | independent_slots / 1 | 134.184 us | 131.839 us - 135.945 us |
| thread_safe_contention | independent_slots / 2 | 269.276 us | 254.175 us - 285.933 us |
| thread_safe_contention | independent_slots / 4 | 675.685 us | 636.806 us - 718.595 us |
| thread_safe_contention | independent_slots / 8 | 1.807 ms | 1.654 ms - 1.962 ms |
| thread_safe_contention | independent_slots / 16 | 5.607 ms | 5.227 ms - 5.989 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 132.477 us | 130.019 us - 134.904 us |
| thread_safe_contention | read_mostly_waiters / 2 | 153.473 us | 151.030 us - 155.813 us |
| thread_safe_contention | read_mostly_waiters / 4 | 230.356 us | 229.200 us - 231.853 us |
| thread_safe_contention | read_mostly_waiters / 8 | 579.273 us | 539.397 us - 618.403 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.181 ms | 1.105 ms - 1.259 ms |
| thread_safe_contention | batched_write_bursts / 1 | 207.095 us | 204.412 us - 209.670 us |
| thread_safe_contention | batched_write_bursts / 2 | 490.798 us | 470.240 us - 514.167 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.308 ms | 1.303 ms - 1.313 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.636 ms | 2.557 ms - 2.700 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.963 ms | 3.820 ms - 4.123 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.242 ms | 1.141 ms - 1.341 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.075 ms | 2.801 ms - 3.346 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.272 ms | 1.205 ms - 1.339 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.118 ms | 2.773 ms - 3.474 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.784 ms | 1.582 ms - 2.009 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.339 ms | 3.699 ms - 5.170 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.042 ms | 3.004 ms - 3.084 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 4.891 ms | 4.822 ms - 4.971 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.769 ms | 1.748 ms - 1.791 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.620 ms | 3.560 ms - 3.695 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.241 ms | 2.076 ms - 2.474 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 7.655 ms | 7.081 ms - 8.117 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 988.830 us | 950.617 us - 1.034 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.745 ms | 1.572 ms - 1.918 ms |
| profile_instrumentation | context_snapshot | 237.589 ns | 236.374 ns - 238.823 ns |
| profile_instrumentation | thread_safe_snapshot | 292.057 us | 290.638 us - 293.266 us |
| async_cached_resolve | async_context | 4.839 us | 4.388 us - 5.331 us |
| async_cached_resolve | sync_context_baseline | 58.748 ns | 58.317 ns - 59.254 ns |
| async_cached_resolve | sync_get | 11.352 ns | 11.305 ns - 11.406 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.316 us | 1.305 us - 1.335 us |
| async_cold_resolve | async_context | 5.175 us | 4.736 us - 5.648 us |
| async_cold_resolve | sync_context_baseline | 88.986 ns | 79.802 ns - 97.627 ns |
| async_cold_resolve | thread_safe_context_baseline | 900.037 ns | 894.912 ns - 905.995 ns |
| async_invalidation_throughput | async_context | 328.752 us | 296.380 us - 363.147 us |
| async_invalidation_throughput | sync_context_baseline | 2.599 us | 2.438 us - 2.767 us |
| async_invalidation_throughput | thread_safe_context_baseline | 56.681 us | 56.518 us - 56.889 us |
| async_cancellation_throughput | async_invalidate_in_flight | 62.153 us | 49.964 us - 74.061 us |
| async_concurrent_contention | async_context / 1 | 73.767 us | 72.092 us - 75.348 us |
| async_concurrent_contention | async_context / 4 | 341.133 us | 308.504 us - 374.751 us |
| async_concurrent_contention | async_context / 16 | 1.566 ms | 1.410 ms - 1.767 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 77.625 us | 75.791 us - 79.440 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 791.157 us | 769.904 us - 806.747 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 4.611 ms | 4.603 ms - 4.619 ms |
| async_effect_throughput | async_context | 188.601 ms | 188.435 ms - 188.769 ms |
| async_batch_throughput | async_context | 114.951 us | 101.149 us - 128.363 us |
| async_batch_throughput | sync_context_baseline | 7.070 us | 7.024 us - 7.116 us |
| tokio_sync_cached_read | single_task | 1.407 us | 1.405 us - 1.408 us |
| tokio_sync_cached_read | spawn_read | 5.788 us | 5.279 us - 6.288 us |
| tokio_sync_cold_first_get | single_task | 1.354 us | 1.353 us - 1.355 us |
| tokio_sync_cold_first_get | spawn_compute | 6.288 us | 5.654 us - 6.941 us |
| tokio_sync_invalidation | single_task | 53.941 us | 53.700 us - 54.207 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 58.618 us | 58.062 us - 59.409 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 483.885 us | 441.085 us - 534.334 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.835 ms | 3.747 ms - 3.893 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 61.897 us | 60.221 us - 63.328 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 435.069 us | 404.442 us - 467.934 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 3.286 ms | 3.207 ms - 3.342 ms |
| tokio_sync_batch | spawn_batch | 46.504 us | 46.415 us - 46.594 us |
| tokio_sync_effect | single_task | 10.098 ms | 10.095 ms - 10.101 ms |
| scale | build | 102.070 ms | 100.380 ms - 104.224 ms |
| scale | cold_full_recalc | 52.113 ms | 51.706 ms - 52.579 ms |
| scale | full_recalc_invalidate_all | 54.009 ms | 53.742 ms - 54.423 ms |
| scale | viewport_recalc | 3.090 us | 3.084 us - 3.098 us |
| lzspec_base64 | decode_base64 / 64 | 1.083 us | 1.079 us - 1.087 us |
| lzspec_base64 | decode_base64 / 1024 | 4.923 us | 4.835 us - 5.027 us |
| lzspec_base64 | decode_base64 / 16384 | 120.274 us | 106.987 us - 137.555 us |
| lzspec_base64 | decode_json_u8 / 64 | 709.051 ns | 703.125 ns - 715.880 ns |
| lzspec_base64 | decode_json_u8 / 1024 | 5.907 us | 5.860 us - 5.956 us |
| lzspec_base64 | decode_json_u8 / 16384 | 176.945 us | 174.529 us - 179.193 us |
| lzspec_base64 | encode_base64 / 64 | 914.254 ns | 909.090 ns - 919.982 ns |
| lzspec_base64 | encode_base64 / 1024 | 5.088 us | 5.026 us - 5.154 us |
| lzspec_base64 | encode_base64 / 16384 | 75.691 us | 72.440 us - 79.457 us |
| lzspec_base64 | encode_json_u8 / 64 | 279.812 ns | 277.703 ns - 282.329 ns |
| lzspec_base64 | encode_json_u8 / 1024 | 2.345 us | 2.333 us - 2.358 us |
| lzspec_base64 | encode_json_u8 / 16384 | 37.983 us | 34.689 us - 41.999 us |
| lzspec_frontier_suppress | decode_ops_only | 1.045 us | 1.038 us - 1.052 us |
| lzspec_frontier_suppress | decode_with_frontier | 1.599 us | 1.589 us - 1.609 us |
| lzspec_frontier_suppress | encode_ops_only | 458.699 ns | 456.213 ns - 461.442 ns |
| lzspec_frontier_suppress | encode_with_frontier | 734.756 ns | 724.947 ns - 745.814 ns |
| lzspec_intern | decode_inline | 39.166 us | 38.798 us - 39.637 us |
| lzspec_intern | decode_intern | 120.855 us | 120.605 us - 121.117 us |
| lzspec_intern | encode_inline | 17.154 us | 15.205 us - 19.003 us |
| lzspec_intern | encode_intern | 104.213 us | 103.884 us - 104.639 us |
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.091 ns | 1.088 ns - 1.094 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 88.727 ns | 88.394 ns - 89.076 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 16.975 ns | 16.848 ns - 17.088 ns |
| revision_write_cost | push / 1 | 247.951 ns | 242.073 ns - 253.899 ns |
| revision_write_cost | push / 16 | 1.097 us | 1.094 us - 1.101 us |
| revision_write_cost | push / 128 | 10.073 us | 10.055 us - 10.093 us |
| revision_write_cost | push / 1024 | 83.239 us | 83.029 us - 83.464 us |
| revision_write_cost | revision / 1 | 124.469 ns | 124.355 ns - 124.585 ns |
| revision_write_cost | revision / 16 | 815.872 ns | 814.055 ns - 817.702 ns |
| revision_write_cost | revision / 128 | 8.420 us | 8.397 us - 8.444 us |
| revision_write_cost | revision / 1024 | 69.536 us | 69.412 us - 69.662 us |
| revision_write_then_read | push / 1 | 113.066 ns | 112.155 ns - 113.974 ns |
| revision_write_then_read | push / 16 | 1.324 us | 1.321 us - 1.326 us |
| revision_write_then_read | push / 128 | 13.822 us | 13.800 us - 13.845 us |
| revision_write_then_read | push / 1024 | 111.778 us | 111.531 us - 112.033 us |
| revision_write_then_read | revision / 1 | 95.244 ns | 95.005 ns - 95.507 ns |
| revision_write_then_read | revision / 16 | 1.212 us | 1.211 us - 1.214 us |
| revision_write_then_read | revision / 128 | 13.397 us | 13.365 us - 13.432 us |
| revision_write_then_read | revision / 1024 | 108.451 us | 108.282 us - 108.621 us |
| typed_cache_reads | context_cell | 2.128 ns | 2.125 ns - 2.130 ns |
| typed_cache_reads | context_rc_cell | 5.548 ns | 5.531 ns - 5.564 ns |
| typed_cache_reads | context_rc_slot | 6.993 ns | 6.974 ns - 7.010 ns |
| typed_cache_reads | context_slot | 3.620 ns | 3.612 ns - 3.628 ns |
| typed_cache_reads | thread_safe_arc_slot | 63.993 ns | 63.781 ns - 64.234 ns |
| typed_cache_reads | thread_safe_arc_string_slot | 63.681 ns | 63.638 ns - 63.751 ns |
| typed_cache_reads | thread_safe_cell | 24.196 ns | 24.149 ns - 24.251 ns |
| typed_cache_reads | thread_safe_slot | 55.654 ns | 55.533 ns - 55.789 ns |
| typed_cache_reads | thread_safe_string_slot | 70.783 ns | 70.469 ns - 71.199 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 2.290 us | 19.680 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 80.000 ns | 512.824 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.890 us | 27.891 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 103 | 60.900 us | 34.880 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 161 | 360.082 us | 66.540 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 292 | 2.039 ms | 136.301 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 556 | 12.289 ms | 325.142 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.590 us | 14.960 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 105 | 39.490 us | 24.830 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 183 | 250.573 us | 51.420 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 348 | 1.564 ms | 110.791 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 665 | 7.042 ms | 236.980 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.520 us | 42.850 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 132 | 59.501 us | 67.691 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 219 | 398.933 us | 120.901 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 372 | 1.273 ms | 197.231 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 712 | 9.145 ms | 418.752 us | 0 | 0 | 0 | 1 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.070 us | 29.210 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 27 | 0 | 1 | 0 | 0 | 0 | 151 | 27.571 us | 53.861 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 59 | 0 | 1 | 0 | 0 | 0 | 373 | 101.810 us | 131.151 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 118 | 0 | 1 | 0 | 0 | 0 | 684 | 473.440 us | 293.531 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 225 | 0 | 1 | 0 | 0 | 0 | 1310 | 1.264 ms | 697.836 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.790 us | 21.820 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 138 | 39.640 us | 44.030 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 249 | 760.734 us | 113.841 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 490 | 3.037 ms | 213.012 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 937 | 16.863 ms | 495.505 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.010 us | 24.090 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 75 | 3.210 us | 24.430 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 95 | 9.270 us | 30.410 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 120 | 68.680 us | 48.070 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 120 | 8.680 us | 79.840 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.110 us | 54.740 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 21 | 0 | 8 | 0 | 0 | 0 | 190 | 64.920 us | 97.310 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 28 | 0 | 16 | 0 | 0 | 0 | 340 | 341.023 us | 183.512 us | 0 | 0 | 0 | 30 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 5 | 0 | 32 | 0 | 0 | 0 | 376 | 2.166 ms | 220.963 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 717 | 8.507 ms | 426.845 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 6 | 1 | 396 | 1.639 ms | 216.692 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 734 | 6.389 ms | 411.392 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 29 | 1 | 399 | 2.213 ms | 155.470 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 30 | 1 | 692 | 9.729 ms | 292.166 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 5 | 0 | 33 | 0 | 7 | 1 | 650 | 4.226 ms | 305.543 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 5 | 1 | 1247 | 18.242 ms | 573.644 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 556 | 0 | 64 | 0 | 49 | 1 | 1159 | 15.683 ms | 3.770 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 556 | 0 | 64 | 0 | 50 | 1 | 1419 | 73.002 ms | 6.860 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 514 | 19.222 ms | 3.157 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 773 | 78.472 ms | 6.261 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1437 | 9.649 ms | 556.722 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2802 | 40.245 ms | 1.108 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 101 | 0 | 65 | 0 | 15 | 1 | 1034 | 3.119 ms | 493.542 us | 0 | 0 | 0 | 175 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 150 | 0 | 129 | 0 | 11 | 1 | 1725 | 9.638 ms | 758.476 us | 0 | 0 | 0 | 185 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 50.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 512.634 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 920.000 ns | 1.511 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 660.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 870.000 ns | 25.190 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 67 | 25.100 us | 2.310 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 35.690 us | 31.880 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 93 | 179.282 us | 3.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 140.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 180.690 us | 62.470 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 160 | 856.974 us | 6.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.182 ms | 129.331 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 296 | 5.072 ms | 15.580 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 7.216 ms | 308.832 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 910.000 ns | 1.900 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 120.000 ns | 1.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 500.000 ns | 9.830 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 1.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 66 | 24.250 us | 2.850 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 100.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 40.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 15.040 us | 20.710 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 60.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 104 | 141.262 us | 3.900 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 230.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 100.000 ns | 1.450 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 108.881 us | 44.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 920.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 189 | 579.105 us | 7.800 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 450.000 ns | 1.100 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 200.000 ns | 2.840 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 983.857 us | 97.321 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 200.000 ns | 1.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 346 | 2.581 ms | 14.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 870.000 ns | 2.260 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 4.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 4.460 ms | 212.490 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 380.000 ns | 3.460 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 1.930 us | 15.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 1.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 410.000 ns | 26.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 108 | 58.871 us | 35.701 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 2.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 13 | 350.000 ns | 29.540 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 187 | 397.983 us | 76.251 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 410.000 ns | 4.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 13 | 460.000 ns | 39.440 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 332 | 1.272 ms | 165.651 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 140.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 890.000 ns | 11.250 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 140.000 ns | 19.960 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 20.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 644 | 9.144 ms | 374.742 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 140.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.660 us | 25.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1 | 150.000 ns | 18.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 990.000 ns | 2.430 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 120.000 ns | 990.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 1.270 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 470.000 ns | 11.490 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 460.000 ns | 13.030 us |
| thread_safe_contention_same_slot_write_read_2 | other | 67 | 15.130 us | 2.020 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 6 | 1.210 us | 1.950 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 220.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 9.351 us | 25.591 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 27 | 1.850 us | 24.080 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 127 | 51.700 us | 4.630 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 38 | 15.920 us | 7.220 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 1.050 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 24.470 us | 49.920 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 59 | 9.700 us | 68.331 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 84 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 253 | 183.680 us | 7.690 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 32 | 32.930 us | 6.410 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 460.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 221.550 us | 105.630 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 118 | 35.250 us | 173.341 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 152 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 471 | 619.898 us | 17.790 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 38 | 72.910 us | 8.550 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 1.290 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 419.372 us | 239.883 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 225 | 151.673 us | 430.323 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 319 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 900.000 ns | 1.200 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 270.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 620.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 380.000 ns | 9.830 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 400.000 ns | 9.900 us |
| thread_safe_contention_independent_slots_2 | other | 68 | 21.760 us | 2.170 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 120.000 ns | 310.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 420.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 10.830 us | 20.830 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 6.880 us | 20.300 us |
| thread_safe_contention_independent_slots_4 | other | 107 | 301.632 us | 5.500 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 250.000 ns | 850.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 1.410 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 237.570 us | 53.101 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 221.162 us | 52.980 us |
| thread_safe_contention_independent_slots_8 | other | 204 | 1.259 ms | 8.910 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 450.000 ns | 1.110 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 200.000 ns | 3.100 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.217 ms | 99.922 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 561.036 us | 99.970 us |
| thread_safe_contention_independent_slots_16 | other | 363 | 5.824 ms | 18.900 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 910.000 ns | 2.500 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 410.000 ns | 5.361 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 4.838 ms | 231.102 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.200 ms | 237.642 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 930.000 ns | 1.530 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 80.000 ns | 460.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 800.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 520.000 ns | 10.650 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 450.000 ns | 10.650 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 1.780 us | 1.020 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 4 | 430.000 ns | 550.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 500.000 ns | 11.490 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 470.000 ns | 11.160 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 2.860 us | 1.210 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 12 | 4.430 us | 1.720 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 20.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 1.010 us | 11.520 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 950.000 ns | 15.510 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 10.890 us | 1.730 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 20 | 55.860 us | 5.030 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 470.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 990.000 ns | 12.010 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 910.000 ns | 28.830 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 29 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 2.620 us | 1.510 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 12 | 4.730 us | 2.940 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 730.000 ns | 16.880 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 570.000 ns | 58.130 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 37 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.030 us | 13.690 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 260.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 1.890 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 420.000 ns | 25.110 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 450.000 ns | 13.790 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 61.480 us | 29.600 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 190.000 ns | 2.320 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 780.000 ns | 45.540 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 21 | 2.410 us | 19.640 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 218 | 319.093 us | 65.102 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 380.000 ns | 5.210 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 30 | 2.510 us | 76.660 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 28 | 18.990 us | 36.380 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 46 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 330 | 2.165 ms | 183.203 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 380.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 870.000 ns | 11.710 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 4 | 140.000 ns | 17.270 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 5 | 220.000 ns | 8.400 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 646 | 8.505 ms | 376.124 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 310.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.660 us | 26.440 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 2 | 50.000 ns | 15.310 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 3 | 80.000 ns | 8.661 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 359 | 1.638 ms | 191.562 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 880.000 ns | 9.470 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 5 | 120.000 ns | 15.660 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 667 | 6.387 ms | 372.472 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.740 us | 21.100 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 100.000 ns | 17.820 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 256 | 918.638 us | 33.070 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 390.000 ns | 5.670 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.294 ms | 116.730 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 4.921 ms | 65.150 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 810.000 ns | 9.270 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 4.807 ms | 217.746 us |
| thread_safe_effect_contention_batch_flush_8 | other | 606 | 4.225 ms | 261.873 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 560.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 850.000 ns | 13.710 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 4 | 250.000 ns | 18.960 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 5 | 250.000 ns | 10.440 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1175 | 18.240 ms | 515.034 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 330.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.680 us | 35.170 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 50.000 ns | 14.550 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 80.000 ns | 8.560 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 347 | 2.345 ms | 95.361 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.780 us | 5.030 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.670 us | 23.020 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 10.695 ms | 3.178 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 556 | 2.640 ms | 468.694 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 479 | 10.861 ms | 105.370 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.850 us | 4.520 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.760 us | 21.000 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 56.455 ms | 6.257 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 556 | 5.682 ms | 472.172 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 226 | 3.957 ms | 10.180 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.840 us | 5.120 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 890.000 ns | 12.570 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 15.261 ms | 3.097 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.960 us | 32.470 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 357 | 13.930 ms | 12.810 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.790 us | 4.550 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 850.000 ns | 11.660 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 64.538 ms | 6.200 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.700 us | 31.851 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 731 | 4.101 ms | 36.930 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 1.960 us | 8.090 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.770 us | 21.540 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 5.542 ms | 445.652 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.730 us | 44.510 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1392 | 17.251 ms | 70.550 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.810 us | 13.910 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.400 us | 45.460 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 22.984 ms | 893.655 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.330 us | 84.351 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 441 | 2.912 ms | 201.291 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 416 | 12.550 us | 48.901 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.660 us | 23.680 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 11 | 450.000 ns | 130.330 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 101 | 193.152 us | 89.340 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 815 | 9.470 ms | 407.103 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 620 | 23.100 us | 65.511 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.410 us | 53.680 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 11 | 112.321 us | 113.471 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 150 | 28.510 us | 118.711 us |

<!-- benchmark-results:end -->

## Scale (≥1M cells) — `#lzscalebench`

The `scale` group in the generated section above is a rigorous criterion benchmark
over a spreadsheet-shaped graph of `N` input cells + `N` formula slots
(`formula[i] = input[i] + input[i-1]`). At the default `N = 1_000_000` that is
~2,000,000 reactive nodes. It is gated behind the `scale-bench` feature so a plain
`cargo bench` skips it; the benchmark generator enables the feature so the group is
tracked by `make benchmark-check`. Run it directly, or at a larger size:

```bash
cargo bench --features scale-bench --bench scale
LAZILY_SCALE_N=2000000 cargo bench --features scale-bench --bench scale
```

What the four cases show at `N = 1_000_000` (reference machine below): `build`
constructs 2M nodes (~0.12 s), `cold_full_recalc` computes every formula from cold
(~0.105 s), `full_recalc_invalidate_all` re-edits every input and recomputes the
whole sheet (~0.080 s), and `viewport_recalc` edits one input and reads only a
1,000-cell viewport — **~3.7 µs**, ~21,000× cheaper than a full recalc because the
lazy pull-based model leaves off-viewport formulas dirty and never recomputes them
(the property a viewport-rendered spreadsheet needs).
(`build`/`cold_full_recalc`/`full_recalc_invalidate_all` are unaffected by the
v0.22.2 `#lzslotfastpath` refresh fast path — they are cold/slow-path — so their
figures are retained from the original run; only `viewport_recalc`, which is
~998/1000 cache-hit reads, moved, by the controlled A/B below. The generated
`scale` rows in the table above reflect the latest single criterion run on this
host and drift with host load for the allocation-heavy `build`/`cold` cases; the
curated baseline here is the reference.)

Memory (not captured by criterion): building 2,000,000 nodes uses ~414 MiB RSS, i.e.
~216 B/node, so 1M populated formula cells land in the low hundreds of MiB.

### Spreadsheet cell-count context

How the two dominant spreadsheets bound a sheet:

| Spreadsheet | Documented limit | Cells |
|---|---|---:|
| **Google Sheets** | 10,000,000 cells per workbook (also 18,278 columns max) | **10,000,000** |
| **Microsoft Excel** | 1,048,576 rows × 16,384 columns per worksheet | **17,179,869,184** |

**Google Sheets (10M cells) — measured.** Modeled as 5,000,000 input cells + 5,000,000
formula cells (= 10M cells) by running the bench at `LAZILY_SCALE_N=5000000`. Criterion
median on the cross-language reference machine (AMD Ryzen 9 9950X3D), pinned to one core
(`taskset -c 4`) and run serially so nothing contends for L3 / memory bandwidth:

| case | mean | per cell |
|---|---:|---:|
| `build` (10M cells) | ~718 ms | ~72 ns |
| `cold_full_recalc` (5M) | ~544 ms | ~109 ns |
| `full_recalc_invalidate_all` (5M) | ~398 ms | ~80 ns |
| `viewport_recalc` (1k) | ~3.8 µs | ~4 ns |

So lazily backs a **full-capacity Google Sheets workbook**: build under a second, full
recompute ~0.5 s, and — crucially — viewport recalc stays ~3.8 µs **independent of sheet
size** (it was ~3.7 µs at 1M too), because the lazy pull-based model only recomputes the
cells you read. Reproduce: `LAZILY_SCALE_N=5000000 cargo bench --features scale-bench --bench scale`.
Across the three implementations lazily-rs holds the **cheapest viewport reads** (3.7–3.8 µs);
see the cross-language table in lazily-zig's `BENCHMARKS.md` for the full head-to-head.

Controlled A/B isolating the v0.22.2 `#lzslotfastpath` refresh fast path on
`viewport_recalc` (`--save-baseline pre_fix`, same session, toggling only
`src/context.rs` between `8c64f33` and `1390a6e`): **13.78 µs → 4.49 µs,
−64.1% (p=0.00)** at `N = 1_000_000`. Only ~2 of the 1,000 viewport cells recompute; the
other ~998 are cache-hit slot reads, each now ~7 ns cheaper because `refresh_slot`
early-returns on a clean hit instead of cloning the dependency `Vec` and walking deps.

**Microsoft Excel (17.18B grid) — sparse, not dense.** Excel's
1,048,576 × 16,384 = 17,179,869,184 is the *grid capacity*, not a populated-cell count.
Building all 17.18B cells densely would need ~7 TB at ~216 B/node — infeasible and
unrepresentative: real sheets populate a tiny fraction of the grid, and lazily's storage
is a **sparse arena** (`Vec<Option<Node>>` with a free-list) that only allocates cells you
actually create. The practical limit is therefore *populated* cells vs. available RAM, not
the 17.18B grid. With the flat per-node cost above (~216 B, ~70–100 ns/cell), capacity ≈
available RAM ÷ ~216 B — e.g. this 186 GB host could hold on the order of ~10⁸–10⁹
populated cells, far beyond any realistically-populated Excel sheet. The `scale` group's
linear scaling (1M → 10M held ~constant per-cell cost) is the evidence that the model
extrapolates rather than degrading at spreadsheet capacity.

### Cross-library comparison — `#lzscalecompare`

Head-to-head against [`leptos_reactive`](https://crates.io/crates/leptos_reactive)
(Leptos 0.6's fine-grained reactivity) on the **identical** spreadsheet graph
(`N` input signals + `N` formula memos, `formula[i] = input[i] + input[i-1]`), in
the same criterion harness on the same host. `leptos_reactive` is the fair
apples-to-apples pick: like lazily it is a **lazy, pull-based memo** system (a memo
recomputes only when read while dirty), so this isolates per-node runtime overhead
and the lazy-pull viewport property rather than comparing a pull model against an
eager push one. (JS signal libraries — Solid, MobX, Preact Signals — are a
different runtime and are excluded; the standard js-reactivity-benchmark / cellx
harnesses also measure small/medium graphs, not a 100k-node sheet.)

Measured at `N = 100_000` (200,000 nodes/library; leptos is far heavier per node,
so this size keeps its wall clock feasible — lazily's own 1M/10M numbers are above):

| case | lazily | leptos_reactive | ratio |
|---|---:|---:|---|
| `build` (200k nodes) | **8.58 ms** | 12.89 ms | lazily **1.5×** faster |
| `cold_full_recalc` (100k formulas) | **8.45 ms** | 30.06 ms | lazily **3.6×** faster |
| `full_recalc_invalidate_all` (100k) | **6.26 ms** | 17.29 ms | lazily **2.8×** faster |
| `viewport_recalc` (edit 1, read 1k) | **~4.5 µs** † | 8.22 µs | lazily **~1.8×** faster |

† lazily's `viewport_recalc` is post-v0.22.2 (`#lzslotfastpath`). Before that refresh
fast path it measured **11.52 µs** and leptos led ~1.4× (the original row this table
shipped with). The v0.22.2 controlled A/B on this case is
**13.78 µs → 4.49 µs, −64.1% (p=0.00)** (`--save-baseline pre_fix`, toggling only
`src/context.rs`). leptos_reactive is an unchanged external library so its 8.22 µs is
retained from the original same-host run; a fresh same-session re-measure under load gave
~10.5 µs, i.e. lazily leads by ~1.8–2.3× depending on leptos's run-to-run variance.

**Honest read:** lazily now leads all four cases — building the sheet (1.5×), computing
it cold (3.6×), recomputing the whole sheet after a full invalidation (2.8×), and the
cached-read-dominated viewport read (~1.8×) — driven by its sparse arena + lean
single-threaded `Context` versus leptos's runtime slotmap and subscriber bookkeeping, plus
the v0.22.2 `refresh_slot` clean-cache-hit fast path that removed the per-read
dependency-walk tax on the ~998/1000 viewport cells that are cache hits. The fairness
evidence is no longer "leptos wins a case" (it did, before v0.22.2, and that historical
result is documented in the footnote above) — it is that leptos's genuine 30 ms cold
recalc proves its memos truly recompute (this is not a straw-man comparison), and that
lazily's viewport lead is a recent code improvement, not an inherent property: the
pre-v0.22.2 code lost this case. The shared headline is unchanged: the lazy-pull property
both exhibit — a one-input edit + bounded-viewport read is **microseconds**, ~1000×
cheaper than a full recalc, *independent of total sheet size* — neither library
recomputes off-viewport formulas. The defensible claim is now "lazily has materially
higher throughput than a comparable native-Rust pull-based reactive system across both
whole-graph and incremental-viewport workloads," **not** a blanket "fastest reactive
library."

Reproduce (gated behind the `scale-compare` feature so the comparison dependency is
never pulled into normal builds / `make check`):

```bash
cargo bench --features scale-compare --bench scale_compare
LAZILY_SCALE_N=250000 cargo bench --features scale-compare --bench scale_compare
```

## Cross-language comparison (lazily-rs / lazily-cpp / lazily-zig)

Head-to-head on the same spreadsheet-shaped workload (`N` input cells + `N`
formula slots, `formula[i] = input[i] + input[i-1]`), measured on `x86_64`
Linux. lazily-rs uses criterion; lazily-cpp uses its `std::chrono` harness;
lazily-zig uses `clock_gettime(.MONOTONIC)` for the scale bench. Numbers are
the current published results from each repo's `BENCHMARKS.md`.

### Micro-benchmarks (single-threaded `Context` unless noted)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| cached read (Context) | 5.7 ns | 23 ns | — † |
| cached read (ThreadSafeContext) | 68 ns | 22 ns | — † |
| cold first get (Context) | 129 ns | 97 ns | — † |
| cold first get (ThreadSafeContext) | 1.17 µs | 107 ns | — † |
| fan-out 256 (Context) | 58.4 µs | 1.12 µs | — † |
| fan-out 256 (ThreadSafeContext) | 182 µs | 1.68 µs | — |
| set_cell high_fan_out 512 | 139 µs | 3.26 µs | — † |
| memo equality suppression (Context) | 3.3 µs | 34 ns | — † |
| effect flushing (Context) | 90 ns | 87 ns | — |
| batch storms 64 (Context) | 3.1 µs | 1.55 µs | — |

† lazily-zig 0.17-dev removed `std.time.Timer`, so its reactive-core
micro-bench is **counter-based** (deterministic work-counts: allocations,
edges, recomputes — not wall-clock). The counters confirm the same zero-work
steady state (cached reads = 0 allocs / 0 recomputes) but are not directly
comparable on a wall-clock axis. See
[lazily-zig BENCHMARKS.md](https://github.com/lazily-hub/lazily-zig/blob/main/BENCHMARKS.md).

### Scale — 1M rows (~2M cells)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build (2N nodes) | 105 ms | 123 ms | 132 ms |
| cold full recalc | 106 ms | 36 ms | 381 ms |
| viewport recalc (edit 1, read 1k) | 4.5 µs | 35.1 µs | 6.4 µs |

### Scale — 10M cells (full Google Sheets workbook capacity)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build | 706 ms | 1.41 s | 1.13 s |
| cold full recalc | 518 ms | 415 ms | 2.26 s |
| viewport recalc | 4.1 µs | 43.8 µs | 6.6 µs |

**Honest read:** lazily-rs's monomorphized `Rc<T>` fast path leads the
spreadsheet-scale **build** wall clock (leanest per-node storage) and — after the
v0.22.2 `#lzslotfastpath` refresh fast path — delivers the **cheapest viewport
reads** of the three (4.5 µs @ 1M, 4.1 µs @ 10M, undercutting lazily-zig's
integer-keyed cache at 6.4/6.6 µs). lazily-cpp's v0.6.0 `SmallAny` inline value
storage (optimization B) + alloc-free batch bookkeeping (E) **flipped the
cold-recalc lead**: lazily-cpp cold full recalc is now ~3× faster than lazily-rs
at both 1M (36 vs 106 ms) and 10M (415 vs 518 ms), and its `batch_storms` now
edges out lazily-rs (1.55 vs 3.1 µs). lazily-cpp's type-erased `SmallFn` +
`SmallVec` node layout still wins the high-fan-out micro-benchmarks (fan-out 256,
set_cell 512, memo equality) by 16–49× over lazily-rs. The **shared headline**
across all three: they back a full-capacity Google Sheets workbook and all
exhibit the **lazy-pull viewport property** — a one-cell edit + bounded-viewport
read stays in the **microsecond** range, independent of sheet size, because
off-viewport formulas are left dirty and never recomputed (~2,000–60,000× cheaper
than a full recalc across the three runtimes).

## Phase 3 Wire-Format Optimizations (`#lzperfaudit`)

Three spec-ratified wire wins (`#lzspecfrontiersuppress`, `#lzspecbase64`,
`#lzspecintern`), measured by `benches/wire_optimizations.rs`. Run with:

```bash
cargo bench --features json-base64 --bench wire_optimizations
```

### `#lzspecfrontiersuppress` — optional CrdtSync frontier

Omitting the stamp frontier when unchanged cuts wire size and encode/decode cost:

| Variant | Wire size | Encode | Decode |
|---|---:|---:|---:|
| with frontier (8 peers) | 879 B | ~740 ns | ~1.6 µs |
| ops only (suppressed) | 514 B (**−42%**) | ~463 ns | ~1.0 µs |

### `#lzspecbase64` — base64 byte arrays vs JSON-u8 arrays

Under the `json-base64` capability flag, `Inline`/`Payload` bytes travel as base64
strings instead of JSON integer arrays:

| Payload | json-u8 wire | base64 wire | Savings | Decode (u8 → b64) |
|---:|---:|---:|---:|---|
| 64 B | 395 B | 228 B | **42%** | 911 ns → 710 ns |
| 1 KiB | 4,235 B | 1,508 B | **64%** | 36 µs → 25 µs |
| 16 KiB | 65,675 B | 21,988 B | **67%** | 89 µs → 65 µs |

### `#lzspecintern` — batch string-intern table

Deduplicating repeated `type_tag` strings into a sidecar intern table (256 nodes,
4 distinct tags):

| Variant | Wire size | Savings |
|---|---:|---:|
| inline tags | 15,729 B | — |
| interned | 14,890 B | **5%** |

Savings grow with the node-to-tag ratio (more nodes sharing fewer tags).

## Revision engine crossover (`#lzspecrevisionengine`)

The revision (pull) invalidation engine gives O(1) writes (no dependent cone
walk) at the cost of O(changed-subpath) reads. Observable values are provably
identical to push mode (`get_equiv_push`, lazily-formal `RevisionEngine.lean`).

Benchmark: 10 writes to a source cell with N dependent slots (construction +
priming included in each measurement). Run with:

```bash
cargo bench --bench revision_engine
```

| Fan-out | Push | Revision | Revision win |
|---:|---:|---:|---:|
| 1 | 194 ns | 127 ns | 1.5× |
| 16 | 1.19 µs | 822 ns | 1.4× |
| 128 | 10.9 µs | 8.75 µs | 1.25× |
| 1024 | 192 µs | 177 µs | 1.08× |

The write cost scales linearly with fan-out in push (O(N) dirty walk) but is
O(1) in revision (revision bump). The construction+priming overhead (same for
both) dilutes the pure write-cost gap; workloads with high write:read ratios
and large fan-out benefit most.

## Multi-Language

lazily is implemented across three languages with shared semantics:

| | [lazily-rs](https://crates.io/crates/lazily) | [lazily-zig](https://github.com/btakita/lazily-zig) | [lazily-py](https://github.com/btakita/lazily-py) |
|---|---|---|---|
| Context | Owned `Context` struct | Explicit allocator | Plain `dict` |
| Slot creation | `Box<dyn Fn>` closures | `comptime` function pointers | Lambdas |
| Cell equality | `PartialEq` trait | `std.meta.eql` | `!=` operator |
| Thread safety | Single-threaded `Context`; explicit `ThreadSafeContext` | Mutex by default | GIL |
| Storage | Unified generics | `.direct` / `.indirect` | Object identity |

## Related

- [lazily-zig](https://github.com/btakita/lazily-zig) — Zig implementation with FFI support
- [lazily-py](https://github.com/btakita/lazily-py) — Python implementation with context-as-dict
- [Blog post: Lazily — Reactive Primitives Done Right](https://briantakita.me/posts/lazily-reactive-signals)

## License

MIT
