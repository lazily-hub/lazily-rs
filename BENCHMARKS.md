# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.9.0`.

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
| typed cache fast-path cached reads | baseline vs typed-cache | `cargo bench --bench context -- cached_reads` | 8.22 ns → 7.92 ns context, 64.41 ns → 62.49 ns thread-safe (p < 0.05) | adopted; inline TypeId eliminates vtable indirection on cached reads |
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
| cached_reads | context | 7.921 ns | 7.817 ns - 8.045 ns |
| cached_reads | thread_safe_context | 62.492 ns | 62.240 ns - 62.695 ns |
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
| typed_cache_reads | context_slot | 7.921 ns | 7.817 ns - 8.045 ns |
| typed_cache_reads | context_cell | 3.086 ns | 2.929 ns - 3.248 ns |
| typed_cache_reads | thread_safe_slot | 62.492 ns | 62.240 ns - 62.695 ns |
| typed_cache_reads | thread_safe_cell | 25.006 ns | 24.931 ns - 25.116 ns |
| typed_cache_reads | context_rc_slot | 8.023 ns | 8.011 ns - 8.040 ns |
| typed_cache_reads | context_rc_cell | 3.086 ns | 2.929 ns - 3.248 ns |
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 26.780 us | 19.550 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 200.000 ns | 1.911 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 960.000 ns | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 900.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 1.010 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 1.230 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 260.000 ns | 2.220 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 500.000 ns | 5.650 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 880.000 ns | 3.710 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 1.760 us | 6.680 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.580 us | 13.871 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.190 us | 57.501 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 132 | 83.761 us | 91.581 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 195 | 431.673 us | 122.552 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 369 | 1.981 ms | 194.730 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 721 | 8.450 ms | 428.103 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 640.000 ns | 16.180 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 51 | 2.100 us | 32.270 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 54 | 0 | 1 | 0 | 0 | 0 | 129 | 2.040 us | 69.620 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 106 | 0 | 1 | 0 | 0 | 0 | 282 | 15.070 us | 193.502 us | 127 | 127 | 1 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 216 | 0 | 1 | 0 | 0 | 0 | 551 | 26.921 us | 307.224 us | 255 | 255 | 1 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 700.000 ns | 12.020 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 2.560 us | 23.980 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 132 | 406.492 us | 92.361 us | 50 | 50 | 13 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 247 | 2.230 ms | 201.082 us | 106 | 106 | 21 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 487 | 8.178 ms | 330.314 us | 214 | 214 | 41 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 710.000 ns | 13.570 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 670.000 ns | 13.630 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 39 | 6.490 us | 18.200 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 46 | 2.250 us | 18.950 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 50 | 2.000 us | 24.750 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.340 us | 61.280 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 24 | 0 | 8 | 0 | 0 | 0 | 199 | 65.511 us | 116.301 us | 0 | 0 | 0 | 24 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 31 | 0 | 16 | 0 | 0 | 0 | 355 | 346.724 us | 213.771 us | 0 | 0 | 0 | 35 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 5 | 0 | 32 | 0 | 0 | 0 | 376 | 2.042 ms | 217.932 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 11 | 0 | 64 | 0 | 0 | 0 | 758 | 5.916 ms | 447.993 us | 0 | 0 | 0 | 10 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 376 | 1.317 ms | 195.852 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 737 | 6.232 ms | 397.322 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 31 | 1 | 412 | 2.095 ms | 173.403 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 30 | 1 | 692 | 9.188 ms | 320.742 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 7 | 1 | 647 | 2.424 ms | 271.523 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 5 | 0 | 65 | 0 | 9 | 1 | 1263 | 10.690 ms | 518.804 us | 0 | 0 | 0 | 4 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 551 | 0 | 64 | 0 | 50 | 1 | 1110 | 22.045 ms | 5.533 ms | 24 | 768 | 104 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 554 | 0 | 64 | 0 | 50 | 1 | 1205 | 59.375 ms | 8.144 ms | 106 | 3392 | 150 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.310 us | 63.090 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.520 us | 54.251 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 9.190 us | 84.882 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 17.810 us | 172.642 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 126 | 0 | 65 | 0 | 19 | 1 | 1185 | 3.860 ms | 668.905 us | 0 | 0 | 0 | 208 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1315 | 10.465 ms | 731.838 us | 0 | 0 | 0 | 146 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 90.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 50.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 471.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 590.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 110.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 110.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 100.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 40.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 20.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 100.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 70.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 120.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 70.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 660.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 240.000 ns | 1.390 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 150.000 ns | 1.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 1.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 400.000 ns | 1.110 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 250.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 130.000 ns | 1.110 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 790.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 820.000 ns | 1.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 420.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 240.000 ns | 2.090 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 280.000 ns | 1.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.760 us | 3.800 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 870.000 ns | 2.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 450.000 ns | 4.271 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 500.000 ns | 3.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.420 us | 16.730 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 90.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 2.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 530.000 ns | 36.341 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 1.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 108 | 82.271 us | 34.900 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 13 | 1.200 us | 52.811 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 171 | 412.203 us | 87.771 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 460.000 ns | 6.531 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 18.930 us | 27.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 330 | 1.979 ms | 158.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 50.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 880.000 ns | 11.860 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 4 | 250.000 ns | 23.860 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 20.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 650 | 8.447 ms | 369.732 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.750 us | 25.911 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 4 | 880.000 ns | 31.790 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 370.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 100.000 ns | 600.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 60.000 ns | 580.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 20.000 ns | 640.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 460.000 ns | 14.360 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 110.000 ns | 260.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 6 | 820.000 ns | 1.940 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 280.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 1.150 us | 29.790 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 110.000 ns | 260.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 8 | 320.000 ns | 1.980 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 280.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 54 | 1.570 us | 67.100 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 62 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 2.720 us | 810.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 22 | 4.670 us | 4.260 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 680.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 40.000 ns | 2.890 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 106 | 7.610 us | 184.862 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 146 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 2.850 us | 760.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 44 | 7.800 us | 10.601 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 20.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 60.000 ns | 5.160 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 216 | 16.191 us | 290.353 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 283 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 110.000 ns | 450.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 40.000 ns | 510.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 490.000 ns | 10.720 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 210.000 ns | 550.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 110.000 ns | 370.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 540.000 ns |
| thread_safe_contention_independent_slots_2 | publish | 33 | 2.180 us | 22.520 us |
| thread_safe_contention_independent_slots_4 | other | 40 | 88.111 us | 2.970 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 230.000 ns | 790.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.530 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 13 | 26.691 us | 21.410 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 291.350 us | 65.661 us |
| thread_safe_contention_independent_slots_8 | other | 67 | 321.953 us | 5.180 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 480.000 ns | 2.500 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 200.000 ns | 2.930 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 21 | 310.182 us | 42.080 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.597 ms | 148.392 us |
| thread_safe_contention_independent_slots_16 | other | 127 | 670.084 us | 7.000 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 830.000 ns | 2.680 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 490.000 ns | 4.210 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 41 | 662.125 us | 52.980 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.844 ms | 263.444 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 700.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 60.000 ns | 510.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 720.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 500.000 ns | 11.640 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 120.000 ns | 250.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 70.000 ns | 180.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 260.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 460.000 ns | 12.940 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 130.000 ns | 510.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 5.780 us | 2.120 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 500.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 550.000 ns | 15.070 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 140.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 10 | 1.540 us | 1.870 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 540.000 ns | 16.450 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 100.000 ns | 270.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 10 | 1.330 us | 1.760 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 40.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 530.000 ns | 22.410 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.120 us | 13.970 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 310.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 2.230 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 470.000 ns | 27.810 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 540.000 ns | 16.960 us |
| thread_safe_contention_batched_write_bursts_2 | other | 130 | 63.121 us | 29.900 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 90.000 ns | 420.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 4.260 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 24 | 1.390 us | 49.901 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 24 | 710.000 ns | 31.820 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 226 | 316.484 us | 63.250 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 6 | 18.800 us | 1.350 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 400.000 ns | 5.860 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 35 | 3.490 us | 79.040 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 31 | 7.550 us | 64.271 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 41 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 330 | 2.041 ms | 173.382 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 410.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 780.000 ns | 13.300 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 4 | 180.000 ns | 19.280 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 5 | 120.000 ns | 11.560 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 661 | 5.913 ms | 325.001 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 430.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.670 us | 27.171 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 10 | 420.000 ns | 37.290 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 11 | 340.000 ns | 58.101 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 341 | 1.316 ms | 171.712 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 850.000 ns | 10.060 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 3 | 110.000 ns | 14.080 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 668 | 6.224 ms | 351.962 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.590 us | 17.740 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 5 | 6.870 us | 27.620 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 269 | 835.658 us | 58.841 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 450.000 ns | 6.710 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.259 ms | 107.852 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 3.428 ms | 89.191 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 810.000 ns | 10.900 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.760 ms | 220.651 us |
| thread_safe_effect_contention_batch_flush_8 | other | 605 | 2.423 ms | 224.441 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 810.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 870.000 ns | 13.311 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 110.000 ns | 14.170 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 150.000 ns | 18.791 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1187 | 10.688 ms | 433.373 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 730.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.720 us | 26.970 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 4 | 160.000 ns | 18.880 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 5 | 140.000 ns | 38.851 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 327 | 5.543 ms | 172.990 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.810 us | 5.410 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.710 us | 22.680 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 104 | 12.592 ms | 4.849 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 551 | 3.906 ms | 482.032 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 373 | 13.432 ms | 183.220 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.740 us | 5.920 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.730 us | 24.430 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 150 | 39.384 ms | 7.438 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 554 | 6.555 ms | 491.766 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.720 us | 7.490 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.810 us | 6.590 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.060 us | 16.210 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.720 us | 32.800 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.760 us | 4.561 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.680 us | 5.250 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.140 us | 14.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.940 us | 29.930 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.540 us | 9.720 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 1.870 us | 7.820 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.830 us | 21.551 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.950 us | 45.791 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 6.970 us | 15.520 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.660 us | 15.102 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.470 us | 48.420 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.710 us | 93.600 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 451 | 3.495 ms | 221.083 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 532 | 21.720 us | 65.580 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.850 us | 23.390 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 11 | 6.050 us | 204.461 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 126 | 335.601 us | 154.391 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 793 | 10.425 ms | 399.035 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 24.800 us | 27.710 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.480 us | 45.410 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 4 | 150.000 ns | 154.021 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 12.170 us | 105.662 us |

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

