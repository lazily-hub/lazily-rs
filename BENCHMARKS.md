# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.40.1`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.724 ms | 3.117 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.199 ms | 7.616 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 2.037 ms | 2.642 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.872 ms | 5.835 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 537.045 us | 563.965 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.353 ms | 1.403 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.844 ms | 2.947 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.564 ms | 4.533 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.178 ms | 1.401 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.073 ms | 3.388 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.356 ms | 1.624 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.143 ms | 3.535 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.007 ms | 2.873 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.987 ms | 6.675 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.773 ms | 3.795 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.366 ms | 6.470 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.507 ms | 2.622 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.038 ms | 5.225 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.640 ms | 3.279 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.548 ms | 7.486 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.190 ms | 1.261 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.020 ms | 2.166 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 3.551 ns | 3.543 ns - 3.561 ns |
| cached_reads | thread_safe_context | 56.817 ns | 56.256 ns - 57.623 ns |
| cold_first_get | context | 99.726 ns | 88.147 ns - 110.841 ns |
| cold_first_get | thread_safe_context | 981.136 ns | 958.220 ns - 1.003 us |
| dependency_fan_out | context / 32 | 2.833 us | 2.426 us - 3.503 us |
| dependency_fan_out | context / 256 | 40.502 us | 39.803 us - 41.160 us |
| dependency_fan_out | thread_safe_context / 32 | 20.992 us | 20.381 us - 21.669 us |
| dependency_fan_out | thread_safe_context / 256 | 164.268 us | 162.633 us - 165.854 us |
| set_cell_invalidation | high_fan_out / 512 | 105.974 us | 101.958 us - 109.185 us |
| set_cell_invalidation | same_slot_contention / 1 | 80.242 us | 78.998 us - 81.116 us |
| set_cell_invalidation | same_slot_contention / 2 | 178.784 us | 175.103 us - 182.262 us |
| set_cell_invalidation | same_slot_contention / 4 | 463.376 us | 445.183 us - 482.419 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.308 ms | 1.252 ms - 1.364 ms |
| set_cell_invalidation | same_slot_contention / 16 | 3.308 ms | 3.101 ms - 3.519 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 79.594 us | 78.046 us - 80.955 us |
| set_cell_invalidation | independent_slot_contention / 2 | 169.701 us | 167.266 us - 172.320 us |
| set_cell_invalidation | independent_slot_contention / 4 | 453.929 us | 446.408 us - 462.184 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.406 ms | 1.344 ms - 1.465 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 3.342 ms | 3.256 ms - 3.417 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 138.518 us | 137.686 us - 139.470 us |
| set_cell_invalidation | batched_write_bursts / 2 | 187.491 us | 182.473 us - 192.264 us |
| set_cell_invalidation | batched_write_bursts / 4 | 435.747 us | 422.157 us - 447.806 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.243 ms | 1.192 ms - 1.292 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.000 ms | 2.784 ms - 3.188 ms |
| memo_equality_suppression | context | 1.352 us | 1.205 us - 1.496 us |
| memo_equality_suppression | thread_safe_context | 29.240 us | 28.832 us - 29.668 us |
| effect_flushing | context | 29.607 ns | 29.522 ns - 29.719 ns |
| effect_flushing | thread_safe_context | 898.833 ns | 896.828 ns - 901.003 ns |
| batch_storms | context / 64 | 1.701 us | 1.698 us - 1.703 us |
| batch_storms | thread_safe_context / 64 | 7.603 us | 7.564 us - 7.650 us |
| thread_safe_contention | same_slot_write_read / 1 | 140.839 us | 140.160 us - 141.536 us |
| thread_safe_contention | same_slot_write_read / 2 | 383.114 us | 372.582 us - 392.656 us |
| thread_safe_contention | same_slot_write_read / 4 | 945.646 us | 905.916 us - 986.137 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.716 ms | 2.539 ms - 2.874 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.970 ms | 6.529 ms - 7.328 ms |
| thread_safe_contention | independent_slots / 1 | 144.161 us | 139.831 us - 150.197 us |
| thread_safe_contention | independent_slots / 2 | 284.279 us | 267.114 us - 301.182 us |
| thread_safe_contention | independent_slots / 4 | 715.451 us | 691.847 us - 740.662 us |
| thread_safe_contention | independent_slots / 8 | 2.092 ms | 1.915 ms - 2.273 ms |
| thread_safe_contention | independent_slots / 16 | 4.782 ms | 4.264 ms - 5.258 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 140.005 us | 139.396 us - 140.549 us |
| thread_safe_contention | read_mostly_waiters / 2 | 159.885 us | 158.021 us - 161.667 us |
| thread_safe_contention | read_mostly_waiters / 4 | 243.371 us | 241.635 us - 245.046 us |
| thread_safe_contention | read_mostly_waiters / 8 | 540.223 us | 529.853 us - 549.960 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.316 ms | 1.236 ms - 1.373 ms |
| thread_safe_contention | batched_write_bursts / 1 | 215.167 us | 213.764 us - 216.389 us |
| thread_safe_contention | batched_write_bursts / 2 | 556.016 us | 525.656 us - 590.330 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.652 ms | 1.638 ms - 1.665 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.835 ms | 2.780 ms - 2.884 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.648 ms | 3.411 ms - 3.911 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.197 ms | 1.086 ms - 1.301 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.950 ms | 2.742 ms - 3.136 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.407 ms | 1.343 ms - 1.478 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.154 ms | 2.991 ms - 3.309 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.106 ms | 1.879 ms - 2.348 ms |
| thread_safe_effect_contention | batch_flush / 16 | 5.077 ms | 4.607 ms - 5.566 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.765 ms | 3.749 ms - 3.779 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.309 ms | 6.228 ms - 6.379 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.517 ms | 2.473 ms - 2.561 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.097 ms | 5.036 ms - 5.159 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.633 ms | 2.463 ms - 2.820 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.764 ms | 6.365 ms - 7.142 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.192 ms | 1.175 ms - 1.213 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.047 ms | 2.006 ms - 2.090 ms |
| profile_instrumentation | context_snapshot | 208.024 ns | 207.433 ns - 208.660 ns |
| profile_instrumentation | thread_safe_snapshot | 296.464 us | 294.858 us - 298.425 us |
| async_cached_resolve | async_context | 4.554 us | 4.150 us - 4.992 us |
| async_cached_resolve | sync_context_baseline | 57.225 ns | 56.685 ns - 57.664 ns |
| async_cached_resolve | sync_get | 11.546 ns | 11.503 ns - 11.591 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.321 us | 1.317 us - 1.326 us |
| async_cold_resolve | async_context | 4.597 us | 4.193 us - 5.072 us |
| async_cold_resolve | sync_context_baseline | 87.596 ns | 78.521 ns - 96.176 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.181 us | 1.022 us - 1.383 us |
| async_invalidation_throughput | async_context | 328.691 us | 293.225 us - 367.560 us |
| async_invalidation_throughput | sync_context_baseline | 2.178 us | 2.174 us - 2.182 us |
| async_invalidation_throughput | thread_safe_context_baseline | 54.303 us | 54.184 us - 54.446 us |
| async_cancellation_throughput | async_invalidate_in_flight | 70.572 us | 60.799 us - 79.761 us |
| async_concurrent_contention | async_context / 1 | 79.658 us | 74.704 us - 87.076 us |
| async_concurrent_contention | async_context / 4 | 292.366 us | 264.171 us - 325.354 us |
| async_concurrent_contention | async_context / 16 | 1.622 ms | 1.444 ms - 1.830 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 87.852 us | 84.979 us - 91.146 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 607.174 us | 551.318 us - 655.191 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 3.924 ms | 3.910 ms - 3.937 ms |
| async_effect_throughput | async_context | 188.142 ms | 187.946 ms - 188.331 ms |
| async_batch_throughput | async_context | 105.848 us | 92.453 us - 120.197 us |
| async_batch_throughput | sync_context_baseline | 6.777 us | 6.755 us - 6.801 us |
| tokio_sync_cached_read | single_task | 1.432 us | 1.427 us - 1.436 us |
| tokio_sync_cached_read | spawn_read | 5.861 us | 5.506 us - 6.248 us |
| tokio_sync_cold_first_get | single_task | 1.372 us | 1.367 us - 1.378 us |
| tokio_sync_cold_first_get | spawn_compute | 5.276 us | 4.936 us - 5.635 us |
| tokio_sync_invalidation | single_task | 55.766 us | 55.443 us - 56.080 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 61.739 us | 60.693 us - 62.899 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 394.686 us | 370.069 us - 420.658 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.511 ms | 3.353 ms - 3.644 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 62.092 us | 60.819 us - 63.475 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 384.396 us | 344.569 us - 423.095 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 2.009 ms | 1.846 ms - 2.190 ms |
| tokio_sync_batch | spawn_batch | 48.544 us | 48.040 us - 49.122 us |
| tokio_sync_effect | single_task | 10.132 ms | 10.094 ms - 10.178 ms |
| scale | build | 109.368 ms | 108.190 ms - 110.717 ms |
| scale | cold_full_recalc | 53.241 ms | 52.505 ms - 54.009 ms |
| scale | full_recalc_invalidate_all | 49.701 ms | 49.283 ms - 50.200 ms |
| scale | viewport_recalc | 3.034 us | 3.024 us - 3.046 us |
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.121 ns | 1.118 ns - 1.124 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 86.902 ns | 86.244 ns - 87.706 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 16.401 ns | 16.320 ns - 16.490 ns |
| typed_cache_reads | context_cell | 2.184 ns | 2.177 ns - 2.192 ns |
| typed_cache_reads | context_rc_cell | 5.807 ns | 5.783 ns - 5.839 ns |
| typed_cache_reads | context_rc_slot | 6.988 ns | 6.974 ns - 7.000 ns |
| typed_cache_reads | context_slot | 3.591 ns | 3.579 ns - 3.604 ns |
| typed_cache_reads | thread_safe_cell | 24.452 ns | 24.358 ns - 24.553 ns |
| typed_cache_reads | thread_safe_slot | 56.438 ns | 56.103 ns - 56.808 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.340 us | 11.400 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 90.000 ns | 976.078 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.610 us | 22.190 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 98 | 67.121 us | 42.981 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 159 | 400.976 us | 78.970 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 296 | 1.927 ms | 134.822 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 553 | 8.205 ms | 272.153 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.470 us | 14.150 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 100 | 40.620 us | 26.150 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 181 | 310.813 us | 56.800 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 340 | 1.764 ms | 122.140 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 666 | 7.403 ms | 253.852 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.780 us | 48.212 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 126 | 76.351 us | 67.501 us | 0 | 0 | 0 | 10 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 213 | 434.253 us | 130.093 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 378 | 2.035 ms | 226.631 us | 0 | 0 | 0 | 7 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 712 | 5.708 ms | 388.254 us | 0 | 0 | 0 | 1 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.050 us | 30.490 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 146 | 35.910 us | 61.980 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 56 | 0 | 1 | 0 | 0 | 0 | 334 | 149.980 us | 132.311 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 120 | 0 | 1 | 0 | 0 | 0 | 750 | 380.651 us | 285.961 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 213 | 0 | 1 | 0 | 0 | 0 | 1269 | 2.610 ms | 703.685 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.800 us | 25.651 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 138 | 45.160 us | 52.781 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 257 | 672.206 us | 115.731 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 475 | 3.198 ms | 236.611 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 922 | 14.108 ms | 477.774 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.010 us | 26.670 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 2.010 us | 27.621 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 97 | 12.811 us | 34.780 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 118 | 17.580 us | 36.681 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 116 | 8.961 us | 53.580 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.090 us | 60.790 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 200 | 62.020 us | 110.260 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 34 | 0 | 16 | 0 | 0 | 0 | 365 | 285.570 us | 225.151 us | 0 | 0 | 0 | 34 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 7 | 0 | 32 | 0 | 0 | 0 | 386 | 2.643 ms | 283.854 us | 0 | 0 | 0 | 6 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 8 | 0 | 64 | 0 | 0 | 0 | 745 | 11.046 ms | 521.742 us | 0 | 0 | 0 | 7 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 4 | 1 | 381 | 2.147 ms | 233.100 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 738 | 8.588 ms | 471.745 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 26 | 1 | 396 | 2.156 ms | 188.150 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 34 | 1 | 704 | 9.547 ms | 345.241 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 4.215 ms | 315.602 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 5 | 1 | 1250 | 20.476 ms | 644.886 us | 0 | 0 | 0 | 3 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 551 | 0 | 64 | 0 | 49 | 1 | 1154 | 33.085 ms | 6.565 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 561 | 0 | 64 | 0 | 50 | 1 | 1424 | 128.916 ms | 12.325 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 520 | 29.108 ms | 5.783 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 762 | 132.639 ms | 11.270 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1439 | 12.040 ms | 740.547 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2802 | 47.849 ms | 1.297 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 162 | 0 | 65 | 0 | 43 | 1 | 2015 | 1.819 ms | 799.886 us | 0 | 0 | 0 | 257 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1319 | 9.972 ms | 781.517 us | 0 | 0 | 0 | 154 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 975.878 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 950.000 ns | 1.590 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 530.000 ns | 19.610 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 62 | 28.691 us | 2.450 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 70.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 38.310 us | 39.621 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 91 | 215.822 us | 3.990 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 185.034 us | 74.180 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 164 | 821.113 us | 6.560 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.106 ms | 127.442 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 293 | 2.588 ms | 11.730 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 140.000 ns | 1.000 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 5.617 ms | 257.103 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 1.060 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 900.000 ns | 1.310 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 70.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 440.000 ns | 11.110 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 61 | 26.980 us | 2.110 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 100.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 750.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 13.430 us | 22.340 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 102 | 177.421 us | 4.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 270.000 ns | 1.040 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 90.000 ns | 1.900 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 132.922 us | 48.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 110.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 181 | 650.736 us | 7.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 470.000 ns | 1.450 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 220.000 ns | 2.830 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.113 ms | 108.500 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 200.000 ns | 2.000 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 347 | 2.634 ms | 14.221 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 940.000 ns | 2.760 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 5.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 4.767 ms | 227.731 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 420.000 ns | 3.600 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.040 us | 16.532 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 130.000 ns | 980.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 2.430 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 470.000 ns | 27.200 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 1.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 104 | 75.411 us | 34.340 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 4.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 11 | 630.000 ns | 28.361 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 183 | 433.193 us | 79.770 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 140.000 ns | 990.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 8.961 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 11 | 450.000 ns | 39.292 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 20.000 ns | 1.080 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 336 | 2.034 ms | 168.821 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 590.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 15.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 7 | 310.000 ns | 41.350 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 644 | 5.707 ms | 343.394 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.740 us | 33.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1 | 30.000 ns | 10.750 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 550.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 940.000 ns | 1.470 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 70.000 ns | 470.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 20.000 ns | 570.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 510.000 ns | 13.530 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 510.000 ns | 14.450 us |
| thread_safe_contention_same_slot_write_read_2 | other | 67 | 20.210 us | 2.210 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 60.000 ns | 160.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 11.330 us | 30.390 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 4.290 us | 28.890 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 121 | 96.750 us | 3.990 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 18 | 5.330 us | 4.350 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 44.750 us | 58.220 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 56 | 3.120 us | 65.361 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 74 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 242 | 156.851 us | 8.280 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 80 | 49.410 us | 13.390 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 800.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 146.950 us | 107.850 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 120 | 27.420 us | 155.641 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 179 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 455 | 1.073 ms | 16.840 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 41 | 25.280 us | 6.550 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 520.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 1.312 ms | 298.412 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 213 | 199.491 us | 381.363 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 303 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 890.000 ns | 1.380 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 250.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 560.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 410.000 ns | 11.590 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 420.000 ns | 11.871 us |
| thread_safe_contention_independent_slots_2 | other | 68 | 21.880 us | 2.380 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 130.000 ns | 400.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 700.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 17.930 us | 24.801 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 5.160 us | 24.500 us |
| thread_safe_contention_independent_slots_4 | other | 115 | 220.253 us | 4.580 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 250.000 ns | 720.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 1.360 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 315.880 us | 54.700 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 135.703 us | 54.371 us |
| thread_safe_contention_independent_slots_8 | other | 189 | 1.321 ms | 7.980 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 510.000 ns | 1.520 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 3.060 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.200 ms | 111.931 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 675.589 us | 112.120 us |
| thread_safe_contention_independent_slots_16 | other | 348 | 5.052 ms | 14.380 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 920.000 ns | 2.820 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 460.000 ns | 5.650 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 5.788 ms | 229.274 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 3.267 ms | 225.650 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 950.000 ns | 1.430 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 90.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 460.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 450.000 ns | 12.250 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 490.000 ns | 12.220 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 950.000 ns | 1.050 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 350.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 500.000 ns | 12.741 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 480.000 ns | 13.290 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 3.610 us | 1.420 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 18 | 7.961 us | 3.160 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 640.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 600.000 ns | 12.750 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 610.000 ns | 16.810 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 5.080 us | 1.230 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 24 | 8.690 us | 3.600 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 500.000 ns | 12.970 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 3.280 us | 18.461 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 24 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 3.721 us | 1.250 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 14 | 3.990 us | 2.680 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 630.000 ns | 15.130 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 590.000 ns | 34.130 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 31 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.040 us | 13.910 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 50.000 ns | 290.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.510 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 440.000 ns | 27.000 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 440.000 ns | 18.080 us |
| thread_safe_contention_batched_write_bursts_2 | other | 128 | 56.180 us | 28.490 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 210.000 ns | 3.860 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 23 | 1.860 us | 47.250 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 3.720 us | 30.460 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 228 | 260.490 us | 64.400 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 12.730 us | 1.140 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 430.000 ns | 7.910 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 34 | 2.930 us | 84.390 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 34 | 8.990 us | 67.311 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 49 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 334 | 2.641 ms | 204.813 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 340.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 15.630 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 6 | 250.000 ns | 38.030 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 7 | 1.060 us | 25.041 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 657 | 11.014 ms | 409.921 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 140.000 ns | 1.260 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.760 us | 33.981 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 8 | 30.190 us | 30.730 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 8 | 470.000 ns | 45.850 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 6 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 346 | 2.146 ms | 203.450 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 890.000 ns | 11.070 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 3 | 310.000 ns | 18.580 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 669 | 8.547 ms | 417.025 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.720 us | 23.170 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 5 | 38.450 us | 31.550 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 253 | 1.000 ms | 50.300 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 420.000 ns | 8.900 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.156 ms | 128.950 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 417 | 4.312 ms | 97.751 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 870.000 ns | 15.060 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.234 ms | 232.430 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 4.214 ms | 275.232 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 750.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 890.000 ns | 15.750 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 150.000 ns | 12.750 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 70.000 ns | 11.120 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1176 | 20.473 ms | 561.125 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 860.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.740 us | 35.721 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 3 | 130.000 ns | 27.140 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 130.000 ns | 20.040 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 347 | 5.258 ms | 174.912 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.840 us | 16.600 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.690 us | 29.230 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 23.035 ms | 5.834 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 551 | 4.788 ms | 510.838 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 479 | 27.406 ms | 192.521 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.780 us | 5.640 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.750 us | 28.101 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 90.050 ms | 11.579 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 561 | 11.457 ms | 520.163 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 232 | 7.723 ms | 12.580 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.900 us | 6.190 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 840.000 ns | 18.440 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 21.381 ms | 5.709 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.800 us | 36.820 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 346 | 35.212 ms | 17.540 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.890 us | 6.150 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 870.000 ns | 17.780 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 97.423 ms | 11.193 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.730 us | 35.550 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 733 | 5.134 ms | 42.000 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.150 us | 10.560 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.750 us | 29.310 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 6.900 ms | 606.256 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.770 us | 52.421 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1392 | 18.821 ms | 66.952 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.010 us | 15.791 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.460 us | 57.561 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 29.017 ms | 1.059 ms |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.520 us | 97.401 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 526 | 1.762 ms | 229.130 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 1236 | 43.100 us | 166.760 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.730 us | 27.491 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 26 | 690.000 ns | 144.081 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 162 | 11.630 us | 232.424 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 795 | 9.773 ms | 417.903 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 7.210 us | 31.490 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.550 us | 57.850 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 6 | 166.841 us | 166.402 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 21.771 us | 107.872 us |

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

