# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.46.0`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.694 ms | 3.299 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.550 ms | 8.606 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.710 ms | 2.226 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 5.198 ms | 6.154 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 504.791 us | 608.463 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.361 ms | 1.648 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.442 ms | 2.751 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 4.070 ms | 4.358 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.237 ms | 1.477 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.522 ms | 3.727 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.326 ms | 1.521 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.017 ms | 3.525 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.894 ms | 2.912 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 6.465 ms | 7.570 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 2.999 ms | 3.101 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 4.918 ms | 5.478 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.821 ms | 1.864 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.680 ms | 3.783 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.326 ms | 2.858 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.463 ms | 7.237 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.061 ms | 1.166 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.993 ms | 2.145 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 3.649 ns | 3.622 ns - 3.679 ns |
| cached_reads | thread_safe_context | 56.195 ns | 55.756 ns - 56.852 ns |
| cold_first_get | context | 98.570 ns | 84.527 ns - 112.357 ns |
| cold_first_get | thread_safe_context | 948.866 ns | 935.380 ns - 962.538 ns |
| dependency_fan_out | context / 32 | 3.016 us | 2.818 us - 3.215 us |
| dependency_fan_out | context / 256 | 19.135 us | 17.815 us - 20.474 us |
| dependency_fan_out | thread_safe_context / 32 | 18.877 us | 18.543 us - 19.258 us |
| dependency_fan_out | thread_safe_context / 256 | 146.886 us | 143.217 us - 151.311 us |
| set_cell_invalidation | high_fan_out / 512 | 103.960 us | 92.680 us - 115.119 us |
| set_cell_invalidation | same_slot_contention / 1 | 81.113 us | 79.654 us - 82.709 us |
| set_cell_invalidation | same_slot_contention / 2 | 176.692 us | 172.033 us - 181.990 us |
| set_cell_invalidation | same_slot_contention / 4 | 463.850 us | 445.782 us - 483.520 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.287 ms | 1.238 ms - 1.334 ms |
| set_cell_invalidation | same_slot_contention / 16 | 3.513 ms | 3.342 ms - 3.681 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 80.242 us | 79.443 us - 81.077 us |
| set_cell_invalidation | independent_slot_contention / 2 | 165.904 us | 161.829 us - 170.146 us |
| set_cell_invalidation | independent_slot_contention / 4 | 435.892 us | 420.901 us - 452.834 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.969 ms | 1.772 ms - 2.147 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 3.659 ms | 3.329 ms - 3.966 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 140.807 us | 137.381 us - 143.968 us |
| set_cell_invalidation | batched_write_bursts / 2 | 162.865 us | 155.870 us - 170.660 us |
| set_cell_invalidation | batched_write_bursts / 4 | 486.830 us | 473.216 us - 500.658 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.178 ms | 1.131 ms - 1.226 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.954 ms | 2.733 ms - 3.200 ms |
| memo_equality_suppression | context | 1.180 us | 1.088 us - 1.268 us |
| memo_equality_suppression | thread_safe_context | 28.165 us | 27.941 us - 28.411 us |
| effect_flushing | context | 33.967 ns | 33.903 ns - 34.036 ns |
| effect_flushing | thread_safe_context | 900.347 ns | 898.313 ns - 902.351 ns |
| batch_storms | context / 64 | 2.116 us | 2.106 us - 2.127 us |
| batch_storms | thread_safe_context / 64 | 7.288 us | 7.263 us - 7.313 us |
| thread_safe_contention | same_slot_write_read / 1 | 137.082 us | 135.350 us - 138.685 us |
| thread_safe_contention | same_slot_write_read / 2 | 370.699 us | 360.110 us - 380.854 us |
| thread_safe_contention | same_slot_write_read / 4 | 931.229 us | 846.290 us - 1.022 ms |
| thread_safe_contention | same_slot_write_read / 8 | 2.745 ms | 2.562 ms - 2.938 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.412 ms | 6.966 ms - 7.822 ms |
| thread_safe_contention | independent_slots / 1 | 135.406 us | 134.201 us - 136.444 us |
| thread_safe_contention | independent_slots / 2 | 259.394 us | 248.708 us - 271.202 us |
| thread_safe_contention | independent_slots / 4 | 799.386 us | 757.831 us - 847.111 us |
| thread_safe_contention | independent_slots / 8 | 1.819 ms | 1.698 ms - 1.954 ms |
| thread_safe_contention | independent_slots / 16 | 5.400 ms | 5.062 ms - 5.723 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 140.634 us | 135.999 us - 147.336 us |
| thread_safe_contention | read_mostly_waiters / 2 | 163.614 us | 159.424 us - 168.658 us |
| thread_safe_contention | read_mostly_waiters / 4 | 237.615 us | 233.269 us - 244.023 us |
| thread_safe_contention | read_mostly_waiters / 8 | 520.921 us | 501.145 us - 545.368 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.430 ms | 1.332 ms - 1.525 ms |
| thread_safe_contention | batched_write_bursts / 1 | 210.568 us | 207.302 us - 213.628 us |
| thread_safe_contention | batched_write_bursts / 2 | 512.359 us | 502.522 us - 522.670 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.309 ms | 1.294 ms - 1.325 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.484 ms | 2.387 ms - 2.584 ms |
| thread_safe_contention | batched_write_bursts / 16 | 4.036 ms | 3.894 ms - 4.158 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.244 ms | 1.170 ms - 1.321 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.349 ms | 3.070 ms - 3.587 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.326 ms | 1.263 ms - 1.391 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.993 ms | 2.772 ms - 3.203 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.072 ms | 1.802 ms - 2.370 ms |
| thread_safe_effect_contention | batch_flush / 16 | 6.546 ms | 6.016 ms - 7.043 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.017 ms | 2.990 ms - 3.045 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 4.981 ms | 4.870 ms - 5.116 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.815 ms | 1.787 ms - 1.840 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.676 ms | 3.625 ms - 3.722 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.367 ms | 2.207 ms - 2.530 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.476 ms | 6.146 ms - 6.796 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.083 ms | 1.047 ms - 1.119 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.990 ms | 1.927 ms - 2.053 ms |
| profile_instrumentation | context_snapshot | 239.850 ns | 238.345 ns - 241.450 ns |
| profile_instrumentation | thread_safe_snapshot | 293.508 us | 291.334 us - 295.458 us |
| async_cached_resolve | async_context | 5.213 us | 4.741 us - 5.707 us |
| async_cached_resolve | sync_context_baseline | 59.202 ns | 58.807 ns - 59.652 ns |
| async_cached_resolve | sync_get | 11.282 ns | 11.254 ns - 11.312 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.315 us | 1.311 us - 1.319 us |
| async_cold_resolve | async_context | 5.193 us | 4.698 us - 5.727 us |
| async_cold_resolve | sync_context_baseline | 97.572 ns | 85.806 ns - 108.592 ns |
| async_cold_resolve | thread_safe_context_baseline | 914.774 ns | 907.165 ns - 922.335 ns |
| async_invalidation_throughput | async_context | 309.246 us | 275.429 us - 345.982 us |
| async_invalidation_throughput | sync_context_baseline | 3.721 us | 3.503 us - 3.935 us |
| async_invalidation_throughput | thread_safe_context_baseline | 64.115 us | 62.706 us - 65.486 us |
| async_cancellation_throughput | async_invalidate_in_flight | 56.838 us | 45.428 us - 68.750 us |
| async_concurrent_contention | async_context / 1 | 69.290 us | 66.099 us - 73.954 us |
| async_concurrent_contention | async_context / 4 | 386.350 us | 357.608 us - 419.418 us |
| async_concurrent_contention | async_context / 16 | 1.831 ms | 1.655 ms - 2.008 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 80.488 us | 80.245 us - 80.757 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 684.344 us | 650.909 us - 710.840 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 4.005 ms | 3.885 ms - 4.130 ms |
| async_effect_throughput | async_context | 188.323 ms | 188.194 ms - 188.441 ms |
| async_batch_throughput | async_context | 94.848 us | 87.358 us - 102.706 us |
| async_batch_throughput | sync_context_baseline | 7.618 us | 7.565 us - 7.676 us |
| tokio_sync_cached_read | single_task | 1.437 us | 1.430 us - 1.444 us |
| tokio_sync_cached_read | spawn_read | 7.603 us | 6.894 us - 8.320 us |
| tokio_sync_cold_first_get | single_task | 1.667 us | 1.630 us - 1.700 us |
| tokio_sync_cold_first_get | spawn_compute | 5.685 us | 5.180 us - 6.219 us |
| tokio_sync_invalidation | single_task | 55.471 us | 55.273 us - 55.658 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 59.899 us | 59.068 us - 60.974 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 460.118 us | 414.776 us - 511.336 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.869 ms | 3.766 ms - 3.968 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 62.546 us | 60.866 us - 64.177 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 392.911 us | 362.862 us - 423.483 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 3.043 ms | 2.876 ms - 3.183 ms |
| tokio_sync_batch | spawn_batch | 47.020 us | 46.797 us - 47.264 us |
| tokio_sync_effect | single_task | 10.089 ms | 10.086 ms - 10.092 ms |
| scale | build | 104.972 ms | 104.379 ms - 105.593 ms |
| scale | cold_full_recalc | 55.276 ms | 54.272 ms - 56.506 ms |
| scale | full_recalc_invalidate_all | 56.115 ms | 55.267 ms - 57.104 ms |
| scale | viewport_recalc | 3.133 us | 3.126 us - 3.141 us |
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
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.123 ns | 1.115 ns - 1.136 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 101.239 ns | 96.095 ns - 110.293 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 15.969 ns | 15.877 ns - 16.066 ns |
| revision_write_cost | push / 1 | 240.400 ns | 235.207 ns - 245.855 ns |
| revision_write_cost | push / 16 | 1.084 us | 1.081 us - 1.088 us |
| revision_write_cost | push / 128 | 10.474 us | 10.451 us - 10.498 us |
| revision_write_cost | push / 1024 | 85.207 us | 85.044 us - 85.375 us |
| revision_write_cost | revision / 1 | 125.206 ns | 124.115 ns - 126.807 ns |
| revision_write_cost | revision / 16 | 808.759 ns | 807.220 ns - 810.318 ns |
| revision_write_cost | revision / 128 | 8.664 us | 8.632 us - 8.698 us |
| revision_write_cost | revision / 1024 | 71.444 us | 71.250 us - 71.650 us |
| revision_write_then_read | push / 1 | 115.071 ns | 113.856 ns - 116.334 ns |
| revision_write_then_read | push / 16 | 1.343 us | 1.341 us - 1.346 us |
| revision_write_then_read | push / 128 | 14.113 us | 14.078 us - 14.152 us |
| revision_write_then_read | push / 1024 | 113.727 us | 112.730 us - 115.180 us |
| revision_write_then_read | revision / 1 | 99.179 ns | 98.491 ns - 100.183 ns |
| revision_write_then_read | revision / 16 | 1.291 us | 1.289 us - 1.294 us |
| revision_write_then_read | revision / 128 | 13.658 us | 13.622 us - 13.697 us |
| revision_write_then_read | revision / 1024 | 110.777 us | 110.522 us - 111.059 us |
| typed_cache_reads | context_cell | 3.388 ns | 3.049 ns - 3.713 ns |
| typed_cache_reads | context_rc_cell | 5.759 ns | 5.725 ns - 5.799 ns |
| typed_cache_reads | context_rc_slot | 7.103 ns | 7.088 ns - 7.118 ns |
| typed_cache_reads | context_slot | 3.683 ns | 3.646 ns - 3.746 ns |
| typed_cache_reads | thread_safe_arc_slot | 64.597 ns | 64.355 ns - 64.888 ns |
| typed_cache_reads | thread_safe_arc_string_slot | 63.908 ns | 63.804 ns - 64.022 ns |
| typed_cache_reads | thread_safe_cell | 24.331 ns | 24.249 ns - 24.430 ns |
| typed_cache_reads | thread_safe_slot | 65.903 ns | 64.591 ns - 67.128 ns |
| typed_cache_reads | thread_safe_string_slot | 71.060 ns | 70.834 ns - 71.326 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.570 us | 18.550 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 90.000 ns | 512.734 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.580 us | 20.410 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 99 | 108.091 us | 54.680 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 160 | 597.543 us | 80.200 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 295 | 2.933 ms | 175.031 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 559 | 10.182 ms | 260.352 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.440 us | 11.670 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 103 | 31.411 us | 24.030 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 183 | 439.105 us | 71.062 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 347 | 2.161 ms | 133.340 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 677 | 12.311 ms | 315.812 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.640 us | 46.431 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 129 | 84.171 us | 71.191 us | 0 | 0 | 0 | 12 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 196 | 415.485 us | 125.501 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 369 | 1.621 ms | 196.861 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 727 | 5.407 ms | 376.083 us | 0 | 0 | 0 | 6 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.020 us | 33.380 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 31 | 0 | 1 | 0 | 0 | 0 | 168 | 81.420 us | 109.012 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 58 | 0 | 1 | 0 | 0 | 0 | 338 | 276.621 us | 203.790 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 114 | 0 | 1 | 0 | 0 | 0 | 653 | 951.446 us | 369.562 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 224 | 0 | 1 | 0 | 0 | 0 | 1324 | 1.225 ms | 680.424 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.860 us | 21.380 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 135 | 40.040 us | 44.700 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 259 | 492.803 us | 99.401 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 493 | 2.756 ms | 212.512 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 933 | 12.547 ms | 426.292 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.140 us | 22.900 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 2.090 us | 23.041 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 74 | 2.110 us | 23.660 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 95 | 51.090 us | 34.690 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 158 | 382.806 us | 115.430 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.010 us | 54.390 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 196 | 75.821 us | 110.112 us | 0 | 0 | 0 | 23 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 29 | 0 | 16 | 0 | 0 | 0 | 339 | 448.936 us | 219.051 us | 0 | 0 | 0 | 30 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 6 | 0 | 32 | 0 | 0 | 0 | 378 | 1.287 ms | 199.401 us | 0 | 0 | 0 | 5 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 6 | 0 | 64 | 0 | 0 | 0 | 734 | 6.264 ms | 397.412 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 382 | 1.988 ms | 215.621 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 7 | 1 | 760 | 9.443 ms | 478.272 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 31 | 1 | 408 | 1.546 ms | 148.941 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 32 | 1 | 700 | 9.096 ms | 280.560 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 5 | 0 | 33 | 0 | 5 | 1 | 647 | 4.097 ms | 297.600 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 5 | 1 | 1256 | 18.415 ms | 599.656 us | 0 | 0 | 0 | 6 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 558 | 0 | 64 | 0 | 50 | 1 | 1165 | 17.658 ms | 3.772 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 560 | 0 | 64 | 0 | 49 | 1 | 1419 | 81.259 ms | 7.229 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 510 | 20.936 ms | 3.247 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 782 | 88.764 ms | 6.363 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1462 | 10.388 ms | 596.123 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2824 | 42.514 ms | 1.143 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 601 | 2.624 ms | 345.572 us | 0 | 0 | 0 | 65 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1311 | 9.438 ms | 658.326 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 60.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 512.504 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 910.000 ns | 1.460 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 40.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 570.000 ns | 18.130 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 63 | 60.340 us | 2.970 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 47.641 us | 51.050 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 92 | 273.900 us | 4.410 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 323.533 us | 75.170 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 163 | 1.204 ms | 9.240 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.728 ms | 165.131 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 299 | 4.086 ms | 14.360 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 130.000 ns | 1.270 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.270 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 6.097 ms | 242.472 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 980.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 870.000 ns | 1.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 70.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 440.000 ns | 9.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 64 | 12.951 us | 2.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 100.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 50.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 18.270 us | 20.610 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 40.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 104 | 299.653 us | 5.960 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 310.000 ns | 1.430 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 100.000 ns | 1.990 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 138.952 us | 60.132 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 90.000 ns | 1.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 188 | 978.416 us | 9.500 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 430.000 ns | 1.320 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 200.000 ns | 2.430 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.182 ms | 118.600 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 190.000 ns | 1.490 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 358 | 6.204 ms | 21.191 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 890.000 ns | 2.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 4.030 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 6.106 ms | 285.201 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 380.000 ns | 2.930 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.000 us | 15.250 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 1.690 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 420.000 ns | 28.301 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 106 | 82.891 us | 35.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 2.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 12 | 1.000 us | 33.551 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 172 | 414.845 us | 94.661 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 390.000 ns | 4.450 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 180.000 ns | 26.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 330 | 1.620 ms | 163.681 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 10.450 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 4 | 130.000 ns | 22.170 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 654 | 5.405 ms | 330.363 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.660 us | 23.790 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 6 | 210.000 ns | 21.410 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 910.000 ns | 2.110 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 60.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 20.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 580.000 ns | 17.810 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 450.000 ns | 12.750 us |
| thread_safe_contention_same_slot_write_read_2 | other | 67 | 36.330 us | 2.980 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 14 | 10.360 us | 4.500 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 210.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 8.450 us | 38.582 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 31 | 26.260 us | 62.740 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 23 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 125 | 102.360 us | 4.890 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 16 | 70.690 us | 5.700 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 220.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 60.061 us | 69.310 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 58 | 43.490 us | 123.670 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 74 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 240 | 382.802 us | 8.840 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 16 | 95.191 us | 12.720 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 300.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 344.432 us | 123.640 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 114 | 129.001 us | 224.062 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 154 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 490 | 427.660 us | 18.180 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 36 | 81.970 us | 14.850 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 569.041 us | 242.233 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 224 | 146.560 us | 404.771 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 317 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 880.000 ns | 1.230 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 320.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 500.000 ns | 10.100 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 390.000 ns | 9.320 us |
| thread_safe_contention_independent_slots_2 | other | 65 | 19.640 us | 2.500 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 110.000 ns | 320.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 550.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 12.930 us | 22.370 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 7.300 us | 18.960 us |
| thread_safe_contention_independent_slots_4 | other | 117 | 187.620 us | 4.640 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 220.000 ns | 620.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.270 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 178.171 us | 48.400 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 126.682 us | 44.471 us |
| thread_safe_contention_independent_slots_8 | other | 207 | 1.144 ms | 9.460 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 450.000 ns | 1.240 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 3.080 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 924.727 us | 101.621 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 686.827 us | 97.111 us |
| thread_safe_contention_independent_slots_16 | other | 359 | 4.717 ms | 15.100 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 900.000 ns | 2.480 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 440.000 ns | 5.220 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 4.038 ms | 205.471 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 3.791 ms | 198.021 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 1.010 us | 1.530 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 50.000 ns | 230.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 570.000 ns | 11.100 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 480.000 ns | 9.710 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 1.000 us | 1.010 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 250.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 580.000 ns | 10.950 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 440.000 ns | 10.681 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 990.000 ns | 1.030 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 2 | 40.000 ns | 140.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 230.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 590.000 ns | 11.190 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 460.000 ns | 11.070 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 7.080 us | 1.490 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 29.830 us | 2.210 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 660.000 ns | 12.370 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 13.490 us | 18.410 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 18.431 us | 3.290 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 41 | 326.665 us | 43.020 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 1.570 us | 16.140 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 36.120 us | 52.710 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 46 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.010 us | 14.650 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 50.000 ns | 220.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 90.000 ns | 1.380 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 430.000 ns | 26.100 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 430.000 ns | 12.040 us |
| thread_safe_contention_batched_write_bursts_2 | other | 128 | 70.791 us | 30.061 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 2.400 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 23 | 2.850 us | 54.090 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 1.920 us | 23.381 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 217 | 410.156 us | 82.320 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 14.280 us | 1.330 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 420.000 ns | 6.430 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 30 | 8.620 us | 89.461 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 29 | 15.460 us | 39.510 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 43 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 332 | 1.285 ms | 157.490 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 840.000 ns | 11.380 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 5 | 130.000 ns | 19.391 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 6 | 290.000 ns | 10.930 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 651 | 6.261 ms | 330.922 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.660 us | 26.020 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 5 | 640.000 ns | 23.720 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 6 | 190.000 ns | 16.550 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 6 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 347 | 1.987 ms | 181.481 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 800.000 ns | 9.100 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 3 | 170.000 ns | 25.040 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 687 | 9.441 ms | 424.502 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.690 us | 19.600 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 9 | 320.000 ns | 34.170 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 265 | 643.148 us | 34.820 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 440.000 ns | 6.310 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 902.385 us | 107.811 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 4.449 ms | 58.370 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 770.000 ns | 10.250 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 4.647 ms | 211.940 us |
| thread_safe_effect_contention_batch_flush_8 | other | 602 | 4.096 ms | 256.250 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 50.000 ns | 300.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 890.000 ns | 12.170 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 5 | 190.000 ns | 22.500 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 5 | 200.000 ns | 6.380 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1179 | 18.408 ms | 538.345 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 310.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.690 us | 26.581 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 6 | 5.720 us | 24.920 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 190.000 ns | 9.500 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 351 | 1.309 ms | 103.111 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.670 us | 5.130 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.710 us | 20.891 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 13.519 ms | 3.226 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 558 | 2.826 ms | 417.193 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 475 | 10.357 ms | 100.410 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.740 us | 5.210 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.660 us | 21.530 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 64.295 ms | 6.662 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 560 | 6.603 ms | 439.973 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 222 | 4.993 ms | 12.360 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.880 us | 5.830 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 840.000 ns | 13.370 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 15.937 ms | 3.184 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.211 us | 30.880 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 366 | 13.444 ms | 15.150 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.860 us | 5.391 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 860.000 ns | 12.190 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 75.315 ms | 6.299 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.710 us | 31.050 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 756 | 4.697 ms | 41.160 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.010 us | 10.740 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.630 us | 21.340 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 5.686 ms | 478.283 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.820 us | 44.600 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1414 | 19.052 ms | 74.580 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.740 us | 14.830 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.340 us | 44.770 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 23.451 ms | 933.568 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.450 us | 74.831 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 401 | 2.619 ms | 227.711 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 1.940 us | 8.550 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.740 us | 21.330 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 1 | 40.000 ns | 48.320 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 1.730 us | 39.661 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 791 | 9.424 ms | 402.813 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 6.990 us | 28.040 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.390 us | 50.081 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 60.000 ns | 94.451 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 3.520 us | 82.941 us |

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
