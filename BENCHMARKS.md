# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.6.0`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.352 ms | 2.440 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.198 ms | 7.727 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.033 ms | 1.124 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 2.361 ms | 2.568 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 580.992 us | 600.177 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.393 ms | 1.449 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.748 ms | 2.872 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 4.072 ms | 4.472 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.229 ms | 1.459 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.141 ms | 3.456 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.364 ms | 1.424 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.396 ms | 3.646 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.156 ms | 2.402 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 6.209 ms | 6.819 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.653 ms | 4.324 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.801 ms | 6.016 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.985 ms | 2.149 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.856 ms | 4.057 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 446.951 us | 477.517 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 948.761 us | 1.208 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.103 ms | 1.529 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.106 ms | 2.231 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 8.219 ns | 8.203 ns - 8.237 ns |
| cached_reads | thread_safe_context | 64.409 ns | 64.149 ns - 64.707 ns |
| cold_first_get | context | 90.514 ns | 76.662 ns - 114.452 ns |
| cold_first_get | thread_safe_context | 1.090 us | 1.041 us - 1.140 us |
| dependency_fan_out | context / 32 | 3.939 us | 3.521 us - 4.493 us |
| dependency_fan_out | context / 256 | 48.407 us | 46.418 us - 51.509 us |
| dependency_fan_out | thread_safe_context / 32 | 21.660 us | 20.945 us - 22.392 us |
| dependency_fan_out | thread_safe_context / 256 | 165.404 us | 163.334 us - 167.680 us |
| set_cell_invalidation | high_fan_out / 512 | 104.664 us | 97.784 us - 110.831 us |
| set_cell_invalidation | same_slot_contention / 1 | 44.926 us | 44.011 us - 45.764 us |
| set_cell_invalidation | same_slot_contention / 2 | 106.822 us | 101.505 us - 112.716 us |
| set_cell_invalidation | same_slot_contention / 4 | 219.595 us | 208.667 us - 231.854 us |
| set_cell_invalidation | same_slot_contention / 8 | 556.262 us | 540.140 us - 572.342 us |
| set_cell_invalidation | same_slot_contention / 16 | 2.150 ms | 2.060 ms - 2.230 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 43.324 us | 42.199 us - 44.266 us |
| set_cell_invalidation | independent_slot_contention / 2 | 79.075 us | 71.814 us - 89.678 us |
| set_cell_invalidation | independent_slot_contention / 4 | 132.249 us | 128.693 us - 135.778 us |
| set_cell_invalidation | independent_slot_contention / 8 | 235.162 us | 229.905 us - 240.152 us |
| set_cell_invalidation | independent_slot_contention / 16 | 426.568 us | 418.691 us - 435.244 us |
| set_cell_invalidation | batched_write_bursts / 1 | 136.048 us | 134.512 us - 137.361 us |
| set_cell_invalidation | batched_write_bursts / 2 | 222.115 us | 210.978 us - 234.124 us |
| set_cell_invalidation | batched_write_bursts / 4 | 501.541 us | 485.651 us - 519.046 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.252 ms | 1.211 ms - 1.282 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.459 ms | 3.359 ms - 3.554 ms |
| memo_equality_suppression | context | 3.631 us | 2.846 us - 4.298 us |
| memo_equality_suppression | thread_safe_context | 35.837 us | 34.660 us - 36.926 us |
| effect_flushing | context | 50.374 ns | 49.256 ns - 52.385 ns |
| effect_flushing | thread_safe_context | 927.235 ns | 919.876 ns - 939.651 ns |
| batch_storms | context / 64 | 2.918 us | 2.881 us - 2.964 us |
| batch_storms | thread_safe_context / 64 | 6.672 us | 6.651 us - 6.692 us |
| thread_safe_contention | same_slot_write_read / 1 | 108.206 us | 106.475 us - 110.487 us |
| thread_safe_contention | same_slot_write_read / 2 | 306.852 us | 302.234 us - 311.632 us |
| thread_safe_contention | same_slot_write_read / 4 | 759.951 us | 743.339 us - 777.093 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.313 ms | 2.240 ms - 2.371 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.190 ms | 6.987 ms - 7.371 ms |
| thread_safe_contention | independent_slots / 1 | 108.277 us | 107.135 us - 109.417 us |
| thread_safe_contention | independent_slots / 2 | 194.375 us | 189.717 us - 198.839 us |
| thread_safe_contention | independent_slots / 4 | 425.308 us | 414.953 us - 435.217 us |
| thread_safe_contention | independent_slots / 8 | 1.044 ms | 1.009 ms - 1.077 ms |
| thread_safe_contention | independent_slots / 16 | 2.356 ms | 2.229 ms - 2.454 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 107.395 us | 106.751 us - 108.181 us |
| thread_safe_contention | read_mostly_waiters / 2 | 159.369 us | 157.150 us - 161.584 us |
| thread_safe_contention | read_mostly_waiters / 4 | 258.289 us | 253.816 us - 263.252 us |
| thread_safe_contention | read_mostly_waiters / 8 | 581.642 us | 573.047 us - 589.006 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.379 ms | 1.354 ms - 1.403 ms |
| thread_safe_contention | batched_write_bursts / 1 | 213.365 us | 212.224 us - 214.565 us |
| thread_safe_contention | batched_write_bursts / 2 | 603.182 us | 589.554 us - 616.256 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.587 ms | 1.573 ms - 1.600 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.710 ms | 2.606 ms - 2.791 ms |
| thread_safe_contention | batched_write_bursts / 16 | 4.120 ms | 3.991 ms - 4.246 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.265 ms | 1.225 ms - 1.318 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.183 ms | 3.089 ms - 3.276 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.352 ms | 1.323 ms - 1.380 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.353 ms | 3.227 ms - 3.467 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.215 ms | 2.138 ms - 2.293 ms |
| thread_safe_effect_contention | batch_flush / 16 | 6.246 ms | 6.031 ms - 6.462 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.806 ms | 3.614 ms - 4.023 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.832 ms | 5.752 ms - 5.904 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.018 ms | 1.970 ms - 2.068 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.845 ms | 3.769 ms - 3.917 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 449.736 us | 443.635 us - 457.356 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 989.916 us | 929.043 us - 1.062 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.176 ms | 1.090 ms - 1.280 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.049 ms | 1.925 ms - 2.151 ms |
| profile_instrumentation | context_snapshot | 267.654 ns | 266.048 ns - 269.225 ns |
| profile_instrumentation | thread_safe_snapshot | 299.793 us | 297.991 us - 301.286 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.020 us | 23.320 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 2.291 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 990.000 ns | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 350.000 ns | 4.930 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 360.000 ns | 4.780 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 1.900 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 7.440 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 480.000 ns | 3.250 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.010 us | 4.870 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.040 us | 8.050 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.980 us | 16.020 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.270 us | 47.521 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 138 | 86.501 us | 74.271 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 199 | 459.623 us | 122.980 us | 0 | 0 | 0 | 6 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 372 | 1.797 ms | 219.632 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 5.792 ms | 365.313 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 950.000 ns | 17.080 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 45 | 1.220 us | 24.301 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 55 | 0 | 1 | 0 | 0 | 0 | 145 | 13.490 us | 89.871 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 113 | 0 | 1 | 0 | 0 | 0 | 300 | 29.720 us | 293.040 us | 127 | 127 | 1 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 212 | 0 | 1 | 0 | 0 | 0 | 584 | 36.731 us | 620.564 us | 255 | 255 | 1 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 810.000 ns | 14.500 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 3.490 us | 26.120 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 125 | 261.323 us | 79.690 us | 53 | 53 | 10 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 234 | 1.340 ms | 147.803 us | 112 | 112 | 15 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 463 | 8.654 ms | 332.722 us | 224 | 224 | 31 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 790.000 ns | 13.930 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 780.000 ns | 13.510 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 42 | 4.870 us | 22.480 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 910.000 ns | 13.660 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 75 | 64.980 us | 62.120 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.740 us | 59.890 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 195 | 65.160 us | 107.981 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 36 | 0 | 16 | 0 | 0 | 0 | 386 | 291.100 us | 231.372 us | 0 | 0 | 0 | 40 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 6 | 0 | 32 | 0 | 0 | 0 | 385 | 1.778 ms | 221.562 us | 0 | 0 | 0 | 5 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 730 | 6.359 ms | 400.191 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 390 | 1.828 ms | 211.751 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 728 | 5.407 ms | 369.662 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 29 | 1 | 400 | 1.581 ms | 149.320 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 36 | 1 | 704 | 9.947 ms | 336.842 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 5 | 1 | 646 | 3.987 ms | 313.623 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 20.468 ms | 608.258 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 551 | 0 | 64 | 0 | 50 | 1 | 1124 | 26.189 ms | 5.945 ms | 17 | 544 | 111 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 559 | 0 | 64 | 0 | 50 | 1 | 1202 | 54.479 ms | 7.741 ms | 111 | 3552 | 145 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.431 us | 71.320 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.981 us | 62.980 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 10.090 us | 101.970 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 19.540 us | 189.081 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 71 | 0 | 65 | 0 | 5 | 1 | 673 | 2.483 ms | 400.402 us | 0 | 0 | 0 | 78 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1311 | 8.753 ms | 724.776 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 110.000 ns | 1.321 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 40.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 210.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 100.000 ns | 1.310 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 1.140 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 190.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 110.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.110 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 1.160 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 110.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 150.000 ns | 2.510 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 100.000 ns | 1.970 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 60.000 ns | 1.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 220.000 ns | 930.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 130.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 80.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 420.000 ns | 1.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 270.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 140.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 180.000 ns | 1.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 890.000 ns | 1.880 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 560.000 ns | 1.240 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 260.000 ns | 2.650 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 330.000 ns | 2.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.720 us | 3.920 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.130 us | 2.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 460.000 ns | 4.890 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 670.000 ns | 4.750 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.440 us | 15.171 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.350 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 580.000 ns | 30.460 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 50.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 112 | 85.311 us | 32.521 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 15 | 880.000 ns | 37.630 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 174 | 458.683 us | 82.310 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 40.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 500.000 ns | 6.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 6 | 380.000 ns | 34.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 332 | 1.795 ms | 173.012 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 90.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 870.000 ns | 12.490 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 250.000 ns | 33.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 50.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 5.790 ms | 324.563 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.730 us | 27.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 130.000 ns | 13.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 300.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 80.000 ns | 570.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 760.000 ns | 15.960 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 100.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 80.000 ns | 180.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 23 | 1.010 us | 23.391 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 80.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 12 | 10.270 us | 6.190 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 55 | 3.120 us | 82.981 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 73 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 150.000 ns | 570.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 26 | 3.450 us | 9.520 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 160.000 ns | 11.280 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 113 | 25.930 us | 271.360 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 153 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 3.020 us | 810.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 38 | 12.730 us | 14.320 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 520.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 70.000 ns | 6.440 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 212 | 20.881 us | 598.474 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 326 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 140.000 ns | 960.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 620.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 890.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 560.000 ns | 12.030 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 210.000 ns | 930.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 120.000 ns | 410.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 650.000 ns |
| thread_safe_contention_independent_slots_2 | publish | 33 | 3.100 us | 24.130 us |
| thread_safe_contention_independent_slots_4 | other | 36 | 21.740 us | 2.320 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 230.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.240 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 10 | 37.990 us | 17.090 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 201.253 us | 58.410 us |
| thread_safe_contention_independent_slots_8 | other | 60 | 151.483 us | 4.110 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 570.000 ns | 1.270 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 320.000 ns | 2.581 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 15 | 176.552 us | 24.580 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.011 ms | 115.262 us |
| thread_safe_contention_independent_slots_16 | other | 113 | 871.056 us | 6.890 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.140 us | 2.490 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 500.000 ns | 5.870 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 31 | 646.525 us | 41.770 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.134 ms | 275.702 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 100.000 ns | 680.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 70.000 ns | 200.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 590.000 ns | 12.690 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 100.000 ns | 400.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 600.000 ns | 12.670 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 100.000 ns | 370.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 3.890 us | 2.420 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 850.000 ns | 19.400 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 2 | 80.000 ns | 150.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 50.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 670.000 ns | 12.830 us |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 150.000 ns | 460.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 16 | 52.220 us | 5.120 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 12.580 us | 56.090 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 37 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.420 us | 14.430 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 1.580 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 650.000 ns | 26.040 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 480.000 ns | 17.650 us |
| thread_safe_contention_batched_write_bursts_2 | other | 128 | 54.680 us | 30.290 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.880 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 23 | 9.600 us | 44.521 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 580.000 ns | 29.110 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 238 | 269.760 us | 59.791 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 8 | 11.180 us | 2.660 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 400.000 ns | 6.100 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 40 | 5.060 us | 87.520 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 36 | 4.700 us | 75.301 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 48 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 332 | 1.777 ms | 163.492 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 920.000 ns | 12.040 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 5 | 270.000 ns | 24.940 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 6 | 190.000 ns | 20.900 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 8 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 6.357 ms | 329.231 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.890 us | 29.770 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 190.000 ns | 20.480 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 120.000 ns | 20.510 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 353 | 1.827 ms | 179.351 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 940.000 ns | 9.090 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 5 | 300.000 ns | 23.310 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 661 | 5.405 ms | 336.482 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.700 us | 19.200 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 110.000 ns | 13.980 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 257 | 483.253 us | 50.480 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 440.000 ns | 8.510 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.097 ms | 90.330 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 417 | 4.008 ms | 100.031 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 860.000 ns | 12.220 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.939 ms | 224.591 us |
| thread_safe_effect_contention_batch_flush_8 | other | 602 | 3.985 ms | 259.832 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 70.000 ns | 470.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 950.000 ns | 13.140 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 5 | 240.000 ns | 27.621 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 110.000 ns | 12.560 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 20.466 ms | 545.657 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 70.000 ns | 790.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.810 us | 30.610 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 40.000 ns | 19.001 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 60.000 ns | 12.200 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 334 | 7.259 ms | 183.652 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.220 us | 5.161 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.810 us | 27.030 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 111 | 14.651 ms | 5.244 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 551 | 4.275 ms | 484.574 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 368 | 5.934 ms | 180.771 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 66 | 2.583 ms | 6.780 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.750 us | 25.690 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 145 | 42.757 ms | 7.029 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 559 | 3.204 ms | 498.712 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.750 us | 6.010 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.130 us | 5.980 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.121 us | 17.790 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.430 us | 41.540 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.691 us | 6.310 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.040 us | 5.690 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 910.000 ns | 15.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.340 us | 35.470 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.540 us | 9.890 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.520 us | 9.830 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.920 us | 24.450 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.110 us | 57.800 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 6.880 us | 15.700 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.550 us | 13.491 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.650 us | 51.370 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 4.460 us | 108.520 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 408 | 2.467 ms | 217.222 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 126 | 4.231 us | 15.120 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.901 us | 25.810 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 3 | 120.000 ns | 80.590 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 71 | 10.080 us | 61.660 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 791 | 8.736 ms | 388.373 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 8.960 us | 28.960 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.700 us | 51.931 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 100.000 ns | 146.181 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 4.270 us | 109.331 us |

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

