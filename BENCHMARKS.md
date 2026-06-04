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

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.480 us | 25.700 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 2.110 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 280.000 ns | 1.200 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 260.000 ns | 1.120 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 1.110 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 1.290 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 260.000 ns | 2.890 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 590.000 ns | 2.170 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.120 us | 4.490 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.350 us | 13.730 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.730 us | 22.650 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.560 us | 59.670 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 141 | 97.871 us | 98.371 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 202 | 823.107 us | 198.433 us | 0 | 0 | 0 | 7 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 371 | 3.171 ms | 300.122 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 712 | 10.672 ms | 520.955 us | 0 | 0 | 0 | 1 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 830.000 ns | 25.240 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 46 | 1.060 us | 33.370 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 59 | 0 | 1 | 0 | 0 | 0 | 139 | 10.760 us | 98.970 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 107 | 0 | 1 | 0 | 0 | 0 | 270 | 20.101 us | 182.512 us | 127 | 127 | 1 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 208 | 0 | 1 | 0 | 0 | 0 | 591 | 57.660 us | 690.046 us | 255 | 255 | 1 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 900.000 ns | 17.860 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 53 | 27.760 us | 45.151 us | 29 | 29 | 2 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 135 | 336.302 us | 97.160 us | 49 | 49 | 14 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 231 | 2.098 ms | 223.882 us | 112 | 112 | 15 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 463 | 10.129 ms | 419.062 us | 223 | 223 | 32 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 920.000 ns | 18.030 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 890.000 ns | 18.230 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 30 | 1.040 us | 22.410 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 59 | 61.240 us | 40.211 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 64 | 453.422 us | 105.181 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.380 us | 73.780 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 26 | 0 | 8 | 0 | 0 | 0 | 224 | 150.521 us | 190.612 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 26 | 0 | 16 | 0 | 0 | 0 | 318 | 660.048 us | 331.773 us | 0 | 0 | 0 | 26 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 24 | 0 | 32 | 0 | 0 | 0 | 511 | 2.116 ms | 421.954 us | 0 | 0 | 0 | 30 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 725 | 8.531 ms | 537.244 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 392 | 2.459 ms | 310.931 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 736 | 11.257 ms | 556.255 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 34 | 1 | 412 | 2.099 ms | 205.212 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 31 | 1 | 692 | 8.863 ms | 368.714 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 3.145 ms | 318.812 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 3 | 1 | 1242 | 13.636 ms | 623.786 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 524 | 0 | 64 | 0 | 47 | 1 | 1080 | 28.196 ms | 7.114 ms | 20 | 640 | 107 | 4064 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 554 | 0 | 64 | 0 | 50 | 1 | 1227 | 91.429 ms | 10.578 ms | 95 | 3040 | 161 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.010 us | 74.451 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.970 us | 67.001 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 11.600 us | 107.172 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 23.141 us | 201.551 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 89 | 0 | 65 | 0 | 9 | 1 | 827 | 2.413 ms | 526.244 us | 0 | 0 | 0 | 116 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 140 | 0 | 129 | 0 | 7 | 1 | 1455 | 13.289 ms | 989.199 us | 0 | 0 | 0 | 195 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 130.000 ns | 1.080 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 80.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 50.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 130.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 130.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 130.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 80.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 40.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 130.000 ns | 750.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 60.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 870.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 270.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 160.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 90.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 500.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 310.000 ns | 790.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 160.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 150.000 ns | 1.050 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.080 us | 3.930 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 600.000 ns | 2.950 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 330.000 ns | 3.710 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 340.000 ns | 3.140 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.140 us | 6.590 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.190 us | 4.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 650.000 ns | 6.340 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 750.000 ns | 5.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.710 us | 18.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 1.750 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 610.000 ns | 38.580 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 40.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 114 | 95.911 us | 42.260 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 270.000 ns | 4.290 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 16 | 1.570 us | 51.341 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 50.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 176 | 822.017 us | 127.892 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 110.000 ns | 1.050 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 570.000 ns | 8.760 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 7 | 380.000 ns | 59.801 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 930.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 331 | 3.140 ms | 249.092 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.080 us | 14.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 30.371 us | 36.280 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 644 | 10.670 ms | 473.855 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.150 us | 32.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1 | 80.000 ns | 13.770 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 40.000 ns | 500.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 160.000 ns | 1.070 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 70.000 ns | 900.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 830.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 560.000 ns | 22.440 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 140.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 80.000 ns | 230.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 23 | 810.000 ns | 32.380 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 150.000 ns | 360.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 6 | 4.180 us | 5.810 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 59 | 6.400 us | 92.440 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 69 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 250.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 14 | 6.821 us | 8.510 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 370.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 80.000 ns | 4.020 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 107 | 12.910 us | 169.202 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 141 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 190.000 ns | 600.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 52 | 35.410 us | 21.770 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 470.000 ns | 5.350 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 208 | 21.560 us | 661.986 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 323 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 210.000 ns | 1.430 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 110.000 ns | 1.090 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 990.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 550.000 ns | 14.350 us |
| thread_safe_contention_independent_slots_2 | other | 12 | 6.160 us | 1.360 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 140.000 ns | 400.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 700.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 2 | 580.000 ns | 9.030 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 20.820 us | 33.661 us |
| thread_safe_contention_independent_slots_4 | other | 42 | 54.200 us | 3.120 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 280.000 ns | 850.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 160.000 ns | 1.390 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 14 | 72.261 us | 23.090 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 209.401 us | 68.710 us |
| thread_safe_contention_independent_slots_8 | other | 57 | 196.661 us | 4.900 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 530.000 ns | 1.600 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 310.000 ns | 2.580 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 15 | 256.123 us | 38.380 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.644 ms | 176.422 us |
| thread_safe_contention_independent_slots_16 | other | 112 | 941.586 us | 9.640 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.090 us | 3.180 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 620.000 ns | 5.630 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 32 | 826.626 us | 68.310 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 8.359 ms | 332.302 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 160.000 ns | 1.180 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 90.000 ns | 790.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 1.050 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 630.000 ns | 15.010 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 140.000 ns | 480.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 60.000 ns | 260.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 400.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 660.000 ns | 17.090 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 150.000 ns | 550.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 200.000 ns | 1.620 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 40.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 650.000 ns | 19.880 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 130.000 ns | 530.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 15 | 50.840 us | 7.830 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 10.240 us | 31.401 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 140.000 ns | 530.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 13 | 445.722 us | 47.081 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 7.530 us | 57.160 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 28 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.810 us | 17.320 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 410.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 1.960 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 700.000 ns | 32.150 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 680.000 ns | 21.940 us |
| thread_safe_contention_batched_write_bursts_2 | other | 142 | 123.801 us | 46.241 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 4.180 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 31 | 5.960 us | 77.491 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 26 | 20.440 us | 62.500 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 215 | 611.526 us | 144.803 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 1.160 us | 1.680 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 500.000 ns | 7.510 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 27 | 36.341 us | 95.570 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 26 | 10.521 us | 82.210 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 30 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 379 | 2.088 ms | 201.371 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 490.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.060 us | 15.320 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 30 | 11.410 us | 111.251 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 24 | 16.310 us | 93.522 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 44 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 8.529 ms | 440.033 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 260.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.100 us | 32.980 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 170.000 ns | 20.180 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 180.000 ns | 43.791 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 354 | 2.421 ms | 267.901 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.070 us | 11.000 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 6 | 36.130 us | 32.030 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 668 | 11.255 ms | 501.835 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.010 us | 22.910 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 4 | 190.000 ns | 31.510 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 269 | 709.806 us | 68.851 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 580.000 ns | 9.490 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.388 ms | 126.871 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 3.158 ms | 107.461 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.130 us | 15.681 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.704 ms | 245.572 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 3.144 ms | 279.921 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 70.000 ns | 650.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.110 us | 16.350 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 50.000 ns | 12.361 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 60.000 ns | 9.530 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1170 | 13.634 ms | 557.296 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.140 us | 32.600 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 120.000 ns | 20.490 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 100.000 ns | 13.060 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 321 | 10.665 ms | 210.961 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.160 us | 6.251 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.260 us | 28.540 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 107 | 12.430 ms | 6.304 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 524 | 5.096 ms | 564.684 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 384 | 26.008 ms | 223.540 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.180 us | 7.170 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.290 us | 31.411 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 161 | 57.362 ms | 9.723 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 554 | 8.054 ms | 593.394 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.270 us | 6.780 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.250 us | 7.080 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.200 us | 19.620 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.290 us | 40.971 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 2.190 us | 5.000 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.220 us | 6.221 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.280 us | 17.250 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.280 us | 38.530 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.350 us | 12.621 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.340 us | 10.410 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.220 us | 26.761 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.690 us | 57.380 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 8.510 us | 18.400 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 5.030 us | 16.340 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 4.551 us | 57.980 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.050 us | 108.831 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 423 | 2.348 ms | 241.252 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 242 | 8.800 us | 35.920 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.290 us | 26.250 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 8 | 530.000 ns | 122.412 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 89 | 53.051 us | 100.410 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 802 | 13.110 ms | 472.274 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 376 | 35.460 us | 57.580 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.660 us | 56.500 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 8 | 3.100 us | 255.822 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 140 | 135.711 us | 147.023 us |

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

