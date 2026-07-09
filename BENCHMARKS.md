# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.23.0`.

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
| cached_reads | context | 4.943 ns | 4.583 ns - 5.357 ns |
| cached_reads | thread_safe_context | 68.820 ns | 67.892 ns - 69.776 ns |
| cold_first_get | context | 106.225 ns | 94.579 ns - 119.095 ns |
| cold_first_get | thread_safe_context | 1.177 us | 1.117 us - 1.240 us |
| dependency_fan_out | context / 32 | 4.510 us | 3.997 us - 5.212 us |
| dependency_fan_out | context / 256 | 54.502 us | 50.635 us - 60.448 us |
| dependency_fan_out | thread_safe_context / 32 | 25.980 us | 24.732 us - 27.160 us |
| dependency_fan_out | thread_safe_context / 256 | 186.500 us | 179.796 us - 193.403 us |
| set_cell_invalidation | high_fan_out / 512 | 135.934 us | 128.064 us - 145.044 us |
| set_cell_invalidation | same_slot_contention / 1 | 44.466 us | 43.414 us - 45.769 us |
| set_cell_invalidation | same_slot_contention / 2 | 85.311 us | 81.768 us - 89.015 us |
| set_cell_invalidation | same_slot_contention / 4 | 164.064 us | 158.373 us - 169.112 us |
| set_cell_invalidation | same_slot_contention / 8 | 462.807 us | 441.433 us - 485.145 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.391 ms | 1.336 ms - 1.443 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 42.812 us | 42.010 us - 43.551 us |
| set_cell_invalidation | independent_slot_contention / 2 | 65.650 us | 63.829 us - 67.558 us |
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
| batch_storms | context / 64 | 3.106 us | 2.901 us - 3.333 us |
| batch_storms | thread_safe_context / 64 | 8.049 us | 7.776 us - 8.338 us |
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 45.550 us | 13.360 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 1.630 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 1.230 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 350.000 ns | 1.130 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 440.000 ns | 5.300 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 350.000 ns | 1.510 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 410.000 ns | 3.560 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 710.000 ns | 2.320 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.350 us | 8.760 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.480 us | 9.060 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.951 us | 19.560 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.210 us | 52.891 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 135 | 95.941 us | 81.400 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 202 | 394.223 us | 117.550 us | 0 | 0 | 0 | 7 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 378 | 2.124 ms | 243.872 us | 0 | 0 | 0 | 7 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 6.693 ms | 413.383 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 920.000 ns | 29.390 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 32 | 0 | 1 | 0 | 0 | 0 | 92 | 23.340 us | 54.450 us | 21 | 21 | 11 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 53 | 0 | 1 | 0 | 0 | 0 | 217 | 100.790 us | 109.230 us | 35 | 35 | 29 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 87 | 0 | 1 | 0 | 0 | 0 | 390 | 145.953 us | 187.701 us | 77 | 77 | 51 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 190 | 0 | 1 | 0 | 0 | 0 | 828 | 899.147 us | 434.263 us | 152 | 152 | 104 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 910.000 ns | 14.570 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 89 | 21.270 us | 41.980 us | 17 | 17 | 14 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 214 | 472.792 us | 106.911 us | 20 | 20 | 43 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 465 | 6.203 ms | 492.244 us | 11 | 11 | 116 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 916 | 14.821 ms | 537.586 us | 12 | 12 | 243 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 840.000 ns | 17.410 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 910.000 ns | 15.820 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 45 | 2.160 us | 40.831 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 46 | 47.200 us | 39.620 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 58 | 7.810 us | 40.522 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.970 us | 67.420 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 21 | 0 | 8 | 0 | 0 | 0 | 188 | 70.522 us | 113.822 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 31 | 0 | 16 | 0 | 0 | 0 | 367 | 366.683 us | 230.114 us | 0 | 0 | 0 | 39 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 11 | 0 | 32 | 0 | 0 | 0 | 418 | 1.982 ms | 281.243 us | 0 | 0 | 0 | 10 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 2 | 0 | 64 | 0 | 0 | 0 | 713 | 6.020 ms | 407.082 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 6 | 1 | 396 | 1.390 ms | 228.323 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 735 | 6.070 ms | 423.023 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 27 | 1 | 395 | 1.737 ms | 165.372 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 37 | 1 | 704 | 7.817 ms | 297.361 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 5 | 1 | 646 | 3.093 ms | 288.334 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 5 | 1 | 1247 | 11.935 ms | 540.664 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 552 | 0 | 64 | 0 | 49 | 1 | 1149 | 28.560 ms | 6.639 ms | 3 | 96 | 125 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 552 | 0 | 64 | 0 | 50 | 1 | 1409 | 123.802 ms | 12.660 ms | 3 | 96 | 253 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.050 us | 77.771 us | 127 | 4064 | 0 | 4064 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 9.070 us | 75.910 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 13.180 us | 108.680 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 25.630 us | 195.634 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 71 | 0 | 65 | 0 | 5 | 1 | 681 | 1.817 ms | 416.102 us | 0 | 0 | 0 | 94 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1311 | 5.814 ms | 713.483 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 150.000 ns | 590.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 100.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 50.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 150.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 100.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 50.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 180.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 90.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 50.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 220.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 140.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.310 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 50.000 ns | 1.240 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 170.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 100.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 50.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 200.000 ns | 930.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 130.000 ns | 900.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 890.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 50.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 310.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 210.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 80.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 110.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 570.000 ns | 2.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 450.000 ns | 1.920 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 150.000 ns | 2.350 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 180.000 ns | 2.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 960.000 ns | 2.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 840.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 250.000 ns | 2.760 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 430.000 ns | 2.620 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.950 us | 4.630 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.701 us | 3.320 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 520.000 ns | 5.960 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 780.000 ns | 5.650 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.980 us | 16.280 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 140.000 ns | 820.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 190.000 ns | 2.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 860.000 ns | 32.891 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 40.000 ns | 830.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 110 | 93.511 us | 36.370 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 110.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 410.000 ns | 4.350 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 1.860 us | 40.120 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 50.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 176 | 392.043 us | 82.170 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 100.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 740.000 ns | 7.310 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 7 | 1.300 us | 27.530 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 336 | 2.122 ms | 175.992 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 120.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.480 us | 15.120 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 7 | 450.000 ns | 51.940 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 40.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 6.685 ms | 344.373 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 100.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 3.170 us | 33.420 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 4.850 us | 34.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 40.000 ns | 460.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 180.000 ns | 920.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 700.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 820.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 580.000 ns | 26.950 us |
| thread_safe_contention_same_slot_write_read_2 | other | 26 | 4.240 us | 1.140 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 6 | 370.000 ns | 1.880 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 11 | 2.980 us | 16.410 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 32 | 15.720 us | 34.670 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 60 | 17.740 us | 2.500 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 6 | 4.940 us | 2.080 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 720.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 29 | 43.940 us | 36.280 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 53 | 34.130 us | 67.650 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 68 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 105 | 26.991 us | 3.790 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 8 | 3.080 us | 3.810 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 780.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 51 | 77.470 us | 58.561 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 87 | 38.392 us | 120.760 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 138 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 206 | 301.253 us | 7.500 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 38 | 30.811 us | 11.491 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 480.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 104 | 460.432 us | 134.821 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 190 | 106.611 us | 279.971 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 289 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 200.000 ns | 640.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 110.000 ns | 250.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 40.000 ns | 610.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 560.000 ns | 13.070 us |
| thread_safe_contention_independent_slots_2 | other | 36 | 1.470 us | 1.390 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 220.000 ns | 440.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 70.000 ns | 730.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 14 | 8.710 us | 11.870 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 10.800 us | 27.550 us |
| thread_safe_contention_independent_slots_4 | other | 92 | 130.670 us | 4.300 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 410.000 ns | 920.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 170.000 ns | 1.960 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 43 | 197.631 us | 38.981 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 143.911 us | 60.750 us |
| thread_safe_contention_independent_slots_8 | other | 190 | 1.656 ms | 13.800 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 900.000 ns | 1.670 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 310.000 ns | 3.710 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 116 | 1.983 ms | 219.161 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 2.563 ms | 253.903 us |
| thread_safe_contention_independent_slots_16 | other | 354 | 2.690 ms | 15.280 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.780 us | 3.500 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 580.000 ns | 6.190 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 243 | 5.146 ms | 240.602 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.982 ms | 272.014 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 830.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 120.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 550.000 ns | 15.830 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 190.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 120.000 ns | 210.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 570.000 ns | 14.980 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 210.000 ns | 920.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 960.000 ns | 3.580 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 40.000 ns | 1.020 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 950.000 ns | 35.311 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 13 | 46.090 us | 3.570 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 970.000 ns | 35.220 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 140.000 ns | 510.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 12 | 6.940 us | 4.671 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 520.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 700.000 ns | 34.821 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 24 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 3.300 us | 14.740 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 270.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 190.000 ns | 1.830 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 810.000 ns | 30.490 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 570.000 ns | 20.090 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 60.992 us | 29.861 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 100.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 390.000 ns | 4.290 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.820 us | 48.141 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 21 | 7.220 us | 31.320 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 232 | 347.913 us | 63.760 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 120.000 ns | 410.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 830.000 ns | 8.750 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 39 | 5.650 us | 96.321 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 31 | 12.170 us | 60.873 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 47 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 344 | 1.960 ms | 174.002 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 350.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.530 us | 16.600 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 11 | 13.750 us | 52.140 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 11 | 6.250 us | 38.151 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 644 | 6.017 ms | 348.841 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 100.000 ns | 560.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 3.250 us | 35.051 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1 | 60.000 ns | 12.840 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 2 | 110.000 ns | 9.790 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 359 | 1.388 ms | 199.883 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.621 us | 11.810 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 5 | 310.000 ns | 16.630 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 667 | 6.063 ms | 373.163 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 3.270 us | 23.040 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 4 | 3.550 us | 26.820 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 252 | 391.657 us | 49.492 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 800.000 ns | 7.250 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.344 ms | 108.630 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 417 | 2.102 ms | 93.590 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.670 us | 13.360 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.713 ms | 190.411 us |
| thread_safe_effect_contention_batch_flush_8 | other | 602 | 3.090 ms | 240.594 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 110.000 ns | 610.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.590 us | 16.360 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 5 | 300.000 ns | 18.840 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 170.000 ns | 11.930 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1175 | 11.931 ms | 470.314 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 100.000 ns | 310.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 3.290 us | 33.310 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 180.000 ns | 16.120 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 140.000 ns | 20.610 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 344 | 7.122 ms | 173.421 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 3.240 us | 6.460 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.690 us | 29.950 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 125 | 16.380 ms | 5.873 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 552 | 5.052 ms | 556.194 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 476 | 27.335 ms | 215.924 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 3.230 us | 6.690 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.570 us | 31.000 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 253 | 85.182 ms | 11.724 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 552 | 11.279 ms | 681.876 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.000 us | 6.340 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 3.330 us | 6.840 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 970.000 ns | 18.410 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.750 us | 46.181 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 2.040 us | 5.930 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 3.330 us | 6.630 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.030 us | 18.840 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.670 us | 44.510 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.090 us | 10.940 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.640 us | 10.520 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.590 us | 29.190 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.860 us | 58.030 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.520 us | 15.521 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 6.990 us | 15.080 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 5.380 us | 58.121 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.740 us | 106.912 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 412 | 1.767 ms | 198.442 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 126 | 20.170 us | 18.140 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.770 us | 27.140 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 7 | 2.920 us | 105.220 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 71 | 24.410 us | 67.160 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 791 | 5.789 ms | 358.111 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 13.310 us | 32.160 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 5.700 us | 58.150 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 100.000 ns | 141.202 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 5.560 us | 123.860 us |

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
| cached read (Context) | 4.9 ns | 23 ns | — † |
| cached read (ThreadSafeContext) | 69 ns | 22 ns | — † |
| cold first get (Context) | 106 ns | 97 ns | — † |
| cold first get (ThreadSafeContext) | 1.18 µs | 107 ns | — † |
| fan-out 256 (Context) | 54.5 µs | 1.12 µs | — † |
| fan-out 256 (ThreadSafeContext) | 187 µs | 1.68 µs | — |
| set_cell high_fan_out 512 | 136 µs | 3.26 µs | — † |
| memo equality suppression (Context) | 3.0 µs | 34 ns | — † |
| effect flushing (Context) | 63 ns | 87 ns | — |
| batch storms 64 (Context) | 3.1 µs | 1.55 µs | — |

