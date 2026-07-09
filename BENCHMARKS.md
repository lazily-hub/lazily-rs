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
| scale | build | 191.790 ms | 178.970 ms - 205.836 ms |
| scale | cold_full_recalc | 144.059 ms | 134.264 ms - 155.676 ms |
| scale | full_recalc_invalidate_all | 86.869 ms | 79.419 ms - 94.074 ms |
| scale | viewport_recalc | 4.115 us | 3.933 us - 4.316 us |
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 13.610 us | 14.811 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 260.000 ns | 1.620 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 1.080 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 930.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 340.000 ns | 5.071 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 340.000 ns | 5.330 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 4.510 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 450.000 ns | 2.380 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 920.000 ns | 4.180 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 1.720 us | 7.490 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.640 us | 16.780 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.260 us | 49.400 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 129 | 57.620 us | 71.370 us | 0 | 0 | 0 | 12 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 193 | 430.735 us | 126.761 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 360 | 2.481 ms | 239.062 us | 0 | 0 | 0 | 1 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 721 | 7.365 ms | 441.953 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 700.000 ns | 16.360 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 29 | 0 | 1 | 0 | 0 | 0 | 75 | 37.421 us | 93.050 us | 26 | 26 | 6 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 47 | 0 | 1 | 0 | 0 | 0 | 195 | 122.440 us | 105.151 us | 40 | 40 | 24 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 98 | 0 | 1 | 0 | 0 | 0 | 429 | 197.781 us | 220.121 us | 76 | 76 | 52 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 189 | 0 | 1 | 0 | 0 | 0 | 820 | 897.116 us | 404.294 us | 149 | 149 | 107 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 630.000 ns | 13.701 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 86 | 82.260 us | 60.360 us | 18 | 18 | 13 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 217 | 520.056 us | 106.612 us | 17 | 17 | 46 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 462 | 3.060 ms | 231.153 us | 14 | 14 | 113 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 924 | 14.699 ms | 505.315 us | 12 | 12 | 243 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 760.000 ns | 13.660 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 750.000 ns | 13.110 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 30 | 870.000 ns | 18.611 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 57 | 25.150 us | 23.331 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 32 | 850.000 ns | 15.770 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.190 us | 61.280 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 193 | 62.730 us | 112.101 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 38 | 0 | 16 | 0 | 0 | 0 | 405 | 386.154 us | 234.682 us | 0 | 0 | 0 | 42 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 3 | 0 | 32 | 0 | 0 | 0 | 365 | 1.430 ms | 199.631 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 729 | 5.728 ms | 402.543 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 381 | 2.640 ms | 282.133 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 739 | 6.087 ms | 429.475 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 34 | 1 | 412 | 1.884 ms | 185.762 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 39 | 1 | 716 | 8.604 ms | 341.402 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 2.530 ms | 268.492 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 6 | 0 | 65 | 0 | 11 | 1 | 1271 | 24.259 ms | 680.126 us | 0 | 0 | 0 | 5 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 561 | 0 | 64 | 0 | 50 | 1 | 1162 | 24.600 ms | 6.268 ms | 3 | 96 | 125 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 554 | 0 | 64 | 0 | 49 | 1 | 1155 | 39.489 ms | 6.525 ms | 129 | 4128 | 127 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.860 us | 77.941 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 6.610 us | 65.570 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 9.100 us | 92.511 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 17.910 us | 178.812 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 605 | 1.673 ms | 350.735 us | 0 | 0 | 0 | 77 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 133 | 0 | 129 | 0 | 3 | 1 | 1184 | 7.272 ms | 647.045 us | 0 | 0 | 0 | 151 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 120.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 110.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 110.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 190.000 ns | 1.381 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 100.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 1.190 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 1.130 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 180.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 110.000 ns | 1.450 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 20.000 ns | 1.270 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 1.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 140.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 1.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 950.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 230.000 ns | 970.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 110.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 480.000 ns | 1.490 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 200.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 120.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 830.000 ns | 2.070 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 450.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 200.000 ns | 2.390 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 240.000 ns | 1.630 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.830 us | 4.780 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 940.000 ns | 3.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 420.000 ns | 4.990 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 450.000 ns | 3.840 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.590 us | 15.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.460 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 460.000 ns | 32.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 106 | 56.410 us | 34.010 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 4.000 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 12 | 940.000 ns | 32.950 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 20.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 170 | 429.945 us | 88.031 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 90.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 430.000 ns | 7.940 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 4 | 240.000 ns | 28.320 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 1.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 324 | 2.480 ms | 205.062 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 50.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 890.000 ns | 12.740 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 1 | 50.000 ns | 20.480 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 650 | 7.358 ms | 361.492 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.810 us | 28.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 4 | 4.940 us | 51.501 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 130.000 ns | 530.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 60.000 ns | 230.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 50.000 ns | 400.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 460.000 ns | 15.200 us |
| thread_safe_contention_same_slot_write_read_2 | other | 16 | 480.000 ns | 840.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 650.000 ns | 2.250 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 560.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 6 | 9.710 us | 13.410 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 29 | 26.561 us | 75.990 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 50 | 29.780 us | 2.630 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 14 | 22.840 us | 4.460 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 700.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 24 | 26.820 us | 34.990 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 47 | 42.970 us | 62.371 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 59 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 106 | 43.330 us | 4.050 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 22 | 20.780 us | 8.340 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 510.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 52 | 82.741 us | 72.610 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 98 | 50.900 us | 134.611 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 150 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 204 | 183.102 us | 7.120 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 38 | 19.670 us | 9.130 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 670.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 107 | 565.923 us | 130.571 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 189 | 128.381 us | 256.803 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 281 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 110.000 ns | 1.170 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 400.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 1.100 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 420.000 ns | 11.031 us |
| thread_safe_contention_independent_slots_2 | other | 34 | 23.810 us | 2.370 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 120.000 ns | 390.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 13 | 16.510 us | 17.830 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 41.760 us | 39.140 us |
| thread_safe_contention_independent_slots_4 | other | 92 | 140.391 us | 4.190 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 200.000 ns | 710.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 100.000 ns | 1.120 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 46 | 196.352 us | 45.631 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 183.013 us | 54.961 us |
| thread_safe_contention_independent_slots_8 | other | 190 | 744.437 us | 8.580 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 460.000 ns | 1.670 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 270.000 ns | 2.770 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 113 | 1.248 ms | 102.910 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.067 ms | 115.223 us |
| thread_safe_contention_independent_slots_16 | other | 362 | 3.622 ms | 14.810 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 860.000 ns | 2.920 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 430.000 ns | 4.770 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 243 | 4.558 ms | 230.162 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.519 ms | 252.653 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 690.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 70.000 ns | 510.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 660.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 520.000 ns | 11.800 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 120.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 530.000 ns | 12.260 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 130.000 ns | 700.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 180.000 ns | 1.450 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 690.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 530.000 ns | 15.771 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 130.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 14 | 18.990 us | 3.121 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 5.990 us | 19.590 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 320.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 4 | 200.000 ns | 1.100 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 370.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 510.000 ns | 13.980 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 6 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.170 us | 14.030 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 80.000 ns | 1.480 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 470.000 ns | 28.120 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 410.000 ns | 17.290 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 59.330 us | 30.380 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 3.660 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 2.030 us | 48.221 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 1.090 us | 29.660 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 238 | 320.152 us | 59.901 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 16 | 43.451 us | 5.270 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 420.000 ns | 6.250 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 43 | 20.221 us | 91.351 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 38 | 1.910 us | 71.910 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 54 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 326 | 1.428 ms | 167.291 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 220.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 12.630 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 2 | 80.000 ns | 10.360 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 3 | 90.000 ns | 9.130 us |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 5.725 ms | 329.753 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 420.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.660 us | 30.150 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 340.000 ns | 21.900 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 180.000 ns | 20.320 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 346 | 2.639 ms | 243.753 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 860.000 ns | 9.780 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 3 | 310.000 ns | 28.600 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 669 | 6.078 ms | 376.455 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.780 us | 21.450 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 6 | 7.171 us | 31.570 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 269 | 687.844 us | 63.641 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 490.000 ns | 7.850 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.196 ms | 114.271 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 429 | 3.308 ms | 110.681 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 910.000 ns | 11.960 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.295 ms | 218.761 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 2.529 ms | 225.951 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 50.000 ns | 760.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 960.000 ns | 14.420 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 70.000 ns | 14.820 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 70.000 ns | 12.541 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1193 | 24.257 ms | 576.195 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 830.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.840 us | 30.180 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 5 | 170.000 ns | 23.970 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 6 | 170.000 ns | 48.951 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 348 | 4.549 ms | 174.850 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.780 us | 6.480 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.810 us | 27.940 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 125 | 15.320 ms | 5.574 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 561 | 4.727 ms | 484.813 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 346 | 8.285 ms | 180.151 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.790 us | 6.130 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.800 us | 29.670 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 127 | 27.027 ms | 5.827 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 554 | 4.173 ms | 482.163 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 3.280 us | 13.410 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.860 us | 8.621 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 930.000 ns | 20.030 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.790 us | 35.880 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.970 us | 5.560 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.870 us | 6.090 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 950.000 ns | 17.190 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.820 us | 36.730 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.540 us | 11.690 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 1.920 us | 8.110 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.710 us | 25.430 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.930 us | 47.281 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.030 us | 16.011 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.650 us | 15.840 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.540 us | 50.361 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.690 us | 96.600 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 403 | 1.646 ms | 179.303 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 14.250 us | 8.970 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.830 us | 24.951 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 3 | 3.140 us | 89.021 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 8.340 us | 48.490 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 787 | 7.229 ms | 345.053 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 3.601 us | 14.321 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.650 us | 49.570 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 210.000 ns | 147.731 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 133 | 35.580 us | 90.370 us |

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
edits one input and reads only a 1,000-cell viewport — **~4.5 µs**, ~22,000× cheaper
than a full recalc because the lazy pull-based model leaves off-viewport formulas
dirty and never recomputes them (the property a viewport-rendered spreadsheet needs).
(`build`/`cold_full_recalc`/`full_recalc_invalidate_all` are unaffected by the
v0.22.2 `#lzslotfastpath` refresh fast path — they are cold/slow-path — so their
figures are retained from the original run; only `viewport_recalc`, which is
~998/1000 cache-hit reads, moved, by the controlled A/B below. The generated
`scale` rows in the table above reflect the latest single criterion run on this
host and drift with host load for the allocation-heavy `build`/`cold` cases; the
curated baseline here is the reference.)

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
| `viewport_recalc` (1k) | ~4.1 µs | ~4 ns |

