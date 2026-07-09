# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.22.0`.

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
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 192 | set_cell_invalidation<=0, dependency_edge<=16, get_refresh<=32, publish<=32 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 900 | other<=800, set_cell_invalidation<=16, dependency_edge<=64, get_refresh<=2, publish<=2 |
| thread_safe_contention_same_slot_write_read_16 | 1000 | get_refresh<=160, publish<=256, in_flight_wait<=700, set_cell_invalidation<=180 |
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

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 1.735 ms | 2.468 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 4.564 ms | 5.388 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.907 ms | 2.012 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 3.856 ms | 4.906 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 514.535 us | 554.651 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.092 ms | 1.253 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 3.249 ms | 3.383 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.193 ms | 3.537 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.057 ms | 1.233 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.448 ms | 2.704 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.256 ms | 1.401 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.823 ms | 3.160 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.085 ms | 2.334 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 5.030 ms | 5.935 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.928 ms | 4.113 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.417 ms | 6.578 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.112 ms | 1.176 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 2.054 ms | 2.197 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 369.602 us | 404.761 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 737.916 us | 778.220 us | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.035 ms | 1.307 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.432 ms | 1.689 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.758 ns | 10.939 ns - 12.595 ns |
| cached_reads | thread_safe_context | 68.926 ns | 67.798 ns - 70.163 ns |
| cold_first_get | context | 109.425 ns | 99.270 ns - 121.107 ns |
| cold_first_get | thread_safe_context | 1.195 us | 1.104 us - 1.285 us |
| dependency_fan_out | context / 32 | 4.216 us | 3.815 us - 4.599 us |
| dependency_fan_out | context / 256 | 54.254 us | 50.952 us - 57.874 us |
| dependency_fan_out | thread_safe_context / 32 | 28.542 us | 27.286 us - 29.755 us |
| dependency_fan_out | thread_safe_context / 256 | 181.151 us | 176.429 us - 185.436 us |
| set_cell_invalidation | high_fan_out / 512 | 121.195 us | 114.372 us - 127.383 us |
| set_cell_invalidation | same_slot_contention / 1 | 38.868 us | 37.748 us - 39.976 us |
| set_cell_invalidation | same_slot_contention / 2 | 72.014 us | 65.914 us - 78.014 us |
| set_cell_invalidation | same_slot_contention / 4 | 161.067 us | 153.524 us - 169.090 us |
| set_cell_invalidation | same_slot_contention / 8 | 453.979 us | 425.098 us - 484.568 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.129 ms | 1.064 ms - 1.205 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 37.939 us | 36.827 us - 38.872 us |
| set_cell_invalidation | independent_slot_contention / 2 | 63.372 us | 60.358 us - 66.497 us |
| set_cell_invalidation | independent_slot_contention / 4 | 105.005 us | 101.671 us - 108.086 us |
| set_cell_invalidation | independent_slot_contention / 8 | 203.733 us | 199.519 us - 208.128 us |
| set_cell_invalidation | independent_slot_contention / 16 | 383.716 us | 369.828 us - 396.557 us |
| set_cell_invalidation | batched_write_bursts / 1 | 129.825 us | 128.331 us - 131.135 us |
| set_cell_invalidation | batched_write_bursts / 2 | 201.736 us | 193.184 us - 209.726 us |
| set_cell_invalidation | batched_write_bursts / 4 | 448.081 us | 424.295 us - 473.127 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.056 ms | 973.395 us - 1.148 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.455 ms | 2.273 ms - 2.630 ms |
| memo_equality_suppression | context | 2.753 us | 2.439 us - 3.053 us |
| memo_equality_suppression | thread_safe_context | 39.336 us | 37.795 us - 40.909 us |
| effect_flushing | context | 69.849 ns | 65.968 ns - 73.789 ns |
| effect_flushing | thread_safe_context | 1.001 us | 987.676 ns - 1.015 us |
| batch_storms | context / 64 | 3.611 us | 3.416 us - 3.852 us |
| batch_storms | thread_safe_context / 64 | 8.336 us | 8.039 us - 8.643 us |
| thread_safe_contention | same_slot_write_read / 1 | 101.739 us | 100.535 us - 102.951 us |
| thread_safe_contention | same_slot_write_read / 2 | 297.089 us | 287.078 us - 307.556 us |
| thread_safe_contention | same_slot_write_read / 4 | 702.576 us | 675.876 us - 729.565 us |
| thread_safe_contention | same_slot_write_read / 8 | 1.885 ms | 1.721 ms - 2.075 ms |
| thread_safe_contention | same_slot_write_read / 16 | 4.580 ms | 4.266 ms - 4.875 ms |
| thread_safe_contention | independent_slots / 1 | 102.102 us | 100.995 us - 103.177 us |
| thread_safe_contention | independent_slots / 2 | 214.511 us | 202.420 us - 230.676 us |
| thread_safe_contention | independent_slots / 4 | 586.100 us | 563.345 us - 610.312 us |
| thread_safe_contention | independent_slots / 8 | 1.812 ms | 1.691 ms - 1.922 ms |
| thread_safe_contention | independent_slots / 16 | 3.887 ms | 3.503 ms - 4.271 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 101.688 us | 100.046 us - 103.417 us |
| thread_safe_contention | read_mostly_waiters / 2 | 140.155 us | 137.232 us - 142.938 us |
| thread_safe_contention | read_mostly_waiters / 4 | 242.136 us | 238.195 us - 245.338 us |
| thread_safe_contention | read_mostly_waiters / 8 | 520.702 us | 502.120 us - 537.792 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.116 ms | 1.068 ms - 1.165 ms |
| thread_safe_contention | batched_write_bursts / 1 | 212.177 us | 210.465 us - 214.033 us |
| thread_safe_contention | batched_write_bursts / 2 | 558.794 us | 541.456 us - 576.429 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.621 ms | 1.614 ms - 1.628 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.219 ms | 3.145 ms - 3.290 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.173 ms | 2.960 ms - 3.342 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.080 ms | 1.008 ms - 1.142 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.391 ms | 2.247 ms - 2.524 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.258 ms | 1.187 ms - 1.323 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.855 ms | 2.774 ms - 2.944 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.066 ms | 1.918 ms - 2.203 ms |
| thread_safe_effect_contention | batch_flush / 16 | 5.280 ms | 5.017 ms - 5.560 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.948 ms | 3.907 ms - 3.995 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.410 ms | 6.364 ms - 6.459 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.083 ms | 1.030 ms - 1.129 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 2.064 ms | 2.003 ms - 2.125 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 371.619 us | 360.955 us - 382.802 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 729.514 us | 705.699 us - 751.035 us |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.017 ms | 940.732 us - 1.101 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.475 ms | 1.393 ms - 1.564 ms |
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
| scale | build | 104.902 ms | 101.020 ms - 109.073 ms |
| scale | cold_full_recalc | 105.532 ms | 101.992 ms - 109.130 ms |
| scale | full_recalc_invalidate_all | 89.283 ms | 84.330 ms - 94.854 ms |
| scale | viewport_recalc | 15.584 us | 14.675 us - 16.536 us |
| typed_cache_reads | context_cell | 3.050 ns | 2.838 ns - 3.272 ns |
| typed_cache_reads | context_rc_cell | 3.544 ns | 3.326 ns - 3.763 ns |
| typed_cache_reads | context_rc_slot | 11.925 ns | 11.019 ns - 12.918 ns |
| typed_cache_reads | context_slot | 12.527 ns | 11.473 ns - 13.567 ns |
| typed_cache_reads | thread_safe_cell | 26.154 ns | 25.774 ns - 26.554 ns |
| typed_cache_reads | thread_safe_slot | 68.491 ns | 67.288 ns - 69.798 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 26.391 us | 25.531 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 1.570 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 360.000 ns | 1.260 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 380.000 ns | 4.351 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.710 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 2.300 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 2.170 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 700.000 ns | 2.120 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.440 us | 4.341 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.870 us | 10.450 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 5.741 us | 21.170 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.320 us | 50.840 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 134 | 90.161 us | 76.430 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 196 | 406.115 us | 126.571 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 374 | 3.129 ms | 332.012 us | 0 | 0 | 0 | 6 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 8.583 ms | 447.812 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 980.000 ns | 20.061 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 32 | 0 | 1 | 0 | 0 | 0 | 84 | 47.500 us | 66.590 us | 23 | 23 | 9 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 52 | 0 | 1 | 0 | 0 | 0 | 172 | 58.430 us | 128.391 us | 54 | 54 | 10 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 105 | 0 | 1 | 0 | 0 | 0 | 395 | 218.671 us | 242.061 us | 83 | 83 | 45 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 202 | 0 | 1 | 0 | 0 | 0 | 822 | 963.014 us | 544.694 us | 170 | 170 | 86 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 910.000 ns | 13.480 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 89 | 49.962 us | 46.450 us | 17 | 17 | 14 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 214 | 712.189 us | 123.680 us | 16 | 16 | 47 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 463 | 4.154 ms | 274.351 us | 12 | 12 | 115 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 912 | 18.508 ms | 579.215 us | 11 | 11 | 244 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 960.000 ns | 17.590 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.110 us | 14.070 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 30 | 1.120 us | 20.050 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 54 | 20.170 us | 35.441 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 74 | 13.250 us | 110.780 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.910 us | 61.381 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 21 | 0 | 8 | 0 | 0 | 0 | 211 | 138.040 us | 154.682 us | 0 | 0 | 0 | 30 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 53 | 0 | 16 | 0 | 0 | 0 | 459 | 324.253 us | 347.033 us | 0 | 0 | 0 | 53 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 8 | 0 | 32 | 0 | 0 | 0 | 389 | 1.550 ms | 236.202 us | 0 | 0 | 0 | 7 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 717 | 8.801 ms | 441.972 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 392 | 1.457 ms | 214.652 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 722 | 7.961 ms | 414.193 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 35 | 1 | 415 | 1.870 ms | 175.851 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 40 | 1 | 720 | 9.736 ms | 339.742 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 3.761 ms | 277.700 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 5 | 1 | 1252 | 17.410 ms | 585.283 us | 0 | 0 | 0 | 4 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 553 | 0 | 64 | 0 | 50 | 1 | 1152 | 27.185 ms | 6.028 ms | 4 | 128 | 124 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 555 | 0 | 64 | 0 | 50 | 1 | 1418 | 118.465 ms | 11.653 ms | 0 | 0 | 256 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.170 us | 73.321 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.810 us | 72.081 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 15.290 us | 101.040 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 29.831 us | 189.302 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 603 | 2.636 ms | 394.493 us | 0 | 0 | 0 | 69 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1311 | 9.422 ms | 760.167 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 180.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 50.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 190.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 90.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 50.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 170.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 130.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 50.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 1.141 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 190.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 70.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 520.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 20.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 140.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 90.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 50.000 ns | 610.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 140.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 520.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 570.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 410.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 160.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 80.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 570.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 830.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 330.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.141 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 160.000 ns | 1.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.610 us | 2.630 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 610.000 ns | 1.810 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 330.000 ns | 3.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 320.000 ns | 2.820 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 3.240 us | 5.090 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.321 us | 3.960 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 550.000 ns | 6.390 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 630.000 ns | 5.730 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 3.250 us | 14.770 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 210.000 ns | 1.700 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 730.000 ns | 33.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 109 | 87.811 us | 33.610 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 320.000 ns | 4.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 1.930 us | 38.200 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 172 | 405.025 us | 84.721 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 70.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 610.000 ns | 7.310 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 380.000 ns | 34.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 333 | 3.127 ms | 245.441 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 120.000 ns | 880.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.260 us | 16.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 6 | 340.000 ns | 68.671 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 860.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 8.580 ms | 393.692 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.910 us | 32.050 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 100.000 ns | 21.420 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 50.000 ns | 370.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 190.000 ns | 520.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 80.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 430.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 680.000 ns | 18.801 us |
| thread_safe_contention_same_slot_write_read_2 | other | 22 | 7.050 us | 1.220 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 8 | 2.500 us | 2.670 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 50.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 9 | 9.920 us | 14.400 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 32 | 27.980 us | 47.920 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 23 | 21.430 us | 1.470 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 26 | 1.340 us | 4.710 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 360.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 10 | 17.140 us | 17.990 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 52 | 18.480 us | 103.861 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 60 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 92 | 26.810 us | 3.210 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 23 | 21.511 us | 6.890 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 45 | 112.350 us | 61.631 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 105 | 57.970 us | 169.910 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 129 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 169 | 264.953 us | 6.250 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 48 | 19.120 us | 14.980 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 60.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 86 | 561.031 us | 117.721 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 202 | 117.850 us | 405.333 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 316 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 200.000 ns | 600.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 270.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 50.000 ns | 580.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 590.000 ns | 12.030 us |
| thread_safe_contention_independent_slots_2 | other | 36 | 14.732 us | 2.160 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 140.000 ns | 390.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 70.000 ns | 600.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 14 | 7.720 us | 14.420 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 27.300 us | 28.880 us |
| thread_safe_contention_independent_slots_4 | other | 88 | 229.944 us | 4.860 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 300.000 ns | 710.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 150.000 ns | 1.280 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 47 | 220.913 us | 45.850 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 260.882 us | 70.980 us |
| thread_safe_contention_independent_slots_8 | other | 189 | 964.716 us | 9.720 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 630.000 ns | 1.530 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 260.000 ns | 3.350 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 115 | 1.317 ms | 119.701 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.871 ms | 140.050 us |
| thread_safe_contention_independent_slots_16 | other | 349 | 5.163 ms | 17.400 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.270 us | 2.950 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 530.000 ns | 6.070 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 244 | 5.605 ms | 254.542 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.738 ms | 298.253 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 120.000 ns | 1.730 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 100.000 ns | 540.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 1.170 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 720.000 ns | 14.150 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 210.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 90.000 ns | 200.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 50.000 ns | 370.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 760.000 ns | 13.170 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 190.000 ns | 350.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 170.000 ns | 500.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 50.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 710.000 ns | 18.840 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 140.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 14 | 13.900 us | 5.120 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 50.000 ns | 370.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 6.080 us | 29.621 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 140.000 ns | 510.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 7 | 12.350 us | 1.380 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 19 | 730.000 ns | 108.550 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 43 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 3.360 us | 14.780 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 270.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 1.630 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 630.000 ns | 27.301 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 680.000 ns | 17.400 us |
| thread_safe_contention_batched_write_bursts_2 | other | 135 | 117.730 us | 37.130 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 330.000 ns | 3.880 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 30 | 9.210 us | 65.932 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 21 | 10.700 us | 47.570 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 264 | 280.462 us | 75.121 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 10 | 22.951 us | 3.870 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 650.000 ns | 7.120 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 53 | 4.860 us | 129.652 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 53 | 15.330 us | 131.270 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 63 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 336 | 1.547 ms | 170.942 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.200 us | 14.600 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 7 | 870.000 ns | 26.790 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 8 | 390.000 ns | 23.690 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 646 | 8.799 ms | 378.162 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 270.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.580 us | 32.500 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 2 | 70.000 ns | 14.320 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 3 | 120.000 ns | 16.720 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 354 | 1.455 ms | 181.522 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.220 us | 11.270 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 6 | 310.000 ns | 21.860 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 656 | 7.957 ms | 372.013 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.210 us | 23.070 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 2 | 1.420 us | 19.110 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 272 | 771.635 us | 60.090 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 520.000 ns | 7.710 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.098 ms | 108.051 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 433 | 4.415 ms | 111.791 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 920.000 ns | 12.590 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.320 ms | 215.361 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 3.759 ms | 247.350 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 80.000 ns | 680.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.340 us | 14.760 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 60.000 ns | 8.560 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 60.000 ns | 6.350 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1177 | 17.406 ms | 510.393 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 90.000 ns | 830.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.930 us | 32.590 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 4 | 130.000 ns | 21.700 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 240.000 ns | 19.770 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 347 | 6.844 ms | 161.491 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.590 us | 5.350 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.270 us | 26.580 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 124 | 15.811 ms | 5.331 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 553 | 4.525 ms | 503.822 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 479 | 21.712 ms | 171.771 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.720 us | 6.180 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.130 us | 29.230 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 86.221 ms | 10.934 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 555 | 10.527 ms | 512.010 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.960 us | 6.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.540 us | 6.270 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.120 us | 17.240 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.550 us | 43.301 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 3.230 us | 6.100 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.690 us | 6.131 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.360 us | 17.940 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.530 us | 41.910 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 7.050 us | 11.580 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.240 us | 9.230 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.150 us | 27.820 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.850 us | 52.410 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 13.981 us | 16.560 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 5.900 us | 13.700 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 4.710 us | 55.600 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.240 us | 103.442 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 402 | 2.612 ms | 227.662 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 18.920 us | 9.210 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.330 us | 25.240 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 2 | 80.000 ns | 79.121 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 2.580 us | 53.260 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 791 | 9.404 ms | 398.113 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 8.990 us | 29.550 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.320 us | 54.290 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 90.000 ns | 156.982 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 4.890 us | 121.232 us |

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

