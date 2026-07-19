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
| thread_safe_contention | same_slot_write_read / 8 | 2.434 ms | 3.092 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.005 ms | 7.971 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.845 ms | 2.090 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 3.691 ms | 5.437 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 509.642 us | 568.492 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.081 ms | 1.315 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.271 ms | 2.554 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.659 ms | 3.979 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.214 ms | 1.353 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.566 ms | 3.996 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.288 ms | 1.428 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.199 ms | 3.382 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.466 ms | 1.936 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 3.497 ms | 5.702 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.077 ms | 3.272 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.083 ms | 7.912 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.869 ms | 1.967 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.684 ms | 4.231 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.240 ms | 2.803 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 5.245 ms | 7.139 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.125 ms | 1.339 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.964 ms | 2.146 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 3.700 ns | 3.649 ns - 3.783 ns |
| cached_reads | thread_safe_context | 56.114 ns | 55.867 ns - 56.394 ns |
| cold_first_get | context | 93.125 ns | 81.721 ns - 104.373 ns |
| cold_first_get | thread_safe_context | 928.005 ns | 911.208 ns - 949.711 ns |
| dependency_fan_out | context / 32 | 3.549 us | 3.050 us - 4.104 us |
| dependency_fan_out | context / 256 | 23.143 us | 21.763 us - 24.482 us |
| dependency_fan_out | thread_safe_context / 32 | 20.490 us | 19.499 us - 21.827 us |
| dependency_fan_out | thread_safe_context / 256 | 144.808 us | 142.322 us - 147.562 us |
| set_cell_invalidation | high_fan_out / 512 | 103.216 us | 91.386 us - 114.396 us |
| set_cell_invalidation | same_slot_contention / 1 | 80.770 us | 79.084 us - 83.042 us |
| set_cell_invalidation | same_slot_contention / 2 | 167.793 us | 164.106 us - 172.067 us |
| set_cell_invalidation | same_slot_contention / 4 | 456.467 us | 436.652 us - 480.360 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.303 ms | 1.242 ms - 1.365 ms |
| set_cell_invalidation | same_slot_contention / 16 | 2.766 ms | 2.554 ms - 2.957 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 85.059 us | 84.359 us - 85.765 us |
| set_cell_invalidation | independent_slot_contention / 2 | 185.484 us | 175.503 us - 199.043 us |
| set_cell_invalidation | independent_slot_contention / 4 | 422.051 us | 403.476 us - 436.971 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.272 ms | 1.188 ms - 1.347 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 2.956 ms | 2.842 ms - 3.064 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 150.043 us | 145.296 us - 155.466 us |
| set_cell_invalidation | batched_write_bursts / 2 | 237.255 us | 232.324 us - 242.879 us |
| set_cell_invalidation | batched_write_bursts / 4 | 495.847 us | 475.871 us - 518.577 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.211 ms | 1.139 ms - 1.277 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.445 ms | 3.282 ms - 3.600 ms |
| memo_equality_suppression | context | 1.198 us | 1.093 us - 1.297 us |
| memo_equality_suppression | thread_safe_context | 27.960 us | 27.817 us - 28.113 us |
| effect_flushing | context | 35.875 ns | 34.313 ns - 37.725 ns |
| effect_flushing | thread_safe_context | 920.734 ns | 915.558 ns - 926.739 ns |
| batch_storms | context / 64 | 2.252 us | 2.144 us - 2.379 us |
| batch_storms | thread_safe_context / 64 | 7.547 us | 7.412 us - 7.701 us |
| thread_safe_contention | same_slot_write_read / 1 | 139.703 us | 137.916 us - 141.770 us |
| thread_safe_contention | same_slot_write_read / 2 | 439.840 us | 405.006 us - 491.939 us |
| thread_safe_contention | same_slot_write_read / 4 | 920.936 us | 860.272 us - 989.984 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.378 ms | 2.099 ms - 2.651 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.041 ms | 6.564 ms - 7.441 ms |
| thread_safe_contention | independent_slots / 1 | 132.648 us | 130.830 us - 134.270 us |
| thread_safe_contention | independent_slots / 2 | 263.521 us | 255.500 us - 272.264 us |
| thread_safe_contention | independent_slots / 4 | 695.526 us | 663.608 us - 727.552 us |
| thread_safe_contention | independent_slots / 8 | 1.850 ms | 1.759 ms - 1.938 ms |
| thread_safe_contention | independent_slots / 16 | 4.034 ms | 3.608 ms - 4.531 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 160.779 us | 148.281 us - 175.991 us |
| thread_safe_contention | read_mostly_waiters / 2 | 179.845 us | 167.298 us - 195.678 us |
| thread_safe_contention | read_mostly_waiters / 4 | 245.892 us | 238.572 us - 257.171 us |
| thread_safe_contention | read_mostly_waiters / 8 | 511.341 us | 486.251 us - 534.019 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.120 ms | 1.046 ms - 1.197 ms |
| thread_safe_contention | batched_write_bursts / 1 | 219.235 us | 214.225 us - 224.445 us |
| thread_safe_contention | batched_write_bursts / 2 | 543.382 us | 525.684 us - 559.275 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.356 ms | 1.329 ms - 1.382 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.326 ms | 2.263 ms - 2.400 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.652 ms | 3.494 ms - 3.798 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.221 ms | 1.157 ms - 1.283 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.512 ms | 3.336 ms - 3.676 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.304 ms | 1.246 ms - 1.361 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.415 ms | 2.161 ms - 2.760 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.511 ms | 1.435 ms - 1.618 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.159 ms | 3.633 ms - 4.728 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.080 ms | 3.021 ms - 3.140 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.497 ms | 5.038 ms - 6.136 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.876 ms | 1.859 ms - 1.900 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.718 ms | 3.621 ms - 3.850 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.353 ms | 2.226 ms - 2.493 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 5.747 ms | 5.220 ms - 6.312 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.135 ms | 1.066 ms - 1.206 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.972 ms | 1.899 ms - 2.046 ms |
| profile_instrumentation | context_snapshot | 222.321 ns | 221.914 ns - 222.844 ns |
| profile_instrumentation | thread_safe_snapshot | 296.491 us | 295.270 us - 297.569 us |
| async_cached_resolve | async_context | 3.990 us | 3.592 us - 4.515 us |
| async_cached_resolve | sync_context_baseline | 69.304 ns | 66.620 ns - 73.202 ns |
| async_cached_resolve | sync_get | 12.717 ns | 12.468 ns - 13.002 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.370 us | 1.350 us - 1.392 us |
| async_cold_resolve | async_context | 4.870 us | 4.261 us - 5.558 us |
| async_cold_resolve | sync_context_baseline | 108.871 ns | 89.765 ns - 135.165 ns |
| async_cold_resolve | thread_safe_context_baseline | 934.705 ns | 911.841 ns - 971.783 ns |
| async_invalidation_throughput | async_context | 246.467 us | 230.334 us - 265.978 us |
| async_invalidation_throughput | sync_context_baseline | 2.841 us | 2.720 us - 2.981 us |
| async_invalidation_throughput | thread_safe_context_baseline | 57.583 us | 56.875 us - 58.304 us |
| async_cancellation_throughput | async_invalidate_in_flight | 50.330 us | 40.874 us - 59.911 us |
| async_concurrent_contention | async_context / 1 | 75.023 us | 73.644 us - 76.485 us |
| async_concurrent_contention | async_context / 4 | 358.884 us | 316.924 us - 407.245 us |
| async_concurrent_contention | async_context / 16 | 1.854 ms | 1.671 ms - 2.012 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 78.549 us | 77.279 us - 79.704 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 636.575 us | 629.426 us - 643.010 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 3.644 ms | 3.115 ms - 4.058 ms |
| async_effect_throughput | async_context | 188.263 ms | 188.080 ms - 188.434 ms |
| async_batch_throughput | async_context | 69.109 us | 66.265 us - 72.597 us |
| async_batch_throughput | sync_context_baseline | 7.749 us | 7.431 us - 8.152 us |
| tokio_sync_cached_read | single_task | 1.437 us | 1.431 us - 1.444 us |
| tokio_sync_cached_read | spawn_read | 5.083 us | 4.782 us - 5.392 us |
| tokio_sync_cold_first_get | single_task | 1.375 us | 1.370 us - 1.381 us |
| tokio_sync_cold_first_get | spawn_compute | 5.411 us | 4.853 us - 6.032 us |
| tokio_sync_invalidation | single_task | 55.231 us | 54.970 us - 55.511 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 61.146 us | 59.807 us - 63.273 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 350.932 us | 340.554 us - 362.050 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.223 ms | 2.686 ms - 3.725 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 64.677 us | 63.441 us - 66.220 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 336.444 us | 301.065 us - 376.418 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 1.746 ms | 1.569 ms - 1.996 ms |
| tokio_sync_batch | spawn_batch | 48.793 us | 47.959 us - 49.935 us |
| tokio_sync_effect | single_task | 10.082 ms | 10.079 ms - 10.085 ms |
| scale | build | 106.496 ms | 105.195 ms - 108.198 ms |
| scale | cold_full_recalc | 57.612 ms | 55.107 ms - 61.081 ms |
| scale | full_recalc_invalidate_all | 58.845 ms | 56.588 ms - 61.286 ms |
| scale | viewport_recalc | 3.311 us | 3.205 us - 3.433 us |
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
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.140 ns | 1.117 ns - 1.171 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 93.492 ns | 92.140 ns - 95.583 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 16.258 ns | 16.229 ns - 16.285 ns |
| revision_write_cost | push / 1 | 247.389 ns | 242.336 ns - 252.634 ns |
| revision_write_cost | push / 16 | 1.581 us | 1.540 us - 1.628 us |
| revision_write_cost | push / 128 | 14.073 us | 13.911 us - 14.249 us |
| revision_write_cost | push / 1024 | 118.279 us | 116.892 us - 119.747 us |
| revision_write_cost | revision / 1 | 128.420 ns | 127.259 ns - 130.079 ns |
| revision_write_cost | revision / 16 | 1.283 us | 1.266 us - 1.301 us |
| revision_write_cost | revision / 128 | 12.428 us | 12.275 us - 12.592 us |
| revision_write_cost | revision / 1024 | 100.142 us | 99.374 us - 100.956 us |
| revision_write_then_read | push / 1 | 127.431 ns | 125.922 ns - 129.052 ns |
| revision_write_then_read | push / 16 | 1.785 us | 1.759 us - 1.815 us |
| revision_write_then_read | push / 128 | 18.511 us | 18.380 us - 18.648 us |
| revision_write_then_read | push / 1024 | 141.721 us | 141.106 us - 142.411 us |
| revision_write_then_read | revision / 1 | 104.082 ns | 102.972 ns - 105.291 ns |
| revision_write_then_read | revision / 16 | 1.722 us | 1.693 us - 1.755 us |
| revision_write_then_read | revision / 128 | 17.401 us | 17.316 us - 17.491 us |
| revision_write_then_read | revision / 1024 | 140.410 us | 139.777 us - 141.097 us |
| typed_cache_reads | context_cell | 2.237 ns | 2.150 ns - 2.372 ns |
| typed_cache_reads | context_rc_cell | 6.247 ns | 5.990 ns - 6.550 ns |
| typed_cache_reads | context_rc_slot | 8.318 ns | 7.949 ns - 8.716 ns |
| typed_cache_reads | context_slot | 3.637 ns | 3.628 ns - 3.646 ns |
| typed_cache_reads | thread_safe_arc_slot | 65.764 ns | 65.197 ns - 66.427 ns |
| typed_cache_reads | thread_safe_arc_string_slot | 66.455 ns | 66.133 ns - 66.799 ns |
| typed_cache_reads | thread_safe_cell | 25.730 ns | 25.316 ns - 26.219 ns |
| typed_cache_reads | thread_safe_slot | 57.192 ns | 56.669 ns - 57.769 ns |
| typed_cache_reads | thread_safe_string_slot | 76.420 ns | 74.298 ns - 78.823 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 2.890 us | 18.840 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 90.000 ns | 521.884 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.760 us | 25.490 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 98 | 59.151 us | 33.210 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 168 | 531.163 us | 101.251 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 301 | 3.182 ms | 198.243 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 549 | 7.456 ms | 224.532 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.470 us | 11.840 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 102 | 37.380 us | 24.290 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 179 | 254.112 us | 49.640 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 354 | 1.651 ms | 108.121 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 678 | 6.747 ms | 219.861 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.580 us | 41.741 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 135 | 69.291 us | 73.060 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 208 | 376.683 us | 135.131 us | 0 | 0 | 0 | 9 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 381 | 2.597 ms | 328.583 us | 0 | 0 | 0 | 8 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 9.813 ms | 450.682 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.030 us | 29.870 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 142 | 29.780 us | 52.511 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 46 | 0 | 1 | 0 | 0 | 0 | 307 | 182.901 us | 102.980 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 106 | 0 | 1 | 0 | 0 | 0 | 689 | 674.667 us | 279.023 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 218 | 0 | 1 | 0 | 0 | 0 | 1290 | 2.108 ms | 699.945 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.850 us | 22.811 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 137 | 37.180 us | 46.390 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 259 | 458.415 us | 96.491 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 490 | 2.575 ms | 204.273 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 947 | 19.610 ms | 531.056 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 1.970 us | 24.390 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 1.920 us | 24.200 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 82 | 4.070 us | 31.830 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 111 | 19.720 us | 46.830 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 110 | 9.090 us | 56.821 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.080 us | 53.741 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 192 | 63.642 us | 93.932 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 16 | 0 | 16 | 0 | 0 | 0 | 262 | 332.411 us | 141.051 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 71 | 0 | 32 | 0 | 0 | 0 | 765 | 926.582 us | 447.363 us | 0 | 0 | 0 | 70 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 7 | 0 | 64 | 0 | 0 | 0 | 741 | 5.424 ms | 381.672 us | 0 | 0 | 0 | 7 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 388 | 1.799 ms | 202.972 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 728 | 6.223 ms | 369.112 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 17 | 1 | 343 | 2.532 ms | 174.021 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 36 | 1 | 708 | 6.675 ms | 267.964 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 2.415 ms | 247.280 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 3 | 1 | 1251 | 10.995 ms | 484.194 us | 0 | 0 | 0 | 6 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 558 | 0 | 64 | 0 | 49 | 1 | 1161 | 16.189 ms | 3.716 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 563 | 0 | 64 | 0 | 49 | 1 | 1422 | 69.628 ms | 6.786 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 518 | 18.081 ms | 3.098 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 771 | 69.872 ms | 6.057 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1452 | 6.605 ms | 504.555 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2786 | 28.825 ms | 1.011 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 86 | 0 | 65 | 0 | 11 | 1 | 889 | 1.608 ms | 390.805 us | 0 | 0 | 0 | 117 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1313 | 10.930 ms | 671.547 us | 0 | 0 | 0 | 150 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 60.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 521.664 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 950.000 ns | 1.540 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 700.000 ns | 23.030 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 62 | 47.121 us | 2.180 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 11.920 us | 30.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 100 | 283.081 us | 5.521 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 247.962 us | 95.090 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 169 | 1.497 ms | 10.530 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 130.000 ns | 1.390 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.685 ms | 184.023 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 20.000 ns | 1.040 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 289 | 2.940 ms | 10.710 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 90.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 4.516 ms | 212.542 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 870.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 70.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 480.000 ns | 9.570 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 63 | 20.130 us | 2.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 17.020 us | 20.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 100 | 147.521 us | 3.660 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 230.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 110.000 ns | 1.320 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 106.131 us | 43.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 120.000 ns | 870.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 195 | 616.502 us | 7.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 430.000 ns | 1.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 220.000 ns | 2.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.034 ms | 95.371 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 230.000 ns | 1.720 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 359 | 2.623 ms | 14.720 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 880.000 ns | 2.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 4.180 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 4.122 ms | 195.291 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 440.000 ns | 3.420 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 1.970 us | 14.250 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 420.000 ns | 25.821 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 110 | 68.611 us | 34.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 2.490 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 370.000 ns | 35.500 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 180 | 375.593 us | 83.270 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 410.000 ns | 4.790 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 9 | 610.000 ns | 46.711 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 338 | 2.596 ms | 271.733 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 821.000 ns | 10.590 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 8 | 480.000 ns | 45.880 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 9.811 ms | 397.752 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.680 us | 24.440 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 90.000 ns | 28.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 220.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 920.000 ns | 1.790 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 110.000 ns | 790.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 760.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 530.000 ns | 13.350 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 440.000 ns | 13.180 us |
| thread_safe_contention_same_slot_write_read_2 | other | 66 | 21.830 us | 2.011 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 60.000 ns | 160.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 240.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 6.900 us | 24.770 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 23 | 970.000 ns | 25.330 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 115 | 97.601 us | 3.720 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 14 | 19.110 us | 5.710 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 59.100 us | 47.980 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 46 | 7.070 us | 45.320 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 67 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 233 | 264.041 us | 7.800 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 54 | 63.491 us | 15.370 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 240.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 286.585 us | 116.632 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 106 | 60.520 us | 138.981 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 167 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 472 | 757.611 us | 16.120 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 22 | 120.930 us | 4.750 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 20.000 ns | 240.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 1.042 ms | 238.031 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 218 | 187.551 us | 440.804 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 321 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 840.000 ns | 1.400 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 550.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 610.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 490.000 ns | 10.480 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 420.000 ns | 9.771 us |
| thread_safe_contention_independent_slots_2 | other | 67 | 13.280 us | 2.310 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 110.000 ns | 320.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 680.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 17.430 us | 22.400 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 6.310 us | 20.680 us |
| thread_safe_contention_independent_slots_4 | other | 117 | 145.520 us | 4.240 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 210.000 ns | 620.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 1.270 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 188.254 us | 45.791 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 124.311 us | 44.570 us |
| thread_safe_contention_independent_slots_8 | other | 204 | 981.451 us | 8.540 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 450.000 ns | 1.110 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 2.530 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 984.894 us | 96.881 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 607.865 us | 95.212 us |
| thread_safe_contention_independent_slots_16 | other | 373 | 7.248 ms | 20.630 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 890.000 ns | 2.340 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 460.000 ns | 5.440 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 5.697 ms | 244.982 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.664 ms | 257.664 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 900.000 ns | 1.370 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 50.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 560.000 ns | 12.140 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 440.000 ns | 10.340 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 880.000 ns | 1.300 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 240.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 520.000 ns | 11.080 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 440.000 ns | 11.430 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 1.180 us | 1.150 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 6 | 1.560 us | 1.630 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 230.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 820.000 ns | 11.310 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 480.000 ns | 17.510 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 6 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 4.140 us | 1.430 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 16 | 13.460 us | 5.280 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 240.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 660.000 ns | 12.330 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 1.430 us | 27.550 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 24 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 2.180 us | 1.180 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 12 | 4.660 us | 2.870 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 240.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 940.000 ns | 12.570 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 1.280 us | 39.961 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 27 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.040 us | 13.260 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 1.950 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 420.000 ns | 25.910 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 460.000 ns | 12.411 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 59.272 us | 28.041 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 2.670 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.360 us | 44.081 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 2.760 us | 18.980 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 191 | 313.281 us | 72.671 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 440.000 ns | 5.370 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 15 | 670.000 ns | 43.050 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 16 | 17.960 us | 19.810 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 462 | 836.801 us | 133.201 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 12 | 39.370 us | 6.430 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 800.000 ns | 11.360 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 70 | 6.091 us | 175.830 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 71 | 43.520 us | 120.542 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 118 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 656 | 5.422 ms | 317.851 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.690 us | 24.491 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 7 | 250.000 ns | 23.080 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 7 | 260.000 ns | 16.080 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 352 | 1.797 ms | 169.902 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 900.000 ns | 8.460 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 4 | 160.000 ns | 24.610 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 661 | 6.221 ms | 326.882 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.710 us | 18.410 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 130.000 ns | 23.820 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 200 | 1.144 ms | 31.120 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 480.000 ns | 7.890 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.388 ms | 135.011 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 421 | 2.916 ms | 65.991 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 810.000 ns | 11.060 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 3.758 ms | 190.913 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 2.414 ms | 219.810 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 860.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 890.000 ns | 11.420 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 30.000 ns | 10.650 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 60.000 ns | 4.540 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1174 | 10.987 ms | 424.184 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.660 us | 24.650 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 6 | 180.000 ns | 28.970 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 6.080 us | 6.190 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 347 | 1.094 ms | 99.912 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.770 us | 4.580 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.640 us | 21.160 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 12.777 ms | 3.145 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 558 | 2.314 ms | 445.012 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 475 | 5.115 ms | 99.541 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.770 us | 4.400 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.720 us | 21.010 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 58.801 ms | 6.202 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 563 | 5.709 ms | 459.551 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 230 | 3.795 ms | 10.440 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.830 us | 5.480 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 830.000 ns | 13.550 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 14.282 ms | 3.035 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.680 us | 33.981 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 355 | 11.732 ms | 12.330 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.810 us | 4.470 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 800.000 ns | 11.740 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 58.136 ms | 5.998 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.680 us | 31.070 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 746 | 2.535 ms | 29.752 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.040 us | 9.330 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.660 us | 20.630 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 4.065 ms | 394.753 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.730 us | 50.090 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1376 | 10.105 ms | 52.180 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.780 us | 13.310 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.320 us | 44.140 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 18.710 ms | 813.585 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.430 us | 87.990 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 429 | 1.577 ms | 176.543 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 300 | 8.400 us | 35.530 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.670 us | 20.801 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 9 | 4.770 us | 84.290 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 86 | 16.340 us | 73.641 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 792 | 10.893 ms | 418.656 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 9.040 us | 25.760 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.400 us | 42.870 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 130.000 ns | 93.731 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 24.511 us | 90.530 us |

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
