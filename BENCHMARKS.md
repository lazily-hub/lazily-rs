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
| scale | build | 132.175 ms | 128.025 ms - 136.724 ms |
| scale | cold_full_recalc | 102.417 ms | 100.229 ms - 105.249 ms |
| scale | full_recalc_invalidate_all | 65.130 ms | 64.600 ms - 65.656 ms |
| scale | viewport_recalc | 11.536 us | 11.501 us - 11.570 us |
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 22.580 us | 23.430 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 360.000 ns | 1.930 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 1.360 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 1.380 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 350.000 ns | 1.320 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 370.000 ns | 1.390 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 500.000 ns | 2.470 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 710.000 ns | 2.420 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.370 us | 5.240 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.730 us | 9.780 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 5.540 us | 19.200 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.740 us | 62.261 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 126 | 110.551 us | 106.221 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 199 | 556.653 us | 145.621 us | 0 | 0 | 0 | 6 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 2.445 ms | 286.803 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 8.676 ms | 478.964 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.200 us | 20.941 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 66 | 9.140 us | 42.751 us | 26 | 26 | 6 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 57 | 0 | 1 | 0 | 0 | 0 | 216 | 237.220 us | 172.531 us | 38 | 38 | 26 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 104 | 0 | 1 | 0 | 0 | 0 | 402 | 443.923 us | 382.913 us | 93 | 93 | 35 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 206 | 0 | 1 | 0 | 0 | 0 | 841 | 1.597 ms | 678.325 us | 156 | 156 | 100 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 1.130 us | 16.590 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 92 | 16.960 us | 46.551 us | 16 | 16 | 15 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 214 | 645.424 us | 129.410 us | 18 | 18 | 45 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 442 | 4.182 ms | 316.102 us | 18 | 18 | 109 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 924 | 19.816 ms | 691.133 us | 14 | 14 | 241 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.170 us | 18.030 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 26 | 1.170 us | 18.750 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 48 | 47.660 us | 50.650 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 56 | 8.840 us | 33.060 us | 15 | 15 | 1 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 72 | 99.660 us | 36.070 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.730 us | 72.130 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 193 | 73.550 us | 134.101 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 23 | 0 | 16 | 0 | 0 | 0 | 315 | 588.386 us | 248.594 us | 0 | 0 | 0 | 27 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 9 | 0 | 32 | 0 | 0 | 0 | 399 | 2.049 ms | 265.272 us | 0 | 0 | 0 | 8 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 725 | 6.852 ms | 501.633 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 11 | 1 | 429 | 1.978 ms | 307.712 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 6 | 1 | 744 | 11.931 ms | 759.975 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 32 | 1 | 408 | 2.701 ms | 234.482 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 34 | 1 | 700 | 12.360 ms | 477.394 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 4.812 ms | 363.381 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 7 | 1 | 1255 | 19.251 ms | 720.306 us | 0 | 0 | 0 | 3 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 550 | 0 | 64 | 0 | 49 | 1 | 1133 | 31.187 ms | 7.262 ms | 10 | 320 | 118 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 570 | 0 | 64 | 0 | 50 | 1 | 1391 | 139.207 ms | 13.497 ms | 21 | 672 | 235 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.530 us | 71.232 us | 127 | 4064 | 0 | 4064 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.830 us | 73.341 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 13.400 us | 107.421 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 26.090 us | 195.551 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 601 | 2.249 ms | 398.787 us | 0 | 0 | 0 | 65 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1311 | 5.782 ms | 734.749 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 160.000 ns | 900.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 110.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 60.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 140.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 110.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 50.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 130.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 110.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 60.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 130.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 110.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 70.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 40.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 160.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 110.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 70.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 300.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 120.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 830.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 590.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 300.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 220.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 120.000 ns | 870.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 70.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 570.000 ns | 1.760 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 450.000 ns | 880.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 210.000 ns | 1.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 140.000 ns | 1.050 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.100 us | 2.970 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 920.000 ns | 1.650 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 430.000 ns | 3.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 280.000 ns | 1.960 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.270 us | 5.040 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.800 us | 3.390 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 930.000 ns | 6.740 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 540.000 ns | 4.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.930 us | 18.591 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 120.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 1.610 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 520.000 ns | 41.380 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 104 | 108.991 us | 51.551 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 110.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 360.000 ns | 4.830 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 11 | 1.050 us | 49.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 40.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 174 | 555.403 us | 108.630 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 110.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 670.000 ns | 7.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 6 | 430.000 ns | 28.801 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 2.444 ms | 223.543 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 120.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.370 us | 15.630 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 220.000 ns | 47.140 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 40.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 8.674 ms | 426.014 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 110.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.560 us | 36.700 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 100.000 ns | 15.680 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 190.000 ns | 980.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 120.000 ns | 490.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 60.000 ns | 601.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 830.000 ns | 18.870 us |
| thread_safe_contention_same_slot_write_read_2 | other | 16 | 2.640 us | 980.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 1.150 us | 2.040 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 60.000 ns | 430.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 6 | 880.000 ns | 8.600 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 4.410 us | 30.701 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 53 | 82.960 us | 3.550 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 14 | 10.190 us | 11.560 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 50.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 26 | 53.760 us | 66.450 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 57 | 90.260 us | 90.551 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 65 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 69 | 134.961 us | 5.950 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 22 | 7.500 us | 11.140 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 1.470 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 35 | 206.252 us | 93.940 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 104 | 95.170 us | 270.413 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 171 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 190 | 216.761 us | 10.390 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 44 | 41.041 us | 20.160 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 60.000 ns | 610.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 100 | 1.085 ms | 214.441 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 206 | 253.882 us | 432.724 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 300 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 170.000 ns | 910.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 120.000 ns | 530.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 60.000 ns | 740.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 780.000 ns | 14.410 us |
| thread_safe_contention_independent_slots_2 | other | 38 | 1.260 us | 1.980 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 240.000 ns | 470.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 120.000 ns | 840.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 15 | 8.450 us | 13.590 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 6.890 us | 29.671 us |
| thread_safe_contention_independent_slots_4 | other | 90 | 165.681 us | 5.140 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 440.000 ns | 910.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 240.000 ns | 1.600 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 45 | 267.281 us | 50.210 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 211.782 us | 71.550 us |
| thread_safe_contention_independent_slots_8 | other | 174 | 1.101 ms | 10.040 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 880.000 ns | 1.590 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 430.000 ns | 3.060 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 109 | 1.755 ms | 141.021 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.325 ms | 160.391 us |
| thread_safe_contention_independent_slots_16 | other | 364 | 5.877 ms | 20.930 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.790 us | 3.200 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 930.000 ns | 6.530 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 241 | 7.386 ms | 318.673 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.550 ms | 341.800 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 150.000 ns | 1.020 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 130.000 ns | 630.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 50.000 ns | 740.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 840.000 ns | 15.640 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 170.000 ns | 500.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 110.000 ns | 220.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 60.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 830.000 ns | 17.610 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 170.000 ns | 530.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 13 | 38.210 us | 5.960 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 60.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 9.220 us | 43.730 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 6 | 190.000 ns | 650.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 12 | 6.010 us | 4.470 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 60.000 ns | 540.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 1 | 440.000 ns | 2.110 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 2.140 us | 25.290 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 130.000 ns | 530.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 24 | 88.880 us | 9.210 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 60.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 10.590 us | 25.900 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 25 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 3.270 us | 17.210 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 110.000 ns | 270.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 150.000 ns | 1.810 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 660.000 ns | 32.120 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 540.000 ns | 20.720 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 69.040 us | 36.290 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 120.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 270.000 ns | 4.780 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 2.940 us | 56.691 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 1.180 us | 36.110 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 211 | 564.766 us | 93.832 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 9.970 us | 1.060 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 630.000 ns | 8.660 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 27 | 8.420 us | 91.712 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 23 | 4.600 us | 53.330 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 34 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 338 | 2.047 ms | 194.281 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.290 us | 15.860 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 8 | 410.000 ns | 27.200 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 9 | 380.000 ns | 27.701 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 6.849 ms | 399.313 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 120.000 ns | 250.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.670 us | 37.730 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 200.000 ns | 22.760 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 200.000 ns | 41.580 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 387 | 1.976 ms | 253.491 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.320 us | 12.300 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 10 | 530.000 ns | 41.921 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 674 | 11.926 ms | 685.855 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.631 us | 26.150 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 6 | 1.850 us | 47.970 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 265 | 1.382 ms | 74.770 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 840.000 ns | 11.450 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.318 ms | 148.262 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 5.103 ms | 127.062 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.750 us | 20.530 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 7.256 ms | 329.802 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 4.810 ms | 319.031 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 100.000 ns | 840.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.460 us | 19.230 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 80.000 ns | 14.380 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 80.000 ns | 9.900 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1181 | 19.248 ms | 623.675 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 130.000 ns | 240.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.740 us | 36.731 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 3 | 140.000 ns | 23.850 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 160.000 ns | 35.810 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 337 | 9.668 ms | 204.550 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 3.540 us | 7.410 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 3.810 us | 36.050 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 118 | 16.197 ms | 6.417 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 550 | 5.315 ms | 596.881 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 458 | 37.962 ms | 206.502 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 3.520 us | 7.250 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 3.700 us | 34.880 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 235 | 89.314 ms | 12.658 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 570 | 11.923 ms | 590.186 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.140 us | 8.120 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 3.220 us | 6.290 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.620 us | 18.660 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.550 us | 38.162 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 2.030 us | 6.300 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 3.260 us | 6.460 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.780 us | 19.050 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.760 us | 41.531 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.940 us | 12.421 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.510 us | 10.530 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 3.400 us | 30.380 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.550 us | 54.090 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.720 us | 18.500 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 6.520 us | 17.050 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 6.790 us | 60.550 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.060 us | 99.451 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 401 | 2.240 ms | 224.034 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 3.210 us | 8.630 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 3.380 us | 28.661 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 1 | 60.000 ns | 85.931 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 2.480 us | 51.531 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 791 | 5.756 ms | 366.545 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 13.220 us | 31.700 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 6.720 us | 56.902 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 120.000 ns | 165.322 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 5.880 us | 114.280 us |

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
