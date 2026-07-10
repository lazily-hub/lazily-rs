# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.25.0`.

Environment: `rustc 1.96.0 (ac68faa20 2026-05-25)` on `x86_64-unknown-linux-gnu`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.607 ms | 3.115 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.304 ms | 6.987 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 2.215 ms | 2.546 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.930 ms | 5.626 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 565.725 us | 602.352 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.175 ms | 1.287 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 3.033 ms | 3.190 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.409 ms | 3.723 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.253 ms | 1.378 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.588 ms | 3.016 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.363 ms | 1.408 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.779 ms | 3.049 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.044 ms | 2.274 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.976 ms | 5.690 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.940 ms | 4.137 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.353 ms | 6.850 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.100 ms | 1.141 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 2.158 ms | 2.313 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 397.125 us | 431.722 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 764.915 us | 797.921 us | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.077 ms | 1.151 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.850 ms | 2.048 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 5.741 ns | 5.361 ns - 6.114 ns |
| cached_reads | thread_safe_context | 67.690 ns | 66.808 ns - 68.625 ns |
| cold_first_get | context | 128.837 ns | 107.935 ns - 152.082 ns |
| cold_first_get | thread_safe_context | 1.166 us | 1.093 us - 1.245 us |
| dependency_fan_out | context / 32 | 4.871 us | 4.521 us - 5.273 us |
| dependency_fan_out | context / 256 | 58.392 us | 51.694 us - 69.935 us |
| dependency_fan_out | thread_safe_context / 32 | 24.565 us | 23.195 us - 25.962 us |
| dependency_fan_out | thread_safe_context / 256 | 182.187 us | 175.066 us - 189.525 us |
| set_cell_invalidation | high_fan_out / 512 | 139.458 us | 128.437 us - 149.949 us |
| set_cell_invalidation | same_slot_contention / 1 | 82.459 us | 81.428 us - 83.465 us |
| set_cell_invalidation | same_slot_contention / 2 | 180.872 us | 175.657 us - 185.666 us |
| set_cell_invalidation | same_slot_contention / 4 | 481.296 us | 468.065 us - 495.654 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.478 ms | 1.411 ms - 1.543 ms |
| set_cell_invalidation | same_slot_contention / 16 | 2.966 ms | 2.846 ms - 3.115 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 82.158 us | 81.053 us - 83.364 us |
| set_cell_invalidation | independent_slot_contention / 2 | 179.969 us | 173.674 us - 186.755 us |
| set_cell_invalidation | independent_slot_contention / 4 | 478.375 us | 459.287 us - 495.326 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.451 ms | 1.398 ms - 1.512 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 3.124 ms | 3.034 ms - 3.214 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 141.548 us | 140.257 us - 142.668 us |
| set_cell_invalidation | batched_write_bursts / 2 | 238.654 us | 230.895 us - 248.746 us |
| set_cell_invalidation | batched_write_bursts / 4 | 503.756 us | 487.290 us - 518.203 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.296 ms | 1.255 ms - 1.341 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.109 ms | 2.921 ms - 3.290 ms |
| memo_equality_suppression | context | 3.349 us | 2.874 us - 3.906 us |
| memo_equality_suppression | thread_safe_context | 35.740 us | 34.408 us - 37.073 us |
| effect_flushing | context | 90.427 ns | 85.069 ns - 96.392 ns |
| effect_flushing | thread_safe_context | 974.332 ns | 953.058 ns - 996.723 ns |
| batch_storms | context / 64 | 3.075 us | 2.875 us - 3.291 us |
| batch_storms | thread_safe_context / 64 | 8.510 us | 8.265 us - 8.762 us |
| thread_safe_contention | same_slot_write_read / 1 | 144.174 us | 142.894 us - 145.418 us |
| thread_safe_contention | same_slot_write_read / 2 | 407.753 us | 396.283 us - 417.566 us |
| thread_safe_contention | same_slot_write_read / 4 | 1.001 ms | 981.755 us - 1.028 ms |
| thread_safe_contention | same_slot_write_read / 8 | 2.599 ms | 2.453 ms - 2.757 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.339 ms | 5.873 ms - 6.713 ms |
| thread_safe_contention | independent_slots / 1 | 142.843 us | 140.593 us - 145.008 us |
| thread_safe_contention | independent_slots / 2 | 289.486 us | 277.117 us - 300.113 us |
| thread_safe_contention | independent_slots / 4 | 759.389 us | 744.841 us - 774.126 us |
| thread_safe_contention | independent_slots / 8 | 2.267 ms | 2.140 ms - 2.392 ms |
| thread_safe_contention | independent_slots / 16 | 4.918 ms | 4.602 ms - 5.209 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 142.343 us | 141.090 us - 143.495 us |
| thread_safe_contention | read_mostly_waiters / 2 | 170.297 us | 168.476 us - 172.326 us |
| thread_safe_contention | read_mostly_waiters / 4 | 248.075 us | 245.170 us - 250.853 us |
| thread_safe_contention | read_mostly_waiters / 8 | 568.504 us | 557.472 us - 579.717 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.198 ms | 1.165 ms - 1.234 ms |
| thread_safe_contention | batched_write_bursts / 1 | 214.626 us | 212.831 us - 216.612 us |
| thread_safe_contention | batched_write_bursts / 2 | 577.939 us | 557.893 us - 596.742 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.668 ms | 1.663 ms - 1.674 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.059 ms | 3.022 ms - 3.101 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.323 ms | 3.130 ms - 3.499 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.254 ms | 1.203 ms - 1.300 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.668 ms | 2.513 ms - 2.818 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.359 ms | 1.338 ms - 1.380 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.778 ms | 2.669 ms - 2.874 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.992 ms | 1.850 ms - 2.118 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.823 ms | 4.438 ms - 5.187 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.952 ms | 3.916 ms - 4.000 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.430 ms | 6.327 ms - 6.556 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.098 ms | 1.078 ms - 1.115 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 2.161 ms | 2.110 ms - 2.216 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 400.077 us | 392.368 us - 409.050 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 763.211 us | 749.944 us - 776.486 us |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.081 ms | 1.053 ms - 1.106 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.823 ms | 1.732 ms - 1.902 ms |
| profile_instrumentation | context_snapshot | 377.565 ns | 338.970 ns - 417.697 ns |
| profile_instrumentation | thread_safe_snapshot | 293.673 us | 291.604 us - 295.885 us |
| async_cached_resolve | async_context | 7.538 us | 7.187 us - 8.040 us |
| async_cached_resolve | sync_context_baseline | 117.151 ns | 115.983 ns - 118.302 ns |
| async_cached_resolve | sync_get | 14.121 ns | 14.023 ns - 14.220 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.698 us | 1.685 us - 1.710 us |
| async_cold_resolve | async_context | 4.637 us | 4.485 us - 4.775 us |
| async_cold_resolve | sync_context_baseline | 90.412 ns | 83.537 ns - 98.123 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.055 us | 1.015 us - 1.098 us |
| async_invalidation_throughput | async_context | 205.442 us | 200.525 us - 210.381 us |
| async_invalidation_throughput | sync_context_baseline | 3.652 us | 3.262 us - 4.083 us |
| async_invalidation_throughput | thread_safe_context_baseline | 50.078 us | 48.620 us - 51.300 us |
| async_cancellation_throughput | async_invalidate_in_flight | 78.726 us | 62.938 us - 93.218 us |
| async_concurrent_contention | async_context / 1 | 71.406 us | 70.840 us - 72.098 us |
| async_concurrent_contention | async_context / 4 | 341.928 us | 326.298 us - 357.194 us |
| async_concurrent_contention | async_context / 16 | 1.675 ms | 1.516 ms - 1.834 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 61.766 us | 60.649 us - 62.938 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 520.371 us | 503.588 us - 534.359 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 3.765 ms | 3.681 ms - 3.825 ms |
| async_effect_throughput | async_context | 188.080 ms | 187.970 ms - 188.175 ms |
| async_batch_throughput | async_context | 76.377 us | 75.162 us - 77.699 us |
| async_batch_throughput | sync_context_baseline | 12.687 us | 11.976 us - 13.437 us |
| tokio_sync_cached_read | single_task | 1.600 us | 1.560 us - 1.640 us |
| tokio_sync_cached_read | spawn_read | 4.548 us | 4.454 us - 4.659 us |
| tokio_sync_cold_first_get | single_task | 1.510 us | 1.481 us - 1.543 us |
| tokio_sync_cold_first_get | spawn_compute | 4.229 us | 4.066 us - 4.428 us |
| tokio_sync_invalidation | single_task | 45.302 us | 43.971 us - 46.582 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 46.235 us | 45.576 us - 46.916 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 306.298 us | 296.639 us - 315.964 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.401 ms | 3.189 ms - 3.577 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 46.081 us | 45.474 us - 46.694 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 337.696 us | 312.181 us - 362.793 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 2.933 ms | 2.891 ms - 2.974 ms |
| tokio_sync_batch | spawn_batch | 47.779 us | 46.743 us - 48.956 us |
| tokio_sync_effect | single_task | 10.071 ms | 10.070 ms - 10.071 ms |
| scale | build | 191.790 ms | 178.970 ms - 205.836 ms |
| scale | cold_full_recalc | 144.059 ms | 134.264 ms - 155.676 ms |
| scale | full_recalc_invalidate_all | 86.869 ms | 79.419 ms - 94.074 ms |
| scale | viewport_recalc | 4.115 us | 3.933 us - 4.316 us |
| typed_cache_reads | context_cell | 3.116 ns | 2.856 ns - 3.408 ns |
| typed_cache_reads | context_rc_cell | 3.241 ns | 3.022 ns - 3.477 ns |
| typed_cache_reads | context_rc_slot | 4.543 ns | 4.278 ns - 4.839 ns |
| typed_cache_reads | context_slot | 4.724 ns | 4.352 ns - 5.145 ns |
| typed_cache_reads | thread_safe_cell | 26.756 ns | 26.487 ns - 27.011 ns |
| typed_cache_reads | thread_safe_slot | 65.850 ns | 65.338 ns - 66.372 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 2.080 us | 20.280 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 60.000 ns | 935.619 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 2.230 us | 28.000 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 102 | 63.891 us | 41.090 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 161 | 333.743 us | 68.990 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 298 | 2.332 ms | 165.811 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 548 | 12.115 ms | 359.941 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.620 us | 13.170 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 105 | 42.540 us | 26.760 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 183 | 328.672 us | 68.681 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 356 | 2.655 ms | 140.121 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 669 | 7.339 ms | 251.551 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.700 us | 47.360 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 126 | 51.020 us | 71.881 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 211 | 430.243 us | 125.592 us | 0 | 0 | 0 | 10 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 369 | 1.831 ms | 206.363 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 727 | 9.593 ms | 449.503 us | 0 | 0 | 0 | 6 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.140 us | 32.610 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 26 | 0 | 1 | 0 | 0 | 0 | 148 | 34.500 us | 70.891 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 54 | 0 | 1 | 0 | 0 | 0 | 350 | 169.516 us | 123.232 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 112 | 0 | 1 | 0 | 0 | 0 | 634 | 793.364 us | 316.694 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 236 | 0 | 1 | 0 | 0 | 0 | 1316 | 980.161 us | 533.052 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.900 us | 24.230 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 136 | 42.410 us | 48.981 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 257 | 592.015 us | 109.762 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 483 | 3.124 ms | 230.563 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 945 | 13.527 ms | 461.545 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.380 us | 26.761 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.300 us | 24.470 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 88 | 4.260 us | 29.310 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 110 | 23.020 us | 40.600 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 124 | 76.304 us | 53.161 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.230 us | 135.642 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 189 | 64.931 us | 109.311 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 34 | 0 | 16 | 0 | 0 | 0 | 374 | 300.903 us | 214.990 us | 0 | 0 | 0 | 38 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 7 | 0 | 32 | 0 | 0 | 0 | 385 | 1.467 ms | 235.891 us | 0 | 0 | 0 | 6 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 6 | 0 | 64 | 0 | 0 | 0 | 730 | 7.081 ms | 436.004 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 3 | 1 | 375 | 2.248 ms | 230.663 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 722 | 5.838 ms | 384.745 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 33 | 1 | 408 | 1.900 ms | 185.601 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 36 | 1 | 704 | 9.274 ms | 349.423 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 5 | 1 | 642 | 3.722 ms | 300.892 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 21.445 ms | 604.875 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 561 | 0 | 64 | 0 | 50 | 1 | 1168 | 31.451 ms | 6.535 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 556 | 0 | 64 | 0 | 49 | 1 | 1415 | 125.017 ms | 11.897 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 518 | 28.432 ms | 5.572 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 791 | 135.411 ms | 11.080 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1464 | 9.952 ms | 634.066 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2825 | 40.583 ms | 1.270 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 75 | 0 | 65 | 0 | 5 | 1 | 681 | 2.840 ms | 404.025 us | 0 | 0 | 0 | 96 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 144 | 0 | 129 | 0 | 7 | 1 | 1453 | 6.112 ms | 719.447 us | 0 | 0 | 0 | 157 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 40.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 20.000 ns | 935.119 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 930.000 ns | 2.370 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 1.180 us | 24.710 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 66 | 53.571 us | 2.510 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 10.200 us | 37.840 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 93 | 196.882 us | 3.680 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 136.741 us | 64.640 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 166 | 934.568 us | 7.590 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.397 ms | 157.521 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 288 | 4.636 ms | 14.650 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 7.479 ms | 344.591 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 970.000 ns | 1.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 80.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 510.000 ns | 10.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 66 | 23.750 us | 2.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 50.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 18.580 us | 22.710 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 40.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 104 | 176.412 us | 5.000 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 240.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 151.790 us | 61.061 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 197 | 1.166 ms | 10.721 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 450.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 230.000 ns | 2.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.488 ms | 124.310 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 210.000 ns | 1.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 350 | 2.594 ms | 15.850 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 880.000 ns | 2.640 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 4.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 4.743 ms | 225.481 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 430.000 ns | 3.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.060 us | 15.700 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 430.000 ns | 29.850 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 104 | 50.430 us | 43.041 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.520 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 11 | 300.000 ns | 24.920 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 182 | 429.343 us | 82.352 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 5.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 10 | 380.000 ns | 36.900 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 330 | 1.830 ms | 169.062 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 790.000 ns | 12.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 4 | 160.000 ns | 24.861 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 20.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 654 | 9.591 ms | 399.962 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.680 us | 26.681 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 6 | 160.000 ns | 22.520 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 180.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 970.000 ns | 2.240 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 1.270 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 1.130 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 560.000 ns | 13.320 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 450.000 ns | 14.650 us |
| thread_safe_contention_same_slot_write_read_2 | other | 65 | 17.960 us | 3.060 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 6 | 1.200 us | 2.830 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 1.180 us |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 8.380 us | 28.741 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 26 | 6.930 us | 35.080 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 124 | 80.985 us | 4.950 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 33 | 23.250 us | 7.880 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 1.200 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 53.941 us | 53.202 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 54 | 11.300 us | 56.000 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 74 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 238 | 339.902 us | 8.110 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 20 | 13.650 us | 5.311 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 260.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 400.781 us | 122.630 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 112 | 39.011 us | 180.383 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 135 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 489 | 387.962 us | 14.550 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 52 | 12.140 us | 7.800 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 514.868 us | 204.990 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 236 | 65.161 us | 305.332 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 282 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 910.000 ns | 1.290 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 50.000 ns | 280.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 370.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 480.000 ns | 11.210 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 440.000 ns | 11.080 us |
| thread_safe_contention_independent_slots_2 | other | 66 | 21.130 us | 2.630 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 100.000 ns | 350.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 560.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 14.130 us | 23.031 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 7.000 us | 22.410 us |
| thread_safe_contention_independent_slots_4 | other | 115 | 191.622 us | 5.170 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 230.000 ns | 660.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 100.000 ns | 1.080 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 293.212 us | 51.890 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 106.851 us | 50.962 us |
| thread_safe_contention_independent_slots_8 | other | 197 | 1.134 ms | 9.230 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 440.000 ns | 1.280 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 2.190 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.302 ms | 109.422 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 686.818 us | 108.441 us |
| thread_safe_contention_independent_slots_16 | other | 371 | 4.671 ms | 16.650 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 910.000 ns | 2.500 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 400.000 ns | 4.510 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 4.845 ms | 220.972 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 4.011 ms | 216.913 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 920.000 ns | 1.500 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 900.000 ns | 12.611 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 470.000 ns | 12.150 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 940.000 ns | 1.110 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 800.000 ns | 11.630 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 480.000 ns | 11.290 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 1.780 us | 1.060 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 1.360 us | 1.280 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 570.000 ns | 12.620 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 520.000 ns | 14.070 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 8 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 2.590 us | 1.090 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 18 | 18.430 us | 3.180 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 560.000 ns | 12.800 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 1.410 us | 23.260 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 2.730 us | 1.320 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 22 | 60.904 us | 16.570 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 640.000 ns | 14.620 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 12.010 us | 20.371 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 31 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.120 us | 39.941 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 90.000 ns | 1.380 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 400.000 ns | 72.041 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 560.000 ns | 22.080 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 62.341 us | 30.260 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 4.120 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.530 us | 46.061 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 810.000 ns | 28.670 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 232 | 277.603 us | 62.780 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 6 | 10.890 us | 1.470 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 390.000 ns | 6.090 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 38 | 4.680 us | 83.560 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 34 | 7.340 us | 61.090 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 48 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 334 | 1.465 ms | 164.571 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 870.000 ns | 12.470 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 6 | 900.000 ns | 35.800 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 7 | 250.000 ns | 22.850 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 652 | 7.078 ms | 344.584 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 130.000 ns | 1.060 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.710 us | 30.760 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 5 | 140.000 ns | 23.330 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 6 | 690.000 ns | 36.270 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 341 | 2.247 ms | 207.782 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 870.000 ns | 9.370 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 2 | 50.000 ns | 13.511 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 656 | 5.836 ms | 344.895 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.640 us | 20.930 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 2 | 50.000 ns | 18.920 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 265 | 748.077 us | 50.200 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 400.000 ns | 8.560 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.152 ms | 126.841 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 417 | 3.821 ms | 90.710 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 800.000 ns | 13.611 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.452 ms | 245.102 us |
| thread_safe_effect_contention_batch_flush_8 | other | 600 | 3.721 ms | 254.382 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 530.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 880.000 ns | 13.240 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 80.000 ns | 20.420 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 220.000 ns | 12.320 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 21.444 ms | 550.134 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 270.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.730 us | 27.911 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 40.000 ns | 15.970 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 50.000 ns | 10.590 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 351 | 6.544 ms | 176.442 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.860 us | 5.880 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.710 us | 27.730 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 20.911 ms | 5.761 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 561 | 3.993 ms | 564.194 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 475 | 37.207 ms | 177.870 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.830 us | 6.000 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.700 us | 27.860 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 80.903 ms | 11.203 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 556 | 6.904 ms | 481.753 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 230 | 9.023 ms | 13.191 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.891 us | 6.620 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 900.000 ns | 16.700 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 19.404 ms | 5.503 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.790 us | 33.140 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 375 | 38.002 ms | 17.400 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.940 us | 6.710 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 930.000 ns | 18.960 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 97.405 ms | 10.999 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.820 us | 38.251 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 758 | 4.154 ms | 39.790 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.170 us | 10.210 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.790 us | 26.160 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 5.792 ms | 512.416 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.710 us | 45.490 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1415 | 15.127 ms | 65.710 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.770 us | 14.160 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.430 us | 50.900 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 25.445 ms | 1.050 ms |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.490 us | 89.011 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 410 | 2.793 ms | 208.293 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 126 | 3.740 us | 17.431 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.770 us | 23.410 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 5 | 160.000 ns | 97.010 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 75 | 41.820 us | 57.881 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 799 | 6.074 ms | 359.142 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 376 | 10.680 us | 40.080 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.340 us | 49.100 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 5 | 160.000 ns | 158.801 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 144 | 23.640 us | 112.324 us |

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
