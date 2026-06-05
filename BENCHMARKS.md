# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.10.0`.

Environment: `rustc 1.94.0 (4a4ef493e 2026-03-02)` on `x86_64-unknown-linux-gnu`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.337 ms | 2.859 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.451 ms | 8.105 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 994.487 us | 1.065 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 2.611 ms | 2.745 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 574.064 us | 686.074 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.370 ms | 1.658 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.681 ms | 3.300 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.876 ms | 4.198 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.294 ms | 1.356 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.388 ms | 3.974 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.443 ms | 1.881 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.482 ms | 3.697 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.298 ms | 2.653 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 6.758 ms | 7.179 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.583 ms | 3.679 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.770 ms | 6.026 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.131 ms | 2.378 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.127 ms | 4.758 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 516.676 us | 532.310 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.035 ms | 1.193 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.114 ms | 1.205 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.107 ms | 2.595 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 7.724 ns | 7.655 ns - 7.851 ns |
| cached_reads | thread_safe_context | 63.641 ns | 63.204 ns - 64.407 ns |
| cold_first_get | context | 80.061 ns | 75.343 ns - 85.052 ns |
| cold_first_get | thread_safe_context | 1.039 us | 1.004 us - 1.071 us |
| dependency_fan_out | context / 32 | 3.829 us | 3.407 us - 4.383 us |
| dependency_fan_out | context / 256 | 50.099 us | 47.257 us - 54.654 us |
| dependency_fan_out | thread_safe_context / 32 | 23.145 us | 22.099 us - 24.206 us |
| dependency_fan_out | thread_safe_context / 256 | 164.851 us | 161.744 us - 168.046 us |
| set_cell_invalidation | high_fan_out / 512 | 105.789 us | 99.131 us - 111.517 us |
| set_cell_invalidation | same_slot_contention / 1 | 42.991 us | 42.517 us - 43.400 us |
| set_cell_invalidation | same_slot_contention / 2 | 115.093 us | 106.245 us - 123.061 us |
| set_cell_invalidation | same_slot_contention / 4 | 214.911 us | 201.770 us - 227.257 us |
| set_cell_invalidation | same_slot_contention / 8 | 504.545 us | 493.489 us - 516.503 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.888 ms | 1.788 ms - 1.978 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 42.466 us | 41.566 us - 43.324 us |
| set_cell_invalidation | independent_slot_contention / 2 | 69.412 us | 67.682 us - 71.155 us |
| set_cell_invalidation | independent_slot_contention / 4 | 138.350 us | 129.243 us - 147.033 us |
| set_cell_invalidation | independent_slot_contention / 8 | 271.167 us | 268.303 us - 273.295 us |
| set_cell_invalidation | independent_slot_contention / 16 | 567.132 us | 562.321 us - 572.022 us |
| set_cell_invalidation | batched_write_bursts / 1 | 136.519 us | 135.651 us - 137.464 us |
| set_cell_invalidation | batched_write_bursts / 2 | 207.252 us | 200.737 us - 214.044 us |
| set_cell_invalidation | batched_write_bursts / 4 | 425.922 us | 416.625 us - 434.320 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.180 ms | 1.087 ms - 1.275 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.382 ms | 3.094 ms - 3.653 ms |
| memo_equality_suppression | context | 2.762 us | 2.468 us - 3.074 us |
| memo_equality_suppression | thread_safe_context | 33.675 us | 32.731 us - 34.711 us |
| effect_flushing | context | 48.954 ns | 48.783 ns - 49.149 ns |
| effect_flushing | thread_safe_context | 910.233 ns | 906.635 ns - 914.568 ns |
| batch_storms | context / 64 | 2.867 us | 2.849 us - 2.886 us |
| batch_storms | thread_safe_context / 64 | 6.783 us | 6.653 us - 6.979 us |
| thread_safe_contention | same_slot_write_read / 1 | 111.253 us | 103.943 us - 122.572 us |
| thread_safe_contention | same_slot_write_read / 2 | 300.271 us | 293.342 us - 306.604 us |
| thread_safe_contention | same_slot_write_read / 4 | 798.333 us | 754.677 us - 859.118 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.402 ms | 2.296 ms - 2.532 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.496 ms | 7.222 ms - 7.750 ms |
| thread_safe_contention | independent_slots / 1 | 105.247 us | 104.205 us - 106.434 us |
| thread_safe_contention | independent_slots / 2 | 189.661 us | 187.543 us - 191.697 us |
| thread_safe_contention | independent_slots / 4 | 440.354 us | 425.917 us - 457.388 us |
| thread_safe_contention | independent_slots / 8 | 993.702 us | 969.326 us - 1.017 ms |
| thread_safe_contention | independent_slots / 16 | 2.602 ms | 2.541 ms - 2.663 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 104.241 us | 103.304 us - 105.002 us |
| thread_safe_contention | read_mostly_waiters / 2 | 147.009 us | 145.429 us - 148.570 us |
| thread_safe_contention | read_mostly_waiters / 4 | 254.906 us | 248.167 us - 262.027 us |
| thread_safe_contention | read_mostly_waiters / 8 | 609.632 us | 575.203 us - 645.450 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.432 ms | 1.356 ms - 1.513 ms |
| thread_safe_contention | batched_write_bursts / 1 | 211.126 us | 210.205 us - 212.003 us |
| thread_safe_contention | batched_write_bursts / 2 | 640.069 us | 596.866 us - 687.541 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.617 ms | 1.584 ms - 1.662 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.754 ms | 2.636 ms - 2.906 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.865 ms | 3.725 ms - 3.996 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.273 ms | 1.227 ms - 1.310 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.469 ms | 3.292 ms - 3.645 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.478 ms | 1.374 ms - 1.595 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.443 ms | 3.327 ms - 3.540 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.332 ms | 2.213 ms - 2.452 ms |
| thread_safe_effect_contention | batch_flush / 16 | 6.769 ms | 6.583 ms - 6.940 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.595 ms | 3.570 ms - 3.621 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.812 ms | 5.739 ms - 5.888 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.150 ms | 2.079 ms - 2.225 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.237 ms | 4.047 ms - 4.438 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 510.593 us | 498.403 us - 519.889 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.045 ms | 1.011 ms - 1.087 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.112 ms | 1.081 ms - 1.142 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.202 ms | 2.100 ms - 2.323 ms |
| profile_instrumentation | context_snapshot | 261.706 ns | 260.811 ns - 262.581 ns |
| profile_instrumentation | thread_safe_snapshot | 302.729 us | 301.035 us - 304.037 us |
| async_cached_resolve | async_context | 4.186 us | 3.882 us - 4.520 us |
| async_cached_resolve | sync_context_baseline | 64.576 ns | 64.228 ns - 64.962 ns |
| async_cached_resolve | sync_get | 11.446 ns | 11.408 ns - 11.488 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.339 us | 1.335 us - 1.343 us |
| async_cold_resolve | async_context | 4.384 us | 3.999 us - 4.821 us |
| async_cold_resolve | sync_context_baseline | 107.948 ns | 100.745 ns - 114.731 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.038 us | 996.779 ns - 1.077 us |
| async_invalidation_throughput | async_context | 253.274 us | 228.497 us - 280.393 us |
| async_invalidation_throughput | sync_context_baseline | 2.730 us | 2.725 us - 2.735 us |
| async_invalidation_throughput | thread_safe_context_baseline | 38.213 us | 38.168 us - 38.261 us |
| async_cancellation_throughput | async_invalidate_in_flight | 62.376 us | 51.795 us - 72.051 us |
| async_concurrent_contention | async_context / 1 | 72.328 us | 70.174 us - 74.699 us |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 66.956 us | 64.358 us - 71.190 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 576.186 us | 570.271 us - 581.101 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 4.620 ms | 4.588 ms - 4.651 ms |
| async_effect_throughput | async_context | 188.216 ms | 188.007 ms - 188.457 ms |
| async_batch_throughput | async_context | 88.215 us | 79.695 us - 97.159 us |
| async_batch_throughput | sync_context_baseline | 11.128 us | 10.599 us - 11.936 us |
| tokio_sync_cached_read | single_task | 661.831 ns | 661.023 ns - 662.814 ns |
| tokio_sync_cached_read | spawn_read | 3.975 us | 3.567 us - 4.448 us |
| tokio_sync_cold_first_get | single_task | 601.894 ns | 600.759 ns - 603.305 ns |
| tokio_sync_cold_first_get | spawn_compute | 4.887 us | 4.298 us - 5.508 us |
| tokio_sync_invalidation | single_task | 30.871 us | 30.690 us - 31.086 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 35.879 us | 34.818 us - 36.893 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 390.829 us | 349.010 us - 435.083 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 4.482 ms | 4.351 ms - 4.579 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 36.322 us | 35.688 us - 36.992 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 165.553 us | 150.145 us - 181.497 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 1.388 ms | 1.353 ms - 1.426 ms |
| tokio_sync_batch | spawn_batch | 28.543 us | 28.466 us - 28.627 us |
| tokio_sync_effect | single_task | 10.067 ms | 10.066 ms - 10.069 ms |
| typed_cache_reads | context_cell | 2.427 ns | 2.424 ns - 2.431 ns |
| typed_cache_reads | context_rc_cell | 2.815 ns | 2.752 ns - 2.921 ns |
| typed_cache_reads | context_rc_slot | 7.964 ns | 7.944 ns - 7.988 ns |
| typed_cache_reads | context_slot | 7.838 ns | 7.652 ns - 8.152 ns |
| typed_cache_reads | thread_safe_cell | 24.622 ns | 24.432 ns - 24.949 ns |
| typed_cache_reads | thread_safe_slot | 63.712 ns | 63.521 ns - 63.919 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 19.540 us | 16.100 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 390.000 ns | 5.800 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 320.000 ns | 1.020 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 340.000 ns | 940.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 340.000 ns | 930.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 400.000 ns | 4.920 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 360.000 ns | 4.890 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 640.000 ns | 1.920 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.330 us | 4.500 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.430 us | 9.210 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.570 us | 19.000 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.690 us | 52.001 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 132 | 89.671 us | 71.781 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 205 | 422.623 us | 123.611 us | 0 | 0 | 0 | 8 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 2.330 ms | 232.851 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 727 | 6.599 ms | 403.982 us | 0 | 0 | 0 | 6 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 940.000 ns | 16.770 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 48 | 2.200 us | 28.561 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 60 | 0 | 1 | 0 | 0 | 0 | 143 | 2.690 us | 68.171 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 102 | 0 | 1 | 0 | 0 | 0 | 248 | 16.960 us | 203.173 us | 127 | 127 | 1 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 219 | 0 | 1 | 0 | 0 | 0 | 532 | 38.440 us | 281.401 us | 255 | 255 | 1 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 890.000 ns | 15.020 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 50 | 10.340 us | 30.720 us | 30 | 30 | 1 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 136 | 230.463 us | 81.331 us | 49 | 49 | 14 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 247 | 2.039 ms | 174.472 us | 105 | 105 | 22 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 506 | 7.335 ms | 327.714 us | 204 | 204 | 51 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.010 us | 14.490 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 28 | 1.070 us | 16.580 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 32 | 5.150 us | 16.030 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 62 | 46.060 us | 23.651 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 68 | 21.010 us | 37.600 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.000 us | 59.260 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 188 | 57.350 us | 100.901 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 26 | 0 | 16 | 0 | 0 | 0 | 327 | 349.390 us | 189.641 us | 0 | 0 | 0 | 29 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 12 | 0 | 32 | 0 | 0 | 0 | 412 | 2.334 ms | 258.902 us | 0 | 0 | 0 | 11 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 4 | 0 | 64 | 0 | 0 | 0 | 722 | 10.722 ms | 475.303 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 6 | 1 | 392 | 2.536 ms | 250.761 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 2 | 1 | 720 | 5.505 ms | 363.792 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 35 | 1 | 416 | 2.183 ms | 176.072 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 40 | 1 | 716 | 7.907 ms | 306.803 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 6 | 0 | 33 | 0 | 9 | 1 | 660 | 3.467 ms | 301.122 us | 0 | 0 | 0 | 6 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 5 | 1 | 1247 | 12.838 ms | 515.683 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 562 | 0 | 64 | 0 | 50 | 1 | 1123 | 22.366 ms | 5.575 ms | 23 | 736 | 105 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 557 | 0 | 64 | 0 | 50 | 1 | 1250 | 75.083 ms | 8.626 ms | 85 | 2720 | 171 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.020 us | 64.740 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.370 us | 64.910 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 13.630 us | 102.090 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 26.640 us | 199.132 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 603 | 2.979 ms | 391.074 us | 0 | 0 | 0 | 69 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 140 | 0 | 129 | 0 | 7 | 1 | 1445 | 9.629 ms | 764.755 us | 0 | 0 | 0 | 147 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 210.000 ns | 1.980 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 110.000 ns | 1.320 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 40.000 ns | 1.240 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 160.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 50.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 40.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 170.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 90.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 40.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 180.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 100.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 40.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 230.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 90.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 1.240 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 40.000 ns | 1.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 190.000 ns | 1.240 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 1.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 1.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 40.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 310.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 180.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 80.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 710.000 ns | 1.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 310.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 160.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 150.000 ns | 990.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.210 us | 2.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 590.000 ns | 1.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 310.000 ns | 2.870 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 320.000 ns | 2.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.080 us | 4.040 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.300 us | 3.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 590.000 ns | 6.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 600.000 ns | 4.910 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.750 us | 14.931 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 820.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 180.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 670.000 ns | 33.450 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 108 | 85.711 us | 32.550 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 90.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 370.000 ns | 4.340 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 13 | 3.460 us | 34.421 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 40.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 178 | 418.313 us | 80.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 90.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 730.000 ns | 6.880 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 8 | 3.450 us | 36.091 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 2.328 ms | 196.181 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.420 us | 13.710 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 170.000 ns | 22.290 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 50.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 654 | 6.596 ms | 332.942 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.600 us | 31.400 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 6 | 270.000 ns | 38.830 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 40.000 ns | 450.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 150.000 ns | 640.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 90.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 490.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 660.000 ns | 15.260 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 170.000 ns | 210.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 880.000 ns | 1.400 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 23 | 1.110 us | 26.611 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 170.000 ns | 200.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 4 | 150.000 ns | 530.000 ns |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 60 | 2.330 us | 67.121 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 74 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 250.000 ns | 440.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 8 | 420.000 ns | 2.261 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 1.060 us | 10.350 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 102 | 15.190 us | 189.702 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 130 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 9.160 us | 450.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 20 | 6.570 us | 7.880 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 60.000 ns | 3.110 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 219 | 22.610 us | 269.581 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 285 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 140.000 ns | 880.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 640.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 950.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 650.000 ns | 12.550 us |
| thread_safe_contention_independent_slots_2 | other | 10 | 3.890 us | 760.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 140.000 ns | 350.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 80.000 ns | 650.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 1 | 40.000 ns | 2.370 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 6.190 us | 26.590 us |
| thread_safe_contention_independent_slots_4 | other | 43 | 40.191 us | 2.810 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 290.000 ns | 990.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 170.000 ns | 1.590 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 14 | 48.601 us | 18.990 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 141.211 us | 56.951 us |
| thread_safe_contention_independent_slots_8 | other | 66 | 221.731 us | 3.790 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 580.000 ns | 1.370 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 340.000 ns | 2.650 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 22 | 288.764 us | 34.222 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.528 ms | 132.440 us |
| thread_safe_contention_independent_slots_16 | other | 136 | 705.994 us | 9.740 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.260 us | 2.750 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 760.000 ns | 6.161 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 51 | 964.740 us | 58.680 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 5.663 ms | 250.383 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 190.000 ns | 630.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 100.000 ns | 350.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 50.000 ns | 580.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 670.000 ns | 12.930 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 150.000 ns | 230.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 4 | 210.000 ns | 630.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 670.000 ns | 15.380 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 200.000 ns | 260.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 6 | 4.110 us | 1.340 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 50.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 790.000 ns | 14.090 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 200.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 19 | 41.710 us | 3.910 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 4.110 us | 19.131 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 210.000 ns | 250.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 16 | 19.000 us | 3.130 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 50.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 1.750 us | 33.860 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 29 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.570 us | 12.980 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 400.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 190.000 ns | 1.580 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 640.000 ns | 26.210 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 530.000 ns | 18.090 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 54.950 us | 26.960 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 330.000 ns | 3.980 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.250 us | 41.600 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 750.000 ns | 28.181 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 214 | 338.590 us | 63.690 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 6 | 6.490 us | 1.300 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 710.000 ns | 6.590 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 29 | 2.760 us | 70.001 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 26 | 840.000 ns | 48.060 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 36 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 344 | 2.326 ms | 168.492 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 90.000 ns | 510.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.310 us | 14.520 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 11 | 6.080 us | 36.540 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 12 | 380.000 ns | 38.840 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 648 | 10.719 ms | 387.601 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 381.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.640 us | 32.140 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 3 | 160.000 ns | 31.100 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 4 | 150.000 ns | 24.081 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 354 | 2.534 ms | 201.611 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.450 us | 11.020 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 6 | 380.000 ns | 38.130 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 655 | 5.502 ms | 331.022 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.950 us | 22.660 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 1 | 50.000 ns | 10.110 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 273 | 677.394 us | 59.651 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 650.000 ns | 7.380 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.505 ms | 109.041 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 429 | 3.278 ms | 100.900 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.520 us | 12.070 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 4.627 ms | 193.833 us |
| thread_safe_effect_contention_batch_flush_8 | other | 613 | 3.465 ms | 244.662 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 100.000 ns | 650.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.620 us | 14.590 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 6 | 260.000 ns | 19.270 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 6 | 250.000 ns | 21.950 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1175 | 12.835 ms | 442.473 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 90.000 ns | 260.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 3.020 us | 31.750 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 110.000 ns | 15.940 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 130.000 ns | 25.260 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 328 | 6.279 ms | 167.772 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 3.100 us | 5.200 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.600 us | 27.290 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 105 | 12.072 ms | 4.868 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 562 | 4.009 ms | 506.662 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 394 | 17.822 ms | 177.412 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 3.140 us | 5.250 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.520 us | 26.920 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 171 | 50.174 ms | 7.893 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 557 | 7.081 ms | 523.483 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.610 us | 6.310 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.470 us | 5.400 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.300 us | 16.690 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.640 us | 36.340 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 2.630 us | 4.640 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.480 us | 5.360 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.380 us | 17.350 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.880 us | 37.560 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.560 us | 10.560 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.560 us | 12.110 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.590 us | 27.290 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.920 us | 52.130 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 8.820 us | 19.180 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 6.740 us | 17.030 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 5.360 us | 57.901 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.720 us | 105.021 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 402 | 2.969 ms | 217.142 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 3.340 us | 7.860 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.540 us | 27.481 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 2 | 1.480 us | 86.061 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 2.740 us | 52.530 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 797 | 9.602 ms | 393.113 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 376 | 15.800 us | 40.530 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.790 us | 54.141 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 140.000 ns | 148.951 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 140 | 5.750 us | 128.020 us |

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

