# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.22.2`.

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
| invalidation-frontier fast-path Arc cache (#lzfrontierarc) | 15d4206 vs this change (controlled --save-baseline before_opt A/B, same session) | `cargo bench --features instrumentation,thread-safe --bench context -- --baseline before_opt` | fan_out_lazy_dirty_epochs/16 -46.8% (p=0.00), fan_in_lazy_dirty_epochs/16 -22.6% (p=0.00), independent_slot_contention/16 -17.3% (p=0.00), independent_slots/16 -5.3% (p=0.37 n.s.) | adopted; the cached Arc reuses the BFS-time fast path in the marking pass, halving uninstrumented slot_fast_paths RwLock read acquisitions whose reader-count atomics dominate under 16-way contention. Deterministic state-mutex acquisition counts (the budget metric) are unchanged because slot_fast_paths is a separate uninstrumented lock; the evidence is the controlled wall-clock A/B. Microbench cases (cached_reads) correctly show no change as they do not touch the invalidation frontier. |
| Context slot clean-cache-hit fast path (#lzslotfastpath) | 8c64f33 vs this change (controlled --save-baseline before_slot A/B, same session) | `cargo bench --features instrumentation,thread-safe --bench context -- --baseline before_slot 'cached_reads|typed_cache_reads'` | typed_cache_reads/context_slot -58.9% (p=0.00), cached_reads/context -51.6% (p=0.00), typed_cache_reads/context_cell -2.1% (p=0.76 n.s.) | adopted; refresh_slot now early-returns when the slot holds a value and is neither dirty nor force-recompute, skipping the cycle-guard borrowMut + guard-drop borrowMut + dependencies Vec clone + per-dep is_slot_node borrows + clear_slot_dirty_flags borrowMut on the cache-hit path. Correctness rests on mark_slot_dirty always being called with force_recompute=true from invalidate_dependent_from_changed_value, so any upstream change sets dirty=true and bypasses the fast path. context_slot 11.8 -> 4.7 ns, now within ~1.5 ns of context_cell (3.0 ns); the previous downcast 'tax' framing was wrong (the cell also downcasts) - the real cost was refresh_slot's redundant work on clean reads. |

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 1.993 ms | 2.100 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 5.339 ms | 5.514 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.825 ms | 2.103 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.279 ms | 5.078 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 565.725 us | 602.352 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.175 ms | 1.287 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 3.033 ms | 3.190 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.409 ms | 3.723 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.253 ms | 1.378 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.588 ms | 3.016 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.363 ms | 1.408 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.779 ms | 3.049 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.044 ms | 2.274 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.976 ms | 5.690 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.940 ms | 4.137 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.353 ms | 6.850 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.100 ms | 1.141 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 2.158 ms | 2.313 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 397.125 us | 431.722 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 764.915 us | 797.921 us | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.077 ms | 1.151 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.850 ms | 2.048 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 4.801 ns | 4.509 ns - 5.121 ns |
| cached_reads | thread_safe_context | 67.899 ns | 66.686 ns - 69.264 ns |
| cold_first_get | context | 117.082 ns | 101.622 ns - 138.418 ns |
| cold_first_get | thread_safe_context | 1.335 us | 1.211 us - 1.462 us |
| dependency_fan_out | context / 32 | 4.525 us | 4.080 us - 5.041 us |
| dependency_fan_out | context / 256 | 60.166 us | 55.069 us - 67.497 us |
| dependency_fan_out | thread_safe_context / 32 | 26.337 us | 24.880 us - 27.810 us |
| dependency_fan_out | thread_safe_context / 256 | 179.694 us | 175.207 us - 184.221 us |
| set_cell_invalidation | high_fan_out / 512 | 128.493 us | 122.807 us - 134.101 us |
| set_cell_invalidation | same_slot_contention / 1 | 40.474 us | 39.859 us - 41.088 us |
| set_cell_invalidation | same_slot_contention / 2 | 75.168 us | 72.196 us - 78.949 us |
| set_cell_invalidation | same_slot_contention / 4 | 166.449 us | 162.034 us - 170.963 us |
| set_cell_invalidation | same_slot_contention / 8 | 466.976 us | 455.061 us - 478.612 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.354 ms | 1.292 ms - 1.415 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 39.918 us | 39.423 us - 40.493 us |
| set_cell_invalidation | independent_slot_contention / 2 | 62.698 us | 61.003 us - 64.396 us |
| set_cell_invalidation | independent_slot_contention / 4 | 112.336 us | 107.997 us - 116.207 us |
| set_cell_invalidation | independent_slot_contention / 8 | 210.314 us | 207.878 us - 212.906 us |
| set_cell_invalidation | independent_slot_contention / 16 | 396.222 us | 388.895 us - 403.501 us |
| set_cell_invalidation | batched_write_bursts / 1 | 131.578 us | 129.491 us - 133.169 us |
| set_cell_invalidation | batched_write_bursts / 2 | 204.786 us | 199.106 us - 210.249 us |
| set_cell_invalidation | batched_write_bursts / 4 | 447.628 us | 426.696 us - 469.189 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.169 ms | 1.115 ms - 1.225 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.558 ms | 2.331 ms - 2.798 ms |
| memo_equality_suppression | context | 3.039 us | 2.672 us - 3.469 us |
| memo_equality_suppression | thread_safe_context | 37.894 us | 36.021 us - 39.888 us |
| effect_flushing | context | 62.681 ns | 59.450 ns - 66.265 ns |
| effect_flushing | thread_safe_context | 974.535 ns | 961.746 ns - 987.522 ns |
| batch_storms | context / 64 | 3.507 us | 3.305 us - 3.726 us |
| batch_storms | thread_safe_context / 64 | 8.169 us | 7.904 us - 8.450 us |
| thread_safe_contention | same_slot_write_read / 1 | 104.059 us | 103.033 us - 105.042 us |
| thread_safe_contention | same_slot_write_read / 2 | 301.169 us | 292.886 us - 310.836 us |
| thread_safe_contention | same_slot_write_read / 4 | 721.638 us | 710.025 us - 734.685 us |
| thread_safe_contention | same_slot_write_read / 8 | 1.977 ms | 1.913 ms - 2.034 ms |
| thread_safe_contention | same_slot_write_read / 16 | 5.269 ms | 5.113 ms - 5.401 ms |
| thread_safe_contention | independent_slots / 1 | 103.898 us | 102.384 us - 105.387 us |
| thread_safe_contention | independent_slots / 2 | 210.909 us | 208.211 us - 213.641 us |
| thread_safe_contention | independent_slots / 4 | 580.966 us | 559.173 us - 601.389 us |
| thread_safe_contention | independent_slots / 8 | 1.866 ms | 1.753 ms - 1.968 ms |
| thread_safe_contention | independent_slots / 16 | 4.332 ms | 4.179 ms - 4.529 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 103.341 us | 102.179 us - 104.469 us |
| thread_safe_contention | read_mostly_waiters / 2 | 144.906 us | 143.806 us - 146.030 us |
| thread_safe_contention | read_mostly_waiters / 4 | 248.075 us | 245.170 us - 250.853 us |
| thread_safe_contention | read_mostly_waiters / 8 | 568.504 us | 557.472 us - 579.717 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.198 ms | 1.165 ms - 1.234 ms |
| thread_safe_contention | batched_write_bursts / 1 | 214.626 us | 212.831 us - 216.612 us |
| thread_safe_contention | batched_write_bursts / 2 | 577.939 us | 557.893 us - 596.742 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.668 ms | 1.663 ms - 1.674 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.059 ms | 3.022 ms - 3.101 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.323 ms | 3.130 ms - 3.499 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.254 ms | 1.203 ms - 1.300 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.668 ms | 2.513 ms - 2.818 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.359 ms | 1.338 ms - 1.380 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.778 ms | 2.669 ms - 2.874 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.992 ms | 1.850 ms - 2.118 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.823 ms | 4.438 ms - 5.187 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.952 ms | 3.916 ms - 4.000 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.430 ms | 6.327 ms - 6.556 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.098 ms | 1.078 ms - 1.115 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 2.161 ms | 2.110 ms - 2.216 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 400.077 us | 392.368 us - 409.050 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 763.211 us | 749.944 us - 776.486 us |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.081 ms | 1.053 ms - 1.106 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.823 ms | 1.732 ms - 1.902 ms |
| profile_instrumentation | context_snapshot | 377.565 ns | 338.970 ns - 417.697 ns |
| profile_instrumentation | thread_safe_snapshot | 293.673 us | 291.604 us - 295.885 us |
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
| typed_cache_reads | context_cell | 3.116 ns | 2.856 ns - 3.408 ns |
| typed_cache_reads | context_rc_cell | 3.241 ns | 3.022 ns - 3.477 ns |
| typed_cache_reads | context_rc_slot | 4.543 ns | 4.278 ns - 4.839 ns |
| typed_cache_reads | context_slot | 4.724 ns | 4.352 ns - 5.145 ns |
| typed_cache_reads | thread_safe_cell | 26.756 ns | 26.487 ns - 27.011 ns |
| typed_cache_reads | thread_safe_slot | 65.850 ns | 65.338 ns - 66.372 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.080 us | 20.211 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 1.880 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 320.000 ns | 1.200 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.010 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 3.490 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 1.840 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 2.390 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 690.000 ns | 6.040 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.240 us | 4.340 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.500 us | 10.450 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.800 us | 23.861 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.230 us | 61.430 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 129 | 61.420 us | 75.930 us | 0 | 0 | 0 | 12 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 205 | 482.232 us | 133.471 us | 0 | 0 | 0 | 8 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 368 | 1.563 ms | 247.172 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 733 | 10.744 ms | 492.772 us | 0 | 0 | 0 | 8 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 950.000 ns | 18.870 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 30 | 0 | 1 | 0 | 0 | 0 | 80 | 21.400 us | 51.050 us | 23 | 23 | 9 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 44 | 0 | 1 | 0 | 0 | 0 | 187 | 129.664 us | 106.791 us | 42 | 42 | 22 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 106 | 0 | 1 | 0 | 0 | 0 | 447 | 198.123 us | 218.391 us | 75 | 75 | 53 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 203 | 0 | 1 | 0 | 0 | 0 | 827 | 844.042 us | 413.443 us | 152 | 152 | 104 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 830.000 ns | 12.650 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 92 | 20.700 us | 38.590 us | 16 | 16 | 15 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 209 | 426.843 us | 97.480 us | 19 | 19 | 44 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 451 | 4.353 ms | 322.331 us | 17 | 17 | 110 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 927 | 21.487 ms | 674.375 us | 8 | 8 | 247 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 750.000 ns | 13.760 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 790.000 ns | 14.220 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 48 | 1.490 us | 23.430 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 47 | 4.440 us | 84.930 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 57 | 198.080 us | 60.180 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.810 us | 61.391 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 25 | 0 | 8 | 0 | 0 | 0 | 217 | 137.251 us | 154.822 us | 0 | 0 | 0 | 29 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 51 | 0 | 16 | 0 | 0 | 0 | 447 | 260.950 us | 333.164 us | 0 | 0 | 0 | 50 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 21 | 0 | 32 | 0 | 0 | 0 | 480 | 1.606 ms | 356.063 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 726 | 7.066 ms | 449.704 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 7 | 1 | 406 | 1.498 ms | 236.342 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 744 | 7.784 ms | 452.222 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 26 | 1 | 396 | 2.966 ms | 263.411 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 35 | 1 | 704 | 8.521 ms | 398.006 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 3 | 1 | 636 | 2.952 ms | 285.473 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 3 | 1 | 1242 | 11.588 ms | 527.014 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 553 | 0 | 64 | 0 | 49 | 1 | 1150 | 32.418 ms | 6.743 ms | 3 | 96 | 125 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 563 | 0 | 64 | 0 | 50 | 1 | 1420 | 124.082 ms | 12.223 ms | 3 | 96 | 253 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.731 us | 70.911 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.510 us | 68.851 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 14.190 us | 98.451 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 28.420 us | 187.902 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 80 | 0 | 65 | 0 | 7 | 1 | 769 | 2.912 ms | 488.375 us | 0 | 0 | 0 | 143 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 138 | 0 | 129 | 0 | 5 | 1 | 1318 | 10.292 ms | 802.296 us | 0 | 0 | 0 | 172 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 130.000 ns | 860.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 100.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 40.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 130.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 100.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 120.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 100.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 40.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 160.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 100.000 ns | 750.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 40.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 130.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 80.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 50.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 160.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 300.000 ns | 1.590 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 220.000 ns | 1.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 90.000 ns | 1.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 80.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 520.000 ns | 1.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 400.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 170.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 150.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 920.000 ns | 3.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 860.000 ns | 2.040 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 390.000 ns | 3.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 330.000 ns | 2.180 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.920 us | 8.731 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.640 us | 4.410 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 630.000 ns | 6.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 610.000 ns | 4.320 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.440 us | 17.460 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 150.000 ns | 1.860 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 2.790 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 470.000 ns | 38.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 40.000 ns | 1.280 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 106 | 59.870 us | 37.480 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 100.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 320.000 ns | 4.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 12 | 1.090 us | 33.850 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 40.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 178 | 481.082 us | 95.270 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 120.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 710.000 ns | 7.980 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 8 | 300.000 ns | 27.761 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 1.130 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 329 | 1.532 ms | 203.631 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 140.000 ns | 950.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.350 us | 13.980 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 4 | 29.031 us | 27.511 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 50.000 ns | 1.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 658 | 10.740 ms | 419.502 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 110.000 ns | 910.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.700 us | 29.390 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 8 | 1.580 us | 42.200 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 770.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 180.000 ns | 1.130 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 1.010 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 960.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 600.000 ns | 15.770 us |
| thread_safe_contention_same_slot_write_read_2 | other | 22 | 2.130 us | 1.050 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 790.000 ns | 1.520 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 9 | 1.520 us | 14.790 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 30 | 16.920 us | 33.360 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 47 | 55.392 us | 3.420 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 10 | 2.910 us | 5.080 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 50.000 ns | 1.170 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 22 | 31.171 us | 39.650 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 44 | 40.141 us | 57.471 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 63 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 109 | 42.990 us | 3.750 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 22 | 8.631 us | 8.080 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 53 | 87.241 us | 68.370 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 106 | 59.221 us | 137.861 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 156 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 199 | 182.660 us | 7.340 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 30 | 4.870 us | 8.650 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 700.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 104 | 493.602 us | 134.921 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 203 | 162.870 us | 261.832 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 290 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 100.000 ns | 590.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 110.000 ns | 380.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 50.000 ns | 420.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 570.000 ns | 11.260 us |
| thread_safe_contention_independent_slots_2 | other | 38 | 1.250 us | 1.680 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 180.000 ns | 370.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 100.000 ns | 620.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 15 | 11.250 us | 12.420 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 7.920 us | 23.500 us |
| thread_safe_contention_independent_slots_4 | other | 86 | 104.400 us | 3.920 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 430.000 ns | 680.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 160.000 ns | 1.140 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 44 | 191.842 us | 38.700 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 130.011 us | 53.040 us |
| thread_safe_contention_independent_slots_8 | other | 182 | 1.251 ms | 9.500 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 800.000 ns | 1.350 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 310.000 ns | 2.430 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 110 | 1.359 ms | 123.351 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.741 ms | 185.700 us |
| thread_safe_contention_independent_slots_16 | other | 361 | 6.383 ms | 19.591 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.620 us | 2.810 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 610.000 ns | 5.100 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 247 | 6.185 ms | 306.142 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 8.917 ms | 340.732 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 80.000 ns | 700.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 90.000 ns | 560.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 560.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 540.000 ns | 11.940 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 100.000 ns | 320.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 90.000 ns | 180.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 560.000 ns | 13.410 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 110.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 510.000 ns | 2.660 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 50.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 820.000 ns | 20.090 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 120.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 3.090 us | 3.080 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 1.190 us | 81.220 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 17 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 140.000 ns | 1.230 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 13 | 174.150 us | 8.430 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 40.000 ns | 1.600 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 23.750 us | 48.920 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.660 us | 14.781 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 90.000 ns | 560.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 150.000 ns | 2.240 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 470.000 ns | 27.260 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 440.000 ns | 16.550 us |
| thread_safe_contention_batched_write_bursts_2 | other | 138 | 113.971 us | 36.791 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 90.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 350.000 ns | 3.730 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 29 | 7.600 us | 65.050 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 25 | 15.240 us | 49.021 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 262 | 214.100 us | 71.352 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 15.040 us | 1.820 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 740.000 ns | 6.540 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 50 | 2.580 us | 130.401 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 51 | 28.490 us | 123.051 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 64 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 363 | 1.595 ms | 175.150 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 110.000 ns | 380.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.400 us | 13.360 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 21 | 1.720 us | 96.083 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 21 | 8.550 us | 71.090 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 41 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 7.062 ms | 361.073 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 120.000 ns | 880.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.880 us | 29.690 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 130.000 ns | 27.611 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 250.000 ns | 30.450 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 363 | 1.494 ms | 185.711 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.340 us | 10.000 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 11 | 2.900 us | 40.631 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 674 | 7.781 ms | 388.632 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.680 us | 22.560 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 6 | 310.000 ns | 41.030 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 253 | 1.661 ms | 77.500 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 670.000 ns | 9.800 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.305 ms | 176.111 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 417 | 2.790 ms | 115.642 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 950.000 ns | 19.320 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.731 ms | 263.044 us |
| thread_safe_effect_contention_batch_flush_8 | other | 595 | 2.950 ms | 240.042 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 100.000 ns | 930.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.320 us | 15.310 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 110.000 ns | 21.551 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 130.000 ns | 7.640 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1170 | 11.583 ms | 461.584 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 90.000 ns | 720.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.740 us | 32.870 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 2.560 us | 22.010 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 120.000 ns | 9.830 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 344 | 8.319 ms | 172.802 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.670 us | 7.500 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.410 us | 34.390 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 125 | 18.811 ms | 6.042 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 553 | 5.283 ms | 485.657 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 476 | 23.266 ms | 184.702 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.980 us | 6.700 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.610 us | 29.630 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 253 | 89.516 ms | 11.520 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 563 | 11.295 ms | 482.033 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.870 us | 10.831 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 3.420 us | 6.750 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.361 us | 17.120 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.080 us | 36.210 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.900 us | 7.400 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 3.200 us | 6.310 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.270 us | 17.371 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.140 us | 37.770 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 5.050 us | 11.160 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.850 us | 9.420 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.810 us | 28.551 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.480 us | 49.320 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 10.550 us | 16.650 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 7.310 us | 16.300 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 5.520 us | 57.310 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.040 us | 97.642 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 425 | 2.785 ms | 215.633 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 184 | 40.720 us | 23.951 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.820 us | 27.950 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 15 | 11.360 us | 144.841 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 80 | 72.580 us | 76.000 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 793 | 10.209 ms | 403.344 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 16.090 us | 30.510 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 5.340 us | 56.570 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 4 | 230.000 ns | 205.842 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 138 | 61.381 us | 106.030 us |

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

## Cross-language comparison (lazily-rs / lazily-cpp / lazily-zig)

Head-to-head on the same spreadsheet-shaped workload (`N` input cells + `N`
formula slots, `formula[i] = input[i] + input[i-1]`), measured on `x86_64`
Linux. lazily-rs uses criterion; lazily-cpp uses its `std::chrono` harness;
lazily-zig uses `clock_gettime(.MONOTONIC)` for the scale bench. Numbers are
the current published results from each repo's `BENCHMARKS.md`.

### Micro-benchmarks (single-threaded `Context` unless noted)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| cached read (Context) | 10.5 ns | 19 ns | — † |
| cached read (ThreadSafeContext) | 67 ns | 22 ns | — † |
| cold first get (Context) | 93 ns | 88 ns | — † |
| cold first get (ThreadSafeContext) | 1.13 µs | 98 ns | — † |
| fan-out 256 (Context) | 72.5 µs | 1.05 µs | — † |
| fan-out 256 (ThreadSafeContext) | 219 µs | 1.68 µs | — |
| set_cell high_fan_out 512 | 145 µs | 3.08 µs | — † |
| memo equality suppression (Context) | 3.29 µs | 34 ns | — † |
| effect flushing (Context) | 99 ns | 127 ns | — |
| batch storms 64 (Context) | 3.85 µs | 4.45 µs | — |

† lazily-zig 0.17-dev removed `std.time.Timer`, so its reactive-core
micro-bench is **counter-based** (deterministic work-counts: allocations,
edges, recomputes — not wall-clock). The counters confirm the same zero-work
steady state (cached reads = 0 allocs / 0 recomputes) but are not directly
comparable on a wall-clock axis. See
[lazily-zig BENCHMARKS.md](https://github.com/lazily-hub/lazily-zig/blob/main/BENCHMARKS.md).

### Scale — 1M rows (~2M cells)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build (2N nodes) | 105 ms | 143 ms | 132 ms |
| cold full recalc | 106 ms | 102 ms | 381 ms |
| viewport recalc (edit 1, read 1k) | 15.6 µs | 47.7 µs | 6.4 µs |

### Scale — 10M cells (full Google Sheets workbook capacity)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build | 706 ms | 1.33 s | 1.13 s |
| cold full recalc | 518 ms | 1.12 s | 2.26 s |
| viewport recalc | 11.4 µs | 71.7 µs | 6.6 µs |

**Honest read:** lazily-rs's monomorphized `Rc<T>` fast path leads the
spreadsheet-scale wall clock (leanest per-node storage → fastest build/cold
recalc) and ties lazily-cpp on effect flushing. lazily-cpp's type-erased
`SmallFn` + `SmallVec` node layout wins the high-fan-out micro-benchmarks
(fan-out 256, set_cell 512, memo equality) by 30–97× over lazily-rs, while
lazily-zig's integer-keyed cache delivers the cheapest viewport reads. The
**shared headline** across all three: they back a full-capacity Google Sheets
workbook and all exhibit the **lazy-pull viewport property** — a one-cell
edit + bounded-viewport read stays in the **microsecond** range, independent
of sheet size, because off-viewport formulas are left dirty and never
recomputed (~5,000–650,000× cheaper than a full recalc).

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
