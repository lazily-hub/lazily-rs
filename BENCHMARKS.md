# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.35.0`.

Environment: `rustc 1.97.0 (2d8144b78 2026-07-07)` on `x86_64-unknown-linux-gnu`.

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
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 700 | set_cell_invalidation<=260, dependency_edge<=16, get_refresh<=32, publish<=32 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 900 | other<=800, set_cell_invalidation<=16, dependency_edge<=64, get_refresh<=2, publish<=2 |
| thread_safe_contention_same_slot_write_read_16 | 1400 | get_refresh<=160, publish<=256, in_flight_wait<=700, set_cell_invalidation<=260 |
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
| thread_safe_contention | same_slot_write_read / 8 | 2.484 ms | 2.749 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 5.915 ms | 7.871 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.971 ms | 2.778 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.231 ms | 5.299 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 561.414 us | 577.967 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.169 ms | 1.334 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 3.440 ms | 3.549 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.710 ms | 4.083 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.310 ms | 1.380 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.813 ms | 3.590 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.327 ms | 1.531 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.792 ms | 5.145 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 4.378 ms | 5.183 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 5.060 ms | 5.960 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.992 ms | 4.371 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.541 ms | 8.903 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.527 ms | 2.599 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.072 ms | 5.261 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.354 ms | 3.100 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 5.198 ms | 6.498 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.092 ms | 1.247 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.026 ms | 2.375 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 4.566 ns | 4.257 ns - 4.903 ns |
| cached_reads | thread_safe_context | 64.233 ns | 63.673 ns - 64.858 ns |
| cold_first_get | context | 100.097 ns | 89.554 ns - 111.805 ns |
| cold_first_get | thread_safe_context | 996.518 ns | 960.889 ns - 1.044 us |
| dependency_fan_out | context / 32 | 3.418 us | 3.098 us - 3.754 us |
| dependency_fan_out | context / 256 | 50.930 us | 47.296 us - 56.244 us |
| dependency_fan_out | thread_safe_context / 32 | 22.402 us | 21.717 us - 23.075 us |
| dependency_fan_out | thread_safe_context / 256 | 178.860 us | 170.208 us - 188.976 us |
| set_cell_invalidation | high_fan_out / 512 | 109.796 us | 104.773 us - 114.393 us |
| set_cell_invalidation | same_slot_contention / 1 | 79.218 us | 77.135 us - 81.647 us |
| set_cell_invalidation | same_slot_contention / 2 | 191.177 us | 172.983 us - 217.154 us |
| set_cell_invalidation | same_slot_contention / 4 | 490.677 us | 468.688 us - 512.291 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.453 ms | 1.356 ms - 1.548 ms |
| set_cell_invalidation | same_slot_contention / 16 | 2.756 ms | 2.600 ms - 2.923 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 78.423 us | 77.640 us - 79.271 us |
| set_cell_invalidation | independent_slot_contention / 2 | 178.014 us | 170.861 us - 185.696 us |
| set_cell_invalidation | independent_slot_contention / 4 | 475.937 us | 459.191 us - 493.692 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.436 ms | 1.375 ms - 1.504 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 2.812 ms | 2.608 ms - 2.989 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 363.131 us | 223.640 us - 540.391 us |
| set_cell_invalidation | batched_write_bursts / 2 | 189.115 us | 175.692 us - 202.476 us |
| set_cell_invalidation | batched_write_bursts / 4 | 394.597 us | 373.033 us - 414.584 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.091 ms | 988.922 us - 1.221 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.431 ms | 2.193 ms - 2.708 ms |
| memo_equality_suppression | context | 2.853 us | 2.470 us - 3.257 us |
| memo_equality_suppression | thread_safe_context | 35.905 us | 34.541 us - 37.384 us |
| effect_flushing | context | 90.958 ns | 85.186 ns - 97.109 ns |
| effect_flushing | thread_safe_context | 978.304 ns | 954.777 ns - 1.003 us |
| batch_storms | context / 64 | 3.381 us | 3.206 us - 3.553 us |
| batch_storms | thread_safe_context / 64 | 8.766 us | 8.576 us - 8.985 us |
| thread_safe_contention | same_slot_write_read / 1 | 138.172 us | 136.761 us - 139.450 us |
| thread_safe_contention | same_slot_write_read / 2 | 390.449 us | 377.582 us - 401.062 us |
| thread_safe_contention | same_slot_write_read / 4 | 982.409 us | 934.721 us - 1.032 ms |
| thread_safe_contention | same_slot_write_read / 8 | 2.400 ms | 2.211 ms - 2.575 ms |
| thread_safe_contention | same_slot_write_read / 16 | 5.935 ms | 5.368 ms - 6.535 ms |
| thread_safe_contention | independent_slots / 1 | 138.001 us | 135.862 us - 140.203 us |
| thread_safe_contention | independent_slots / 2 | 278.243 us | 264.187 us - 292.841 us |
| thread_safe_contention | independent_slots / 4 | 758.422 us | 713.944 us - 795.570 us |
| thread_safe_contention | independent_slots / 8 | 2.042 ms | 1.809 ms - 2.288 ms |
| thread_safe_contention | independent_slots / 16 | 4.366 ms | 3.956 ms - 4.755 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 138.637 us | 137.187 us - 140.322 us |
| thread_safe_contention | read_mostly_waiters / 2 | 166.010 us | 162.679 us - 168.817 us |
| thread_safe_contention | read_mostly_waiters / 4 | 259.685 us | 256.201 us - 263.238 us |
| thread_safe_contention | read_mostly_waiters / 8 | 560.880 us | 552.087 us - 569.030 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.159 ms | 1.072 ms - 1.236 ms |
| thread_safe_contention | batched_write_bursts / 1 | 239.524 us | 231.857 us - 247.920 us |
| thread_safe_contention | batched_write_bursts / 2 | 612.064 us | 578.866 us - 644.482 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.649 ms | 1.600 ms - 1.719 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.448 ms | 3.426 ms - 3.475 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.549 ms | 3.276 ms - 3.781 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.263 ms | 1.181 ms - 1.328 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.875 ms | 2.673 ms - 3.092 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.309 ms | 1.218 ms - 1.396 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.988 ms | 3.314 ms - 4.639 ms |
| thread_safe_effect_contention | batch_flush / 8 | 4.324 ms | 3.945 ms - 4.669 ms |
| thread_safe_effect_contention | batch_flush / 16 | 5.185 ms | 4.840 ms - 5.508 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 4.042 ms | 3.969 ms - 4.132 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.908 ms | 6.522 ms - 7.428 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.532 ms | 2.499 ms - 2.563 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.055 ms | 5.001 ms - 5.114 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.523 ms | 2.332 ms - 2.728 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 5.426 ms | 5.140 ms - 5.752 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.105 ms | 1.057 ms - 1.155 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.064 ms | 1.964 ms - 2.174 ms |
| profile_instrumentation | context_snapshot | 457.633 ns | 433.003 ns - 479.437 ns |
| profile_instrumentation | thread_safe_snapshot | 290.545 us | 286.284 us - 295.252 us |
| async_cached_resolve | async_context | 4.056 us | 3.743 us - 4.519 us |
| async_cached_resolve | sync_context_baseline | 70.250 ns | 68.508 ns - 72.505 ns |
| async_cached_resolve | sync_get | 11.790 ns | 11.667 ns - 11.937 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.387 us | 1.366 us - 1.409 us |
| async_cold_resolve | async_context | 3.904 us | 3.704 us - 4.150 us |
| async_cold_resolve | sync_context_baseline | 105.633 ns | 87.250 ns - 132.233 ns |
| async_cold_resolve | thread_safe_context_baseline | 988.499 ns | 956.458 ns - 1.025 us |
| async_invalidation_throughput | async_context | 239.943 us | 230.891 us - 249.656 us |
| async_invalidation_throughput | sync_context_baseline | 3.339 us | 3.182 us - 3.534 us |
| async_invalidation_throughput | thread_safe_context_baseline | 61.547 us | 60.286 us - 62.809 us |
| async_cancellation_throughput | async_invalidate_in_flight | 70.275 us | 54.869 us - 84.616 us |
| async_concurrent_contention | async_context / 1 | 104.124 us | 95.895 us - 112.134 us |
| async_concurrent_contention | async_context / 4 | 314.703 us | 290.140 us - 339.520 us |
| async_concurrent_contention | async_context / 16 | 1.744 ms | 1.534 ms - 1.968 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 93.443 us | 86.466 us - 103.133 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 636.605 us | 620.670 us - 649.323 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 3.830 ms | 3.789 ms - 3.875 ms |
| async_effect_throughput | async_context | 188.287 ms | 188.061 ms - 188.600 ms |
| async_batch_throughput | async_context | 79.956 us | 75.622 us - 85.014 us |
| async_batch_throughput | sync_context_baseline | 9.374 us | 8.820 us - 9.998 us |
| tokio_sync_cached_read | single_task | 1.601 us | 1.574 us - 1.627 us |
| tokio_sync_cached_read | spawn_read | 4.653 us | 4.445 us - 4.978 us |
| tokio_sync_cold_first_get | single_task | 1.477 us | 1.453 us - 1.502 us |
| tokio_sync_cold_first_get | spawn_compute | 4.852 us | 4.631 us - 5.096 us |
| tokio_sync_invalidation | single_task | 58.334 us | 57.216 us - 59.559 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 62.816 us | 61.948 us - 63.731 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 443.604 us | 401.815 us - 482.432 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.453 ms | 3.295 ms - 3.590 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 62.755 us | 62.152 us - 63.449 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 405.372 us | 372.182 us - 434.818 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 2.758 ms | 2.592 ms - 2.922 ms |
| tokio_sync_batch | spawn_batch | 50.306 us | 48.823 us - 51.922 us |
| tokio_sync_effect | single_task | 10.077 ms | 10.075 ms - 10.079 ms |
| scale | build | 157.865 ms | 151.023 ms - 164.964 ms |
| scale | cold_full_recalc | 135.267 ms | 125.072 ms - 143.138 ms |
| scale | full_recalc_invalidate_all | 85.396 ms | 80.569 ms - 90.279 ms |
| scale | viewport_recalc | 5.038 us | 4.682 us - 5.459 us |
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.194 ns | 1.143 ns - 1.263 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 189.188 ns | 179.718 ns - 199.633 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 24.794 ns | 23.279 ns - 26.456 ns |
| typed_cache_reads | context_cell | 2.948 ns | 2.746 ns - 3.177 ns |
| typed_cache_reads | context_rc_cell | 3.913 ns | 3.686 ns - 4.140 ns |
| typed_cache_reads | context_rc_slot | 5.133 ns | 4.727 ns - 5.600 ns |
| typed_cache_reads | context_slot | 4.785 ns | 4.409 ns - 5.200 ns |
| typed_cache_reads | thread_safe_cell | 26.272 ns | 25.869 ns - 26.664 ns |
| typed_cache_reads | thread_safe_slot | 67.520 ns | 66.610 ns - 68.448 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 2.770 us | 15.160 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 80.000 ns | 943.218 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.500 us | 22.140 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 100 | 47.440 us | 27.990 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 164 | 390.062 us | 81.320 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 284 | 1.938 ms | 134.460 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 551 | 7.745 ms | 260.551 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.430 us | 13.210 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 105 | 45.250 us | 27.301 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 186 | 290.293 us | 74.221 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 346 | 1.832 ms | 131.531 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 668 | 7.650 ms | 256.860 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.900 us | 55.500 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 135 | 66.260 us | 73.671 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 193 | 346.672 us | 106.792 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 1.762 ms | 204.101 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 5.814 ms | 371.133 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.140 us | 31.020 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 27 | 0 | 1 | 0 | 0 | 0 | 149 | 27.340 us | 57.671 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 56 | 0 | 1 | 0 | 0 | 0 | 353 | 142.040 us | 136.052 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 120 | 0 | 1 | 0 | 0 | 0 | 715 | 245.591 us | 282.860 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 213 | 0 | 1 | 0 | 0 | 0 | 1299 | 2.121 ms | 976.520 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.930 us | 28.800 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 137 | 41.260 us | 52.370 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 254 | 925.360 us | 154.320 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 493 | 4.786 ms | 353.722 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 927 | 20.166 ms | 626.554 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.020 us | 25.440 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 78 | 7.160 us | 34.290 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 75 | 6.560 us | 29.510 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 98 | 42.810 us | 46.331 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 108 | 128.140 us | 52.630 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 2.990 us | 59.731 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 196 | 54.000 us | 109.321 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 31 | 0 | 16 | 0 | 0 | 0 | 357 | 359.614 us | 230.351 us | 0 | 0 | 0 | 34 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 10 | 0 | 32 | 0 | 0 | 0 | 409 | 1.808 ms | 245.063 us | 0 | 0 | 0 | 10 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 727 | 9.303 ms | 500.123 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 6 | 1 | 391 | 2.314 ms | 254.781 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 728 | 6.918 ms | 423.084 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 30 | 1 | 399 | 2.718 ms | 217.131 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 37 | 1 | 708 | 10.684 ms | 376.403 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 4 | 0 | 33 | 0 | 7 | 1 | 647 | 3.584 ms | 311.033 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 5 | 1 | 1247 | 16.869 ms | 568.475 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 556 | 0 | 64 | 0 | 49 | 1 | 1159 | 29.321 ms | 6.627 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 559 | 0 | 64 | 0 | 50 | 1 | 1422 | 140.257 ms | 12.393 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 512 | 33.536 ms | 6.072 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 784 | 134.846 ms | 11.419 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1451 | 10.475 ms | 675.335 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2811 | 40.704 ms | 1.263 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 66 | 0 | 65 | 0 | 3 | 1 | 603 | 1.610 ms | 348.992 us | 0 | 0 | 0 | 69 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1179 | 6.218 ms | 643.356 us | 0 | 0 | 0 | 133 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 60.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 20.000 ns | 942.928 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 940.000 ns | 2.250 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 450.000 ns | 18.890 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 64 | 25.870 us | 2.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 21.470 us | 24.920 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 96 | 199.210 us | 5.210 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 1.250 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 190.722 us | 73.550 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 1.030 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 152 | 830.028 us | 6.830 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 140.000 ns | 980.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 1.200 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.108 ms | 124.440 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 20.000 ns | 1.010 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 291 | 2.932 ms | 11.940 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 80.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 4.813 ms | 247.291 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 920.000 ns | 1.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 400.000 ns | 10.640 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 66 | 26.820 us | 2.580 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 110.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 18.210 us | 23.191 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 107 | 197.692 us | 22.960 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 250.000 ns | 1.080 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 1.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 92.121 us | 47.251 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 187 | 786.526 us | 9.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 430.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 190.000 ns | 2.570 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.044 ms | 116.891 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 230.000 ns | 1.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 349 | 2.639 ms | 16.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 980.000 ns | 3.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 5.480 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 5.009 ms | 227.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 390.000 ns | 3.690 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.220 us | 18.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.700 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 470.000 ns | 34.780 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 110 | 64.270 us | 37.361 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 210.000 ns | 3.820 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 1.690 us | 32.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 170 | 345.982 us | 82.951 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 460.000 ns | 6.461 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 4 | 160.000 ns | 17.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 1.761 ms | 167.551 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 12.680 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 140.000 ns | 23.240 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 5.812 ms | 325.522 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 120.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.780 us | 30.081 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 80.000 ns | 13.970 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 840.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 980.000 ns | 1.970 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 140.000 ns | 1.020 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 1.140 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 510.000 ns | 13.090 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 470.000 ns | 13.800 us |
| thread_safe_contention_same_slot_write_read_2 | other | 66 | 12.490 us | 2.290 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 50.000 ns | 240.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 8.390 us | 27.101 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 27 | 6.390 us | 27.730 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 119 | 61.690 us | 4.010 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 32 | 14.980 us | 6.620 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 590.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 49.370 us | 56.151 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 56 | 15.970 us | 68.681 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 81 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 248 | 73.040 us | 7.190 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 60 | 32.511 us | 9.310 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 110.870 us | 106.360 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 120 | 29.130 us | 159.590 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 158 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 478 | 687.979 us | 23.380 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 32 | 97.310 us | 22.590 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 1.036 ms | 462.806 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 213 | 299.533 us | 467.364 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 319 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 910.000 ns | 1.910 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 140.000 ns | 970.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 1.190 us |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 420.000 ns | 12.500 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 440.000 ns | 12.230 us |
| thread_safe_contention_independent_slots_2 | other | 67 | 22.380 us | 3.420 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 110.000 ns | 510.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 70.000 ns | 1.540 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 12.590 us | 24.540 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 6.110 us | 22.360 us |
| thread_safe_contention_independent_slots_4 | other | 112 | 338.744 us | 7.730 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 320.000 ns | 1.800 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 2.080 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 333.126 us | 77.490 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 253.050 us | 65.220 us |
| thread_safe_contention_independent_slots_8 | other | 207 | 1.387 ms | 12.560 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 440.000 ns | 1.370 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 2.440 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.815 ms | 174.150 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.583 ms | 163.202 us |
| thread_safe_contention_independent_slots_16 | other | 353 | 5.914 ms | 20.350 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 920.000 ns | 2.530 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 460.000 ns | 4.930 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 7.115 ms | 315.594 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.135 ms | 283.150 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 920.000 ns | 1.460 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 70.000 ns | 460.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 580.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 510.000 ns | 11.770 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 500.000 ns | 11.170 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 1.580 us | 980.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 6 | 3.630 us | 2.530 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 620.000 ns | 13.460 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 1.300 us | 16.990 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 1.010 us | 1.130 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 4.620 us | 1.430 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 440.000 ns | 13.190 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 470.000 ns | 13.430 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 15.160 us | 1.180 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 10 | 19.650 us | 6.751 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 520.000 ns | 13.370 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 7.450 us | 24.670 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 17 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 19.230 us | 1.610 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 16 | 89.890 us | 6.910 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 430.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 1.210 us | 21.090 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 17.780 us | 22.590 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 1.970 us | 14.150 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 270.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 90.000 ns | 1.590 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 440.000 ns | 26.801 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 430.000 ns | 16.920 us |
| thread_safe_contention_batched_write_bursts_2 | other | 128 | 50.970 us | 28.590 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 210.000 ns | 3.670 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 23 | 1.650 us | 46.240 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 1.110 us | 30.611 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 227 | 336.324 us | 68.421 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 6 | 8.670 us | 1.820 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 7.380 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 34 | 8.320 us | 86.930 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 31 | 5.850 us | 65.800 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 43 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 342 | 1.801 ms | 161.833 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 430.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 820.000 ns | 13.060 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 10 | 5.760 us | 39.470 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 10 | 350.000 ns | 30.270 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 9.301 ms | 404.602 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.710 us | 29.410 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 130.000 ns | 34.330 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 190.000 ns | 31.421 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 353 | 2.313 ms | 209.601 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 860.000 ns | 9.930 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 6 | 930.000 ns | 35.250 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 661 | 6.916 ms | 382.523 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.710 us | 20.331 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 110.000 ns | 20.230 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 256 | 1.319 ms | 62.720 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 450.000 ns | 9.170 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.399 ms | 145.241 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 421 | 4.301 ms | 94.760 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 810.000 ns | 13.330 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 6.381 ms | 268.313 us |
| thread_safe_effect_contention_batch_flush_8 | other | 605 | 3.583 ms | 264.012 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 600.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 880.000 ns | 14.331 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 280.000 ns | 16.010 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 130.000 ns | 16.080 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1175 | 16.867 ms | 501.315 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 750.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.720 us | 30.350 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 80.000 ns | 16.690 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 80.000 ns | 19.370 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 347 | 6.871 ms | 173.421 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.750 us | 5.280 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.750 us | 25.550 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 17.617 ms | 5.950 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 556 | 4.829 ms | 472.414 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 479 | 25.102 ms | 190.791 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.770 us | 5.980 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.720 us | 27.960 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 105.978 ms | 11.699 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 559 | 9.174 ms | 469.563 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 224 | 11.732 ms | 15.260 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.800 us | 5.950 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 860.000 ns | 15.530 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 21.800 ms | 6.000 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.840 us | 35.060 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 368 | 31.514 ms | 19.130 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.920 us | 5.960 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 840.000 ns | 17.250 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 103.327 ms | 11.343 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.660 us | 33.460 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 745 | 4.296 ms | 39.871 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.100 us | 9.920 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.710 us | 25.480 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 6.173 ms | 547.974 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.880 us | 52.090 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1401 | 16.141 ms | 69.240 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.930 us | 15.641 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.260 us | 49.950 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 24.553 ms | 1.040 ms |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.310 us | 87.561 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 402 | 1.601 ms | 192.011 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 68 | 4.130 us | 9.100 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.710 us | 23.320 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 2 | 830.000 ns | 78.791 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 66 | 1.790 us | 45.770 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 786 | 6.200 ms | 347.274 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 8.590 us | 14.030 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.330 us | 51.260 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 2.230 us | 144.011 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 3.400 us | 86.781 us |

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