What the four cases show at `N = 1_000_000`: `build` constructs 2M nodes (~0.13 s),
`cold_full_recalc` computes every formula from cold (~0.10 s), `full_recalc_invalidate_all`
re-edits every input and recomputes the whole sheet (~0.065 s), and `viewport_recalc`
edits one input and reads only a 1,000-cell viewport — **~11.5 µs**, ~5,000× cheaper
than a full recalc because the lazy pull-based model leaves off-viewport formulas
dirty and never recomputes them (the property a viewport-rendered spreadsheet needs).

Memory (not captured by criterion): building 2,000,000 nodes uses ~414 MiB RSS, i.e.
~216 B/node, so 1M populated formula cells land in the low hundreds of MiB.

### Spreadsheet cell-count context

How the two dominant spreadsheets bound a sheet:

| Spreadsheet | Documented limit | Cells |
|---|---|---:|
| **Google Sheets** | 10,000,000 cells per workbook (also 18,278 columns max) | **10,000,000** |
| **Microsoft Excel** | 1,048,576 rows × 16,384 columns per worksheet | **17,179,869,184** |

**Google Sheets (10M cells) — measured.** Modeled as 5,000,000 input cells + 5,000,000
formula cells (= 10M cells) by running the bench at `LAZILY_SCALE_N=5000000`. Single
criterion run on this host (186 GB RAM):

