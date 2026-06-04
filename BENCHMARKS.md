# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.7.0`.

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
| async_cached_resolve | async_context | 4.602 us | 4.199 us - 5.016 us |
| async_cached_resolve | sync_context_baseline | 64.156 ns | 63.444 ns - 64.994 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.349 us | 1.339 us - 1.361 us |
| async_cold_resolve | async_context | 4.735 us | 4.351 us - 5.192 us |
| async_cold_resolve | sync_context_baseline | 74.954 ns | 72.769 ns - 76.902 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.029 us | 991.197 ns - 1.065 us |
| async_invalidation_throughput | async_context | 282.558 us | 266.969 us - 298.227 us |
| async_invalidation_throughput | sync_context_baseline | 2.917 us | 2.911 us - 2.922 us |
| async_invalidation_throughput | thread_safe_context_baseline | 39.160 us | 39.107 us - 39.215 us |
| async_cancellation_throughput | async_invalidate_in_flight | 62.471 us | 53.231 us - 70.406 us |
| async_concurrent_contention | async_context / 1 | 71.842 us | 69.848 us - 73.955 us |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 54.585 us | 53.876 us - 55.301 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 576.186 us | 570.271 us - 581.101 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 4.620 ms | 4.588 ms - 4.651 ms |
| async_effect_throughput | async_context | 188.216 ms | 188.007 ms - 188.457 ms |
| async_batch_throughput | async_context | 88.215 us | 79.695 us - 97.159 us |
| async_batch_throughput | sync_context_baseline | 11.128 us | 10.599 us - 11.936 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.020 us | 16.950 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 1.260 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 210.000 ns | 910.000 ns | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 910.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 920.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 1.200 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 4.750 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 450.000 ns | 1.860 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 920.000 ns | 3.770 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 1.860 us | 6.750 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.540 us | 13.600 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.190 us | 54.391 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 138 | 143.952 us | 96.041 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 193 | 579.884 us | 140.420 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 375 | 1.838 ms | 218.481 us | 0 | 0 | 0 | 6 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 9.799 ms | 455.143 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 690.000 ns | 16.320 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 51 | 2.880 us | 28.870 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 57 | 0 | 1 | 0 | 0 | 0 | 137 | 5.780 us | 66.760 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 99 | 0 | 1 | 0 | 0 | 0 | 266 | 11.890 us | 140.781 us | 128 | 128 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 209 | 0 | 1 | 0 | 0 | 0 | 547 | 14.030 us | 259.980 us | 256 | 256 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 640.000 ns | 11.721 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 2.500 us | 24.620 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 130 | 233.672 us | 80.621 us | 50 | 50 | 13 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 247 | 1.548 ms | 153.042 us | 105 | 105 | 22 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 485 | 6.490 ms | 279.201 us | 218 | 218 | 37 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 750.000 ns | 12.650 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 700.000 ns | 13.400 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 39 | 870.000 ns | 19.490 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 55 | 25.140 us | 20.170 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 50 | 3.520 us | 35.490 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.560 us | 56.850 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 30 | 0 | 8 | 0 | 0 | 0 | 229 | 103.011 us | 159.440 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 53 | 0 | 16 | 0 | 0 | 0 | 457 | 262.822 us | 351.994 us | 0 | 0 | 0 | 54 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 62 | 0 | 32 | 0 | 0 | 0 | 730 | 1.137 ms | 491.101 us | 0 | 0 | 0 | 72 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 718 | 8.562 ms | 426.353 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 8 | 1 | 406 | 1.679 ms | 220.813 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 7 | 1 | 756 | 5.776 ms | 424.312 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 36 | 1 | 416 | 1.628 ms | 158.331 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 31 | 1 | 696 | 6.607 ms | 278.052 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 5 | 0 | 33 | 0 | 9 | 1 | 655 | 2.626 ms | 273.405 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 18.394 ms | 626.865 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 550 | 0 | 64 | 0 | 50 | 1 | 1117 | 24.274 ms | 5.626 ms | 20 | 640 | 108 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 556 | 0 | 64 | 0 | 50 | 1 | 1217 | 68.577 ms | 7.947 ms | 101 | 3232 | 155 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.340 us | 58.992 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.390 us | 53.691 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 9.160 us | 85.680 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 17.920 us | 159.761 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 93 | 0 | 65 | 0 | 11 | 1 | 904 | 2.549 ms | 469.834 us | 0 | 0 | 0 | 152 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1181 | 7.967 ms | 669.614 us | 0 | 0 | 0 | 157 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 110.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 100.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 170.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 150.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 150.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 100.000 ns | 1.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 230.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 130.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 50.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 40.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 440.000 ns | 1.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 250.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 810.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 890.000 ns | 1.780 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 440.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 240.000 ns | 2.150 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 290.000 ns | 1.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.760 us | 3.800 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 800.000 ns | 2.520 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 470.000 ns | 4.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 510.000 ns | 3.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.440 us | 16.760 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 110.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 2.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 500.000 ns | 33.151 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 112 | 118.702 us | 45.541 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 3.390 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 15 | 24.960 us | 46.700 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 170 | 579.144 us | 103.610 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 410.000 ns | 6.290 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 4 | 240.000 ns | 30.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 334 | 1.837 ms | 176.881 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 11.590 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 6 | 270.000 ns | 29.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 20.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 9.797 ms | 399.243 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.730 us | 26.880 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 110.000 ns | 28.610 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 130.000 ns | 610.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 50.000 ns | 270.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 480.000 ns | 15.090 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 100.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 860.000 ns | 1.810 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 280.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 1.900 us | 26.390 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 17 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 130.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 12 | 2.420 us | 4.060 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 57 | 3.200 us | 62.010 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 63 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 4 | 120.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 16 | 4.700 us | 5.990 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 290.000 ns |
| thread_safe_contention_same_slot_write_read_8 | publish | 99 | 7.030 us | 134.111 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 146 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 4 | 130.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 48 | 6.730 us | 9.690 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_16 | publish | 209 | 7.140 us | 249.650 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 285 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 120.000 ns | 480.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 430.000 ns | 10.751 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 250.000 ns | 850.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 130.000 ns | 320.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 580.000 ns |
| thread_safe_contention_independent_slots_2 | publish | 33 | 2.060 us | 22.870 us |
| thread_safe_contention_independent_slots_4 | other | 38 | 39.971 us | 3.010 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 220.000 ns | 650.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.090 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 13 | 23.240 us | 22.040 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 170.131 us | 53.831 us |
| thread_safe_contention_independent_slots_8 | other | 66 | 219.643 us | 3.980 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 370.000 ns | 1.250 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 2.100 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 22 | 152.301 us | 30.240 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.175 ms | 115.472 us |
| thread_safe_contention_independent_slots_16 | other | 129 | 547.543 us | 6.860 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 870.000 ns | 2.410 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 490.000 ns | 4.170 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 37 | 347.433 us | 42.070 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 5.593 ms | 223.691 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 140.000 ns | 580.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 50.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 520.000 ns | 11.530 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 130.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 80.000 ns | 160.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 450.000 ns | 12.570 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 100.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 250.000 ns | 1.190 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 20.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 500.000 ns | 17.640 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 120.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 17 | 24.380 us | 3.940 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 610.000 ns | 15.570 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 8 | 2.720 us | 1.530 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 650.000 ns | 33.290 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.350 us | 13.560 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 1.220 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 560.000 ns | 24.840 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 490.000 ns | 17.040 us |
| thread_safe_contention_batched_write_bursts_2 | other | 142 | 83.371 us | 34.800 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 150.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 2.940 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 31 | 3.870 us | 62.030 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 30 | 15.500 us | 59.520 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 269 | 248.251 us | 73.521 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 460.000 ns | 5.670 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 54 | 2.490 us | 127.652 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 53 | 11.561 us | 144.971 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 63 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 464 | 1.109 ms | 138.490 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 8 | 10.800 us | 2.970 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 860.000 ns | 11.550 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 72 | 7.580 us | 154.900 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 62 | 8.760 us | 183.191 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 92 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 646 | 8.557 ms | 372.352 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.650 us | 26.790 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 2 | 2.640 us | 16.440 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 3 | 80.000 ns | 10.601 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 366 | 1.677 ms | 185.422 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 870.000 ns | 9.040 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 8 | 390.000 ns | 26.351 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 685 | 5.774 ms | 380.192 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.720 us | 18.370 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 7 | 350.000 ns | 25.750 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 273 | 547.144 us | 57.100 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 450.000 ns | 8.100 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.081 ms | 93.131 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 409 | 2.660 ms | 86.600 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 910.000 ns | 15.020 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 3.946 ms | 176.432 us |
| thread_safe_effect_contention_batch_flush_8 | other | 611 | 2.625 ms | 221.104 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 560.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 930.000 ns | 14.300 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 4 | 150.000 ns | 14.880 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 5 | 200.000 ns | 22.561 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 18.391 ms | 550.205 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 110.000 ns | 1.500 us |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.750 us | 51.830 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 40.000 ns | 12.950 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 60.000 ns | 10.380 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 331 | 7.768 ms | 171.052 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.800 us | 8.750 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.860 us | 23.820 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 108 | 12.530 ms | 4.953 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 550 | 3.972 ms | 469.605 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 378 | 15.848 ms | 177.392 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.730 us | 5.140 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.840 us | 22.900 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 155 | 46.604 ms | 7.249 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 556 | 6.122 ms | 492.064 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.760 us | 6.900 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.850 us | 5.501 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 920.000 ns | 14.460 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.810 us | 32.131 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.750 us | 5.370 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.890 us | 5.030 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 970.000 ns | 13.550 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.780 us | 29.741 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.570 us | 10.300 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 1.990 us | 7.150 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.680 us | 21.050 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.920 us | 47.180 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 6.870 us | 13.690 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.720 us | 13.450 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.560 us | 46.930 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.770 us | 85.691 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 433 | 2.455 ms | 200.252 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 300 | 39.630 us | 34.510 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.820 us | 21.390 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 13 | 1.050 us | 122.782 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 93 | 51.891 us | 90.900 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 787 | 7.886 ms | 341.682 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 16.620 us | 13.960 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.570 us | 45.480 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 150.000 ns | 179.391 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 60.170 us | 89.101 us |

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

