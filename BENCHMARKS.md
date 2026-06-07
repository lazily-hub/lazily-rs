# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.10.1`.

Environment: `rustc 1.96.0 (ac68faa20 2026-05-25)` on `x86_64-unknown-linux-gnu`.

Refresh command:

```bash
python3 scripts/update-benchmark-results.py
```

Regression workflow:

```bash
cargo bench --features instrumentation -- --save-baseline before
# apply the performance patch
cargo bench --features instrumentation -- --baseline before
python3 scripts/update-benchmark-results.py --no-run
```

Regression budgets enforced by `python3 scripts/update-benchmark-results.py --check`:

| Profile | Max lock acquisitions | Site lock budgets |
|---|---:|---|
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 192 | set_cell_invalidation<=0, dependency_edge<=16, get_refresh<=32, publish<=32 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 900 | other<=800, set_cell_invalidation<=16, dependency_edge<=64, get_refresh<=2, publish<=2 |
| thread_safe_contention_same_slot_write_read_16 | 1000 | get_refresh<=160, publish<=256, in_flight_wait<=700, set_cell_invalidation<=32 |
| thread_safe_contention_independent_slots_16 | 700 | other<=160, get_refresh<=64, publish<=320, dependency_edge<=16, set_cell_invalidation<=64 |
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
| cached ThreadSafeContext read latency | a8b6fc3 vs c917401 | `cargo bench --features instrumentation --bench context -- cached_reads/thread_safe_context` | 73.48 ns baseline vs 73.20 ns current on warm-cache repeat | no tuning; the archived 56.5 ns row did not reproduce under controlled A/B |
| effect cleanup contention at 16 workers | a8b6fc3 vs c917401 | `cargo bench --features instrumentation --bench context -- thread_safe_effect_contention/cleanup_execution/16` | 2.31 ms baseline vs 2.43 ms current on warm-cache repeat with overlapping CIs | keep watching; Criterion reported no statistically significant change |

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 2.578 ms | 2.746 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.185 ms | 8.642 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.133 ms | 1.220 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 2.885 ms | 3.035 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 533.466 us | 578.630 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.105 ms | 1.186 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.734 ms | 3.132 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 5.097 ms | 5.433 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.464 ms | 1.739 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 4.039 ms | 4.208 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.457 ms | 1.719 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.948 ms | 4.323 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.646 ms | 2.929 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 7.896 ms | 8.064 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.538 ms | 3.629 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.130 ms | 6.357 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.551 ms | 2.671 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.442 ms | 4.535 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 574.260 us | 585.218 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.126 ms | 1.138 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.167 ms | 1.287 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.271 ms | 2.452 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 8.027 ns | 7.987 ns - 8.066 ns |
| cached_reads | thread_safe_context | 65.223 ns | 64.895 ns - 65.584 ns |
| cold_first_get | context | 80.054 ns | 77.778 ns - 82.208 ns |
| cold_first_get | thread_safe_context | 990.971 ns | 959.974 ns - 1.021 us |
| dependency_fan_out | context / 32 | 3.107 us | 2.962 us - 3.248 us |
| dependency_fan_out | context / 256 | 45.763 us | 43.643 us - 48.510 us |
| dependency_fan_out | thread_safe_context / 32 | 20.950 us | 20.551 us - 21.347 us |
| dependency_fan_out | thread_safe_context / 256 | 160.588 us | 159.203 us - 162.024 us |
| set_cell_invalidation | high_fan_out / 512 | 92.241 us | 89.998 us - 94.223 us |
| set_cell_invalidation | same_slot_contention / 1 | 38.338 us | 37.310 us - 39.419 us |
| set_cell_invalidation | same_slot_contention / 2 | 80.472 us | 78.918 us - 81.907 us |
| set_cell_invalidation | same_slot_contention / 4 | 186.897 us | 180.693 us - 192.872 us |
| set_cell_invalidation | same_slot_contention / 8 | 541.804 us | 517.595 us - 568.932 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.759 ms | 1.718 ms - 1.794 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 39.527 us | 38.588 us - 40.454 us |
| set_cell_invalidation | independent_slot_contention / 2 | 73.353 us | 71.989 us - 74.760 us |
| set_cell_invalidation | independent_slot_contention / 4 | 128.280 us | 126.976 us - 129.514 us |
| set_cell_invalidation | independent_slot_contention / 8 | 281.716 us | 269.854 us - 293.557 us |
| set_cell_invalidation | independent_slot_contention / 16 | 612.042 us | 607.064 us - 617.024 us |
| set_cell_invalidation | batched_write_bursts / 1 | 128.103 us | 126.066 us - 130.111 us |
| set_cell_invalidation | batched_write_bursts / 2 | 226.325 us | 218.580 us - 232.553 us |
| set_cell_invalidation | batched_write_bursts / 4 | 480.877 us | 463.297 us - 496.610 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.401 ms | 1.353 ms - 1.449 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 4.002 ms | 3.922 ms - 4.075 ms |
| memo_equality_suppression | context | 1.881 us | 1.758 us - 2.005 us |
| memo_equality_suppression | thread_safe_context | 33.002 us | 32.645 us - 33.355 us |
| effect_flushing | context | 47.561 ns | 47.315 ns - 47.835 ns |
| effect_flushing | thread_safe_context | 919.973 ns | 914.308 ns - 926.589 ns |
| batch_storms | context / 64 | 2.764 us | 2.744 us - 2.786 us |
| batch_storms | thread_safe_context / 64 | 7.082 us | 6.899 us - 7.306 us |
| thread_safe_contention | same_slot_write_read / 1 | 98.686 us | 96.377 us - 100.859 us |
| thread_safe_contention | same_slot_write_read / 2 | 266.923 us | 261.168 us - 272.455 us |
| thread_safe_contention | same_slot_write_read / 4 | 871.034 us | 824.425 us - 916.400 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.558 ms | 2.488 ms - 2.625 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.411 ms | 7.144 ms - 7.749 ms |
| thread_safe_contention | independent_slots / 1 | 101.937 us | 100.782 us - 102.937 us |
| thread_safe_contention | independent_slots / 2 | 212.770 us | 206.378 us - 217.797 us |
| thread_safe_contention | independent_slots / 4 | 463.904 us | 453.067 us - 474.423 us |
| thread_safe_contention | independent_slots / 8 | 1.133 ms | 1.109 ms - 1.159 ms |
| thread_safe_contention | independent_slots / 16 | 2.850 ms | 2.739 ms - 2.949 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 103.037 us | 101.347 us - 104.750 us |
| thread_safe_contention | read_mostly_waiters / 2 | 138.765 us | 134.847 us - 142.827 us |
| thread_safe_contention | read_mostly_waiters / 4 | 277.379 us | 271.246 us - 282.068 us |
| thread_safe_contention | read_mostly_waiters / 8 | 522.464 us | 489.359 us - 552.208 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.116 ms | 1.097 ms - 1.136 ms |
| thread_safe_contention | batched_write_bursts / 1 | 208.558 us | 206.766 us - 210.767 us |
| thread_safe_contention | batched_write_bursts / 2 | 547.941 us | 521.314 us - 575.389 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.467 ms | 1.454 ms - 1.480 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.796 ms | 2.716 ms - 2.890 ms |
| thread_safe_contention | batched_write_bursts / 16 | 4.970 ms | 4.733 ms - 5.184 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.491 ms | 1.437 ms - 1.558 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 4.055 ms | 4.013 ms - 4.102 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.498 ms | 1.445 ms - 1.561 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 4.042 ms | 3.893 ms - 4.185 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.689 ms | 2.607 ms - 2.771 ms |
| thread_safe_effect_contention | batch_flush / 16 | 7.856 ms | 7.715 ms - 7.961 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.558 ms | 3.531 ms - 3.587 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.154 ms | 6.104 ms - 6.211 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.544 ms | 2.503 ms - 2.584 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.451 ms | 4.427 ms - 4.477 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 572.971 us | 567.279 us - 578.239 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.109 ms | 1.074 ms - 1.130 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.169 ms | 1.144 ms - 1.201 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.296 ms | 2.257 ms - 2.341 ms |
| profile_instrumentation | context_snapshot | 267.600 ns | 265.902 ns - 269.343 ns |
| profile_instrumentation | thread_safe_snapshot | 295.465 us | 294.338 us - 296.488 us |
| async_cached_resolve | async_context | 4.005 us | 3.829 us - 4.197 us |
| async_cached_resolve | sync_context_baseline | 73.063 ns | 69.741 ns - 77.406 ns |
| async_cached_resolve | sync_get | 11.537 ns | 11.488 ns - 11.585 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.394 us | 1.388 us - 1.399 us |
| async_cold_resolve | async_context | 4.261 us | 4.114 us - 4.397 us |
| async_cold_resolve | sync_context_baseline | 83.178 ns | 79.646 ns - 86.312 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.010 us | 983.439 ns - 1.036 us |
| async_invalidation_throughput | async_context | 245.023 us | 232.026 us - 258.919 us |
| async_invalidation_throughput | sync_context_baseline | 2.785 us | 2.763 us - 2.808 us |
| async_invalidation_throughput | thread_safe_context_baseline | 39.314 us | 39.079 us - 39.593 us |
| async_cancellation_throughput | async_invalidate_in_flight | 77.176 us | 64.769 us - 87.137 us |
| async_concurrent_contention | async_context / 1 | 71.400 us | 70.290 us - 72.484 us |
| async_concurrent_contention | async_context / 4 | 391.647 us | 375.605 us - 406.235 us |
| async_concurrent_contention | async_context / 16 | 1.939 ms | 1.834 ms - 2.040 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 60.390 us | 58.969 us - 61.565 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 509.481 us | 496.452 us - 522.465 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 3.494 ms | 3.408 ms - 3.553 ms |
| async_effect_throughput | async_context | 188.121 ms | 187.959 ms - 188.267 ms |
| async_batch_throughput | async_context | 76.304 us | 73.323 us - 79.503 us |
| async_batch_throughput | sync_context_baseline | 10.755 us | 10.676 us - 10.843 us |
| tokio_sync_cached_read | single_task | 1.488 us | 1.476 us - 1.501 us |
| tokio_sync_cached_read | spawn_read | 5.051 us | 4.882 us - 5.218 us |
| tokio_sync_cold_first_get | single_task | 1.425 us | 1.414 us - 1.435 us |
| tokio_sync_cold_first_get | spawn_compute | 5.226 us | 4.983 us - 5.462 us |
| tokio_sync_invalidation | single_task | 39.148 us | 38.805 us - 39.520 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 43.792 us | 43.051 us - 44.677 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 402.701 us | 389.818 us - 416.472 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.734 ms | 3.570 ms - 3.882 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 44.150 us | 43.337 us - 44.963 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 254.247 us | 241.953 us - 268.158 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 1.460 ms | 1.401 ms - 1.518 ms |
| tokio_sync_batch | spawn_batch | 43.228 us | 42.846 us - 43.603 us |
| tokio_sync_effect | single_task | 10.087 ms | 10.083 ms - 10.093 ms |
| typed_cache_reads | context_cell | 2.513 ns | 2.497 ns - 2.530 ns |
| typed_cache_reads | context_rc_cell | 2.771 ns | 2.754 ns - 2.788 ns |
| typed_cache_reads | context_rc_slot | 8.245 ns | 8.197 ns - 8.297 ns |
| typed_cache_reads | context_slot | 7.961 ns | 7.925 ns - 7.998 ns |
| typed_cache_reads | thread_safe_cell | 25.089 ns | 24.867 ns - 25.314 ns |
| typed_cache_reads | thread_safe_slot | 64.338 ns | 64.048 ns - 64.615 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 20.750 us | 24.460 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 430.000 ns | 1.770 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 450.000 ns | 1.420 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 410.000 ns | 1.310 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 460.000 ns | 1.430 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 640.000 ns | 6.220 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 440.000 ns | 2.540 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 940.000 ns | 2.490 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.820 us | 5.640 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 3.710 us | 9.940 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 7.340 us | 22.610 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 5.951 us | 64.820 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 189 | 11.450 us | 92.271 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 307 | 178.663 us | 208.342 us | 0 | 0 | 0 | 41 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 2.175 ms | 251.482 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 11.154 ms | 551.614 us | 0 | 0 | 0 | 1 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.350 us | 21.970 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 29 | 0 | 1 | 0 | 0 | 0 | 57 | 3.140 us | 43.000 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 57 | 0 | 1 | 0 | 0 | 0 | 139 | 10.420 us | 76.440 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 108 | 0 | 1 | 0 | 0 | 0 | 266 | 34.970 us | 305.881 us | 127 | 127 | 1 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 215 | 0 | 1 | 0 | 0 | 0 | 566 | 57.420 us | 626.366 us | 254 | 254 | 2 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 1.340 us | 18.680 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 50 | 10.740 us | 35.580 us | 30 | 30 | 1 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 127 | 438.873 us | 110.692 us | 52 | 52 | 11 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 237 | 2.150 ms | 224.142 us | 109 | 109 | 18 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 492 | 10.671 ms | 440.973 us | 211 | 211 | 44 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.460 us | 21.051 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.220 us | 15.971 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 1.300 us | 21.230 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 47 | 2.470 us | 35.420 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 64 | 112.171 us | 66.751 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 6.400 us | 73.461 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 190 | 67.222 us | 128.930 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 20 | 0 | 16 | 0 | 0 | 0 | 271 | 279.001 us | 196.802 us | 0 | 0 | 0 | 19 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 5 | 0 | 32 | 0 | 0 | 0 | 376 | 2.976 ms | 308.191 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 4 | 0 | 64 | 0 | 0 | 0 | 723 | 11.867 ms | 565.682 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 8 | 1 | 406 | 1.873 ms | 284.094 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 727 | 11.547 ms | 547.994 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 34 | 1 | 416 | 3.181 ms | 249.242 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 30 | 1 | 683 | 12.983 ms | 408.903 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 4.488 ms | 347.764 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 7 | 0 | 65 | 0 | 11 | 1 | 1278 | 20.244 ms | 777.755 us | 0 | 0 | 0 | 8 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 562 | 0 | 64 | 0 | 50 | 1 | 1113 | 27.970 ms | 6.622 ms | 28 | 896 | 100 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 563 | 0 | 64 | 0 | 50 | 1 | 1422 | 156.259 ms | 13.831 ms | 2 | 64 | 254 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 11.220 us | 73.460 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 11.130 us | 65.970 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 15.851 us | 102.800 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 33.060 us | 203.800 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 131 | 0 | 65 | 0 | 25 | 1 | 1397 | 928.746 us | 593.322 us | 0 | 0 | 0 | 214 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 145 | 0 | 129 | 0 | 9 | 1 | 1583 | 10.314 ms | 863.326 us | 0 | 0 | 0 | 164 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 220.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 100.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 60.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 50.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 230.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 90.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 70.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 210.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 90.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 60.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 50.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 260.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 90.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 60.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 50.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 370.000 ns | 1.630 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 150.000 ns | 1.530 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 70.000 ns | 1.490 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 50.000 ns | 1.570 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 220.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 610.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 60.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 70.000 ns | 590.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 490.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 210.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 120.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 120.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 980.000 ns | 1.880 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 430.000 ns | 810.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 240.000 ns | 1.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 170.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 2.000 us | 2.420 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 820.000 ns | 1.610 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 490.000 ns | 3.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 400.000 ns | 2.610 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 3.960 us | 5.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.640 us | 3.990 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 940.000 ns | 7.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 800.000 ns | 5.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 4.550 us | 18.570 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 140.000 ns | 1.600 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 230.000 ns | 2.930 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 981.000 ns | 40.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 50.000 ns | 1.560 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 146 | 8.850 us | 28.511 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 90.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 450.000 ns | 5.270 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 32 | 2.010 us | 57.920 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 50.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 246 | 172.763 us | 87.401 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 90.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 800.000 ns | 8.650 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 42 | 4.950 us | 111.701 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 2.173 ms | 207.152 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 90.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.680 us | 17.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 230.000 ns | 26.590 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 50.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 11.101 ms | 482.194 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 100.000 ns | 520.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 3.330 us | 38.760 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 50.460 us | 29.580 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 50.000 ns | 560.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 250.000 ns | 1.130 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 90.000 ns | 880.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 70.000 ns | 810.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 940.000 ns | 19.150 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 260.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 1.220 us | 1.520 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 60.000 ns | 460.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 29 | 1.600 us | 40.710 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 260.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 10 | 6.640 us | 2.890 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 70.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 57 | 3.450 us | 72.820 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 67 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 370.000 ns | 630.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 14 | 16.810 us | 6.660 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 50.000 ns | 660.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 430.000 ns | 7.990 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 108 | 17.310 us | 289.941 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 136 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 8 | 5.240 us | 1.060 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 44 | 11.790 us | 16.891 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 70.000 ns | 540.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 2 | 210.000 ns | 8.980 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 215 | 40.110 us | 598.895 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 296 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 250.000 ns | 1.500 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 160.000 ns | 1.170 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 70.000 ns | 1.490 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 860.000 ns | 14.520 us |
| thread_safe_contention_independent_slots_2 | other | 10 | 4.550 us | 1.210 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 220.000 ns | 700.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 120.000 ns | 1.070 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 1 | 60.000 ns | 1.840 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 5.790 us | 30.760 us |
| thread_safe_contention_independent_slots_4 | other | 37 | 81.881 us | 3.670 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 400.000 ns | 850.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 250.000 ns | 1.740 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 11 | 33.390 us | 21.500 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 322.952 us | 82.932 us |
| thread_safe_contention_independent_slots_8 | other | 60 | 276.522 us | 5.980 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 860.000 ns | 1.650 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 490.000 ns | 3.620 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 18 | 160.781 us | 38.870 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.712 ms | 174.022 us |
| thread_safe_contention_independent_slots_16 | other | 129 | 1.659 ms | 10.970 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.720 us | 3.360 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 1.000 us | 7.130 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 44 | 1.431 ms | 81.901 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.578 ms | 337.612 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 320.000 ns | 1.870 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 160.000 ns | 1.510 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 70.000 ns | 1.500 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 910.000 ns | 16.171 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 250.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 90.000 ns | 240.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 50.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 830.000 ns | 14.961 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 230.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 2 | 100.000 ns | 190.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 60.000 ns | 410.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 910.000 ns | 20.330 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 230.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 14 | 860.000 ns | 2.780 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 60.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 1.320 us | 31.930 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 240.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 15 | 110.441 us | 9.540 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 60.000 ns | 610.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 1.430 us | 56.171 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 26 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 4.110 us | 16.730 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 110.000 ns | 280.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 200.000 ns | 2.070 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 960.000 ns | 33.371 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 1.020 us | 21.010 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 58.982 us | 35.240 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 90.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 410.000 ns | 5.100 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 2.750 us | 54.790 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 4.990 us | 33.570 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 199 | 267.241 us | 83.390 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 90.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 850.000 ns | 11.150 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 19 | 2.390 us | 55.430 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 20 | 8.430 us | 46.622 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 330 | 2.973 ms | 242.721 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 390.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.760 us | 18.460 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 4 | 290.000 ns | 27.280 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 5 | 290.000 ns | 19.340 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 648 | 11.863 ms | 459.682 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 130.000 ns | 440.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 3.430 us | 39.630 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 3 | 240.000 ns | 39.210 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 4 | 250.000 ns | 26.720 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 366 | 1.824 ms | 228.013 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.970 us | 13.420 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 8 | 46.481 us | 42.661 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 661 | 11.543 ms | 502.113 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 3.970 us | 27.190 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 2 | 120.000 ns | 18.691 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 273 | 1.401 ms | 84.340 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 1.010 us | 11.020 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.779 ms | 153.882 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 396 | 5.894 ms | 118.101 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 2.180 us | 17.831 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 7.087 ms | 272.971 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 4.486 ms | 303.164 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 110.000 ns | 1.000 us |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.760 us | 19.400 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 50.000 ns | 13.830 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 100.000 ns | 10.370 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1196 | 20.239 ms | 637.954 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 110.000 ns | 420.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 3.451 us | 41.010 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 8 | 570.000 ns | 41.270 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 7 | 390.000 ns | 57.101 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 323 | 6.614 ms | 215.621 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.850 us | 6.921 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 3.070 us | 35.760 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 100 | 16.697 ms | 5.685 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 562 | 4.653 ms | 679.473 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 477 | 29.240 ms | 197.811 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.740 us | 6.740 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 3.110 us | 34.510 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 254 | 114.034 ms | 13.015 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 563 | 12.979 ms | 577.195 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 3.570 us | 7.990 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.930 us | 6.200 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.710 us | 19.340 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 3.010 us | 39.930 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 3.650 us | 5.580 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.880 us | 5.330 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.600 us | 16.680 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 3.000 us | 38.380 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 6.820 us | 11.840 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.340 us | 10.360 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.640 us | 27.680 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 3.051 us | 52.920 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 15.870 us | 18.510 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 6.150 us | 15.720 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 4.940 us | 57.190 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 6.100 us | 112.380 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 475 | 771.426 us | 164.091 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 706 | 37.410 us | 95.690 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.600 us | 28.490 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 20 | 69.530 us | 128.740 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 131 | 47.780 us | 176.311 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 805 | 10.254 ms | 429.652 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 498 | 28.050 us | 67.291 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 5.400 us | 60.590 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 6 | 10.500 us | 163.072 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 145 | 16.210 us | 142.721 us |

<!-- benchmark-results:end -->

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