What the four cases show at `N = 1_000_000` (reference machine below): `build`
constructs 2M nodes (~0.12 s), `cold_full_recalc` computes every formula from cold
(~0.105 s), `full_recalc_invalidate_all` re-edits every input and recomputes the
whole sheet (~0.080 s), and `viewport_recalc` edits one input and reads only a
1,000-cell viewport — **~3.7 µs**, ~21,000× cheaper than a full recalc because the
lazy pull-based model leaves off-viewport formulas dirty and never recomputes them
(the property a viewport-rendered spreadsheet needs).
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
formula cells (= 10M cells) by running the bench at `LAZILY_SCALE_N=5000000`. Criterion
median on the cross-language reference machine (AMD Ryzen 9 9950X3D), pinned to one core
(`taskset -c 4`) and run serially so nothing contends for L3 / memory bandwidth:

| case | mean | per cell |
|---|---:|---:|
| `build` (10M cells) | ~718 ms | ~72 ns |
| `cold_full_recalc` (5M) | ~544 ms | ~109 ns |
| `full_recalc_invalidate_all` (5M) | ~398 ms | ~80 ns |
| `viewport_recalc` (1k) | ~3.8 µs | ~4 ns |

So lazily backs a **full-capacity Google Sheets workbook**: build under a second, full
recompute ~0.5 s, and — crucially — viewport recalc stays ~3.8 µs **independent of sheet
size** (it was ~3.7 µs at 1M too), because the lazy pull-based model only recomputes the
cells you read. Reproduce: `LAZILY_SCALE_N=5000000 cargo bench --features scale-bench --bench scale`.
Across the three implementations lazily-rs holds the **cheapest viewport reads** (3.7–3.8 µs);
see the cross-language table in lazily-zig's `BENCHMARKS.md` for the full head-to-head.

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
| cached read (Context) | 5.7 ns | 23 ns | — † |
| cached read (ThreadSafeContext) | 68 ns | 22 ns | — † |
| cold first get (Context) | 129 ns | 97 ns | — † |
| cold first get (ThreadSafeContext) | 1.17 µs | 107 ns | — † |
| fan-out 256 (Context) | 58.4 µs | 1.12 µs | — † |
| fan-out 256 (ThreadSafeContext) | 182 µs | 1.68 µs | — |
| set_cell high_fan_out 512 | 139 µs | 3.26 µs | — † |
| memo equality suppression (Context) | 3.3 µs | 34 ns | — † |
| effect flushing (Context) | 90 ns | 87 ns | — |
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