So lazily backs a **full-capacity Google Sheets workbook**: build under a second, full
recompute ~0.5 s, and — crucially — viewport recalc stays ~4 µs **independent of sheet
size** (it was ~4.5 µs at 1M too), because the lazy pull-based model only recomputes the
cells you read. Reproduce: `LAZILY_SCALE_N=5000000 cargo bench --features scale-bench --bench scale`.

Controlled A/B isolating the v0.22.2 `#lzslotfastpath` refresh fast path on
`viewport_recalc` (`--save-baseline pre_fix`, same session, toggling only
`src/context.rs` between `8c64f33` and `1390a6e`): **13.78 µs → 4.49 µs,
−64.1% (p=0.00)** at `N = 1_000_000`. Only ~2 of the 1,000 viewport cells recompute; the
other ~998 are cache-hit slot reads, each now ~7 ns cheaper because `refresh_slot`
early-returns on a clean hit instead of cloning the dependency `Vec` and walking deps.

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
| `viewport_recalc` (edit 1, read 1k) | **~4.5 µs** † | 8.22 µs | lazily **~1.8×** faster |

† lazily's `viewport_recalc` is post-v0.22.2 (`#lzslotfastpath`). Before that refresh
fast path it measured **11.52 µs** and leptos led ~1.4× (the original row this table
shipped with). The v0.22.2 controlled A/B on this case is
**13.78 µs → 4.49 µs, −64.1% (p=0.00)** (`--save-baseline pre_fix`, toggling only
`src/context.rs`). leptos_reactive is an unchanged external library so its 8.22 µs is
retained from the original same-host run; a fresh same-session re-measure under load gave
~10.5 µs, i.e. lazily leads by ~1.8–2.3× depending on leptos's run-to-run variance.

