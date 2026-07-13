# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.33.0`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.652 ms | 3.036 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.721 ms | 7.712 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.994 ms | 2.331 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 4.771 ms | 5.583 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 578.895 us | 623.778 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.372 ms | 1.634 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.994 ms | 3.100 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.634 ms | 4.221 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.330 ms | 1.541 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.876 ms | 3.311 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.328 ms | 1.497 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.158 ms | 3.481 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.308 ms | 2.506 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 5.705 ms | 6.983 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.799 ms | 4.132 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.342 ms | 6.490 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.534 ms | 2.642 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.015 ms | 5.222 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.801 ms | 3.188 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.573 ms | 7.789 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.166 ms | 1.313 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.870 ms | 2.246 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 4.358 ns | 4.055 ns - 4.736 ns |
| cached_reads | thread_safe_context | 66.763 ns | 65.254 ns - 68.523 ns |
| cold_first_get | context | 103.586 ns | 92.040 ns - 115.603 ns |
| cold_first_get | thread_safe_context | 1.101 us | 1.017 us - 1.194 us |
| dependency_fan_out | context / 32 | 3.687 us | 3.333 us - 4.086 us |
| dependency_fan_out | context / 256 | 47.812 us | 44.498 us - 52.156 us |
| dependency_fan_out | thread_safe_context / 32 | 21.958 us | 21.323 us - 22.605 us |
| dependency_fan_out | thread_safe_context / 256 | 171.949 us | 166.511 us - 177.400 us |
| set_cell_invalidation | high_fan_out / 512 | 113.274 us | 105.946 us - 122.011 us |
| set_cell_invalidation | same_slot_contention / 1 | 77.010 us | 75.856 us - 78.149 us |
| set_cell_invalidation | same_slot_contention / 2 | 185.728 us | 177.817 us - 194.541 us |
| set_cell_invalidation | same_slot_contention / 4 | 493.230 us | 465.559 us - 524.081 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.314 ms | 1.205 ms - 1.422 ms |
| set_cell_invalidation | same_slot_contention / 16 | 2.594 ms | 2.384 ms - 2.796 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 77.583 us | 76.549 us - 78.602 us |
| set_cell_invalidation | independent_slot_contention / 2 | 180.705 us | 172.876 us - 189.127 us |
| set_cell_invalidation | independent_slot_contention / 4 | 455.817 us | 435.892 us - 476.056 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.375 ms | 1.292 ms - 1.456 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 2.747 ms | 2.576 ms - 2.935 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 134.141 us | 132.298 us - 136.058 us |
| set_cell_invalidation | batched_write_bursts / 2 | 221.920 us | 206.240 us - 238.827 us |
| set_cell_invalidation | batched_write_bursts / 4 | 495.259 us | 455.063 us - 542.275 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.430 ms | 1.243 ms - 1.660 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.519 ms | 2.286 ms - 2.755 ms |
| memo_equality_suppression | context | 3.543 us | 3.050 us - 4.234 us |
| memo_equality_suppression | thread_safe_context | 34.781 us | 32.852 us - 36.730 us |
| effect_flushing | context | 109.404 ns | 104.469 ns - 114.322 ns |
| effect_flushing | thread_safe_context | 977.040 ns | 948.555 ns - 1.006 us |
| batch_storms | context / 64 | 3.209 us | 2.990 us - 3.439 us |
| batch_storms | thread_safe_context / 64 | 8.375 us | 8.061 us - 8.706 us |
| thread_safe_contention | same_slot_write_read / 1 | 139.634 us | 137.529 us - 141.561 us |
| thread_safe_contention | same_slot_write_read / 2 | 370.711 us | 353.259 us - 389.874 us |
| thread_safe_contention | same_slot_write_read / 4 | 915.152 us | 894.547 us - 937.808 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.734 ms | 2.591 ms - 2.868 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.663 ms | 6.133 ms - 7.135 ms |
| thread_safe_contention | independent_slots / 1 | 137.144 us | 135.175 us - 138.834 us |
| thread_safe_contention | independent_slots / 2 | 278.179 us | 265.242 us - 291.348 us |
| thread_safe_contention | independent_slots / 4 | 793.789 us | 751.305 us - 846.860 us |
| thread_safe_contention | independent_slots / 8 | 2.072 ms | 1.950 ms - 2.196 ms |
| thread_safe_contention | independent_slots / 16 | 4.879 ms | 4.644 ms - 5.130 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 138.393 us | 137.065 us - 139.668 us |
| thread_safe_contention | read_mostly_waiters / 2 | 161.906 us | 158.799 us - 164.976 us |
| thread_safe_contention | read_mostly_waiters / 4 | 247.972 us | 244.229 us - 251.688 us |
| thread_safe_contention | read_mostly_waiters / 8 | 589.489 us | 576.877 us - 602.386 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.376 ms | 1.271 ms - 1.481 ms |
| thread_safe_contention | batched_write_bursts / 1 | 214.458 us | 212.077 us - 216.830 us |
| thread_safe_contention | batched_write_bursts / 2 | 644.904 us | 617.502 us - 671.684 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.595 ms | 1.587 ms - 1.602 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.000 ms | 2.951 ms - 3.047 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.596 ms | 3.363 ms - 3.814 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.260 ms | 1.126 ms - 1.377 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.906 ms | 2.766 ms - 3.053 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.339 ms | 1.271 ms - 1.401 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.142 ms | 2.982 ms - 3.286 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.350 ms | 2.278 ms - 2.417 ms |
| thread_safe_effect_contention | batch_flush / 16 | 5.793 ms | 5.411 ms - 6.177 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.856 ms | 3.801 ms - 3.929 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.332 ms | 6.267 ms - 6.393 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.548 ms | 2.525 ms - 2.574 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 5.039 ms | 4.975 ms - 5.106 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.764 ms | 2.596 ms - 2.927 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.611 ms | 6.167 ms - 7.047 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.197 ms | 1.155 ms - 1.243 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.865 ms | 1.719 ms - 2.008 ms |
| profile_instrumentation | context_snapshot | 328.675 ns | 306.256 ns - 352.583 ns |
| profile_instrumentation | thread_safe_snapshot | 288.377 us | 286.707 us - 289.867 us |
| async_cached_resolve | async_context | 4.022 us | 3.760 us - 4.342 us |
| async_cached_resolve | sync_context_baseline | 76.860 ns | 73.180 ns - 81.054 ns |
| async_cached_resolve | sync_get | 11.818 ns | 11.705 ns - 11.938 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.451 us | 1.422 us - 1.481 us |
| async_cold_resolve | async_context | 3.628 us | 3.520 us - 3.743 us |
| async_cold_resolve | sync_context_baseline | 137.545 ns | 120.522 ns - 157.226 ns |
| async_cold_resolve | thread_safe_context_baseline | 1.187 us | 1.086 us - 1.299 us |
| async_invalidation_throughput | async_context | 271.897 us | 244.870 us - 304.900 us |
| async_invalidation_throughput | sync_context_baseline | 4.362 us | 4.035 us - 4.694 us |
| async_invalidation_throughput | thread_safe_context_baseline | 59.115 us | 57.810 us - 60.515 us |
| async_cancellation_throughput | async_invalidate_in_flight | 63.867 us | 52.092 us - 74.593 us |
| async_concurrent_contention | async_context / 1 | 74.576 us | 72.876 us - 76.273 us |
| async_concurrent_contention | async_context / 4 | 332.121 us | 300.580 us - 361.351 us |
| async_concurrent_contention | async_context / 16 | 1.689 ms | 1.563 ms - 1.803 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 78.658 us | 76.778 us - 80.328 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 675.986 us | 661.405 us - 692.748 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 4.146 ms | 4.106 ms - 4.182 ms |
| async_effect_throughput | async_context | 188.523 ms | 187.950 ms - 189.340 ms |
| async_batch_throughput | async_context | 91.251 us | 84.135 us - 98.984 us |
| async_batch_throughput | sync_context_baseline | 8.782 us | 8.408 us - 9.173 us |
| tokio_sync_cached_read | single_task | 1.501 us | 1.479 us - 1.526 us |
| tokio_sync_cached_read | spawn_read | 4.921 us | 4.766 us - 5.079 us |
| tokio_sync_cold_first_get | single_task | 1.401 us | 1.387 us - 1.417 us |
| tokio_sync_cold_first_get | spawn_compute | 4.883 us | 4.609 us - 5.184 us |
| tokio_sync_invalidation | single_task | 58.047 us | 56.907 us - 59.342 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 61.377 us | 60.635 us - 62.062 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 483.294 us | 453.463 us - 511.343 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 3.533 ms | 3.350 ms - 3.668 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 62.524 us | 61.632 us - 63.407 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 446.415 us | 425.700 us - 469.296 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 2.566 ms | 2.287 ms - 2.779 ms |
| tokio_sync_batch | spawn_batch | 48.705 us | 47.678 us - 49.804 us |
| tokio_sync_effect | single_task | 10.084 ms | 10.081 ms - 10.087 ms |
| scale | build | 145.020 ms | 138.408 ms - 153.919 ms |
| scale | cold_full_recalc | 112.945 ms | 104.438 ms - 123.380 ms |
| scale | full_recalc_invalidate_all | 72.228 ms | 69.860 ms - 74.759 ms |
| scale | viewport_recalc | 3.652 us | 3.535 us - 3.765 us |
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.198 ns | 1.155 ns - 1.246 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 208.019 ns | 194.915 ns - 224.104 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 24.081 ns | 23.087 ns - 25.429 ns |
| typed_cache_reads | context_cell | 2.593 ns | 2.538 ns - 2.656 ns |
| typed_cache_reads | context_rc_cell | 3.503 ns | 3.213 ns - 3.827 ns |
| typed_cache_reads | context_rc_slot | 4.365 ns | 4.216 ns - 4.526 ns |
| typed_cache_reads | context_slot | 4.115 ns | 3.984 ns - 4.284 ns |
| typed_cache_reads | thread_safe_cell | 25.046 ns | 24.769 ns - 25.344 ns |
| typed_cache_reads | thread_safe_slot | 66.223 ns | 65.057 ns - 67.674 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 2.340 us | 13.940 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 90.000 ns | 900.637 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.590 us | 22.020 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 100 | 49.880 us | 33.560 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 158 | 342.343 us | 66.680 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 291 | 2.304 ms | 162.321 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 556 | 8.348 ms | 267.881 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.510 us | 12.690 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 105 | 40.800 us | 25.110 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 192 | 262.691 us | 53.880 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 348 | 1.877 ms | 124.921 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 676 | 7.353 ms | 244.781 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.580 us | 41.280 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 129 | 69.400 us | 66.111 us | 0 | 0 | 0 | 12 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 214 | 423.844 us | 125.601 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 372 | 2.041 ms | 216.302 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 712 | 5.784 ms | 373.966 us | 0 | 0 | 0 | 1 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.080 us | 29.721 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 145 | 37.520 us | 54.601 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 58 | 0 | 1 | 0 | 0 | 0 | 373 | 159.710 us | 146.622 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 108 | 0 | 1 | 0 | 0 | 0 | 667 | 686.947 us | 293.262 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 232 | 0 | 1 | 0 | 0 | 0 | 1329 | 969.535 us | 547.454 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 1.910 us | 24.770 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 137 | 63.821 us | 61.001 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 255 | 609.634 us | 125.072 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 482 | 3.173 ms | 256.591 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 937 | 14.472 ms | 502.973 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.060 us | 26.571 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 2.060 us | 27.250 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 82 | 9.330 us | 32.540 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 102 | 12.430 us | 39.310 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 109 | 46.711 us | 63.071 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.120 us | 61.010 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 193 | 63.110 us | 107.371 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 30 | 0 | 16 | 0 | 0 | 0 | 346 | 302.601 us | 231.783 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 19 | 0 | 32 | 0 | 0 | 0 | 472 | 2.966 ms | 452.552 us | 0 | 0 | 0 | 20 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 2 | 0 | 64 | 0 | 0 | 0 | 713 | 9.655 ms | 456.623 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 13 | 1 | 447 | 2.220 ms | 273.772 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 8 | 1 | 762 | 7.778 ms | 469.943 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 28 | 1 | 400 | 2.920 ms | 208.722 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 34 | 1 | 700 | 9.387 ms | 350.956 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 33 | 0 | 3 | 1 | 631 | 4.232 ms | 300.643 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 6 | 0 | 65 | 0 | 11 | 1 | 1271 | 15.205 ms | 573.584 us | 0 | 0 | 0 | 5 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 559 | 0 | 64 | 0 | 50 | 1 | 1166 | 32.967 ms | 6.804 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 551 | 0 | 64 | 0 | 49 | 1 | 1410 | 139.617 ms | 12.097 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 506 | 31.831 ms | 5.584 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 768 | 126.032 ms | 10.886 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1478 | 10.715 ms | 694.856 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2831 | 42.850 ms | 1.277 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 71 | 0 | 65 | 0 | 5 | 1 | 675 | 2.428 ms | 395.172 us | 0 | 0 | 0 | 86 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1181 | 10.211 ms | 728.197 us | 0 | 0 | 0 | 137 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 60.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 900.347 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 930.000 ns | 2.530 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 540.000 ns | 18.580 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 64 | 19.370 us | 2.400 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 30.420 us | 30.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 90 | 230.601 us | 3.620 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 111.632 us | 62.320 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 159 | 915.455 us | 6.600 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 70.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.389 ms | 154.961 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 296 | 2.956 ms | 11.590 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 50.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 20.000 ns | 320.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 5.392 ms | 255.501 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 870.000 ns | 1.520 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 80.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 500.000 ns | 10.240 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 66 | 27.340 us | 2.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 50.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 13.240 us | 21.100 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 50.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 113 | 136.991 us | 4.650 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 240.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 100.000 ns | 1.270 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 125.260 us | 46.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 920.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 189 | 599.357 us | 7.610 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 420.000 ns | 1.340 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 220.000 ns | 2.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.276 ms | 111.721 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 210.000 ns | 1.780 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 357 | 2.561 ms | 14.860 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 900.000 ns | 2.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 430.000 ns | 4.770 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 4.791 ms | 219.001 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 410.000 ns | 3.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 1.990 us | 13.540 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 90.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 420.000 ns | 25.920 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 106 | 68.800 us | 32.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 210.000 ns | 3.640 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 12 | 310.000 ns | 29.071 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 184 | 420.824 us | 82.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 420.000 ns | 6.310 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 11 | 2.520 us | 36.771 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 332 | 2.040 ms | 173.102 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 50.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 830.000 ns | 12.550 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 220.000 ns | 30.260 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 644 | 5.782 ms | 328.305 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.710 us | 27.921 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1 | 30.000 ns | 17.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 20.000 ns | 250.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 970.000 ns | 1.320 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 80.000 ns | 210.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 530.000 ns | 13.750 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 470.000 ns | 14.081 us |
| thread_safe_contention_same_slot_write_read_2 | other | 66 | 21.940 us | 2.110 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 13.710 us | 25.690 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 23 | 1.780 us | 26.281 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 124 | 81.300 us | 3.910 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 44 | 20.860 us | 8.551 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 36.740 us | 57.960 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 58 | 20.790 us | 75.891 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 82 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 235 | 303.453 us | 7.560 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 40 | 24.601 us | 10.700 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 294.532 us | 131.911 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 108 | 64.341 us | 142.761 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 155 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 483 | 293.801 us | 14.610 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 60 | 34.221 us | 7.290 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 20.000 ns | 300.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 561.482 us | 221.221 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 232 | 80.011 us | 304.033 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 297 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 890.000 ns | 1.300 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 340.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 520.000 ns | 11.340 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 410.000 ns | 11.580 us |
| thread_safe_contention_independent_slots_2 | other | 67 | 30.280 us | 2.580 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 130.000 ns | 370.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 640.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 17.240 us | 31.101 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 16.121 us | 26.310 us |
| thread_safe_contention_independent_slots_4 | other | 113 | 223.782 us | 5.530 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 260.000 ns | 710.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.250 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 252.682 us | 61.560 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 132.800 us | 56.022 us |
| thread_safe_contention_independent_slots_8 | other | 196 | 1.228 ms | 9.420 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 470.000 ns | 1.400 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 200.000 ns | 2.460 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.256 ms | 122.181 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 689.196 us | 121.130 us |
| thread_safe_contention_independent_slots_16 | other | 363 | 5.121 ms | 17.470 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 920.000 ns | 2.710 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 420.000 ns | 4.940 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 5.099 ms | 240.752 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 4.250 ms | 237.101 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 950.000 ns | 1.430 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 70.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 380.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 540.000 ns | 12.410 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 470.000 ns | 12.061 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 980.000 ns | 1.450 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 50.000 ns | 150.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 550.000 ns | 12.460 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 460.000 ns | 12.860 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 2.570 us | 1.190 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 5.170 us | 2.180 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 630.000 ns | 13.260 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 930.000 ns | 15.600 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 2.320 us | 1.150 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 16 | 5.700 us | 3.710 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 590.000 ns | 12.980 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 3.790 us | 21.170 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 3.250 us | 1.330 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 16 | 41.511 us | 5.030 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 1.010 us | 15.030 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 910.000 ns | 41.351 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 23 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.040 us | 14.210 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 210.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.360 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 440.000 ns | 27.320 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 460.000 ns | 17.910 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 60.880 us | 30.030 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 210.000 ns | 3.330 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 1.200 us | 44.240 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 760.000 ns | 29.591 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 223 | 278.780 us | 73.400 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 620.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 470.000 ns | 10.250 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 31 | 7.040 us | 88.842 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 30 | 16.231 us | 58.671 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 44 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 362 | 2.908 ms | 250.581 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 4 | 15.710 us | 1.780 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 860.000 ns | 14.700 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 20 | 800.000 ns | 89.040 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 19 | 40.330 us | 96.451 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 35 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 644 | 9.653 ms | 396.703 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.680 us | 32.180 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1 | 30.000 ns | 16.980 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 2 | 60.000 ns | 10.420 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 398 | 2.218 ms | 225.892 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 880.000 ns | 10.770 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 17 | 1.170 us | 37.110 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 691 | 7.776 ms | 428.663 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.730 us | 21.920 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 7 | 180.000 ns | 19.360 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 257 | 1.181 ms | 47.040 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 400.000 ns | 9.720 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.739 ms | 151.962 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 3.767 ms | 91.603 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 820.000 ns | 16.371 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.619 ms | 242.982 us |
| thread_safe_effect_contention_batch_flush_8 | other | 593 | 4.230 ms | 264.843 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 60.000 ns | 790.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 940.000 ns | 15.460 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 30.000 ns | 12.360 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 2 | 60.000 ns | 7.190 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1193 | 15.202 ms | 473.983 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 250.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.780 us | 29.760 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 5 | 140.000 ns | 25.920 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 6 | 150.000 ns | 43.671 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 351 | 8.221 ms | 206.201 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.890 us | 5.680 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.710 us | 26.240 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 21.069 ms | 5.961 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 559 | 3.673 ms | 605.302 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 475 | 24.670 ms | 187.660 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.820 us | 6.230 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.720 us | 29.320 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 105.134 ms | 11.355 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 551 | 9.809 ms | 518.653 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 218 | 10.614 ms | 12.320 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.850 us | 9.020 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 860.000 ns | 17.330 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 21.212 ms | 5.507 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.730 us | 37.930 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 352 | 32.514 ms | 15.720 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.790 us | 5.510 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 880.000 ns | 15.100 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 93.514 ms | 10.807 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.870 us | 43.070 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 772 | 4.818 ms | 44.400 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.111 us | 9.540 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.730 us | 27.390 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 5.891 ms | 564.536 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.820 us | 48.990 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1421 | 17.707 ms | 67.470 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.860 us | 16.870 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.480 us | 54.320 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 25.132 ms | 1.044 ms |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.490 us | 94.610 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 409 | 2.399 ms | 204.361 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 126 | 10.770 us | 15.960 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.720 us | 25.580 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 4 | 1.270 us | 86.750 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 71 | 14.920 us | 62.521 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 787 | 10.020 ms | 406.604 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 21.370 us | 15.730 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.380 us | 53.261 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 162.381 us | 158.141 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 3.460 us | 94.461 us |

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
