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
| thread_safe_contention | same_slot_write_read / 8 | 2.134 ms | 2.377 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.692 ms | 7.432 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 783.994 us | 831.301 us | 10 |
| thread_safe_contention | independent_slots / 16 | 1.838 ms | 2.001 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 548.960 us | 568.782 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.006 ms | 1.158 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.711 ms | 2.907 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 2.677 ms | 2.908 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 840.178 us | 963.316 us | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 1.960 ms | 2.037 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.054 ms | 1.150 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.601 ms | 2.732 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.450 ms | 1.614 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 3.624 ms | 3.856 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.619 ms | 3.766 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.850 ms | 6.739 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.082 ms | 2.279 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.261 ms | 5.080 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 552.728 us | 582.234 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.110 ms | 1.147 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.030 ms | 1.137 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.507 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 8.117 ns | 8.080 ns - 8.161 ns |
| cached_reads | thread_safe_context | 63.543 ns | 63.385 ns - 63.732 ns |
| cold_first_get | context | 77.786 ns | 69.003 ns - 93.463 ns |
| cold_first_get | thread_safe_context | 967.242 ns | 941.695 ns - 992.335 ns |
| dependency_fan_out | context / 32 | 3.313 us | 3.063 us - 3.605 us |
| dependency_fan_out | context / 256 | 45.457 us | 43.522 us - 48.657 us |
| dependency_fan_out | thread_safe_context / 32 | 19.402 us | 19.177 us - 19.645 us |
| dependency_fan_out | thread_safe_context / 256 | 149.795 us | 148.707 us - 150.916 us |
| set_cell_invalidation | high_fan_out / 512 | 89.509 us | 86.166 us - 92.964 us |
| set_cell_invalidation | same_slot_contention / 1 | 41.540 us | 40.818 us - 42.169 us |
| set_cell_invalidation | same_slot_contention / 2 | 87.136 us | 83.375 us - 92.995 us |
| set_cell_invalidation | same_slot_contention / 4 | 175.925 us | 170.159 us - 182.436 us |
| set_cell_invalidation | same_slot_contention / 8 | 442.402 us | 431.593 us - 452.822 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.654 ms | 1.607 ms - 1.708 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 40.084 us | 39.098 us - 40.930 us |
| set_cell_invalidation | independent_slot_contention / 2 | 72.537 us | 70.248 us - 75.143 us |
| set_cell_invalidation | independent_slot_contention / 4 | 125.985 us | 123.416 us - 128.517 us |
| set_cell_invalidation | independent_slot_contention / 8 | 228.266 us | 220.853 us - 236.610 us |
| set_cell_invalidation | independent_slot_contention / 16 | 511.470 us | 486.688 us - 535.344 us |
| set_cell_invalidation | batched_write_bursts / 1 | 126.702 us | 125.683 us - 127.761 us |
| set_cell_invalidation | batched_write_bursts / 2 | 194.366 us | 190.286 us - 198.066 us |
| set_cell_invalidation | batched_write_bursts / 4 | 387.899 us | 379.095 us - 397.180 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.159 ms | 1.105 ms - 1.214 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.959 ms | 2.850 ms - 3.056 ms |
| memo_equality_suppression | context | 2.193 us | 1.851 us - 2.728 us |
| memo_equality_suppression | thread_safe_context | 29.958 us | 29.475 us - 30.478 us |
| effect_flushing | context | 50.931 ns | 50.689 ns - 51.173 ns |
| effect_flushing | thread_safe_context | 889.741 ns | 886.157 ns - 893.315 ns |
| batch_storms | context / 64 | 2.850 us | 2.843 us - 2.858 us |
| batch_storms | thread_safe_context / 64 | 7.278 us | 7.263 us - 7.294 us |
| thread_safe_contention | same_slot_write_read / 1 | 103.267 us | 102.491 us - 104.014 us |
| thread_safe_contention | same_slot_write_read / 2 | 283.857 us | 274.602 us - 294.001 us |
| thread_safe_contention | same_slot_write_read / 4 | 709.244 us | 677.785 us - 753.481 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.116 ms | 2.016 ms - 2.210 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.675 ms | 6.315 ms - 7.001 ms |
| thread_safe_contention | independent_slots / 1 | 107.925 us | 106.496 us - 109.339 us |
| thread_safe_contention | independent_slots / 2 | 194.165 us | 184.699 us - 203.986 us |
| thread_safe_contention | independent_slots / 4 | 367.296 us | 359.324 us - 376.280 us |
| thread_safe_contention | independent_slots / 8 | 780.957 us | 757.999 us - 802.508 us |
| thread_safe_contention | independent_slots / 16 | 1.871 ms | 1.823 ms - 1.921 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 108.671 us | 108.295 us - 109.031 us |
| thread_safe_contention | read_mostly_waiters / 2 | 155.218 us | 151.854 us - 159.080 us |
| thread_safe_contention | read_mostly_waiters / 4 | 250.229 us | 246.292 us - 254.618 us |
| thread_safe_contention | read_mostly_waiters / 8 | 552.543 us | 545.203 us - 559.713 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.027 ms | 1.003 ms - 1.061 ms |
| thread_safe_contention | batched_write_bursts / 1 | 245.995 us | 239.444 us - 254.044 us |
| thread_safe_contention | batched_write_bursts / 2 | 583.761 us | 571.768 us - 597.808 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.370 ms | 1.354 ms - 1.387 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.703 ms | 2.630 ms - 2.777 ms |
| thread_safe_contention | batched_write_bursts / 16 | 2.659 ms | 2.540 ms - 2.763 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 848.411 us | 811.332 us - 887.583 us |
| thread_safe_effect_contention | queue_coalescing / 16 | 1.956 ms | 1.912 ms - 1.996 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.046 ms | 1.005 ms - 1.082 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.612 ms | 2.551 ms - 2.672 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.493 ms | 1.438 ms - 1.546 ms |
| thread_safe_effect_contention | batch_flush / 16 | 3.643 ms | 3.552 ms - 3.735 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.628 ms | 3.598 ms - 3.665 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.931 ms | 5.781 ms - 6.136 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.119 ms | 2.048 ms - 2.189 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.323 ms | 4.176 ms - 4.520 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 558.593 us | 552.279 us - 566.048 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.116 ms | 1.104 ms - 1.129 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.043 ms | 1.023 ms - 1.069 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.448 ms - 1.481 ms |
| profile_instrumentation | context_snapshot | 421.918 ns | 416.285 ns - 428.322 ns |
| profile_instrumentation | thread_safe_snapshot | 305.223 us | 300.698 us - 312.876 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.530 us | 18.830 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 1.230 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 210.000 ns | 1.020 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 190.000 ns | 920.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 920.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 920.000 ns | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 190.000 ns | 1.290 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 450.000 ns | 1.790 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 800.000 ns | 3.470 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 1.730 us | 7.200 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.400 us | 18.010 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.670 us | 36.471 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 150 | 58.840 us | 57.030 us | 0 | 0 | 0 | 19 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 201 | 334.633 us | 78.750 us | 0 | 0 | 0 | 7 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 2.266 ms | 137.322 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 10.256 ms | 295.694 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 660.000 ns | 12.900 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 22 | 0 | 1 | 0 | 0 | 0 | 44 | 770.000 ns | 20.771 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 53 | 0 | 1 | 0 | 0 | 0 | 136 | 22.400 us | 120.761 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 103 | 0 | 1 | 0 | 0 | 0 | 274 | 8.710 us | 163.291 us | 128 | 128 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 205 | 0 | 1 | 0 | 0 | 0 | 552 | 41.650 us | 262.053 us | 252 | 252 | 4 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 620.000 ns | 10.240 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 6.210 us | 23.001 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 122 | 195.880 us | 68.090 us | 54 | 54 | 9 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 235 | 1.556 ms | 144.430 us | 110 | 110 | 17 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 491 | 6.110 ms | 282.843 us | 215 | 215 | 40 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 930.000 ns | 16.430 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 910.000 ns | 16.580 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 30 | 10.110 us | 17.880 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 35 | 48.720 us | 27.850 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 86 | 12.541 us | 53.971 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.400 us | 51.051 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 20 | 0 | 8 | 0 | 0 | 0 | 187 | 46.570 us | 86.740 us | 0 | 0 | 0 | 20 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 35 | 0 | 16 | 0 | 0 | 0 | 367 | 250.690 us | 193.143 us | 0 | 0 | 0 | 37 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 58 | 0 | 32 | 0 | 0 | 0 | 678 | 847.100 us | 397.551 us | 0 | 0 | 0 | 60 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 4 | 0 | 64 | 0 | 0 | 0 | 723 | 7.209 ms | 285.143 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 2 | 1 | 368 | 1.412 ms | 129.631 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 734 | 7.076 ms | 291.412 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 36 | 1 | 415 | 1.875 ms | 184.562 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 40 | 1 | 716 | 6.149 ms | 255.764 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 3 | 1 | 636 | 2.650 ms | 179.411 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 12.252 ms | 377.163 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 564 | 0 | 64 | 0 | 50 | 1 | 1151 | 7.983 ms | 2.080 ms | 10 | 320 | 118 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 553 | 0 | 64 | 0 | 49 | 1 | 1318 | 26.863 ms | 3.100 ms | 47 | 1504 | 209 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.690 us | 59.980 us | 127 | 4064 | 0 | 4064 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.330 us | 59.640 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 9.080 us | 88.530 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 18.191 us | 154.671 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 96 | 0 | 65 | 0 | 15 | 1 | 1029 | 1.860 ms | 358.432 us | 0 | 0 | 0 | 171 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1183 | 7.580 ms | 477.291 us | 0 | 0 | 0 | 145 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 110.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 110.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 90.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 50.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 150.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 70.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 130.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 110.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 40.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 210.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 110.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 90.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 40.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 400.000 ns | 930.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 210.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 100.000 ns | 1.100 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 90.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 870.000 ns | 1.970 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 450.000 ns | 1.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 230.000 ns | 2.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 180.000 ns | 1.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.710 us | 5.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 840.000 ns | 3.820 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 430.000 ns | 5.150 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 420.000 ns | 3.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.050 us | 10.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 410.000 ns | 23.561 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 120 | 56.760 us | 23.140 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.400 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 19 | 1.760 us | 30.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 175 | 333.493 us | 44.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 5.680 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 7 | 610.000 ns | 28.380 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 2.265 ms | 106.822 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 790.000 ns | 13.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 130.000 ns | 15.170 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 1.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 10.254 ms | 240.474 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.670 us | 26.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 80.000 ns | 28.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 430.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 120.000 ns | 630.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 60.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 20.000 ns | 450.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 460.000 ns | 11.430 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 130.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 22 | 570.000 ns | 20.021 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 140.000 ns | 470.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 11 | 13.840 us | 6.130 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 560.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 53 | 8.390 us | 113.601 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 67 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 4 | 150.000 ns | 480.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 19 | 5.280 us | 5.490 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_8 | publish | 103 | 3.260 us | 156.991 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 147 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 12 | 960.000 ns | 990.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 38 | 8.180 us | 8.681 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 4 | 5.620 us | 14.590 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 205 | 26.850 us | 237.482 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 292 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 90.000 ns | 450.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 260.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 650.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 430.000 ns | 8.880 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 240.000 ns | 870.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 100.000 ns | 390.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 40.000 ns | 550.000 ns |
| thread_safe_contention_independent_slots_2 | publish | 33 | 5.830 us | 21.191 us |
| thread_safe_contention_independent_slots_4 | other | 34 | 43.010 us | 2.910 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 200.000 ns | 770.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.170 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 9 | 21.370 us | 14.220 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 131.190 us | 49.020 us |
| thread_safe_contention_independent_slots_8 | other | 59 | 106.170 us | 3.830 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 390.000 ns | 1.420 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 230.000 ns | 2.310 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 17 | 240.022 us | 23.290 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.209 ms | 113.580 us |
| thread_safe_contention_independent_slots_16 | other | 132 | 786.424 us | 11.390 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 940.000 ns | 3.670 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 400.000 ns | 5.230 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 40 | 798.795 us | 44.751 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 4.523 ms | 217.802 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 160.000 ns | 1.420 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 160.000 ns | 1.000 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 1.310 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 570.000 ns | 12.700 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 140.000 ns | 1.410 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 130.000 ns | 920.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 1.120 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 610.000 ns | 13.130 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 120.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 9.440 us | 2.240 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 20.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 530.000 ns | 14.940 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 150.000 ns | 520.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 45.260 us | 7.110 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 3.280 us | 19.800 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 160.000 ns | 920.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 24 | 11.541 us | 8.721 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 930.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 820.000 ns | 43.400 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 39 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.170 us | 10.910 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 280.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 90.000 ns | 1.500 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 630.000 ns | 22.550 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 440.000 ns | 15.811 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 36.710 us | 22.710 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.550 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 8.760 us | 33.700 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 20 | 810.000 ns | 26.570 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 231 | 244.640 us | 51.481 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 280.000 ns | 1.840 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 7.440 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 37 | 4.230 us | 67.742 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 35 | 1.090 us | 64.640 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 44 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 442 | 829.970 us | 97.580 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 8 | 8.720 us | 3.160 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 870.000 ns | 11.940 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 60 | 4.920 us | 111.360 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 58 | 2.620 us | 173.511 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 78 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 648 | 7.207 ms | 225.332 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 450.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.650 us | 25.941 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 3 | 120.000 ns | 14.040 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 4 | 130.000 ns | 19.380 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 335 | 1.411 ms | 110.701 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 920.000 ns | 9.580 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 1 | 30.000 ns | 9.350 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 667 | 7.074 ms | 257.822 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.770 us | 19.680 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 70.000 ns | 13.910 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 272 | 874.498 us | 76.242 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 400.000 ns | 7.150 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.000 ms | 101.170 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 429 | 2.870 ms | 113.341 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 870.000 ns | 10.841 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 3.278 ms | 131.582 us |
| thread_safe_effect_contention_batch_flush_8 | other | 595 | 2.649 ms | 145.131 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 50.000 ns | 640.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 940.000 ns | 12.590 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 170.000 ns | 14.400 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 100.000 ns | 6.650 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 12.250 ms | 325.683 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 780.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.800 us | 27.990 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 30.000 ns | 11.140 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 50.000 ns | 11.570 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 341 | 426.622 us | 175.752 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.710 us | 5.950 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.760 us | 23.730 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 118 | 6.273 ms | 1.510 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 564 | 1.280 ms | 365.341 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 428 | 3.092 ms | 183.517 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.800 us | 5.980 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.810 us | 23.990 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 209 | 21.389 ms | 2.530 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 553 | 2.378 ms | 356.473 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.960 us | 5.860 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.960 us | 6.310 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 930.000 ns | 15.300 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.840 us | 32.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.860 us | 3.970 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.840 us | 5.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 840.000 ns | 13.590 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.790 us | 36.570 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.590 us | 9.940 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 1.970 us | 12.000 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.760 us | 23.130 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.760 us | 43.460 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.361 us | 17.330 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.800 us | 16.270 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.580 us | 47.631 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.450 us | 73.440 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 441 | 1.761 ms | 116.611 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 416 | 21.961 us | 47.520 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.710 us | 22.810 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 11 | 4.540 us | 73.710 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 96 | 70.810 us | 97.781 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 788 | 7.555 ms | 247.540 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 3.590 us | 15.630 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.440 us | 49.200 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 4 | 190.000 ns | 89.361 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 17.600 us | 75.560 us |

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

