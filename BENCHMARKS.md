# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.45.0`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.495 ms | 2.915 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.040 ms | 7.679 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 1.862 ms | 2.320 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 5.159 ms | 5.709 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 495.365 us | 530.056 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.203 ms | 1.312 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 3.016 ms | 3.120 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 3.371 ms | 3.835 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.245 ms | 1.515 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.032 ms | 3.480 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.235 ms | 1.579 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.091 ms | 3.611 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.020 ms | 2.215 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.628 ms | 7.204 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.762 ms | 3.837 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.100 ms | 6.413 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.462 ms | 2.591 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.944 ms | 5.208 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.099 ms | 2.688 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.960 ms | 7.655 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.055 ms | 1.256 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.082 ms | 2.272 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 3.597 ns | 3.586 ns - 3.610 ns |
| cached_reads | thread_safe_context | 55.499 ns | 55.342 ns - 55.699 ns |
| cold_first_get | context | 102.980 ns | 89.079 ns - 116.831 ns |
| cold_first_get | thread_safe_context | 964.073 ns | 943.345 ns - 984.276 ns |
| dependency_fan_out | context / 32 | 3.361 us | 2.801 us - 4.303 us |
| dependency_fan_out | context / 256 | 40.320 us | 39.661 us - 40.946 us |
| dependency_fan_out | thread_safe_context / 32 | 21.954 us | 20.804 us - 23.493 us |
| dependency_fan_out | thread_safe_context / 256 | 165.625 us | 162.275 us - 169.164 us |
| set_cell_invalidation | high_fan_out / 512 | 125.457 us | 118.015 us - 133.499 us |
| set_cell_invalidation | same_slot_contention / 1 | 79.535 us | 78.126 us - 80.829 us |
| set_cell_invalidation | same_slot_contention / 2 | 170.771 us | 165.218 us - 178.183 us |
| set_cell_invalidation | same_slot_contention / 4 | 467.422 us | 448.890 us - 486.087 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.321 ms | 1.207 ms - 1.442 ms |
| set_cell_invalidation | same_slot_contention / 16 | 3.094 ms | 2.922 ms - 3.287 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 82.156 us | 79.811 us - 84.720 us |
| set_cell_invalidation | independent_slot_contention / 2 | 174.732 us | 171.713 us - 177.754 us |
| set_cell_invalidation | independent_slot_contention / 4 | 450.875 us | 430.120 us - 472.297 us |
| set_cell_invalidation | independent_slot_contention / 8 | 1.261 ms | 1.178 ms - 1.330 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 2.933 ms | 2.691 ms - 3.163 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 136.674 us | 135.481 us - 138.007 us |
| set_cell_invalidation | batched_write_bursts / 2 | 243.385 us | 227.033 us - 261.672 us |
| set_cell_invalidation | batched_write_bursts / 4 | 516.990 us | 483.351 us - 557.722 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.273 ms | 1.114 ms - 1.422 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 2.921 ms | 2.680 ms - 3.168 ms |
| memo_equality_suppression | context | 1.733 us | 1.491 us - 1.962 us |
| memo_equality_suppression | thread_safe_context | 28.778 us | 28.377 us - 29.209 us |
| effect_flushing | context | 30.153 ns | 29.951 ns - 30.345 ns |
| effect_flushing | thread_safe_context | 895.168 ns | 891.360 ns - 899.514 ns |
| batch_storms | context / 64 | 1.670 us | 1.660 us - 1.678 us |
| batch_storms | thread_safe_context / 64 | 7.468 us | 7.424 us - 7.528 us |
| thread_safe_contention | same_slot_write_read / 1 | 142.331 us | 140.341 us - 144.488 us |
| thread_safe_contention | same_slot_write_read / 2 | 413.656 us | 390.524 us - 437.460 us |
| thread_safe_contention | same_slot_write_read / 4 | 950.018 us | 899.132 us - 1.000 ms |
| thread_safe_contention | same_slot_write_read / 8 | 2.367 ms | 2.108 ms - 2.607 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.292 ms | 5.864 ms - 6.733 ms |
| thread_safe_contention | independent_slots / 1 | 137.585 us | 136.295 us - 138.908 us |
| thread_safe_contention | independent_slots / 2 | 274.993 us | 261.741 us - 288.724 us |
| thread_safe_contention | independent_slots / 4 | 757.748 us | 717.343 us - 791.407 us |
| thread_safe_contention | independent_slots / 8 | 1.950 ms | 1.786 ms - 2.113 ms |
| thread_safe_contention | independent_slots / 16 | 5.046 ms | 4.615 ms - 5.366 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 136.739 us | 135.387 us - 138.087 us |
| thread_safe_contention | read_mostly_waiters / 2 | 161.501 us | 160.115 us - 162.912 us |
| thread_safe_contention | read_mostly_waiters / 4 | 237.076 us | 235.064 us - 239.260 us |
| thread_safe_contention | read_mostly_waiters / 8 | 500.406 us | 490.902 us - 510.593 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.222 ms | 1.181 ms - 1.260 ms |
| thread_safe_contention | batched_write_bursts / 1 | 212.246 us | 209.936 us - 214.212 us |
| thread_safe_contention | batched_write_bursts / 2 | 600.450 us | 572.254 us - 621.025 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.607 ms | 1.593 ms - 1.621 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.004 ms | 2.920 ms - 3.075 ms |
| thread_safe_contention | batched_write_bursts / 16 | 3.451 ms | 3.295 ms - 3.603 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.233 ms | 1.132 ms - 1.331 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.030 ms | 2.856 ms - 3.203 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.269 ms | 1.188 ms - 1.362 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.189 ms | 3.061 ms - 3.327 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.997 ms | 1.879 ms - 2.096 ms |
| thread_safe_effect_contention | batch_flush / 16 | 5.238 ms | 4.647 ms - 5.902 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.752 ms | 3.726 ms - 3.779 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.167 ms | 6.096 ms - 6.246 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.465 ms | 2.430 ms - 2.501 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.983 ms | 4.922 ms - 5.051 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 2.251 ms | 2.091 ms - 2.421 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 6.716 ms | 6.193 ms - 7.187 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.088 ms | 1.026 ms - 1.151 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.060 ms | 1.962 ms - 2.156 ms |
| profile_instrumentation | context_snapshot | 208.430 ns | 207.562 ns - 209.504 ns |
| profile_instrumentation | thread_safe_snapshot | 297.584 us | 295.492 us - 299.389 us |
| async_cached_resolve | async_context | 4.801 us | 4.370 us - 5.310 us |
| async_cached_resolve | sync_context_baseline | 56.523 ns | 56.001 ns - 57.134 ns |
| async_cached_resolve | sync_get | 11.688 ns | 11.613 ns - 11.782 ns |
| async_cached_resolve | thread_safe_context_baseline | 1.324 us | 1.319 us - 1.330 us |
| async_cold_resolve | async_context | 4.572 us | 4.203 us - 4.974 us |
| async_cold_resolve | sync_context_baseline | 92.419 ns | 78.966 ns - 106.260 ns |
| async_cold_resolve | thread_safe_context_baseline | 944.148 ns | 925.259 ns - 964.914 ns |
| async_invalidation_throughput | async_context | 271.839 us | 244.558 us - 305.783 us |
| async_invalidation_throughput | sync_context_baseline | 2.416 us | 2.401 us - 2.431 us |
| async_invalidation_throughput | thread_safe_context_baseline | 54.385 us | 54.163 us - 54.632 us |
| async_cancellation_throughput | async_invalidate_in_flight | 61.061 us | 52.334 us - 69.449 us |
| async_concurrent_contention | async_context / 1 | 70.478 us | 69.824 us - 71.178 us |
| async_concurrent_contention | async_context / 4 | 312.592 us | 278.877 us - 344.379 us |
| async_concurrent_contention | async_context / 16 | 1.609 ms | 1.438 ms - 1.795 ms |
| async_concurrent_contention | thread_safe_context_baseline / 1 | 83.114 us | 82.050 us - 84.324 us |
| async_concurrent_contention | thread_safe_context_baseline / 4 | 677.040 us | 667.452 us - 686.820 us |
| async_concurrent_contention | thread_safe_context_baseline / 16 | 3.794 ms | 3.741 ms - 3.851 ms |
| async_effect_throughput | async_context | 188.207 ms | 188.055 ms - 188.346 ms |
| async_batch_throughput | async_context | 87.236 us | 79.206 us - 95.636 us |
| async_batch_throughput | sync_context_baseline | 6.616 us | 6.582 us - 6.652 us |
| tokio_sync_cached_read | single_task | 1.424 us | 1.416 us - 1.435 us |
| tokio_sync_cached_read | spawn_read | 6.027 us | 5.580 us - 6.494 us |
| tokio_sync_cold_first_get | single_task | 1.363 us | 1.361 us - 1.365 us |
| tokio_sync_cold_first_get | spawn_compute | 6.465 us | 5.718 us - 7.234 us |
| tokio_sync_invalidation | single_task | 54.898 us | 54.664 us - 55.166 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 1 | 61.507 us | 60.703 us - 62.358 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 4 | 476.237 us | 436.598 us - 518.003 us |
| tokio_sync_concurrent_contention | same_slot_write_read / 16 | 4.007 ms | 3.872 ms - 4.108 ms |
| tokio_sync_concurrent_contention | independent_slots / 1 | 63.491 us | 62.598 us - 64.367 us |
| tokio_sync_concurrent_contention | independent_slots / 4 | 413.675 us | 381.244 us - 452.489 us |
| tokio_sync_concurrent_contention | independent_slots / 16 | 3.256 ms | 3.142 ms - 3.346 ms |
| tokio_sync_batch | spawn_batch | 45.645 us | 45.525 us - 45.766 us |
| tokio_sync_effect | single_task | 10.098 ms | 10.094 ms - 10.101 ms |
| scale | build | 106.640 ms | 104.865 ms - 108.666 ms |
| scale | cold_full_recalc | 52.868 ms | 52.118 ms - 53.574 ms |
| scale | full_recalc_invalidate_all | 49.484 ms | 48.605 ms - 50.628 ms |
| scale | viewport_recalc | 3.100 us | 3.095 us - 3.108 us |
| lzspec_base64 | decode_base64 / 64 | 1.083 us | 1.079 us - 1.087 us |
| lzspec_base64 | decode_base64 / 1024 | 4.923 us | 4.835 us - 5.027 us |
| lzspec_base64 | decode_base64 / 16384 | 120.274 us | 106.987 us - 137.555 us |
| lzspec_base64 | decode_json_u8 / 64 | 709.051 ns | 703.125 ns - 715.880 ns |
| lzspec_base64 | decode_json_u8 / 1024 | 5.907 us | 5.860 us - 5.956 us |
| lzspec_base64 | decode_json_u8 / 16384 | 176.945 us | 174.529 us - 179.193 us |
| lzspec_base64 | encode_base64 / 64 | 914.254 ns | 909.090 ns - 919.982 ns |
| lzspec_base64 | encode_base64 / 1024 | 5.088 us | 5.026 us - 5.154 us |
| lzspec_base64 | encode_base64 / 16384 | 75.691 us | 72.440 us - 79.457 us |
| lzspec_base64 | encode_json_u8 / 64 | 279.812 ns | 277.703 ns - 282.329 ns |
| lzspec_base64 | encode_json_u8 / 1024 | 2.345 us | 2.333 us - 2.358 us |
| lzspec_base64 | encode_json_u8 / 16384 | 37.983 us | 34.689 us - 41.999 us |
| lzspec_frontier_suppress | decode_ops_only | 1.045 us | 1.038 us - 1.052 us |
| lzspec_frontier_suppress | decode_with_frontier | 1.599 us | 1.589 us - 1.609 us |
| lzspec_frontier_suppress | encode_ops_only | 458.699 ns | 456.213 ns - 461.442 ns |
| lzspec_frontier_suppress | encode_with_frontier | 734.756 ns | 724.947 ns - 745.814 ns |
| lzspec_intern | decode_inline | 39.166 us | 38.798 us - 39.637 us |
| lzspec_intern | decode_intern | 120.855 us | 120.605 us - 121.117 us |
| lzspec_intern | encode_inline | 17.154 us | 15.205 us - 19.003 us |
| lzspec_intern | encode_intern | 104.213 us | 103.884 us - 104.639 us |
| queue_reactive_shell_overhead | raw_vecdeque_push_pop | 1.115 ns | 1.109 ns - 1.121 ns |
| queue_reactive_shell_overhead | subscribed_len_push_pop | 87.257 ns | 86.985 ns - 87.594 ns |
| queue_reactive_shell_overhead | unsubscribed_push_pop | 15.852 ns | 15.790 ns - 15.916 ns |
| revision_write_cost | push / 1 | 185.513 ns | 185.280 ns - 185.775 ns |
| revision_write_cost | push / 16 | 1.129 us | 1.122 us - 1.138 us |
| revision_write_cost | push / 128 | 10.300 us | 10.251 us - 10.368 us |
| revision_write_cost | push / 1024 | 182.805 us | 181.586 us - 184.573 us |
| revision_write_cost | revision / 1 | 120.690 ns | 120.489 ns - 120.917 ns |
| revision_write_cost | revision / 16 | 799.775 ns | 797.460 ns - 802.249 ns |
| revision_write_cost | revision / 128 | 8.506 us | 8.482 us - 8.533 us |
| revision_write_cost | revision / 1024 | 168.191 us | 167.389 us - 169.024 us |
| revision_write_then_read | push / 1 | 103.631 ns | 103.140 ns - 104.128 ns |
| revision_write_then_read | push / 16 | 1.289 us | 1.284 us - 1.295 us |
| revision_write_then_read | push / 128 | 17.256 us | 17.195 us - 17.319 us |
| revision_write_then_read | push / 1024 | 521.260 us | 520.200 us - 522.424 us |
| revision_write_then_read | revision / 1 | 92.092 ns | 91.741 ns - 92.459 ns |
| revision_write_then_read | revision / 16 | 1.182 us | 1.176 us - 1.188 us |
| revision_write_then_read | revision / 128 | 16.609 us | 16.547 us - 16.676 us |
| revision_write_then_read | revision / 1024 | 515.975 us | 514.874 us - 517.130 us |
| typed_cache_reads | context_cell | 2.167 ns | 2.160 ns - 2.176 ns |
| typed_cache_reads | context_rc_cell | 5.647 ns | 5.622 ns - 5.670 ns |
| typed_cache_reads | context_rc_slot | 6.965 ns | 6.948 ns - 6.982 ns |
| typed_cache_reads | context_slot | 3.573 ns | 3.568 ns - 3.578 ns |
| typed_cache_reads | thread_safe_arc_slot | 63.199 ns | 63.006 ns - 63.425 ns |
| typed_cache_reads | thread_safe_arc_string_slot | 63.076 ns | 62.877 ns - 63.327 ns |
| typed_cache_reads | thread_safe_cell | 24.081 ns | 24.015 ns - 24.161 ns |
| typed_cache_reads | thread_safe_slot | 55.129 ns | 55.100 ns - 55.170 ns |
| typed_cache_reads | thread_safe_string_slot | 72.557 ns | 72.367 ns - 72.781 ns |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 2.090 us | 15.400 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 90.000 ns | 916.158 us | 0 | 0 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.600 us | 23.090 us | 0 | 0 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 100 | 59.580 us | 33.720 us | 0 | 0 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 166 | 367.932 us | 66.870 us | 0 | 0 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 302 | 1.881 ms | 130.390 us | 0 | 0 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 556 | 9.867 ms | 289.163 us | 0 | 0 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 53 | 1.540 us | 13.120 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 104 | 35.520 us | 25.340 us | 0 | 0 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 190 | 284.472 us | 52.661 us | 0 | 0 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 351 | 1.593 ms | 116.273 us | 0 | 0 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 669 | 7.369 ms | 234.972 us | 0 | 0 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 2.900 us | 43.150 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 129 | 67.610 us | 67.170 us | 0 | 0 | 0 | 12 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 199 | 358.453 us | 114.491 us | 0 | 0 | 0 | 6 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 378 | 1.654 ms | 205.251 us | 0 | 0 | 0 | 7 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 724 | 6.681 ms | 411.894 us | 0 | 0 | 0 | 4 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 2.010 us | 29.810 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 22 | 0 | 1 | 0 | 0 | 0 | 138 | 37.410 us | 53.130 us | 0 | 0 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 53 | 0 | 1 | 0 | 0 | 0 | 326 | 175.823 us | 121.960 us | 0 | 0 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 112 | 0 | 1 | 0 | 0 | 0 | 691 | 315.896 us | 247.952 us | 0 | 0 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 220 | 0 | 1 | 0 | 0 | 0 | 1351 | 959.268 us | 541.854 us | 0 | 0 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 68 | 2.150 us | 24.081 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 136 | 40.080 us | 53.850 us | 0 | 0 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 253 | 483.592 us | 107.931 us | 0 | 0 | 0 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 482 | 3.139 ms | 230.432 us | 0 | 0 | 0 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 945 | 14.922 ms | 492.731 us | 0 | 0 | 0 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 72 | 1.990 us | 26.240 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 2.010 us | 27.530 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 92 | 23.760 us | 53.150 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 103 | 42.690 us | 37.820 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 130 | 187.200 us | 64.780 us | 0 | 0 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.110 us | 63.750 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 196 | 69.641 us | 117.391 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 30 | 0 | 16 | 0 | 0 | 0 | 342 | 272.024 us | 207.382 us | 0 | 0 | 0 | 29 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 8 | 0 | 32 | 0 | 0 | 0 | 393 | 1.501 ms | 237.203 us | 0 | 0 | 0 | 8 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 11 | 0 | 64 | 0 | 0 | 0 | 762 | 11.492 ms | 616.795 us | 0 | 0 | 0 | 10 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 9 | 1 | 418 | 1.726 ms | 252.931 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 729 | 5.944 ms | 425.183 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 36 | 1 | 416 | 1.637 ms | 167.050 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 35 | 1 | 700 | 7.946 ms | 328.242 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 4.000 ms | 309.780 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 6 | 0 | 65 | 0 | 9 | 1 | 1270 | 19.699 ms | 667.085 us | 0 | 0 | 0 | 7 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 562 | 0 | 64 | 0 | 50 | 1 | 1169 | 30.085 ms | 6.412 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 555 | 0 | 64 | 0 | 50 | 1 | 1418 | 117.592 ms | 11.675 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 517 | 33.417 ms | 5.577 ms | 0 | 0 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 792 | 113.199 ms | 10.979 ms | 0 | 0 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 1452 | 7.497 ms | 589.074 us | 0 | 0 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 2793 | 37.249 ms | 1.154 ms | 0 | 0 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 140 | 0 | 65 | 0 | 23 | 1 | 1335 | 3.625 ms | 727.894 us | 0 | 0 | 0 | 279 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 140 | 0 | 129 | 0 | 7 | 1 | 1451 | 10.489 ms | 787.078 us | 0 | 0 | 0 | 159 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 915.978 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 36 | 920.000 ns | 1.490 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 40.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 590.000 ns | 20.570 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 64 | 29.310 us | 2.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 30.180 us | 30.580 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 98 | 185.741 us | 3.720 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 182.081 us | 62.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 170 | 804.237 us | 6.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 50.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 20.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.076 ms | 123.140 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 296 | 4.006 ms | 12.630 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 5.861 ms | 275.553 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 34 | 930.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 80.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 15 | 480.000 ns | 10.260 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 65 | 14.250 us | 2.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 130.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 890.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 31 | 21.000 us | 20.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 70.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 111 | 146.060 us | 4.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 200.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 100.000 ns | 1.500 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 63 | 138.012 us | 44.971 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 820.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 192 | 552.591 us | 7.671 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 430.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 220.000 ns | 3.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 127 | 1.040 ms | 102.272 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 190.000 ns | 1.880 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 350 | 2.898 ms | 14.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 890.000 ns | 2.660 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 420.000 ns | 6.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 255 | 4.469 ms | 208.152 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 430.000 ns | 3.490 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.310 us | 14.280 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 40.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 90.000 ns | 1.650 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 430.000 ns | 26.750 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 106 | 66.950 us | 33.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 220.000 ns | 3.970 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 12 | 350.000 ns | 29.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 174 | 352.533 us | 81.621 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 50.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 380.000 ns | 7.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 6 | 5.460 us | 25.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 336 | 1.652 ms | 164.271 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 850.000 ns | 15.000 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 7 | 200.000 ns | 25.570 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 652 | 6.640 ms | 353.694 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.670 us | 33.920 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 5 | 38.491 us | 23.770 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 270.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 970.000 ns | 1.380 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 40.000 ns | 230.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 460.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 540.000 ns | 13.180 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 430.000 ns | 14.560 us |
| thread_safe_contention_same_slot_write_read_2 | other | 62 | 23.800 us | 2.030 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 50.000 ns | 190.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 12.770 us | 25.500 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 22 | 760.000 ns | 25.050 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 114 | 100.052 us | 3.720 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 12 | 8.590 us | 8.940 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 360.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 58.970 us | 51.290 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 53 | 8.181 us | 57.650 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 82 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 241 | 148.884 us | 7.580 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 50 | 18.850 us | 8.101 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 420.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 122.612 us | 100.900 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 112 | 25.520 us | 130.951 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 159 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 481 | 381.323 us | 15.220 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 84 | 33.960 us | 13.290 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 20.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 464.543 us | 220.202 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 220 | 79.422 us | 292.792 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 309 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 34 | 900.000 ns | 1.100 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 50.000 ns | 250.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 440.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 15 | 780.000 ns | 10.820 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 400.000 ns | 11.471 us |
| thread_safe_contention_independent_slots_2 | other | 66 | 17.590 us | 3.260 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 200.000 ns | 1.450 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 1.620 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 31 | 16.640 us | 22.670 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 5.590 us | 24.850 us |
| thread_safe_contention_independent_slots_4 | other | 111 | 223.230 us | 5.140 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 210.000 ns | 680.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 1.590 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 63 | 167.551 us | 48.871 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 92.491 us | 51.650 us |
| thread_safe_contention_independent_slots_8 | other | 196 | 986.837 us | 8.831 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 430.000 ns | 1.270 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 210.000 ns | 2.970 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 127 | 1.349 ms | 106.951 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 803.405 us | 110.410 us |
| thread_safe_contention_independent_slots_16 | other | 371 | 5.023 ms | 17.230 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 890.000 ns | 2.620 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 450.000 ns | 6.050 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 255 | 6.021 ms | 232.231 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 3.876 ms | 234.600 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 930.000 ns | 1.410 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 60.000 ns | 300.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 450.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 560.000 ns | 11.820 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 420.000 ns | 12.260 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 900.000 ns | 1.140 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 400.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 560.000 ns | 11.900 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 460.000 ns | 13.910 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 1.830 us | 1.360 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 12 | 17.170 us | 4.320 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 640.000 ns | 12.930 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 4.090 us | 34.150 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 2.600 us | 1.270 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 18 | 32.850 us | 5.500 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 390.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 1.160 us | 12.710 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 6.050 us | 17.950 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 22.920 us | 1.480 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 25 | 136.730 us | 8.050 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 400.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 13.110 us | 17.500 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 14.420 us | 37.350 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 35 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.090 us | 14.430 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 330.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 100.000 ns | 1.760 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 420.000 ns | 29.400 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 440.000 ns | 17.830 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 67.570 us | 30.180 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 40.000 ns | 230.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 200.000 ns | 4.900 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 1.101 us | 49.811 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 730.000 ns | 32.270 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 220 | 263.324 us | 67.340 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 1.160 us | 980.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 400.000 ns | 7.590 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 29 | 1.950 us | 74.662 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 30 | 5.190 us | 56.810 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 43 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 338 | 1.499 ms | 163.701 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 830.000 ns | 20.031 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 8 | 460.000 ns | 26.480 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 8 | 240.000 ns | 26.811 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 662 | 11.482 ms | 409.024 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 50.000 ns | 200.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.650 us | 36.070 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 10 | 1.070 us | 74.801 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 11 | 8.060 us | 96.700 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 376 | 1.724 ms | 204.541 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 810.000 ns | 11.950 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 10 | 960.000 ns | 36.440 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 662 | 5.942 ms | 381.903 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.650 us | 23.240 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 170.000 ns | 20.040 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 273 | 653.645 us | 56.120 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 420.000 ns | 9.180 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 983.405 us | 101.750 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 3.313 ms | 94.101 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 840.000 ns | 15.690 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 4.632 ms | 218.451 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 3.999 ms | 263.140 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 50.000 ns | 530.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 850.000 ns | 16.930 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 40.000 ns | 17.180 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 90.000 ns | 12.000 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1190 | 19.689 ms | 554.414 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 50.000 ns | 170.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.750 us | 34.781 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 7 | 7.480 us | 41.430 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 6 | 240.000 ns | 36.290 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 351 | 4.187 ms | 178.412 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 1.750 us | 5.720 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.610 us | 31.391 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 128 | 21.023 ms | 5.694 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 562 | 4.872 ms | 502.505 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 479 | 19.571 ms | 188.472 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.780 us | 5.830 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.680 us | 31.600 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 256 | 87.955 ms | 10.955 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 555 | 10.062 ms | 494.527 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 229 | 8.118 ms | 13.330 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.820 us | 6.470 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 980.000 ns | 19.790 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | set_cell_invalidation | 128 | 25.294 ms | 5.498 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 1.670 us | 39.871 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 376 | 26.520 ms | 15.200 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.610 us | 5.520 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 830.000 ns | 17.190 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | set_cell_invalidation | 256 | 86.674 ms | 10.905 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 1.780 us | 36.110 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 746 | 2.561 ms | 33.450 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.030 us | 8.730 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.680 us | 30.830 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | set_cell_invalidation | 508 | 4.931 ms | 466.883 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 1.780 us | 49.181 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 1383 | 15.050 ms | 60.612 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 3.611 us | 13.680 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.320 us | 60.220 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | set_cell_invalidation | 1020 | 22.189 ms | 928.207 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 3.340 us | 91.161 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 466 | 3.311 ms | 192.793 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 648 | 18.260 us | 84.120 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.700 us | 28.020 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 16 | 520.000 ns | 258.620 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 140 | 293.700 us | 164.341 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 800 | 10.453 ms | 415.125 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 376 | 10.670 us | 44.980 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.320 us | 60.270 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 6 | 5.410 us | 152.961 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 140 | 16.270 us | 113.742 us |

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