**Honest read:** lazily now leads all four cases — building the sheet (1.5×), computing
it cold (3.6×), recomputing the whole sheet after a full invalidation (2.8×), and the
cached-read-dominated viewport read (~1.8×) — driven by its sparse arena + lean
single-threaded `Context` versus leptos's runtime slotmap and subscriber bookkeeping, plus
the v0.22.2 `refresh_slot` clean-cache-hit fast path that removed the per-read
dependency-walk tax on the ~998/1000 viewport cells that are cache hits. The fairness
evidence is no longer "leptos wins a case" (it did, before v0.22.2, and that historical
result is documented in the footnote above) — it is that leptos's genuine 30 ms cold
recalc proves its memos truly recompute (this is not a straw-man comparison), and that
lazily's viewport lead is a recent code improvement, not an inherent property: the
pre-v0.22.2 code lost this case. The shared headline is unchanged: the lazy-pull property
both exhibit — a one-input edit + bounded-viewport read is **microseconds**, ~1000×
cheaper than a full recalc, *independent of total sheet size* — neither library
recomputes off-viewport formulas. The defensible claim is now "lazily has materially
higher throughput than a comparable native-Rust pull-based reactive system across both
whole-graph and incremental-viewport workloads," **not** a blanket "fastest reactive
library."

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
| cached read (Context) | 4.8 ns | 19 ns | — † |
| cached read (ThreadSafeContext) | 67 ns | 22 ns | — † |
| cold first get (Context) | 93 ns | 88 ns | — † |
| cold first get (ThreadSafeContext) | 1.13 µs | 98 ns | — † |
| fan-out 256 (Context) | 72.5 µs | 1.05 µs | — † |
| fan-out 256 (ThreadSafeContext) | 219 µs | 1.68 µs | — |
| set_cell high_fan_out 512 | 145 µs | 3.08 µs | — † |
| memo equality suppression (Context) | 3.0 µs | 34 ns | — † |
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
| viewport recalc (edit 1, read 1k) | 4.5 µs | 47.7 µs | 6.4 µs |

