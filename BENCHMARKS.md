# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.21.3`.

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

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 2.082 ms | 2.421 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 5.976 ms | 7.034 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 2.263 ms | 2.640 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.516 ms | 5.415 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 578.844 us | 626.543 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.288 ms | 1.397 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 3.229 ms | 3.467 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.852 ms | 4.521 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.392 ms | 1.672 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.902 ms | 3.257 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.426 ms | 1.544 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.359 ms | 4.800 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.384 ms | 2.725 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.710 ms | 5.328 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 6.496 ms | 6.927 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 10.730 ms | 12.438 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 3.036 ms | 3.476 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.343 ms | 6.532 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 575.618 us | 985.454 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 952.253 us | 1.063 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.198 ms | 1.391 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.920 ms | 2.048 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 10.533 ns | 9.789 ns - 11.383 ns |
| cached_reads | thread_safe_context | 67.034 ns | 66.259 ns - 67.924 ns |
| cold_first_get | context | 93.090 ns | 84.389 ns - 102.828 ns |
| cold_first_get | thread_safe_context | 1.134 us | 1.086 us - 1.192 us |
| dependency_fan_out | context / 32 | 5.199 us | 4.636 us - 6.003 us |
| dependency_fan_out | context / 256 | 72.478 us | 70.339 us - 75.339 us |
| dependency_fan_out | thread_safe_context / 32 | 30.752 us | 29.727 us - 31.725 us |
| dependency_fan_out | thread_safe_context / 256 | 219.430 us | 217.278 us - 221.476 us |
| set_cell_invalidation | high_fan_out / 512 | 145.213 us | 138.414 us - 152.164 us |
| set_cell_invalidation | same_slot_contention / 1 | 46.869 us | 43.829 us - 51.816 us |
| set_cell_invalidation | same_slot_contention / 2 | 94.745 us | 92.038 us - 97.149 us |
| set_cell_invalidation | same_slot_contention / 4 | 184.718 us | 176.498 us - 191.972 us |
| set_cell_invalidation | same_slot_contention / 8 | 530.410 us | 504.724 us - 555.745 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.664 ms | 1.563 ms - 1.764 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 39.724 us | 38.329 us - 41.303 us |
| set_cell_invalidation | independent_slot_contention / 2 | 73.447 us | 70.293 us - 76.545 us |
| set_cell_invalidation | independent_slot_contention / 4 | 133.925 us | 127.916 us - 139.860 us |
| set_cell_invalidation | independent_slot_contention / 8 | 266.039 us | 258.968 us - 272.591 us |
| set_cell_invalidation | independent_slot_contention / 16 | 489.931 us | 482.974 us - 497.075 us |
| set_cell_invalidation | batched_write_bursts / 1 | 134.297 us | 132.633 us - 135.978 us |
| set_cell_invalidation | batched_write_bursts / 2 | 237.077 us | 216.774 us - 260.066 us |
| set_cell_invalidation | batched_write_bursts / 4 | 506.118 us | 488.435 us - 522.480 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.572 ms | 1.446 ms - 1.728 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.748 ms | 2.665 ms - 2.827 ms |
| memo_equality_suppression | context | 3.292 us | 2.932 us - 3.838 us |
| memo_equality_suppression | thread_safe_context | 50.055 us | 48.327 us - 51.840 us |
| effect_flushing | context | 98.625 ns | 97.490 ns - 99.666 ns |
| effect_flushing | thread_safe_context | 1.058 us | 1.042 us - 1.075 us |
| batch_storms | context / 64 | 3.845 us | 3.592 us - 4.133 us |
| batch_storms | thread_safe_context / 64 | 9.030 us | 8.676 us - 9.395 us |
| thread_safe_contention | same_slot_write_read / 1 | 104.720 us | 102.480 us - 107.150 us |
| thread_safe_contention | same_slot_write_read / 2 | 322.654 us | 304.055 us - 341.550 us |
| thread_safe_contention | same_slot_write_read / 4 | 794.425 us | 758.607 us - 832.761 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.164 ms | 2.083 ms - 2.252 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.052 ms | 5.676 ms - 6.430 ms |
| thread_safe_contention | independent_slots / 1 | 105.123 us | 103.662 us - 106.453 us |
| thread_safe_contention | independent_slots / 2 | 242.901 us | 221.213 us - 266.385 us |
| thread_safe_contention | independent_slots / 4 | 637.423 us | 616.095 us - 655.962 us |
| thread_safe_contention | independent_slots / 8 | 2.209 ms | 2.055 ms - 2.354 ms |
| thread_safe_contention | independent_slots / 16 | 4.666 ms | 4.363 ms - 4.985 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 106.062 us | 105.111 us - 106.913 us |
| thread_safe_contention | read_mostly_waiters / 2 | 150.421 us | 147.193 us - 153.172 us |
| thread_safe_contention | read_mostly_waiters / 4 | 245.500 us | 240.209 us - 250.647 us |
| thread_safe_contention | read_mostly_waiters / 8 | 588.132 us | 568.780 us - 606.812 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.276 ms | 1.213 ms - 1.331 ms |
| thread_safe_contention | batched_write_bursts / 1 | 219.591 us | 217.255 us - 221.890 us |
| thread_safe_contention | batched_write_bursts / 2 | 618.410 us | 571.559 us - 660.993 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.618 ms | 1.598 ms - 1.645 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.240 ms | 3.167 ms - 3.312 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.919 ms | 3.714 ms - 4.128 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.419 ms | 1.334 ms - 1.505 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.911 ms | 2.730 ms - 3.082 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.381 ms | 1.284 ms - 1.463 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.568 ms | 3.215 ms - 3.993 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.311 ms | 2.094 ms - 2.502 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.775 ms | 4.639 ms - 4.933 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 6.555 ms | 6.372 ms - 6.739 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 10.952 ms | 10.669 ms - 11.338 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 3.177 ms | 3.048 ms - 3.310 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.493 ms | 5.204 ms - 5.807 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 607.559 us | 535.173 us - 705.513 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 980.652 us | 957.762 us - 1.008 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.172 ms | 1.087 ms - 1.253 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.900 ms | 1.830 ms - 1.963 ms |
| profile_instrumentation | context_snapshot | 380.895 ns | 332.612 ns - 433.516 ns |
| profile_instrumentation | thread_safe_snapshot | 287.880 us | 286.663 us - 289.207 us |
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
| typed_cache_reads | context_cell | 3.735 ns | 3.338 ns - 4.143 ns |
| typed_cache_reads | context_rc_cell | 3.732 ns | 3.404 ns - 4.064 ns |
| typed_cache_reads | context_rc_slot | 11.798 ns | 10.628 ns - 13.038 ns |
| typed_cache_reads | context_slot | 11.802 ns | 10.716 ns - 12.917 ns |
| typed_cache_reads | thread_safe_cell | 27.403 ns | 26.623 ns - 28.221 ns |
| typed_cache_reads | thread_safe_slot | 72.557 ns | 70.595 ns - 74.537 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.390 us | 20.470 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 1.420 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 260.000 ns | 1.020 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 940.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 340.000 ns | 4.930 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 1.660 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 1.940 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 520.000 ns | 1.820 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.020 us | 4.150 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.040 us | 11.731 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.880 us | 14.940 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.130 us | 56.580 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 135 | 90.131 us | 83.981 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 196 | 355.323 us | 114.420 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 2.326 ms | 229.280 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 724 | 9.619 ms | 474.304 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 740.000 ns | 21.490 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 30 | 0 | 1 | 0 | 0 | 0 | 96 | 23.170 us | 47.780 us | 19 | 19 | 13 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 52 | 0 | 1 | 0 | 0 | 0 | 162 | 81.534 us | 131.110 us | 53 | 53 | 11 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 104 | 0 | 1 | 0 | 0 | 0 | 371 | 252.804 us | 334.124 us | 101 | 101 | 27 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 201 | 0 | 1 | 0 | 0 | 0 | 743 | 1.219 ms | 623.656 us | 194 | 194 | 62 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 680.000 ns | 12.550 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 89 | 53.621 us | 48.281 us | 17 | 17 | 14 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 226 | 732.344 us | 122.692 us | 13 | 13 | 50 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 464 | 3.843 ms | 278.232 us | 10 | 10 | 117 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 900 | 21.040 ms | 651.655 us | 15 | 15 | 240 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 840.000 ns | 16.540 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 750.000 ns | 14.440 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 37 | 960.000 ns | 17.210 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 68 | 38.910 us | 32.921 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 77 | 42.601 us | 52.020 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.520 us | 59.620 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 193 | 58.891 us | 115.971 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 32 | 0 | 16 | 0 | 0 | 0 | 365 | 333.643 us | 217.950 us | 0 | 0 | 0 | 38 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 4 | 0 | 32 | 0 | 0 | 0 | 371 | 1.749 ms | 218.161 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 8 | 0 | 64 | 0 | 0 | 0 | 741 | 5.886 ms | 448.413 us | 0 | 0 | 0 | 7 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 8 | 1 | 410 | 1.919 ms | 261.221 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 744 | 8.996 ms | 475.525 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 37 | 1 | 424 | 2.343 ms | 204.962 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 32 | 1 | 700 | 10.197 ms | 353.584 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 7 | 1 | 647 | 4.088 ms | 322.062 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 17.460 ms | 599.546 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 552 | 0 | 64 | 0 | 49 | 1 | 1107 | 20.056 ms | 5.428 ms | 24 | 768 | 104 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 551 | 0 | 64 | 0 | 49 | 1 | 1380 | 115.563 ms | 11.246 ms | 15 | 480 | 241 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.110 us | 64.580 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.220 us | 63.450 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 10.171 us | 96.752 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 19.930 us | 169.571 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 609 | 3.096 ms | 406.164 us | 0 | 0 | 0 | 89 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1317 | 6.286 ms | 695.267 us | 0 | 0 | 0 | 150 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 120.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 50.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 110.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 70.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 50.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 120.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 50.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 170.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 110.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.180 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 1.090 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 110.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 50.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 120.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 70.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 240.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 130.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 90.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 60.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 470.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 290.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 160.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 970.000 ns | 3.380 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 530.000 ns | 2.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 340.000 ns | 3.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 200.000 ns | 2.511 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.760 us | 4.260 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.000 us | 2.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 730.000 ns | 4.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 390.000 ns | 3.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.370 us | 16.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 2.660 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 550.000 ns | 34.010 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 110 | 87.551 us | 34.851 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 250.000 ns | 3.880 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 2.240 us | 44.800 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 172 | 354.553 us | 82.940 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 60.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 430.000 ns | 6.630 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 250.000 ns | 24.390 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 2.325 ms | 201.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 900.000 ns | 12.400 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 170.000 ns | 15.400 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 652 | 9.572 ms | 411.494 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.740 us | 28.960 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 5 | 44.780 us | 32.520 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 700.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 130.000 ns | 1.640 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 110.000 ns | 1.310 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 50.000 ns | 1.320 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 450.000 ns | 17.220 us |
| thread_safe_contention_same_slot_write_read_2 | other | 30 | 3.190 us | 1.050 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 1.120 us | 1.150 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 13 | 1.030 us | 16.230 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 30 | 17.790 us | 29.030 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 24 | 25.242 us | 1.480 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 18 | 5.230 us | 5.470 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 11 | 29.462 us | 16.280 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 52 | 21.570 us | 107.580 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 56 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 54 | 54.290 us | 3.200 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 24 | 16.230 us | 8.040 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 27 | 102.212 us | 69.252 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 104 | 80.032 us | 253.322 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 161 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 118 | 302.103 us | 6.600 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 44 | 49.682 us | 12.070 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 900.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 62 | 703.476 us | 136.031 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 201 | 163.491 us | 468.055 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 317 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 110.000 ns | 750.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 230.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 40.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 460.000 ns | 10.940 us |
| thread_safe_contention_independent_slots_2 | other | 36 | 14.900 us | 2.300 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 110.000 ns | 390.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 80.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 14 | 20.060 us | 14.400 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 18.471 us | 30.561 us |
| thread_safe_contention_independent_slots_4 | other | 97 | 231.800 us | 4.870 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 280.000 ns | 720.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 190.000 ns | 1.240 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 50 | 192.151 us | 49.681 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 307.923 us | 66.181 us |
| thread_safe_contention_independent_slots_8 | other | 188 | 1.116 ms | 9.840 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 550.000 ns | 1.470 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 320.000 ns | 3.120 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 117 | 1.177 ms | 127.370 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.549 ms | 136.432 us |
| thread_safe_contention_independent_slots_16 | other | 341 | 6.043 ms | 21.180 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.040 us | 3.010 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 670.000 ns | 5.470 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 240 | 6.669 ms | 301.960 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 8.326 ms | 320.035 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 190.000 ns | 1.630 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 130.000 ns | 1.370 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 1.190 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 500.000 ns | 12.350 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 130.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 50.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 510.000 ns | 13.520 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 110.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 340.000 ns | 1.440 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 480.000 ns | 15.040 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 440.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 18 | 28.710 us | 6.440 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 10.050 us | 25.701 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 28 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 100.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 17 | 41.181 us | 7.270 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 1.280 us | 43.990 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 37 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.360 us | 13.840 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 270.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 1.390 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 500.000 ns | 27.740 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 490.000 ns | 16.380 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 55.301 us | 31.580 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 260.000 ns | 4.100 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 2.000 us | 51.381 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 1.260 us | 28.730 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 232 | 317.003 us | 67.410 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 8.940 us | 1.200 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 6.560 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 38 | 4.610 us | 86.480 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 32 | 2.640 us | 56.300 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 43 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 328 | 1.748 ms | 176.920 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 220.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 870.000 ns | 12.391 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 3 | 120.000 ns | 17.280 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 4 | 140.000 ns | 11.350 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 656 | 5.884 ms | 343.342 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.710 us | 28.660 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 7 | 270.000 ns | 34.291 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 8 | 290.000 ns | 41.890 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 370 | 1.918 ms | 221.511 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 910.000 ns | 11.060 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 8 | 370.000 ns | 28.650 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 674 | 8.993 ms | 426.225 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.740 us | 19.580 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 6 | 680.000 ns | 29.720 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 281 | 1.171 ms | 68.690 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 550.000 ns | 7.150 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.171 ms | 129.122 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 4.993 ms | 101.092 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 890.000 ns | 15.411 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.204 ms | 237.081 us |
| thread_safe_effect_contention_batch_flush_8 | other | 605 | 4.087 ms | 273.552 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 50.000 ns | 790.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 940.000 ns | 14.310 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 120.000 ns | 15.650 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 100.000 ns | 17.760 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 17.458 ms | 542.336 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 300.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.850 us | 29.130 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 50.000 ns | 16.260 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 70.000 ns | 11.520 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 323 | 3.132 ms | 169.111 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.090 us | 5.270 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.330 us | 24.361 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 104 | 13.241 ms | 4.706 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 552 | 3.679 ms | 523.972 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 460 | 25.585 ms | 169.261 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.160 us | 5.990 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.260 us | 27.110 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 241 | 80.165 ms | 10.555 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 551 | 9.808 ms | 489.553 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.870 us | 6.990 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.180 us | 6.450 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.370 us | 16.380 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.690 us | 34.760 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.820 us | 5.280 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.140 us | 6.000 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.320 us | 16.660 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.940 us | 35.510 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.540 us | 11.900 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.350 us | 11.001 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.291 us | 23.850 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.990 us | 50.001 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.130 us | 16.980 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.330 us | 13.770 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 4.540 us | 49.181 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.930 us | 89.640 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 405 | 3.015 ms | 211.802 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 34.960 us | 8.210 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.330 us | 26.170 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 5 | 230.000 ns | 110.461 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 43.990 us | 49.521 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 794 | 6.245 ms | 365.403 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 8.140 us | 27.530 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.670 us | 49.382 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 5 | 6.180 us | 147.642 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 22.641 us | 105.310 us |

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
