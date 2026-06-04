# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.5.1`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.370 ms | 2.619 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.527 ms | 7.881 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.066 ms | 1.242 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 2.311 ms | 2.421 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 587.723 us | 724.693 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.168 ms | 1.453 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.760 ms | 3.034 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.680 ms | 4.147 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.255 ms | 1.329 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.121 ms | 3.485 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.268 ms | 1.383 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.176 ms | 3.385 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.044 ms | 2.432 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 6.178 ms | 6.834 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.523 ms | 3.591 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.777 ms | 5.878 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.879 ms | 1.987 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.658 ms | 3.984 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 404.079 us | 422.068 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 724.784 us | 763.716 us | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.055 ms | 1.112 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.922 ms | 2.093 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 8.248 ns | 8.233 ns - 8.264 ns |
| cached_reads | thread_safe_context | 63.998 ns | 63.900 ns - 64.116 ns |
| cold_first_get | context | 80.169 ns | 77.264 ns - 82.789 ns |
| cold_first_get | thread_safe_context | 1.071 us | 1.035 us - 1.108 us |
| dependency_fan_out | context / 32 | 4.723 us | 4.047 us - 5.597 us |
| dependency_fan_out | context / 256 | 47.478 us | 44.865 us - 51.617 us |
| dependency_fan_out | thread_safe_context / 32 | 21.701 us | 21.279 us - 22.112 us |
| dependency_fan_out | thread_safe_context / 256 | 159.388 us | 157.499 us - 161.348 us |
| set_cell_invalidation | high_fan_out / 512 | 99.846 us | 95.948 us - 103.220 us |
| set_cell_invalidation | same_slot_contention / 1 | 42.628 us | 42.071 us - 43.184 us |
| set_cell_invalidation | same_slot_contention / 2 | 104.619 us | 94.704 us - 115.352 us |
| set_cell_invalidation | same_slot_contention / 4 | 198.043 us | 194.181 us - 202.144 us |
| set_cell_invalidation | same_slot_contention / 8 | 516.870 us | 509.872 us - 523.696 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.739 ms | 1.708 ms - 1.776 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 42.608 us | 41.625 us - 43.494 us |
| set_cell_invalidation | independent_slot_contention / 2 | 76.703 us | 75.218 us - 78.358 us |
| set_cell_invalidation | independent_slot_contention / 4 | 135.459 us | 131.959 us - 139.014 us |
| set_cell_invalidation | independent_slot_contention / 8 | 272.698 us | 262.020 us - 282.715 us |
| set_cell_invalidation | independent_slot_contention / 16 | 532.630 us | 516.673 us - 545.014 us |
| set_cell_invalidation | batched_write_bursts / 1 | 133.836 us | 133.218 us - 134.418 us |
| set_cell_invalidation | batched_write_bursts / 2 | 224.196 us | 219.374 us - 230.117 us |
| set_cell_invalidation | batched_write_bursts / 4 | 479.772 us | 468.989 us - 489.699 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.201 ms | 1.177 ms - 1.225 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.069 ms | 2.987 ms - 3.154 ms |
| memo_equality_suppression | context | 2.528 us | 2.136 us - 3.153 us |
| memo_equality_suppression | thread_safe_context | 33.315 us | 32.716 us - 33.904 us |
| effect_flushing | context | 50.883 ns | 50.782 ns - 51.002 ns |
| effect_flushing | thread_safe_context | 904.910 ns | 902.306 ns - 908.009 ns |
| batch_storms | context / 64 | 2.823 us | 2.810 us - 2.836 us |
| batch_storms | thread_safe_context / 64 | 6.809 us | 6.789 us - 6.832 us |
| thread_safe_contention | same_slot_write_read / 1 | 103.414 us | 102.684 us - 104.181 us |
| thread_safe_contention | same_slot_write_read / 2 | 300.196 us | 286.777 us - 312.022 us |
| thread_safe_contention | same_slot_write_read / 4 | 762.097 us | 704.790 us - 828.477 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.363 ms | 2.213 ms - 2.484 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.645 ms | 6.097 ms - 7.187 ms |
| thread_safe_contention | independent_slots / 1 | 103.109 us | 101.960 us - 104.484 us |
| thread_safe_contention | independent_slots / 2 | 201.306 us | 193.077 us - 209.756 us |
| thread_safe_contention | independent_slots / 4 | 399.095 us | 387.661 us - 411.641 us |
| thread_safe_contention | independent_slots / 8 | 1.092 ms | 1.028 ms - 1.155 ms |
| thread_safe_contention | independent_slots / 16 | 2.309 ms | 2.257 ms - 2.352 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 103.950 us | 103.048 us - 104.823 us |
| thread_safe_contention | read_mostly_waiters / 2 | 147.193 us | 144.785 us - 148.898 us |
| thread_safe_contention | read_mostly_waiters / 4 | 246.454 us | 243.686 us - 249.587 us |
| thread_safe_contention | read_mostly_waiters / 8 | 618.479 us | 578.854 us - 659.117 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.179 ms | 1.092 ms - 1.264 ms |
| thread_safe_contention | batched_write_bursts / 1 | 210.041 us | 209.019 us - 211.035 us |
| thread_safe_contention | batched_write_bursts / 2 | 544.633 us | 527.712 us - 561.515 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.580 ms | 1.569 ms - 1.590 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.804 ms | 2.723 ms - 2.887 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.708 ms | 3.546 ms - 3.876 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.239 ms | 1.199 ms - 1.277 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.211 ms | 3.107 ms - 3.321 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.289 ms | 1.258 ms - 1.323 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.172 ms | 3.052 ms - 3.273 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.103 ms | 2.011 ms - 2.205 ms |
| thread_safe_effect_contention | batch_flush / 16 | 6.302 ms | 6.091 ms - 6.518 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.526 ms | 3.493 ms - 3.556 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.757 ms | 5.702 ms - 5.808 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.871 ms | 1.811 ms - 1.925 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.706 ms | 3.628 ms - 3.791 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 405.837 us | 400.624 us - 411.452 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 729.181 us | 721.709 us - 738.649 us |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.068 ms | 1.053 ms - 1.085 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.932 ms | 1.865 ms - 1.999 ms |
| profile_instrumentation | context_snapshot | 258.870 ns | 257.828 ns - 260.034 ns |
| profile_instrumentation | thread_safe_snapshot | 299.852 us | 298.255 us - 301.410 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 18.450 us | 15.320 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 2.480 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 1.040 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 970.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 970.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 280.000 ns | 1.010 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 400.000 ns | 4.770 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 610.000 ns | 2.040 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.210 us | 4.190 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.320 us | 10.750 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.410 us | 18.340 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.160 us | 46.901 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 132 | 87.741 us | 81.831 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 198 | 397.652 us | 117.431 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 363 | 1.570 ms | 189.521 us | 0 | 0 | 0 | 2 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 8.125 ms | 436.231 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.140 us | 20.910 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 49 | 2.860 us | 33.001 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 61 | 0 | 1 | 0 | 0 | 0 | 143 | 6.020 us | 75.431 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 111 | 0 | 1 | 0 | 0 | 0 | 281 | 12.460 us | 157.532 us | 128 | 128 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 211 | 0 | 1 | 0 | 0 | 0 | 590 | 34.720 us | 551.802 us | 255 | 255 | 1 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 1.000 us | 15.640 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 2.430 us | 28.720 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 129 | 260.923 us | 79.970 us | 51 | 51 | 12 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 262 | 1.558 ms | 161.450 us | 101 | 101 | 26 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 480 | 9.405 ms | 372.643 us | 216 | 216 | 39 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.020 us | 14.860 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 1.040 us | 15.520 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 31 | 1.200 us | 17.750 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 47 | 1.560 us | 20.900 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 54 | 1.470 us | 30.730 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.800 us | 60.220 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 20 | 0 | 8 | 0 | 0 | 0 | 182 | 58.930 us | 103.671 us | 0 | 0 | 0 | 20 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 27 | 0 | 16 | 0 | 0 | 0 | 328 | 300.120 us | 191.942 us | 0 | 0 | 0 | 29 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 21 | 0 | 32 | 0 | 0 | 0 | 477 | 1.484 ms | 290.960 us | 0 | 0 | 0 | 23 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 726 | 5.638 ms | 393.772 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 381 | 1.346 ms | 197.602 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 6 | 1 | 742 | 8.560 ms | 444.942 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 33 | 1 | 408 | 2.084 ms | 175.710 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 34 | 1 | 700 | 6.711 ms | 278.762 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 5 | 1 | 642 | 2.637 ms | 277.245 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 5 | 0 | 65 | 0 | 9 | 1 | 1263 | 10.352 ms | 502.933 us | 0 | 0 | 0 | 4 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 557 | 0 | 64 | 0 | 50 | 1 | 1114 | 20.189 ms | 5.374 ms | 25 | 800 | 103 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 554 | 0 | 64 | 0 | 50 | 1 | 1171 | 46.717 ms | 7.142 ms | 123 | 3936 | 133 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.390 us | 64.420 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.550 us | 62.770 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 12.510 us | 97.371 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 27.120 us | 185.001 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 154 | 0 | 65 | 0 | 29 | 1 | 1542 | 3.354 ms | 714.606 us | 0 | 0 | 0 | 244 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1313 | 8.857 ms | 729.815 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 150.000 ns | 1.490 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 40.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 160.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 40.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 130.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 50.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 200.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 110.000 ns | 1.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 1.130 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 50.000 ns | 1.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 300.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 150.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 80.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 80.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 530.000 ns | 1.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 330.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 160.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 190.000 ns | 1.000 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.020 us | 2.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 640.000 ns | 1.990 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 310.000 ns | 3.140 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 350.000 ns | 2.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.940 us | 5.270 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.090 us | 2.930 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 640.000 ns | 5.610 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 740.000 ns | 4.530 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.430 us | 13.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 170.000 ns | 1.520 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 430.000 ns | 30.871 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 50.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 108 | 84.751 us | 35.050 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 390.000 ns | 3.750 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 13 | 2.470 us | 42.551 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 50.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 173 | 376.062 us | 83.701 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 790.000 ns | 6.780 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 6 | 20.680 us | 26.480 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 326 | 1.569 ms | 160.391 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.570 us | 13.380 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 2 | 140.000 ns | 15.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 8.117 ms | 378.491 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 3.130 us | 30.680 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 4.790 us | 26.570 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 50.000 ns | 290.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 160.000 ns | 1.410 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 120.000 ns | 1.090 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 1.080 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 820.000 ns | 17.330 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 120.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 850.000 ns | 1.800 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 1.850 us | 30.451 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 120.000 ns | 400.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 8 | 3.050 us | 2.840 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 61 | 2.810 us | 71.881 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 69 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 4 | 130.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 20 | 5.530 us | 6.270 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_8 | publish | 111 | 6.760 us | 150.542 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 145 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 220.000 ns | 520.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 44 | 16.500 us | 12.770 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 60.000 ns | 4.310 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 211 | 17.900 us | 533.862 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 327 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 180.000 ns | 640.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 350.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 50.000 ns | 490.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 700.000 ns | 14.160 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 340.000 ns | 860.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 150.000 ns | 350.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 80.000 ns | 690.000 ns |
| thread_safe_contention_independent_slots_2 | publish | 33 | 1.860 us | 26.820 us |
| thread_safe_contention_independent_slots_4 | other | 38 | 48.471 us | 2.470 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 280.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 160.000 ns | 1.270 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 12 | 61.211 us | 17.820 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 150.801 us | 57.780 us |
| thread_safe_contention_independent_slots_8 | other | 77 | 137.290 us | 4.390 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 560.000 ns | 1.300 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 310.000 ns | 2.630 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 26 | 355.315 us | 35.070 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.065 ms | 118.060 us |
| thread_safe_contention_independent_slots_16 | other | 122 | 1.054 ms | 8.011 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.120 us | 2.540 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 630.000 ns | 5.210 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 39 | 975.326 us | 63.372 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.374 ms | 293.510 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 100.000 ns | 470.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 80.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 50.000 ns | 500.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 790.000 ns | 13.610 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 120.000 ns | 400.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 90.000 ns | 190.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 790.000 ns | 14.600 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 180.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 230.000 ns | 880.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 40.000 ns | 320.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 750.000 ns | 16.170 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 130.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 6 | 560.000 ns | 1.250 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 350.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 830.000 ns | 18.910 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 190.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 10 | 430.000 ns | 1.780 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 50.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 800.000 ns | 28.230 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.370 us | 13.590 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 180.000 ns | 1.440 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 430.000 ns | 26.750 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 720.000 ns | 18.210 us |
| thread_safe_contention_batched_write_bursts_2 | other | 122 | 56.480 us | 30.111 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 380.000 ns | 4.110 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 20 | 1.070 us | 41.380 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 20 | 940.000 ns | 27.920 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 216 | 295.910 us | 64.060 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 740.000 ns | 6.770 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 29 | 2.150 us | 70.331 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 27 | 1.240 us | 50.621 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 38 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 364 | 1.455 ms | 150.750 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 8 | 20.320 us | 2.940 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.550 us | 13.210 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 23 | 2.460 us | 61.620 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 21 | 5.140 us | 62.440 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 29 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 5.635 ms | 320.741 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 3.150 us | 29.871 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 150.000 ns | 15.700 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 270.000 ns | 27.270 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 346 | 1.341 ms | 175.292 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.540 us | 9.960 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 3 | 2.810 us | 12.350 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 673 | 8.557 ms | 403.472 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.970 us | 21.280 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 5 | 170.000 ns | 20.190 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 265 | 844.214 us | 58.440 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 740.000 ns | 8.370 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.239 ms | 108.900 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 2.829 ms | 95.110 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.550 us | 13.350 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 3.880 ms | 170.302 us |
| thread_safe_effect_contention_batch_flush_8 | other | 600 | 2.635 ms | 228.213 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 70.000 ns | 770.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.540 us | 15.021 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 160.000 ns | 20.641 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 200.000 ns | 12.600 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1187 | 10.348 ms | 420.093 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 70.000 ns | 670.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 3.160 us | 29.990 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 4 | 191.000 ns | 15.860 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 5 | 220.000 ns | 36.320 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 326 | 7.303 ms | 169.520 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.210 us | 5.640 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.700 us | 26.140 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 103 | 9.027 ms | 4.653 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 557 | 3.854 ms | 519.562 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 356 | 7.402 ms | 179.611 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.210 us | 5.320 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.740 us | 25.300 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 133 | 34.084 ms | 6.398 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 554 | 5.226 ms | 533.825 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.830 us | 5.400 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.270 us | 5.410 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.310 us | 15.350 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.980 us | 38.260 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.910 us | 4.180 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.280 us | 5.090 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.350 us | 15.270 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 3.010 us | 38.230 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.120 us | 10.190 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.640 us | 9.810 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.740 us | 25.810 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 3.010 us | 51.561 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.860 us | 15.630 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.780 us | 15.660 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 5.730 us | 54.260 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 8.750 us | 99.451 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 483 | 2.844 ms | 211.673 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 822 | 22.030 us | 84.241 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.931 us | 25.340 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 18 | 114.761 us | 188.771 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 154 | 370.003 us | 204.581 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 792 | 8.677 ms | 401.793 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 7.680 us | 25.850 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 5.820 us | 50.090 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 159.371 us | 141.041 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 6.730 us | 111.041 us |

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