## Phase 3 Wire-Format Optimizations (`#lzperfaudit`)

Three spec-ratified wire wins (`#lzspecfrontiersuppress`, `#lzspecbase64`,
`#lzspecintern`), measured by `benches/wire_optimizations.rs`. Run with:

```bash
cargo bench --features json-base64 --bench wire_optimizations
```

### `#lzspecfrontiersuppress` — optional CrdtSync frontier

Omitting the stamp frontier when unchanged cuts wire size and encode/decode cost:

| Variant | Wire size | Encode | Decode |
|---|---:|---:|---:|
| with frontier (8 peers) | 879 B | ~740 ns | ~1.6 µs |
| ops only (suppressed) | 514 B (**−42%**) | ~463 ns | ~1.0 µs |

### `#lzspecbase64` — base64 byte arrays vs JSON-u8 arrays

Under the `json-base64` capability flag, `Inline`/`Payload` bytes travel as base64
strings instead of JSON integer arrays:

| Payload | json-u8 wire | base64 wire | Savings | Decode (u8 → b64) |
|---:|---:|---:|---:|---|
| 64 B | 395 B | 228 B | **42%** | 911 ns → 710 ns |
| 1 KiB | 4,235 B | 1,508 B | **64%** | 36 µs → 25 µs |
| 16 KiB | 65,675 B | 21,988 B | **67%** | 89 µs → 65 µs |

