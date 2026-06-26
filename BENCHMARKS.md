# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.13.0`.

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
| cached ThreadSafeContext read latency | a8b6fc3 vs c917401 | `cargo bench --features instrumentation --bench context -- cached_reads/thread_safe_context` | 73.48 ns baseline vs 73.20 ns current on warm-cache repeat | no tuning; the archived 56.5 ns row did not reproduce under controlled A/B |
| effect cleanup contention at 16 workers | a8b6fc3 vs c917401 | `cargo bench --features instrumentation --bench context -- thread_safe_effect_contention/cleanup_execution/16` | 2.31 ms baseline vs 2.43 ms current on warm-cache repeat with overlapping CIs | keep watching; Criterion reported no statistically significant change |

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 2.669 ms | 3.040 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.075 ms | 8.204 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 2.091 ms | 2.413 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.607 ms | 5.382 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 593.644 us | 604.772 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.213 ms | 1.332 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.652 ms | 2.803 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.702 ms | 4.146 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.330 ms | 1.517 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.250 ms | 3.539 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.362 ms | 1.525 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.240 ms | 3.494 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.442 ms | 2.864 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 5.834 ms | 6.873 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.607 ms | 3.863 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.968 ms | 6.218 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.072 ms | 2.248 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.085 ms | 4.204 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 461.992 us | 478.225 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 957.850 us | 1.238 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.171 ms | 1.234 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.095 ms | 2.464 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 16.844 ns | 16.472 ns - 17.454 ns |
| cached_reads | thread_safe_context | 80.189 ns | 78.921 ns - 81.414 ns |
| cold_first_get | context | 311.046 ns | 260.641 ns - 362.486 ns |
| cold_first_get | thread_safe_context | 1.146 us | 1.096 us - 1.195 us |
| dependency_fan_out | context / 32 | 3.954 us | 3.528 us - 4.525 us |
| dependency_fan_out | context / 256 | 71.091 us | 59.329 us - 85.959 us |
| dependency_fan_out | thread_safe_context / 32 | 26.667 us | 23.590 us - 30.366 us |
| dependency_fan_out | thread_safe_context / 256 | 237.263 us | 206.859 us - 268.845 us |
| set_cell_invalidation | high_fan_out / 512 | 271.697 us | 257.587 us - 286.265 us |
| set_cell_invalidation | same_slot_contention / 1 | 623.215 us | 596.612 us - 647.877 us |
| set_cell_invalidation | same_slot_contention / 2 | 975.343 us | 947.116 us - 1.001 ms |
| set_cell_invalidation | same_slot_contention / 4 | 1.431 ms | 1.376 ms - 1.481 ms |
| set_cell_invalidation | same_slot_contention / 8 | 1.890 ms | 1.844 ms - 1.935 ms |
| set_cell_invalidation | same_slot_contention / 16 | 2.276 ms | 2.193 ms - 2.360 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 648.533 us | 592.003 us - 691.819 us |
| set_cell_invalidation | independent_slot_contention / 2 | 119.854 us | 102.519 us - 143.975 us |
| set_cell_invalidation | independent_slot_contention / 4 | 123.044 us | 117.341 us - 128.884 us |
| set_cell_invalidation | independent_slot_contention / 8 | 229.728 us | 218.381 us - 239.955 us |
| set_cell_invalidation | independent_slot_contention / 16 | 468.519 us | 450.851 us - 486.819 us |
| set_cell_invalidation | batched_write_bursts / 1 | 137.637 us | 136.503 us - 138.818 us |
| set_cell_invalidation | batched_write_bursts / 2 | 213.590 us | 204.856 us - 221.296 us |
| set_cell_invalidation | batched_write_bursts / 4 | 494.473 us | 474.613 us - 511.524 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.273 ms | 1.222 ms - 1.322 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.920 ms | 2.776 ms - 3.066 ms |
| memo_equality_suppression | context | 3.359 us | 2.786 us - 3.955 us |
| memo_equality_suppression | thread_safe_context | 39.419 us | 34.920 us - 45.083 us |
| effect_flushing | context | 80.287 ns | 75.208 ns - 84.773 ns |
| effect_flushing | thread_safe_context | 1.011 us | 967.591 ns - 1.056 us |
| batch_storms | context / 64 | 2.714 us | 2.698 us - 2.730 us |
| batch_storms | thread_safe_context / 64 | 6.705 us | 6.676 us - 6.737 us |
| thread_safe_contention | same_slot_write_read / 1 | 109.761 us | 107.229 us - 112.383 us |
| thread_safe_contention | same_slot_write_read / 2 | 310.063 us | 299.186 us - 320.237 us |
| thread_safe_contention | same_slot_write_read / 4 | 770.761 us | 717.895 us - 832.679 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.617 ms | 2.372 ms - 2.841 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.334 ms | 5.832 ms - 6.913 ms |
| thread_safe_contention | independent_slots / 1 | 114.333 us | 111.137 us - 118.005 us |
| thread_safe_contention | independent_slots / 2 | 237.042 us | 224.899 us - 250.134 us |
| thread_safe_contention | independent_slots / 4 | 655.032 us | 635.793 us - 678.544 us |
| thread_safe_contention | independent_slots / 8 | 2.095 ms | 1.992 ms - 2.198 ms |
| thread_safe_contention | independent_slots / 16 | 4.760 ms | 4.420 ms - 5.070 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 106.983 us | 106.002 us - 107.931 us |
| thread_safe_contention | read_mostly_waiters / 2 | 152.713 us | 151.454 us - 154.282 us |
| thread_safe_contention | read_mostly_waiters / 4 | 265.597 us | 258.096 us - 275.672 us |
| thread_safe_contention | read_mostly_waiters / 8 | 590.493 us | 581.746 us - 598.357 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.240 ms | 1.194 ms - 1.285 ms |
| thread_safe_contention | batched_write_bursts / 1 | 215.078 us | 213.792 us - 216.471 us |
| thread_safe_contention | batched_write_bursts / 2 | 591.617 us | 567.678 us - 616.740 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.610 ms | 1.599 ms - 1.622 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.677 ms | 2.643 ms - 2.715 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.773 ms | 3.635 ms - 3.921 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.334 ms | 1.252 ms - 1.417 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.256 ms | 3.145 ms - 3.364 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.378 ms | 1.315 ms - 1.434 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.186 ms | 3.055 ms - 3.310 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.471 ms | 2.280 ms - 2.655 ms |
| thread_safe_effect_contention | batch_flush / 16 | 5.895 ms | 5.330 ms - 6.354 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.655 ms | 3.604 ms - 3.718 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.000 ms | 5.952 ms - 6.059 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.071 ms | 2.004 ms - 2.137 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.102 ms | 4.052 ms - 4.149 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 462.780 us | 456.396 us - 468.901 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.007 ms | 960.645 us - 1.069 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.176 ms | 1.153 ms - 1.198 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.152 ms | 2.089 ms - 2.235 ms |
| profile_instrumentation | context_snapshot | 265.129 ns | 263.001 ns - 267.270 ns |
| profile_instrumentation | thread_safe_snapshot | 292.822 us | 291.211 us - 294.456 us |
| async_cached_resolve | async_context | 3.863 us | 3.623 us - 4.140 us |
| async_cached_resolve | sync_context_baseline | 70.085 ns | 69.694 ns - 70.533 ns |
| async_cached_resolve | sync_get | 11.985 ns | 11.904 ns - 12.065 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.353 us | 1.348 us - 1.358 us |
| async_cold_resolve | async_context | 3.724 us | 3.534 us - 3.939 us |
| async_cold_resolve | sync_context_baseline | 98.712 ns | 87.324 ns - 112.109 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.351 us | 1.088 us - 1.676 us |
| async_invalidation_throughput | async_context | 264.564 us | 241.035 us - 291.823 us |
| async_invalidation_throughput | sync_context_baseline | 2.966 us | 2.779 us - 3.211 us |
| async_invalidation_throughput | thread_safe_context_baseline | 39.162 us | 39.011 us - 39.314 us |
| async_cancellation_throughput | async_invalidate_in_flight | 74.831 us | 58.353 us - 89.856 us |
| async_concurrent_contention | async_context / 1 | 81.216 us | 78.566 us - 84.076 us |
| async_concurrent_contention | async_context / 4 | 320.293 us | 287.559 us - 360.092 us |
| async_concurrent_contention | async_context / 16 | 1.419 ms | 1.233 ms - 1.607 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 80.819 us | 70.096 us - 96.386 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 639.268 us | 542.218 us - 760.536 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 2.539 ms | 2.397 ms - 2.706 ms |
| async_effect_throughput | async_context | 189.554 ms | 189.263 ms - 189.900 ms |
| async_batch_throughput | async_context | 91.011 us | 84.187 us - 99.506 us |
| async_batch_throughput | sync_context_baseline | 19.443 us | 19.258 us - 19.625 us |
| tokio_sync_cached_read | single_task | 1.452 us | 1.448 us - 1.457 us |
| tokio_sync_cached_read | spawn_read | 4.559 us | 4.413 us - 4.709 us |
| tokio_sync_cold_first_get | single_task | 1.410 us | 1.399 us - 1.423 us |
| tokio_sync_cold_first_get | spawn_compute | 4.625 us | 4.402 us - 4.889 us |
| tokio_sync_invalidation | single_task | 38.835 us | 38.629 us - 39.073 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 42.775 us | 42.167 us - 43.500 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 265.588 us | 257.390 us - 273.318 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 2.454 ms | 2.105 ms - 2.835 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 42.263 us | 41.939 us - 42.663 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 280.514 us | 254.045 us - 310.292 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 1.758 ms | 1.624 ms - 1.909 ms |
| tokio_sync_batch | spawn_batch | 42.992 us | 42.818 us - 43.171 us |
| tokio_sync_effect | single_task | 10.085 ms | 10.082 ms - 10.088 ms |
| scale | build | 139.323 ms | 136.332 ms - 142.560 ms |
| scale | cold_full_recalc | 103.871 ms | 101.869 ms - 105.887 ms |
| scale | full_recalc_invalidate_all | 69.329 ms | 68.354 ms - 70.513 ms |
| scale | viewport_recalc | 11.402 us | 11.357 us - 11.449 us |
| typed_cache_reads | context_cell | 2.489 ns | 2.478 ns - 2.503 ns |
| typed_cache_reads | context_rc_cell | 2.752 ns | 2.737 ns - 2.770 ns |
| typed_cache_reads | context_rc_slot | 7.990 ns | 7.980 ns - 8.003 ns |
| typed_cache_reads | context_slot | 7.778 ns | 7.757 ns - 7.800 ns |
| typed_cache_reads | thread_safe_cell | 24.645 ns | 24.537 ns - 24.768 ns |
| typed_cache_reads | thread_safe_slot | 65.488 ns | 65.131 ns - 65.804 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 19.300 us | 12.300 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 2.200 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 970.000 ns | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 900.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 910.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 360.000 ns | 5.480 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 2.030 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 650.000 ns | 1.940 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.330 us | 4.230 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.640 us | 7.400 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 5.270 us | 18.760 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.740 us | 49.801 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 135 | 92.481 us | 72.511 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 210 | 469.022 us | 124.770 us | 0 | 0 | 0 | 10 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 372 | 1.914 ms | 230.932 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 721 | 7.828 ms | 416.403 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 760.000 ns | 18.050 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 68 | 11.940 us | 35.570 us | 25 | 25 | 7 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 54 | 0 | 1 | 0 | 0 | 0 | 219 | 111.243 us | 107.310 us | 37 | 37 | 27 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 105 | 0 | 1 | 0 | 0 | 0 | 403 | 288.140 us | 325.653 us | 92 | 92 | 36 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 203 | 0 | 1 | 0 | 0 | 0 | 805 | 1.062 ms | 634.264 us | 185 | 185 | 71 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 710.000 ns | 14.520 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 89 | 21.431 us | 40.811 us | 17 | 17 | 14 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 233 | 702.338 us | 124.690 us | 11 | 11 | 52 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 449 | 4.348 ms | 284.801 us | 14 | 14 | 113 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 917 | 17.820 ms | 572.926 us | 14 | 14 | 241 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 760.000 ns | 15.290 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 820.000 ns | 13.230 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 36 | 9.300 us | 21.900 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 49 | 8.020 us | 32.210 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 79 | 126.532 us | 84.521 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.580 us | 68.130 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 21 | 0 | 8 | 0 | 0 | 0 | 188 | 64.370 us | 112.990 us | 0 | 0 | 0 | 20 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 31 | 0 | 16 | 0 | 0 | 0 | 359 | 416.545 us | 248.963 us | 0 | 0 | 0 | 35 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 14 | 0 | 32 | 0 | 0 | 0 | 417 | 2.373 ms | 294.391 us | 0 | 0 | 0 | 14 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 12 | 0 | 64 | 0 | 0 | 0 | 763 | 10.835 ms | 585.859 us | 0 | 0 | 0 | 11 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 392 | 2.234 ms | 233.902 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 740 | 9.991 ms | 467.345 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 31 | 1 | 404 | 1.687 ms | 162.030 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 32 | 1 | 696 | 6.824 ms | 281.702 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 2.791 ms | 257.650 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 16.295 ms | 556.346 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 561 | 0 | 64 | 0 | 50 | 1 | 1160 | 27.824 ms | 6.322 ms | 4 | 128 | 124 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 550 | 0 | 64 | 0 | 49 | 1 | 1409 | 120.774 ms | 12.195 ms | 0 | 0 | 256 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.280 us | 71.310 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.340 us | 57.450 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 12.870 us | 98.091 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 25.100 us | 173.331 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 71 | 0 | 65 | 0 | 5 | 1 | 679 | 3.186 ms | 422.083 us | 0 | 0 | 0 | 98 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1315 | 8.186 ms | 729.737 us | 0 | 0 | 0 | 146 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 120.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 100.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 140.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 90.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 170.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 150.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 180.000 ns | 1.550 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 130.000 ns | 1.460 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 1.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 140.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 80.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 370.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 160.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 60.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 800.000 ns | 1.570 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 320.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 110.000 ns | 1.130 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.590 us | 2.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 620.000 ns | 1.310 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 220.000 ns | 2.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 210.000 ns | 1.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 3.150 us | 5.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.290 us | 3.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 430.000 ns | 5.310 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 400.000 ns | 4.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 3.700 us | 15.231 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 110.000 ns | 750.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.780 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 790.000 ns | 31.270 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 770.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 110 | 91.021 us | 30.891 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 3.910 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 1.160 us | 37.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 181 | 467.762 us | 77.560 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 420.000 ns | 6.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 10 | 730.000 ns | 40.680 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 332 | 1.913 ms | 177.162 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 830.000 ns | 12.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 290.000 ns | 41.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 650 | 7.826 ms | 365.493 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.830 us | 26.890 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 4 | 210.000 ns | 23.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 280.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 150.000 ns | 1.500 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 860.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 870.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 450.000 ns | 14.820 us |
| thread_safe_contention_same_slot_write_read_2 | other | 18 | 2.230 us | 730.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 70.000 ns | 200.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 7 | 1.650 us | 9.700 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 7.970 us | 24.650 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 56 | 34.522 us | 2.230 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 16 | 7.620 us | 5.130 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 400.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 27 | 32.110 us | 34.220 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 54 | 36.961 us | 65.330 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 65 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 73 | 75.020 us | 3.430 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 30 | 15.490 us | 8.790 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 36 | 136.710 us | 64.370 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 105 | 60.900 us | 248.673 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 158 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 137 | 193.262 us | 6.420 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 62 | 31.651 us | 13.570 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 20.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 71 | 632.042 us | 155.811 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 203 | 204.851 us | 458.043 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 331 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 130.000 ns | 1.280 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 90.000 ns | 330.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 1.130 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 470.000 ns | 11.780 us |
| thread_safe_contention_independent_slots_2 | other | 36 | 1.750 us | 2.280 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 170.000 ns | 710.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 1.080 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 14 | 7.250 us | 12.090 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 12.211 us | 24.651 us |
| thread_safe_contention_independent_slots_4 | other | 102 | 148.391 us | 4.270 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 310.000 ns | 730.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.120 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 52 | 184.531 us | 51.810 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 368.996 us | 66.760 us |
| thread_safe_contention_independent_slots_8 | other | 177 | 1.199 ms | 9.060 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 620.000 ns | 1.290 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 200.000 ns | 2.250 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 113 | 1.270 ms | 126.541 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.879 ms | 145.660 us |
| thread_safe_contention_independent_slots_16 | other | 357 | 3.973 ms | 16.550 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.220 us | 2.810 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 390.000 ns | 4.561 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 241 | 5.925 ms | 258.581 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.920 ms | 290.424 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 160.000 ns | 1.110 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 90.000 ns | 630.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 830.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 490.000 ns | 12.720 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 190.000 ns | 320.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 70.000 ns | 220.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 540.000 ns | 12.380 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 170.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 7.820 us | 2.730 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 1.280 us | 18.490 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 6 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 170.000 ns | 480.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 5.970 us | 1.710 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 1.840 us | 29.690 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 190.000 ns | 1.400 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 25 | 103.252 us | 8.520 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 1.150 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 23.060 us | 73.451 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 31 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 3.160 us | 14.740 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 240.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 1.630 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 790.000 ns | 32.240 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 450.000 ns | 19.280 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 55.450 us | 30.080 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.910 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.600 us | 49.820 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 21 | 7.020 us | 28.980 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 230 | 393.055 us | 80.041 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 8.440 us | 1.250 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 430.000 ns | 7.351 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 35 | 3.450 us | 89.841 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 31 | 11.170 us | 70.480 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 43 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 349 | 2.370 ms | 186.151 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 110.000 ns | 430.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 840.000 ns | 13.250 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 14 | 920.000 ns | 41.420 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 14 | 650.000 ns | 53.140 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 6 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 666 | 10.797 ms | 416.235 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 280.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.730 us | 29.821 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 12 | 35.820 us | 56.512 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 12 | 640.000 ns | 83.011 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 354 | 2.233 ms | 202.512 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 890.000 ns | 9.450 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 6 | 300.000 ns | 21.940 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 672 | 9.989 ms | 420.974 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.700 us | 20.080 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 4 | 230.000 ns | 26.291 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 261 | 440.002 us | 50.870 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 550.000 ns | 6.600 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.247 ms | 104.560 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 409 | 2.435 ms | 86.021 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.140 us | 10.810 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 4.387 ms | 184.871 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 2.790 ms | 227.350 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 80.000 ns | 720.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 910.000 ns | 13.120 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 50.000 ns | 9.110 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 60.000 ns | 7.350 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 16.293 ms | 506.716 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 90.000 ns | 630.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.850 us | 29.120 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 40.000 ns | 9.950 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 60.000 ns | 9.930 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 347 | 4.347 ms | 165.511 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.500 us | 5.000 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.720 us | 24.280 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 124 | 18.614 ms | 5.607 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 561 | 4.858 ms | 520.134 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 475 | 20.635 ms | 171.351 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.430 us | 5.821 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.720 us | 26.030 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 88.778 ms | 11.493 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 550 | 11.357 ms | 498.723 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 3.190 us | 6.720 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.390 us | 5.540 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 860.000 ns | 14.880 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.840 us | 44.170 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 3.220 us | 4.350 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.450 us | 5.200 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 900.000 ns | 13.150 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.770 us | 34.750 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 6.430 us | 10.970 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.870 us | 10.650 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.790 us | 23.790 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.780 us | 52.681 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 12.880 us | 18.730 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 5.160 us | 13.040 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.560 us | 48.191 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.500 us | 93.370 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 411 | 3.070 ms | 213.713 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 126 | 5.140 us | 15.810 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.750 us | 22.820 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 6 | 41.751 us | 107.000 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 71 | 66.970 us | 62.740 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 793 | 8.145 ms | 380.433 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 29.401 us | 29.510 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.500 us | 47.230 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 4 | 4.110 us | 161.742 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 3.690 us | 110.822 us |

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

**Reading the result honestly:** lazily wins the bulk-graph operations — building
the sheet (1.5×), computing it cold (3.6×), and recomputing the whole sheet after a
full invalidation (2.8×) — driven by its sparse arena + lean single-threaded
`Context` versus leptos's runtime slotmap and subscriber bookkeeping. On the
cached-read-dominated `viewport_recalc` case the two are close and leptos is
actually a touch faster (its memo cache-hit read path is slightly leaner at this
size; only ~2 of the 1,000 viewport cells actually recompute). The shared headline
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
