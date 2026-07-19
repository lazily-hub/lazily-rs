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
| thread_safe_contention | same_slot_write_read / 8 | 2.761 ms | 3.068 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.294 ms | 8.291 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.673 ms | 2.417 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 5.402 ms | 6.319 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 582.596 us | 635.579 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.245 ms | 1.472 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.513 ms | 2.750 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.185 ms | 3.924 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.108 ms | 1.541 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.811 ms | 3.694 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.244 ms | 1.449 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.722 ms | 3.934 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.791 ms | 2.128 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 6.735 ms | 7.777 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 2.997 ms | 3.076 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 4.732 ms | 4.898 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.791 ms | 1.878 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.568 ms | 3.687 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.230 ms | 3.922 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 7.716 ms | 8.549 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.040 ms | 1.256 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.247 ms | 2.494 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 3.604 ns | 3.601 ns - 3.608 ns |
| cached_reads | thread_safe_context | 55.628 ns | 55.476 ns - 55.823 ns |
| cold_first_get | context | 88.941 ns | 79.433 ns - 97.935 ns |
| cold_first_get | thread_safe_context | 921.156 ns | 909.757 ns - 937.316 ns |
| dependency_fan_out | context / 32 | 2.320 us | 2.184 us - 2.451 us |
| dependency_fan_out | context / 256 | 17.689 us | 16.826 us - 18.545 us |
| dependency_fan_out | thread_safe_context / 32 | 18.347 us | 18.204 us - 18.510 us |
| dependency_fan_out | thread_safe_context / 256 | 143.021 us | 140.412 us - 146.248 us |
| set_cell_invalidation | high_fan_out / 512 | 94.618 us | 84.723 us - 104.967 us |
| set_cell_invalidation | same_slot_contention / 1 | 78.990 us | 77.130 us - 80.894 us |
| set_cell_invalidation | same_slot_contention / 2 | 164.253 us | 161.201 us - 167.134 us |
| set_cell_invalidation | same_slot_contention / 4 | 457.743 us | 434.680 us - 479.716 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.493 ms | 1.302 ms - 1.696 ms |
| set_cell_invalidation | same_slot_contention / 16 | 3.393 ms | 3.096 ms - 3.695 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 77.766 us | 75.416 us - 79.774 us |
| set_cell_invalidation | independent_slot_contention / 2 | 157.508 us | 153.168 us - 162.613 us |
| set_cell_invalidation | independent_slot_contention / 4 | 447.213 us | 424.479 us - 468.504 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.178 ms | 1.137 ms - 1.212 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 3.402 ms | 3.026 ms - 3.698 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 139.575 us | 136.818 us - 142.182 us |
| set_cell_invalidation | batched_write_bursts / 2 | 237.252 us | 228.417 us - 248.608 us |
| set_cell_invalidation | batched_write_bursts / 4 | 474.380 us | 443.119 us - 507.183 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.019 ms | 951.012 us - 1.102 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.360 ms | 3.157 ms - 3.551 ms |
| memo_equality_suppression | context | 1.175 us | 1.080 us - 1.266 us |
| memo_equality_suppression | thread_safe_context | 27.883 us | 27.646 us - 28.171 us |
| effect_flushing | context | 30.993 ns | 30.698 ns - 31.314 ns |
| effect_flushing | thread_safe_context | 881.187 ns | 880.052 ns - 882.477 ns |
| batch_storms | context / 64 | 2.033 us | 2.023 us - 2.048 us |
| batch_storms | thread_safe_context / 64 | 7.221 us | 7.201 us - 7.242 us |
| thread_safe_contention | same_slot_write_read / 1 | 134.695 us | 132.626 us - 136.207 us |
| thread_safe_contention | same_slot_write_read / 2 | 358.585 us | 341.350 us - 378.560 us |
| thread_safe_contention | same_slot_write_read / 4 | 881.259 us | 795.088 us - 960.852 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.700 ms | 2.494 ms - 2.875 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.138 ms | 6.423 ms - 7.783 ms |
| thread_safe_contention | independent_slots / 1 | 132.993 us | 130.586 us - 135.033 us |
| thread_safe_contention | independent_slots / 2 | 238.691 us | 232.919 us - 245.118 us |
| thread_safe_contention | independent_slots / 4 | 711.645 us | 661.829 us - 765.131 us |
| thread_safe_contention | independent_slots / 8 | 1.825 ms | 1.680 ms - 2.001 ms |
| thread_safe_contention | independent_slots / 16 | 5.322 ms | 4.805 ms - 5.777 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 129.842 us | 127.321 us - 132.440 us |
| thread_safe_contention | read_mostly_waiters / 2 | 152.319 us | 149.528 us - 155.103 us |
| thread_safe_contention | read_mostly_waiters / 4 | 232.276 us | 231.473 us - 233.040 us |
| thread_safe_contention | read_mostly_waiters / 8 | 571.151 us | 538.431 us - 600.210 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.277 ms | 1.200 ms - 1.355 ms |
| thread_safe_contention | batched_write_bursts / 1 | 210.877 us | 207.492 us - 214.013 us |
| thread_safe_contention | batched_write_bursts / 2 | 524.622 us | 484.363 us - 579.706 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.359 ms | 1.343 ms - 1.373 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.489 ms | 2.391 ms - 2.584 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.355 ms | 3.088 ms - 3.620 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.178 ms | 1.081 ms - 1.291 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.901 ms | 2.580 ms - 3.223 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.268 ms | 1.207 ms - 1.333 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.476 ms | 3.181 ms - 3.739 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.789 ms | 1.636 ms - 1.944 ms |
| thread_safe_effect_contention | batch_flush / 16 | 6.738 ms | 6.168 ms - 7.275 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 2.998 ms | 2.972 ms - 3.024 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 4.740 ms | 4.694 ms - 4.789 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.802 ms | 1.773 ms - 1.830 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.557 ms | 3.508 ms - 3.603 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.668 ms | 2.320 ms - 3.059 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 7.692 ms | 7.403 ms - 7.961 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.053 ms | 1.000 ms - 1.112 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.238 ms | 2.118 ms - 2.349 ms |
| profile_instrumentation | context_snapshot | 222.867 ns | 222.354 ns - 223.345 ns |
| profile_instrumentation | thread_safe_snapshot | 294.459 us | 293.423 us - 295.584 us |
| async_cached_resolve | async_context | 5.066 us | 4.580 us - 5.590 us |
| async_cached_resolve | sync_context_baseline | 60.352 ns | 59.993 ns - 60.704 ns |
| async_cached_resolve | sync_get | 11.307 ns | 11.274 ns - 11.345 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.307 us | 1.306 us - 1.310 us |
| async_cold_resolve | async_context | 5.781 us | 5.292 us - 6.351 us |
| async_cold_resolve | sync_context_baseline | 90.490 ns | 80.434 ns - 100.130 ns |
| async_cold_resolve | thread_safe_context_baseline | 894.943 ns | 891.399 ns - 898.589 ns |
| async_invalidation_throughput | async_context | 295.227 us | 265.760 us - 327.657 us |
| async_invalidation_throughput | sync_context_baseline | 2.420 us | 2.407 us - 2.431 us |
| async_invalidation_throughput | thread_safe_context_baseline | 53.428 us | 53.356 us - 53.555 us |
| async_cancellation_throughput | async_invalidate_in_flight | 54.441 us | 41.316 us - 66.752 us |
| async_concurrent_contention | async_context / 1 | 71.503 us | 69.522 us - 73.334 us |
| async_concurrent_contention | async_context / 4 | 354.356 us | 322.920 us - 388.955 us |
| async_concurrent_contention | async_context / 16 | 1.719 ms | 1.551 ms - 1.890 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 78.670 us | 76.824 us - 80.308 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 757.611 us | 729.790 us - 783.611 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 4.236 ms | 4.229 ms - 4.244 ms |
| async_effect_throughput | async_context | 188.473 ms | 188.349 ms - 188.576 ms |
| async_batch_throughput | async_context | 94.341 us | 84.935 us - 104.357 us |
| async_batch_throughput | sync_context_baseline | 7.042 us | 7.012 us - 7.080 us |
| tokio_sync_cached_read | single_task | 1.409 us | 1.407 us - 1.411 us |
| tokio_sync_cached_read | spawn_read | 5.977 us | 5.497 us - 6.457 us |
| tokio_sync_cold_first_get | single_task | 1.350 us | 1.348 us - 1.353 us |
| tokio_sync_cold_first_get | spawn_compute | 6.600 us | 5.941 us - 7.269 us |
| tokio_sync_invalidation | single_task | 54.717 us | 54.265 us - 55.185 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 58.999 us | 58.071 us - 60.236 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 426.613 us | 387.766 us - 467.380 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.931 ms | 3.806 ms - 4.023 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 58.701 us | 58.111 us - 59.329 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 389.576 us | 348.499 us - 427.364 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 3.247 ms | 3.165 ms - 3.305 ms |
| tokio_sync_batch | spawn_batch | 46.864 us | 46.786 us - 46.946 us |
| tokio_sync_effect | single_task | 10.103 ms | 10.097 ms - 10.109 ms |
| scale | build | 102.783 ms | 102.018 ms - 103.560 ms |
| scale | cold_full_recalc | 52.064 ms | 51.774 ms - 52.383 ms |
| scale | full_recalc_invalidate_all | 55.266 ms | 54.489 ms - 56.269 ms |
| scale | viewport_recalc | 3.089 us | 3.083 us - 3.096 us |
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
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.092 ns | 1.089 ns - 1.095 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 87.090 ns | 86.857 ns - 87.342 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 16.997 ns | 16.938 ns - 17.058 ns |
| revision_write_cost | push / 1 | 256.068 ns | 250.419 ns - 261.699 ns |
| revision_write_cost | push / 16 | 1.088 us | 1.086 us - 1.090 us |
| revision_write_cost | push / 128 | 9.973 us | 9.960 us - 9.987 us |
| revision_write_cost | push / 1024 | 82.811 us | 82.570 us - 83.092 us |
| revision_write_cost | revision / 1 | 129.429 ns | 128.474 ns - 130.424 ns |
| revision_write_cost | revision / 16 | 834.305 ns | 831.976 ns - 836.701 ns |
| revision_write_cost | revision / 128 | 8.385 us | 8.356 us - 8.418 us |
| revision_write_cost | revision / 1024 | 69.305 us | 69.163 us - 69.463 us |
| revision_write_then_read | push / 1 | 112.272 ns | 111.623 ns - 112.925 ns |
| revision_write_then_read | push / 16 | 1.310 us | 1.308 us - 1.312 us |
| revision_write_then_read | push / 128 | 13.652 us | 13.638 us - 13.667 us |
| revision_write_then_read | push / 1024 | 110.650 us | 110.538 us - 110.790 us |
| revision_write_then_read | revision / 1 | 95.420 ns | 95.121 ns - 95.738 ns |
| revision_write_then_read | revision / 16 | 1.220 us | 1.216 us - 1.223 us |
| revision_write_then_read | revision / 128 | 13.160 us | 13.138 us - 13.182 us |
| revision_write_then_read | revision / 1024 | 108.047 us | 107.870 us - 108.256 us |
| typed_cache_reads | context_cell | 2.122 ns | 2.121 ns - 2.124 ns |
| typed_cache_reads | context_rc_cell | 5.563 ns | 5.554 ns - 5.571 ns |
| typed_cache_reads | context_rc_slot | 7.012 ns | 6.980 ns - 7.045 ns |
| typed_cache_reads | context_slot | 3.626 ns | 3.617 ns - 3.634 ns |
| typed_cache_reads | thread_safe_arc_slot | 63.747 ns | 63.675 ns - 63.841 ns |
| typed_cache_reads | thread_safe_arc_string_slot | 63.922 ns | 63.769 ns - 64.115 ns |
| typed_cache_reads | thread_safe_cell | 24.192 ns | 24.156 ns - 24.235 ns |
| typed_cache_reads | thread_safe_slot | 55.445 ns | 55.422 ns - 55.470 ns |
| typed_cache_reads | thread_safe_string_slot | 71.035 ns | 70.570 ns - 71.578 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 4.530 us | 25.580 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 70.000 ns | 504.484 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 2.670 us | 35.801 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 98 | 148.391 us | 76.730 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 164 | 753.246 us | 128.570 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 292 | 4.041 ms | 246.014 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 562 | 12.899 ms | 349.083 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.530 us | 14.360 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 98 | 34.270 us | 23.080 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 182 | 282.983 us | 49.740 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 343 | 1.978 ms | 115.120 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 666 | 9.207 ms | 254.811 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.550 us | 41.561 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 126 | 70.220 us | 74.150 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 211 | 398.744 us | 119.681 us | 0 | 0 | 0 | 10 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 369 | 1.730 ms | 193.362 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 8.171 ms | 424.742 us | 0 | 0 | 0 | 1 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 1.970 us | 33.700 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 24 | 0 | 1 | 0 | 0 | 0 | 143 | 30.560 us | 51.770 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 54 | 0 | 1 | 0 | 0 | 0 | 305 | 126.891 us | 147.420 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 108 | 0 | 1 | 0 | 0 | 0 | 643 | 816.228 us | 292.313 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 212 | 0 | 1 | 0 | 0 | 0 | 1267 | 1.551 ms | 653.505 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.710 us | 21.590 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 137 | 85.421 us | 54.461 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 256 | 691.326 us | 112.900 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 478 | 4.037 ms | 253.622 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 942 | 18.568 ms | 547.704 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 1.930 us | 24.020 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 1.990 us | 23.260 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 81 | 5.441 us | 37.420 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 102 | 112.910 us | 53.080 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 115 | 9.520 us | 85.170 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 2.950 us | 52.720 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 194 | 63.460 us | 96.962 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 23 | 0 | 16 | 0 | 0 | 0 | 308 | 451.934 us | 166.972 us | 0 | 0 | 0 | 25 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 10 | 0 | 32 | 0 | 0 | 0 | 403 | 2.412 ms | 263.543 us | 0 | 0 | 0 | 9 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 8 | 0 | 64 | 0 | 0 | 0 | 742 | 9.194 ms | 473.433 us | 0 | 0 | 0 | 7 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 9 | 1 | 416 | 2.379 ms | 246.193 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 734 | 8.730 ms | 451.913 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 31 | 1 | 404 | 2.067 ms | 158.911 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 30 | 1 | 692 | 10.028 ms | 296.111 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 4.103 ms | 293.623 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 3 | 1 | 1244 | 17.398 ms | 599.674 us | 0 | 0 | 0 | 3 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 554 | 0 | 64 | 0 | 50 | 1 | 1161 | 16.698 ms | 3.716 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 563 | 0 | 64 | 0 | 50 | 1 | 1426 | 71.888 ms | 6.836 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 505 | 16.847 ms | 3.024 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 762 | 67.457 ms | 6.023 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1446 | 7.048 ms | 525.433 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2812 | 36.011 ms | 1.135 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 603 | 1.942 ms | 310.353 us | 0 | 0 | 0 | 69 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1311 | 5.374 ms | 564.772 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 50.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 20.000 ns | 504.214 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 1.700 us | 2.370 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 850.000 ns | 32.561 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 62 | 85.610 us | 4.140 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 140.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 62.671 us | 71.900 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 96 | 371.163 us | 6.700 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 381.993 us | 121.200 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 160 | 1.647 ms | 11.800 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 2.394 ms | 233.574 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 302 | 5.345 ms | 19.720 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 270.000 ns | 1.810 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 60.000 ns | 2.460 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 7.554 ms | 323.123 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 60.000 ns | 1.970 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 890.000 ns | 1.770 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 130.000 ns | 930.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 450.000 ns | 9.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 960.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 59 | 17.400 us | 2.140 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 140.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 40.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 16.630 us | 19.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 60.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 103 | 168.911 us | 3.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 220.000 ns | 570.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 113.622 us | 43.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 910.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 184 | 821.406 us | 7.320 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 430.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 200.000 ns | 2.390 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.156 ms | 102.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 220.000 ns | 1.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 347 | 4.165 ms | 17.310 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 900.000 ns | 2.660 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 400.000 ns | 4.950 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 5.040 ms | 226.151 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 410.000 ns | 3.740 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 1.920 us | 14.151 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 1.210 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 440.000 ns | 25.660 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 104 | 69.650 us | 38.690 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 2.230 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 11 | 290.000 ns | 32.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 182 | 397.764 us | 76.471 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 130.000 ns | 870.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 410.000 ns | 5.900 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 10 | 410.000 ns | 35.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 970.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 330 | 1.729 ms | 155.762 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 820.000 ns | 10.590 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 4 | 230.000 ns | 26.480 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 20.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 8.132 ms | 376.072 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 140.000 ns | 880.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.720 us | 26.840 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 36.600 us | 20.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 910.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 930.000 ns | 2.030 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 1.240 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 1.200 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 430.000 ns | 16.430 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 450.000 ns | 12.800 us |
| thread_safe_contention_same_slot_write_read_2 | other | 64 | 18.380 us | 1.920 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 1.100 us | 1.560 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 230.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 9.230 us | 24.250 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 24 | 1.830 us | 23.810 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 124 | 54.811 us | 3.680 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 6 | 3.440 us | 1.650 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 220.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 58.400 us | 56.100 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 54 | 10.210 us | 85.770 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 56 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 236 | 317.023 us | 9.060 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 18 | 14.160 us | 5.830 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 431.525 us | 109.831 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 108 | 53.490 us | 167.312 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 152 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 466 | 651.732 us | 17.140 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 24 | 66.320 us | 7.590 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 20.000 ns | 480.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 639.277 us | 227.822 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 212 | 194.141 us | 400.473 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 308 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 810.000 ns | 1.200 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 400.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 490.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 390.000 ns | 9.840 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 420.000 ns | 9.660 us |
| thread_safe_contention_independent_slots_2 | other | 67 | 54.291 us | 2.610 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 110.000 ns | 320.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 750.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 16.980 us | 27.521 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 13.980 us | 23.260 us |
| thread_safe_contention_independent_slots_4 | other | 114 | 255.975 us | 5.140 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 210.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 1.220 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 195.410 us | 52.210 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 239.611 us | 53.700 us |
| thread_safe_contention_independent_slots_8 | other | 192 | 1.527 ms | 10.410 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 420.000 ns | 1.090 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 220.000 ns | 2.790 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.206 ms | 118.432 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.304 ms | 120.900 us |
| thread_safe_contention_independent_slots_16 | other | 368 | 6.047 ms | 21.420 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 870.000 ns | 2.190 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 450.000 ns | 5.910 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 5.463 ms | 248.272 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.056 ms | 269.912 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 860.000 ns | 1.390 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 80.000 ns | 410.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 560.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 510.000 ns | 11.380 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 460.000 ns | 10.280 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 940.000 ns | 1.010 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 230.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 470.000 ns | 11.530 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 490.000 ns | 10.340 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 3.030 us | 1.150 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 6 | 771.000 ns | 1.500 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 660.000 ns | 14.830 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 950.000 ns | 19.670 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 32.830 us | 1.570 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 15 | 74.570 us | 9.420 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 20.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 1.230 us | 13.560 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 4.260 us | 28.320 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 17 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 990.000 ns | 1.310 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 12 | 6.960 us | 3.970 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 640.000 ns | 14.090 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 900.000 ns | 65.420 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 32 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 1.970 us | 13.510 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 310.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 1.430 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 400.000 ns | 24.800 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 420.000 ns | 12.670 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 55.500 us | 27.730 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 40.000 ns | 140.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 190.000 ns | 2.201 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 800.000 ns | 46.111 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 6.930 us | 20.780 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 209 | 433.773 us | 68.730 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 400.000 ns | 5.330 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 25 | 1.920 us | 62.691 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 23 | 15.791 us | 30.061 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 33 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 341 | 2.381 ms | 193.392 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 550.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 800.000 ns | 12.350 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 10 | 29.670 us | 35.770 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 10 | 660.000 ns | 21.481 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 8 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 656 | 9.191 ms | 383.441 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 400.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.660 us | 27.590 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 7 | 200.000 ns | 35.261 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 8 | 440.000 ns | 26.741 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 376 | 2.378 ms | 208.133 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 830.000 ns | 9.950 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 8 | 310.000 ns | 28.110 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 667 | 8.728 ms | 412.493 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.610 us | 21.170 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 90.000 ns | 18.250 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 261 | 895.817 us | 37.901 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 410.000 ns | 5.260 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.171 ms | 115.750 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 4.466 ms | 59.820 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 810.000 ns | 10.380 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.561 ms | 225.911 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 4.102 ms | 265.592 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 40.000 ns | 680.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 790.000 ns | 12.941 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 170.000 ns | 10.480 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 40.000 ns | 3.930 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1171 | 17.384 ms | 541.564 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 820.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.730 us | 27.730 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 3 | 12.040 us | 23.300 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 170.000 ns | 6.260 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 351 | 2.863 ms | 95.251 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.820 us | 4.600 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.640 us | 21.590 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 11.062 ms | 3.130 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 554 | 2.769 ms | 464.204 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 479 | 8.882 ms | 97.751 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.770 us | 4.430 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.690 us | 21.530 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 57.800 ms | 6.256 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 563 | 5.203 ms | 456.214 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 217 | 5.274 ms | 9.790 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.850 us | 5.090 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 880.000 ns | 12.480 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 11.569 ms | 2.963 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.720 us | 33.200 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 346 | 13.021 ms | 11.070 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.810 us | 4.570 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 830.000 ns | 11.030 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 54.432 ms | 5.966 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.560 us | 30.461 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 740 | 2.508 ms | 30.420 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.000 us | 8.510 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.640 us | 23.000 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 4.535 ms | 415.232 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.760 us | 48.271 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1402 | 13.785 ms | 59.282 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 6.370 us | 15.440 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.430 us | 42.561 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 22.212 ms | 898.073 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 4.490 us | 119.201 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 402 | 1.931 ms | 189.742 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 2.440 us | 7.710 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.760 us | 21.100 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 2 | 4.560 us | 49.971 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 1.920 us | 41.830 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 791 | 5.360 ms | 327.072 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 6.970 us | 25.880 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.410 us | 43.280 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 100.000 ns | 80.930 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 3.500 us | 87.610 us |

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