### `#lzspecintern` — batch string-intern table

Deduplicating repeated `type_tag` strings into a sidecar intern table (256 nodes,
4 distinct tags):

| Variant | Wire size | Savings |
|---|---:|---:|
| inline tags | 15,729 B | — |
| interned | 14,890 B | **5%** |

Savings grow with the node-to-tag ratio (more nodes sharing fewer tags).

## Revision engine crossover (`#lzspecrevisionengine`)

The revision (pull) invalidation engine gives O(1) writes (no dependent cone
walk) at the cost of O(changed-subpath) reads. Observable values are provably
identical to push mode (`get_equiv_push`, lazily-formal `RevisionEngine.lean`).

Benchmark: 10 writes to a source cell with N dependent slots (construction +
priming included in each measurement). Run with:

```bash
cargo bench --bench revision_engine
```

| Fan-out | Push | Revision | Revision win |
|---:|---:|---:|---:|
| 1 | 194 ns | 127 ns | 1.5× |
| 16 | 1.19 µs | 822 ns | 1.4× |
| 128 | 10.9 µs | 8.75 µs | 1.25× |
| 1024 | 192 µs | 177 µs | 1.08× |

The write cost scales linearly with fan-out in push (O(N) dirty walk) but is
O(1) in revision (revision bump). The construction+priming overhead (same for
both) dilutes the pure write-cost gap; workloads with high write:read ratios
and large fan-out benefit most.

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