### Scale — 10M cells (full Google Sheets workbook capacity)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build | 706 ms | 1.33 s | 1.13 s |
| cold full recalc | 518 ms | 1.12 s | 2.26 s |
| viewport recalc | 4.1 µs | 71.7 µs | 6.6 µs |

**Honest read:** lazily-rs's monomorphized `Rc<T>` fast path leads the
spreadsheet-scale wall clock (leanest per-node storage → fastest build/cold
recalc), ties lazily-cpp on effect flushing, and — after the v0.22.2
`#lzslotfastpath` refresh fast path — now also delivers the **cheapest viewport
reads** of the three (4.5 µs @ 1M, 4.1 µs @ 10M, undercutting lazily-zig's
integer-keyed cache at 6.4/6.6 µs; before v0.22.2 lazily-rs was 15.6/11.4 µs and
lazily-zig led). lazily-cpp's type-erased `SmallFn` + `SmallVec` node layout
still wins the high-fan-out micro-benchmarks (fan-out 256, set_cell 512, memo
equality) by 30–97× over lazily-rs. The **shared headline** across all three:
they back a full-capacity Google Sheets workbook and all exhibit the
**lazy-pull viewport property** — a one-cell edit + bounded-viewport read stays
in the **microsecond** range, independent of sheet size, because off-viewport
formulas are left dirty and never recomputed (~2,000–60,000× cheaper than a full
recalc across the three runtimes).

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