† lazily-zig 0.17-dev removed `std.time.Timer`, so its reactive-core
micro-bench is **counter-based** (deterministic work-counts: allocations,
edges, recomputes — not wall-clock). The counters confirm the same zero-work
steady state (cached reads = 0 allocs / 0 recomputes) but are not directly
comparable on a wall-clock axis. See
[lazily-zig BENCHMARKS.md](https://github.com/lazily-hub/lazily-zig/blob/main/BENCHMARKS.md).

### Scale — 1M rows (~2M cells)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build (2N nodes) | 105 ms | 123 ms | 132 ms |
| cold full recalc | 106 ms | 36 ms | 381 ms |
| viewport recalc (edit 1, read 1k) | 4.5 µs | 35.1 µs | 6.4 µs |

### Scale — 10M cells (full Google Sheets workbook capacity)

| Metric | lazily-rs | lazily-cpp | lazily-zig |
|---|---:|---:|---:|
| build | 706 ms | 1.41 s | 1.13 s |
| cold full recalc | 518 ms | 415 ms | 2.26 s |
| viewport recalc | 4.1 µs | 43.8 µs | 6.6 µs |

**Honest read:** lazily-rs's monomorphized `Rc<T>` fast path leads the
spreadsheet-scale **build** wall clock (leanest per-node storage) and — after the
v0.22.2 `#lzslotfastpath` refresh fast path — delivers the **cheapest viewport
reads** of the three (4.5 µs @ 1M, 4.1 µs @ 10M, undercutting lazily-zig's
integer-keyed cache at 6.4/6.6 µs). lazily-cpp's v0.6.0 `SmallAny` inline value
storage (optimization B) + alloc-free batch bookkeeping (E) **flipped the
cold-recalc lead**: lazily-cpp cold full recalc is now ~3× faster than lazily-rs
at both 1M (36 vs 106 ms) and 10M (415 vs 518 ms), and its `batch_storms` now
edges out lazily-rs (1.55 vs 3.1 µs). lazily-cpp's type-erased `SmallFn` +
`SmallVec` node layout still wins the high-fan-out micro-benchmarks (fan-out 256,
set_cell 512, memo equality) by 16–49× over lazily-rs. The **shared headline**
across all three: they back a full-capacity Google Sheets workbook and all
exhibit the **lazy-pull viewport property** — a one-cell edit + bounded-viewport
read stays in the **microsecond** range, independent of sheet size, because
off-viewport formulas are left dirty and never recomputed (~2,000–60,000× cheaper
than a full recalc across the three runtimes).

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