| case | mean | per cell |
|---|---:|---:|
| `build` (10M cells) | ~706 ms | ~71 ns |
| `cold_full_recalc` (5M) | ~518 ms | ~104 ns |
| `full_recalc_invalidate_all` (5M) | ~329 ms | ~66 ns |
| `viewport_recalc` (1k) | ~11.4 µs | ~11 ns |

So lazily backs a **full-capacity Google Sheets workbook**: build under a second, full
recompute ~0.5 s, and — crucially — viewport recalc stays ~11 µs **independent of sheet
size** (it was ~11.5 µs at 1M too), because the lazy pull-based model only recomputes the
cells you read. Reproduce: `LAZILY_SCALE_N=5000000 cargo bench --features scale-bench --bench scale`.

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
| `viewport_recalc` (edit 1, read 1k) | 11.52 µs | **8.22 µs** | leptos **1.4×** faster |

**Honest read:** lazily wins the bulk-graph operations — building
the sheet (1.5×), computing it cold (3.6×), and recomputing the whole sheet after a
full invalidation (2.8×) — driven by its sparse arena + lean single-threaded
`Context` versus leptos's runtime slotmap and subscriber bookkeeping. On the
cached-read-dominated `viewport_recalc` case the two are close and leptos is
actually a touch faster (its memo cache-hit read path is slightly leaner at this
size; only ~2 of the 1,000 viewport cells actually recompute). That leptos *wins*
a case — and that its 30 ms cold recalc proves its memos genuinely recompute — is
the evidence this comparison is fair rather than cherry-picked. The shared headline
is the lazy-pull property both exhibit: a one-input edit + bounded-viewport read is
**microseconds**, ~1000× cheaper than a full recalc, *independent of total sheet
size* — neither library recomputes off-viewport formulas. So the defensible claim
is "lazily has materially higher whole-graph throughput than a comparable
native-Rust pull-based reactive system, and matches it on incremental viewport
reads," **not** a blanket "fastest reactive library."

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
| cached read (Context) | 10.5 ns | 19 ns | — † |
| cached read (ThreadSafeContext) | 67 ns | 22 ns | — † |
| cold first get (Context) | 93 ns | 88 ns | — † |
| cold first get (ThreadSafeContext) | 1.13 µs | 98 ns | — † |
| fan-out 256 (Context) | 72.5 µs | 1.05 µs | — † |
| fan-out 256 (ThreadSafeContext) | 219 µs | 1.68 µs | — |
| set_cell high_fan_out 512 | 145 µs | 3.08 µs | — † |
| memo equality suppression (Context) | 3.29 µs | 34 ns | — † |
| effect flushing (Context) | 99 ns | 127 ns | — |
| batch storms 64 (Context) | 3.85 µs | 4.45 µs | — |

