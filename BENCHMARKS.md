# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.12.2`.

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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 4.850 us | 14.490 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 1.400 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 210.000 ns | 1.180 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.010 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 940.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 1.010 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 5.130 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 430.000 ns | 2.190 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 840.000 ns | 4.230 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 1.910 us | 9.020 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.480 us | 18.451 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.940 us | 48.670 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 141 | 73.200 us | 77.421 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 199 | 432.105 us | 128.612 us | 0 | 0 | 0 | 6 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 1.499 ms | 184.031 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 724 | 8.703 ms | 408.336 us | 0 | 0 | 0 | 5 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 660.000 ns | 14.730 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 26 | 0 | 1 | 0 | 0 | 0 | 71 | 9.930 us | 37.311 us | 25 | 25 | 7 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 52 | 0 | 1 | 0 | 0 | 0 | 167 | 183.130 us | 158.891 us | 49 | 49 | 15 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 96 | 0 | 1 | 0 | 0 | 0 | 415 | 448.914 us | 308.193 us | 85 | 85 | 43 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 206 | 0 | 1 | 0 | 0 | 0 | 778 | 1.297 ms | 623.994 us | 187 | 187 | 69 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 590.000 ns | 12.320 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 89 | 16.010 us | 35.160 us | 17 | 17 | 14 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 220 | 701.816 us | 116.761 us | 16 | 16 | 47 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 471 | 4.597 ms | 279.461 us | 10 | 10 | 117 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 934 | 20.394 ms | 588.275 us | 8 | 8 | 247 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 710.000 ns | 12.480 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 26 | 730.000 ns | 14.330 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 41 | 830.000 ns | 16.920 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 59 | 14.340 us | 57.000 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 26 | 800.000 ns | 20.750 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.330 us | 57.131 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 188 | 54.670 us | 99.690 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 31 | 0 | 16 | 0 | 0 | 0 | 359 | 301.534 us | 199.611 us | 0 | 0 | 0 | 34 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 3 | 0 | 32 | 0 | 0 | 0 | 368 | 1.508 ms | 199.201 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 10 | 0 | 64 | 0 | 0 | 0 | 756 | 10.247 ms | 471.926 us | 0 | 0 | 0 | 9 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 383 | 2.836 ms | 263.582 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 736 | 12.323 ms | 501.154 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 31 | 1 | 404 | 1.847 ms | 185.981 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 32 | 1 | 696 | 8.872 ms | 314.572 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 2.576 ms | 266.973 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 5 | 1 | 1252 | 10.790 ms | 521.898 us | 0 | 0 | 0 | 4 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 551 | 0 | 64 | 0 | 50 | 1 | 1158 | 27.114 ms | 6.319 ms | 0 | 0 | 128 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 559 | 0 | 64 | 0 | 50 | 1 | 1378 | 113.173 ms | 11.099 ms | 22 | 704 | 234 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.181 us | 62.910 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.210 us | 58.522 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 9.060 us | 89.360 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 17.501 us | 167.121 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 106 | 0 | 65 | 0 | 17 | 1 | 1106 | 4.060 ms | 610.558 us | 0 | 0 | 0 | 185 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1179 | 6.057 ms | 656.179 us | 0 | 0 | 0 | 133 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 130.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 40.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 100.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 190.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 110.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 160.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 1.340 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 1.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 1.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 230.000 ns | 770.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 90.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 50.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 60.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 390.000 ns | 1.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 220.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.020 us | 2.620 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 480.000 ns | 1.830 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 200.000 ns | 2.630 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 210.000 ns | 1.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.710 us | 5.021 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 920.000 ns | 3.780 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 410.000 ns | 5.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 440.000 ns | 4.210 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.260 us | 14.280 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 490.000 ns | 32.400 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 114 | 71.000 us | 32.521 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 6.760 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 16 | 1.890 us | 37.740 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 174 | 426.165 us | 81.011 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 6.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 6 | 5.410 us | 41.141 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 1.498 ms | 156.931 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 50.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 870.000 ns | 12.500 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 160.000 ns | 14.000 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 652 | 8.699 ms | 360.955 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.710 us | 28.161 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 5 | 2.610 us | 18.690 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 100.000 ns | 680.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 80.000 ns | 520.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 440.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 450.000 ns | 13.090 us |
| thread_safe_contention_same_slot_write_read_2 | other | 18 | 1.950 us | 680.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 980.000 ns | 1.690 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 7 | 870.000 ns | 7.300 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 26 | 6.110 us | 27.351 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 32 | 40.790 us | 1.720 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 14 | 37.340 us | 4.780 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 270.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 15 | 58.750 us | 34.100 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 52 | 46.220 us | 118.021 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 53 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 85 | 91.870 us | 4.900 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 28 | 78.172 us | 7.310 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 43 | 174.171 us | 98.751 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 96 | 104.681 us | 196.942 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 162 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 127 | 374.550 us | 5.830 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 36 | 19.291 us | 10.380 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 69 | 735.122 us | 124.871 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 206 | 167.771 us | 482.533 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 339 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 100.000 ns | 780.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 390.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 560.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 390.000 ns | 10.590 us |
| thread_safe_contention_independent_slots_2 | other | 36 | 1.030 us | 1.410 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 90.000 ns | 360.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 600.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 14 | 6.230 us | 10.540 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 8.600 us | 22.250 us |
| thread_safe_contention_independent_slots_4 | other | 94 | 202.672 us | 4.630 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 220.000 ns | 720.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 80.000 ns | 1.060 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 47 | 241.901 us | 49.210 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 256.943 us | 61.141 us |
| thread_safe_contention_independent_slots_8 | other | 195 | 1.453 ms | 9.880 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 430.000 ns | 1.630 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 230.000 ns | 3.030 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 117 | 1.270 ms | 120.391 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.873 ms | 144.530 us |
| thread_safe_contention_independent_slots_16 | other | 368 | 5.321 ms | 17.710 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 880.000 ns | 2.780 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 440.000 ns | 4.560 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 247 | 6.827 ms | 279.444 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 8.245 ms | 283.781 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 580.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 50.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 500.000 ns | 11.240 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 100.000 ns | 260.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 540.000 ns | 13.620 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 110.000 ns | 220.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 260.000 ns | 1.370 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 20.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 440.000 ns | 15.050 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 11.180 us | 2.000 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 20.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 3.030 us | 54.390 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 29 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 100.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 620.000 ns | 19.960 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.250 us | 13.031 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 330.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.430 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 480.000 ns | 26.290 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 420.000 ns | 16.050 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 51.820 us | 27.280 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 40.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.610 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.700 us | 42.020 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 890.000 ns | 26.620 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 226 | 279.354 us | 62.690 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 8 | 14.380 us | 2.100 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 410.000 ns | 6.010 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 34 | 4.020 us | 75.060 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 31 | 3.370 us | 53.751 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 44 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 326 | 1.507 ms | 161.940 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 810.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 860.000 ns | 14.100 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 2 | 100.000 ns | 15.941 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 3 | 110.000 ns | 6.410 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 660 | 10.233 ms | 359.814 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 6 | 8.280 us | 3.591 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.800 us | 29.401 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 9 | 3.820 us | 34.160 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 10 | 360.000 ns | 44.960 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 347 | 2.801 ms | 231.821 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 860.000 ns | 10.521 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 4 | 33.361 us | 21.240 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 667 | 12.288 ms | 449.393 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.670 us | 20.140 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 5 | 33.280 us | 31.621 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 261 | 665.159 us | 55.300 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 420.000 ns | 9.610 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.181 ms | 121.071 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 409 | 2.794 ms | 90.152 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 860.000 ns | 13.910 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 6.077 ms | 210.510 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 2.575 ms | 227.062 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 640.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 890.000 ns | 12.991 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 90.000 ns | 13.400 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 80.000 ns | 12.880 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1177 | 10.788 ms | 444.015 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.760 us | 28.941 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 4 | 150.000 ns | 28.071 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 130.000 ns | 20.681 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 351 | 8.025 ms | 193.350 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.630 us | 5.230 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.590 us | 25.830 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 14.173 ms | 5.595 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 551 | 4.914 ms | 499.884 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 457 | 31.483 ms | 174.832 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.710 us | 5.600 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.780 us | 25.040 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 234 | 72.114 ms | 10.396 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 559 | 9.573 ms | 496.676 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.790 us | 7.150 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.650 us | 5.530 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 900.000 ns | 14.870 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.841 us | 35.360 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.830 us | 5.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.750 us | 5.500 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 850.000 ns | 14.081 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.780 us | 33.431 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.800 us | 10.300 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 1.890 us | 9.110 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.610 us | 23.260 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.760 us | 46.690 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 6.921 us | 17.510 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.680 us | 13.820 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.340 us | 47.721 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.560 us | 88.070 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 448 | 3.830 ms | 220.903 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 474 | 24.090 us | 52.792 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.730 us | 22.540 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 13 | 2.540 us | 194.921 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 106 | 200.781 us | 119.402 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 786 | 6.045 ms | 360.957 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 3.560 us | 14.850 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.520 us | 51.921 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 970.000 ns | 139.861 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 3.640 us | 88.590 us |

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
