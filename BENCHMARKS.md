# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.12.3`.

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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 20.010 us | 18.050 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.600 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.040 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 980.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.060 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 440.000 ns | 5.020 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 2.950 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 580.000 ns | 1.980 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.270 us | 8.070 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.230 us | 11.850 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.470 us | 19.220 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.810 us | 48.300 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 126 | 75.811 us | 69.541 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 196 | 359.344 us | 109.241 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 360 | 1.222 ms | 202.261 us | 0 | 0 | 0 | 1 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 5.932 ms | 375.332 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.220 us | 25.361 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 30 | 0 | 1 | 0 | 0 | 0 | 81 | 52.100 us | 74.720 us | 23 | 23 | 9 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 53 | 0 | 1 | 0 | 0 | 0 | 218 | 126.530 us | 114.181 us | 36 | 36 | 28 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 98 | 0 | 1 | 0 | 0 | 0 | 429 | 262.364 us | 213.784 us | 74 | 74 | 54 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 213 | 0 | 1 | 0 | 0 | 0 | 876 | 891.205 us | 412.414 us | 134 | 134 | 122 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 940.000 ns | 14.461 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 86 | 46.330 us | 62.640 us | 18 | 18 | 13 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 229 | 765.796 us | 147.251 us | 14 | 14 | 49 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 465 | 4.487 ms | 312.541 us | 8 | 8 | 119 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 908 | 14.321 ms | 532.184 us | 14 | 14 | 241 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.010 us | 15.370 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 990.000 ns | 16.050 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 36 | 1.270 us | 22.090 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 44 | 2.400 us | 23.160 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 58 | 2.500 us | 68.831 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.740 us | 71.470 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 28 | 0 | 8 | 0 | 0 | 0 | 222 | 120.221 us | 170.722 us | 0 | 0 | 0 | 30 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 46 | 0 | 16 | 0 | 0 | 0 | 430 | 292.744 us | 300.931 us | 0 | 0 | 0 | 46 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 5 | 0 | 32 | 0 | 0 | 0 | 377 | 2.330 ms | 248.951 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 718 | 6.460 ms | 424.745 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 378 | 2.516 ms | 258.933 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 732 | 9.759 ms | 474.175 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 31 | 1 | 408 | 2.332 ms | 200.012 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 40 | 1 | 712 | 10.318 ms | 343.433 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 10 | 0 | 33 | 0 | 15 | 1 | 693 | 2.689 ms | 318.563 us | 0 | 0 | 0 | 13 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 3 | 1 | 1246 | 17.179 ms | 616.195 us | 0 | 0 | 0 | 4 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 553 | 0 | 64 | 0 | 49 | 1 | 1136 | 24.225 ms | 7.358 ms | 10 | 320 | 118 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 550 | 0 | 64 | 0 | 49 | 1 | 1207 | 66.258 ms | 7.866 ms | 101 | 3232 | 155 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.551 us | 68.780 us | 127 | 4064 | 0 | 4064 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.250 us | 59.171 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 12.590 us | 95.610 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 23.980 us | 199.632 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 81 | 0 | 65 | 0 | 9 | 1 | 811 | 2.227 ms | 417.643 us | 0 | 0 | 0 | 92 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1185 | 9.574 ms | 716.935 us | 0 | 0 | 0 | 145 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 130.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 90.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 140.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 90.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 150.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 90.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 140.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 90.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 40.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 220.000 ns | 1.320 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 140.000 ns | 1.350 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 50.000 ns | 1.180 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 1.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 150.000 ns | 780.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 110.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 800.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 230.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 200.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 90.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 60.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 540.000 ns | 2.260 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 430.000 ns | 1.860 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 190.000 ns | 2.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 1.740 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 850.000 ns | 3.060 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 810.000 ns | 2.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 370.000 ns | 3.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 200.000 ns | 2.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.700 us | 4.830 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.660 us | 3.950 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 700.000 ns | 6.380 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 410.000 ns | 4.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.100 us | 14.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 1.680 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 450.000 ns | 31.660 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 104 | 75.061 us | 32.710 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 100.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 250.000 ns | 4.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 11 | 370.000 ns | 32.371 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 172 | 358.504 us | 79.551 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 7.140 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 260.000 ns | 21.820 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 50.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 324 | 1.221 ms | 175.361 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 90.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 970.000 ns | 12.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 1 | 50.000 ns | 13.730 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 5.930 ms | 323.431 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.090 us | 28.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 120.000 ns | 22.801 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 160.000 ns | 1.320 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 140.000 ns | 1.090 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 50.000 ns | 1.190 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 870.000 ns | 21.761 us |
| thread_safe_contention_same_slot_write_read_2 | other | 22 | 8.900 us | 1.430 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 4.570 us | 1.610 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 400.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 9 | 4.720 us | 22.120 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 30 | 33.870 us | 49.160 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 58 | 35.510 us | 2.380 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 10 | 2.150 us | 3.540 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 28 | 44.960 us | 41.191 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 53 | 43.870 us | 66.660 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 68 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 110 | 61.732 us | 5.221 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 6 | 1.770 us | 4.120 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 1.280 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 54 | 99.661 us | 76.093 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 98 | 99.161 us | 127.070 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 160 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 239 | 240.794 us | 8.060 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 22 | 6.991 us | 7.600 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 50.000 ns | 630.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 122 | 476.848 us | 142.821 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 213 | 166.522 us | 253.303 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 279 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 160.000 ns | 1.210 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 90.000 ns | 440.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 50.000 ns | 1.291 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 640.000 ns | 11.520 us |
| thread_safe_contention_independent_slots_2 | other | 34 | 940.000 ns | 2.280 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 200.000 ns | 460.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 100.000 ns | 750.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 13 | 9.610 us | 25.860 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 35.480 us | 33.290 us |
| thread_safe_contention_independent_slots_4 | other | 101 | 236.510 us | 6.600 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 430.000 ns | 1.910 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 160.000 ns | 2.280 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 49 | 244.453 us | 60.060 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 284.243 us | 76.401 us |
| thread_safe_contention_independent_slots_8 | other | 187 | 995.607 us | 10.460 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 780.000 ns | 2.640 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 380.000 ns | 3.610 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 119 | 1.590 ms | 135.891 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.900 ms | 159.940 us |
| thread_safe_contention_independent_slots_16 | other | 348 | 3.552 ms | 15.231 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.600 us | 2.990 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 760.000 ns | 6.140 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 241 | 5.876 ms | 246.291 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 4.891 ms | 261.532 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 150.000 ns | 920.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 120.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 780.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 700.000 ns | 13.280 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 140.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 110.000 ns | 190.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 700.000 ns | 15.070 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 130.000 ns | 460.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 6 | 300.000 ns | 1.230 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 50.000 ns | 400.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 790.000 ns | 20.000 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 8 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 530.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 1.500 us | 1.560 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 50.000 ns | 440.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 740.000 ns | 20.630 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 130.000 ns | 500.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 8 | 1.590 us | 2.430 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 50.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 730.000 ns | 65.471 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 28 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.560 us | 24.170 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 90.000 ns | 310.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 150.000 ns | 1.780 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 450.000 ns | 27.970 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 490.000 ns | 17.240 us |
| thread_safe_contention_batched_write_bursts_2 | other | 140 | 95.021 us | 37.012 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 100.000 ns | 240.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 260.000 ns | 3.920 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 30 | 5.180 us | 68.830 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 28 | 19.660 us | 60.720 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 253 | 263.954 us | 65.951 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 10 | 14.900 us | 3.410 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 530.000 ns | 6.720 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 46 | 6.460 us | 113.720 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 46 | 6.900 us | 111.130 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 59 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 330 | 2.329 ms | 199.411 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 110.000 ns | 280.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 930.000 ns | 13.320 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 4 | 140.000 ns | 23.130 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 5 | 210.000 ns | 12.810 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 646 | 6.454 ms | 350.484 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 120.000 ns | 590.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.110 us | 31.571 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 2 | 3.220 us | 28.480 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 3 | 150.000 ns | 13.620 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 342 | 2.486 ms | 215.913 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.140 us | 10.000 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 4 | 28.920 us | 33.020 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 665 | 9.743 ms | 427.865 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.230 us | 20.710 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 13.720 us | 25.600 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 265 | 1.021 ms | 63.402 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 690.000 ns | 9.810 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.310 ms | 126.800 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 425 | 4.877 ms | 106.730 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.480 us | 13.300 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.440 ms | 223.403 us |
| thread_safe_effect_contention_batch_flush_8 | other | 635 | 2.680 ms | 226.111 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 100.000 ns | 600.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.140 us | 14.301 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 13 | 4.860 us | 39.961 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 10 | 3.200 us | 37.590 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1172 | 17.176 ms | 533.184 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 100.000 ns | 200.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.290 us | 30.920 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 4 | 320.000 ns | 40.201 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 190.000 ns | 11.690 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 337 | 5.389 ms | 219.754 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 3.210 us | 6.550 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 3.050 us | 29.940 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 118 | 13.590 ms | 6.427 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 553 | 5.239 ms | 674.747 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 374 | 11.390 ms | 179.461 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.970 us | 7.000 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 3.230 us | 30.910 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 155 | 49.011 ms | 7.151 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 550 | 5.851 ms | 498.092 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.020 us | 5.530 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 3.230 us | 6.820 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.511 us | 18.750 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.790 us | 37.680 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.930 us | 4.910 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 3.200 us | 5.760 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.500 us | 16.441 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.620 us | 32.060 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.590 us | 9.230 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.480 us | 10.320 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 3.070 us | 26.940 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.450 us | 49.120 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.170 us | 35.550 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 5.900 us | 15.230 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 5.970 us | 56.541 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 4.940 us | 92.311 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 419 | 2.210 ms | 202.372 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 242 | 11.331 us | 30.150 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 3.150 us | 27.070 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 4 | 160.000 ns | 80.831 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 81 | 2.910 us | 77.220 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 789 | 9.520 ms | 396.993 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 26.801 us | 15.350 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 6.590 us | 57.030 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 5 | 4.560 us | 156.162 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 15.750 us | 91.400 us |

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