† lazily-zig 0.17-dev removed `std.time.Timer`, so its reactive-core
micro-bench is **counter-based** (deterministic work-counts: allocations,
edges, recomputes — not wall-clock). The counters confirm the same zero-work
steady state (cached reads = 0 allocs / 0 recomputes) but are not directly
comparable on a wall-clock axis. See
[lazily-zig BENCHMARKS.md](https://github.com/lazily-hub/lazily-zig/blob/main/BENCHMARKS.md).

### Scale — 1M rows (~2M cells)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build (2N nodes) | 105 ms | 143 ms | 132 ms |
| cold full recalc | 106 ms | 102 ms | 381 ms |
| viewport recalc (edit 1, read 1k) | 15.6 µs | 47.7 µs | 6.4 µs |

### Scale — 10M cells (full Google Sheets workbook capacity)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build | 706 ms | 1.33 s | 1.13 s |
| cold full recalc | 518 ms | 1.12 s | 2.26 s |
| viewport recalc | 11.4 µs | 71.7 µs | 6.6 µs |

**Honest read:** lazily-rs's monomorphized `Rc<T>` fast path leads the
spreadsheet-scale wall clock (leanest per-node storage → fastest build/cold
recalc) and ties lazily-cpp on effect flushing. lazily-cpp's type-erased
`SmallFn` + `SmallVec` node layout wins the high-fan-out micro-benchmarks
(fan-out 256, set_cell 512, memo equality) by 30–97× over lazily-rs, while
lazily-zig's integer-keyed cache delivers the cheapest viewport reads. The
**shared headline** across all three: they back a full-capacity Google Sheets
workbook and all exhibit the **lazy-pull viewport property** — a one-cell
edit + bounded-viewport read stays in the **microsecond** range, independent
of sheet size, because off-viewport formulas are left dirty and never
recomputed (~5,000–650,000× cheaper than a full recalc).

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
