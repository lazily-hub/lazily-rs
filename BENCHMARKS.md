# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.6.0`.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.156 ms | 2.444 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.835 ms | 8.182 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 969.303 us | 1.042 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 2.263 ms | 2.482 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 532.516 us | 741.880 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.242 ms | 1.522 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.780 ms | 2.896 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 4.085 ms | 4.350 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.291 ms | 1.376 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.424 ms | 3.508 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.307 ms | 1.495 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.305 ms | 3.814 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 2.180 ms | 2.506 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 6.538 ms | 7.617 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.598 ms | 6.031 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.742 ms | 5.860 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.867 ms | 1.975 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.714 ms | 3.892 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 444.323 us | 476.591 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 900.385 us | 917.957 us | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.076 ms | 1.123 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.948 ms | 2.199 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 8.299 ns | 8.289 ns - 8.309 ns |
| cached_reads | thread_safe_context | 64.171 ns | 64.080 ns - 64.271 ns |
| cold_first_get | context | 106.867 ns | 94.820 ns - 118.994 ns |
| cold_first_get | thread_safe_context | 1.016 us | 977.334 ns - 1.056 us |
| dependency_fan_out | context / 32 | 3.905 us | 3.492 us - 4.393 us |
| dependency_fan_out | context / 256 | 50.806 us | 48.048 us - 55.439 us |
| dependency_fan_out | thread_safe_context / 32 | 22.994 us | 21.887 us - 24.166 us |
| dependency_fan_out | thread_safe_context / 256 | 164.520 us | 161.160 us - 168.050 us |
| set_cell_invalidation | high_fan_out / 512 | 109.043 us | 102.996 us - 114.722 us |
| set_cell_invalidation | same_slot_contention / 1 | 45.191 us | 44.709 us - 45.704 us |
| set_cell_invalidation | same_slot_contention / 2 | 88.710 us | 87.104 us - 90.342 us |
| set_cell_invalidation | same_slot_contention / 4 | 195.867 us | 190.139 us - 202.147 us |
| set_cell_invalidation | same_slot_contention / 8 | 557.906 us | 546.057 us - 570.184 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.816 ms | 1.784 ms - 1.845 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 44.363 us | 44.046 us - 44.716 us |
| set_cell_invalidation | independent_slot_contention / 2 | 75.403 us | 74.231 us - 76.532 us |
| set_cell_invalidation | independent_slot_contention / 4 | 121.770 us | 118.989 us - 124.599 us |
| set_cell_invalidation | independent_slot_contention / 8 | 227.897 us | 224.668 us - 231.175 us |
| set_cell_invalidation | independent_slot_contention / 16 | 435.648 us | 428.046 us - 443.577 us |
| set_cell_invalidation | batched_write_bursts / 1 | 136.483 us | 135.395 us - 137.547 us |
| set_cell_invalidation | batched_write_bursts / 2 | 225.439 us | 217.158 us - 233.929 us |
| set_cell_invalidation | batched_write_bursts / 4 | 481.592 us | 466.123 us - 499.581 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.289 ms | 1.257 ms - 1.330 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.380 ms | 3.296 ms - 3.466 ms |
| memo_equality_suppression | context | 2.625 us | 2.241 us - 3.132 us |
| memo_equality_suppression | thread_safe_context | 34.114 us | 33.142 us - 35.237 us |
| effect_flushing | context | 51.235 ns | 50.999 ns - 51.486 ns |
| effect_flushing | thread_safe_context | 900.610 ns | 899.957 ns - 901.274 ns |
| batch_storms | context / 64 | 2.742 us | 2.730 us - 2.755 us |
| batch_storms | thread_safe_context / 64 | 6.794 us | 6.778 us - 6.814 us |
| thread_safe_contention | same_slot_write_read / 1 | 107.010 us | 106.284 us - 107.765 us |
| thread_safe_contention | same_slot_write_read / 2 | 295.370 us | 290.461 us - 301.318 us |
| thread_safe_contention | same_slot_write_read / 4 | 758.597 us | 724.198 us - 789.390 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.184 ms | 2.115 ms - 2.262 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.154 ms | 6.841 ms - 7.505 ms |
| thread_safe_contention | independent_slots / 1 | 104.769 us | 103.322 us - 105.879 us |
| thread_safe_contention | independent_slots / 2 | 191.058 us | 183.742 us - 201.832 us |
| thread_safe_contention | independent_slots / 4 | 423.420 us | 416.056 us - 431.355 us |
| thread_safe_contention | independent_slots / 8 | 971.196 us | 951.097 us - 991.911 us |
| thread_safe_contention | independent_slots / 16 | 2.286 ms | 2.223 ms - 2.350 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 105.621 us | 105.150 us - 106.126 us |
| thread_safe_contention | read_mostly_waiters / 2 | 154.383 us | 152.820 us - 156.012 us |
| thread_safe_contention | read_mostly_waiters / 4 | 246.598 us | 245.225 us - 247.940 us |
| thread_safe_contention | read_mostly_waiters / 8 | 556.276 us | 528.168 us - 600.703 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.322 ms | 1.251 ms - 1.401 ms |
| thread_safe_contention | batched_write_bursts / 1 | 213.639 us | 212.433 us - 214.723 us |
| thread_safe_contention | batched_write_bursts / 2 | 578.822 us | 561.737 us - 595.179 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.545 ms | 1.532 ms - 1.556 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.796 ms | 2.752 ms - 2.840 ms |
| thread_safe_contention | batched_write_bursts / 16 | 4.067 ms | 3.935 ms - 4.190 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 1.303 ms | 1.276 ms - 1.330 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.404 ms | 3.331 ms - 3.466 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.307 ms | 1.247 ms - 1.367 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 3.408 ms | 3.317 ms - 3.520 ms |
| thread_safe_effect_contention | batch_flush / 8 | 2.219 ms | 2.127 ms - 2.315 ms |
| thread_safe_effect_contention | batch_flush / 16 | 6.619 ms | 6.317 ms - 6.934 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.914 ms | 3.592 ms - 4.416 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.754 ms | 5.718 ms - 5.792 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.881 ms | 1.852 ms - 1.912 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.696 ms | 3.606 ms - 3.780 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 445.189 us | 435.848 us - 455.246 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 898.080 us | 890.965 us - 904.792 us |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.063 ms | 1.037 ms - 1.087 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 2.013 ms | 1.963 ms - 2.070 ms |
| profile_instrumentation | context_snapshot | 267.654 ns | 266.048 ns - 269.225 ns |
| profile_instrumentation | thread_safe_snapshot | 299.793 us | 297.991 us - 301.286 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 11.971 us | 12.400 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 1.310 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 1.090 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 910.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 350.000 ns | 4.810 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 360.000 ns | 4.630 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 370.000 ns | 4.780 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 520.000 ns | 1.960 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.130 us | 8.121 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.880 us | 7.450 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.530 us | 15.011 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.680 us | 49.651 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 134 | 73.440 us | 72.421 us | 0 | 0 | 0 | 14 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 196 | 379.272 us | 120.641 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 1.271 ms | 183.734 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 5.270 ms | 364.975 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.060 us | 17.371 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 31 | 0 | 1 | 0 | 0 | 0 | 70 | 6.790 us | 79.961 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 53 | 0 | 1 | 0 | 0 | 0 | 134 | 6.160 us | 80.802 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 97 | 0 | 1 | 0 | 0 | 0 | 277 | 5.820 us | 201.333 us | 128 | 128 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 225 | 0 | 1 | 0 | 0 | 0 | 587 | 27.430 us | 304.333 us | 256 | 256 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 1.050 us | 12.510 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 53 | 24.000 us | 37.690 us | 29 | 29 | 2 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 121 | 388.594 us | 97.292 us | 54 | 54 | 9 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 234 | 2.127 ms | 191.231 us | 110 | 110 | 17 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 463 | 11.185 ms | 387.342 us | 223 | 223 | 32 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.050 us | 16.240 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 930.000 ns | 12.980 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 33 | 5.300 us | 28.542 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 48 | 121.510 us | 36.110 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 50 | 102.010 us | 37.000 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.370 us | 63.081 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 21 | 0 | 8 | 0 | 0 | 0 | 185 | 66.302 us | 105.831 us | 0 | 0 | 0 | 20 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 37 | 0 | 16 | 0 | 0 | 0 | 388 | 339.366 us | 250.082 us | 0 | 0 | 0 | 42 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 4 | 0 | 32 | 0 | 0 | 0 | 370 | 1.391 ms | 202.561 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 4 | 0 | 64 | 0 | 0 | 0 | 722 | 5.477 ms | 407.545 us | 0 | 0 | 0 | 3 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 7 | 1 | 406 | 1.936 ms | 286.402 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 728 | 6.341 ms | 388.384 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 35 | 1 | 411 | 1.912 ms | 180.650 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 31 | 1 | 692 | 10.453 ms | 340.033 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 4.056 ms | 318.174 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 20.260 ms | 614.978 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 555 | 0 | 64 | 0 | 50 | 1 | 1128 | 26.668 ms | 5.985 ms | 17 | 544 | 111 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 552 | 0 | 64 | 0 | 49 | 1 | 1147 | 43.768 ms | 6.832 ms | 132 | 4224 | 124 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.930 us | 61.142 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.520 us | 58.500 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 12.530 us | 99.141 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 24.391 us | 211.753 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 81 | 0 | 65 | 0 | 9 | 1 | 817 | 1.892 ms | 425.106 us | 0 | 0 | 0 | 112 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1177 | 10.862 ms | 702.979 us | 0 | 0 | 0 | 129 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 120.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 130.000 ns | 270.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 50.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 50.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 100.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 50.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 180.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 110.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 1.140 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 180.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 110.000 ns | 1.180 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 50.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 20.000 ns | 1.100 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 160.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 110.000 ns | 1.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 1.190 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 50.000 ns | 1.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 210.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 130.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 80.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 100.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 450.000 ns | 2.430 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 350.000 ns | 1.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 170.000 ns | 2.081 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 160.000 ns | 1.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.000 us | 1.820 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 600.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 290.000 ns | 2.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 990.000 ns | 2.030 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.040 us | 4.140 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.160 us | 2.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 580.000 ns | 4.601 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 750.000 ns | 3.900 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.700 us | 14.790 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 190.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 160.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 710.000 ns | 33.151 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 220.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 109 | 71.830 us | 31.740 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 160.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 310.000 ns | 3.710 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 14 | 1.170 us | 36.551 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 172 | 378.242 us | 84.021 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 590.000 ns | 6.210 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 310.000 ns | 29.980 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 1.270 ms | 161.424 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 150.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.380 us | 12.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 130.000 ns | 9.870 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 60.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 5.267 ms | 326.265 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.770 us | 27.420 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 100.000 ns | 10.860 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 60.000 ns | 260.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 100.000 ns | 670.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 80.000 ns | 200.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 850.000 ns | 16.191 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 160.000 ns | 1.210 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 14 | 4.050 us | 5.340 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 1.120 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 31 | 2.540 us | 72.291 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 20 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 110.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 10 | 3.010 us | 3.760 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 50.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 53 | 2.990 us | 76.382 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 66 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 4 | 100.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 28 | 1.250 us | 5.030 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 50.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_8 | publish | 97 | 4.420 us | 195.663 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 147 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 4 | 150.000 ns | 320.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 66 | 16.120 us | 13.890 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 290.000 ns |
| thread_safe_contention_same_slot_write_read_16 | publish | 225 | 11.130 us | 289.833 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 291 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 190.000 ns | 570.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 190.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 50.000 ns | 330.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 730.000 ns | 11.420 us |
| thread_safe_contention_independent_slots_2 | other | 12 | 330.000 ns | 1.080 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 150.000 ns | 330.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 110.000 ns | 630.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 2 | 240.000 ns | 7.330 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 23.170 us | 28.320 us |
| thread_safe_contention_independent_slots_4 | other | 33 | 76.211 us | 2.720 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 300.000 ns | 640.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 160.000 ns | 1.170 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 9 | 31.150 us | 17.300 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 280.773 us | 75.462 us |
| thread_safe_contention_independent_slots_8 | other | 58 | 249.432 us | 6.010 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 660.000 ns | 1.500 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 340.000 ns | 3.310 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 17 | 231.183 us | 34.770 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.645 ms | 145.641 us |
| thread_safe_contention_independent_slots_16 | other | 112 | 1.119 ms | 8.971 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.210 us | 2.440 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 750.000 ns | 5.070 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 32 | 1.784 ms | 61.430 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 8.279 ms | 309.431 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 2.110 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 90.000 ns | 480.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 1.390 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 800.000 ns | 12.260 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 120.000 ns | 260.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 80.000 ns | 190.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 710.000 ns | 12.220 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 120.000 ns | 381.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 4 | 3.620 us | 2.760 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 280.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 1.530 us | 25.121 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 150.000 ns | 460.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 16 | 120.470 us | 5.270 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 50.000 ns | 320.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 840.000 ns | 30.060 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 100.000 ns | 490.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 12 | 84.780 us | 6.720 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 60.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 17.070 us | 29.370 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.770 us | 14.370 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 240.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 190.000 ns | 1.370 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 740.000 ns | 30.141 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 590.000 ns | 16.960 us |
| thread_safe_contention_batched_write_bursts_2 | other | 122 | 61.042 us | 30.220 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 150.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 310.000 ns | 3.630 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 20 | 4.210 us | 44.781 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 21 | 680.000 ns | 27.050 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 241 | 311.346 us | 66.630 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 6 | 6.080 us | 2.540 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 710.000 ns | 6.880 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 42 | 10.130 us | 100.821 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 37 | 11.100 us | 73.211 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 46 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 328 | 1.386 ms | 166.161 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.460 us | 13.860 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 3 | 2.590 us | 10.770 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 4 | 180.000 ns | 11.610 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 648 | 5.469 ms | 341.164 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 370.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.920 us | 28.450 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 3 | 4.690 us | 18.371 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 4 | 150.000 ns | 19.190 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 366 | 1.902 ms | 250.561 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.400 us | 9.920 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 8 | 32.511 us | 25.921 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 661 | 6.338 ms | 347.344 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.760 us | 19.710 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 180.000 ns | 21.330 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 268 | 707.221 us | 59.980 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 680.000 ns | 8.010 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.204 ms | 112.660 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 5.091 ms | 93.270 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.130 us | 12.131 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.361 ms | 234.632 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 4.054 ms | 277.654 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 80.000 ns | 730.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.410 us | 13.610 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 70.000 ns | 13.810 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 90.000 ns | 12.370 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 20.257 ms | 555.418 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 70.000 ns | 640.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.440 us | 30.190 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 40.000 ns | 15.380 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 70.000 ns | 13.350 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 334 | 9.957 ms | 176.372 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.690 us | 5.160 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.230 us | 25.691 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 111 | 12.295 ms | 5.257 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 555 | 4.410 ms | 520.446 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 343 | 13.595 ms | 167.211 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.660 us | 4.830 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.100 us | 23.960 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 124 | 24.755 ms | 6.135 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 552 | 5.414 ms | 501.646 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.690 us | 5.230 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.680 us | 5.191 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.330 us | 14.900 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 3.230 us | 35.821 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.680 us | 4.250 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.400 us | 4.770 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.280 us | 14.170 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 3.160 us | 35.310 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.040 us | 13.090 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.350 us | 10.360 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.030 us | 24.741 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 3.110 us | 50.950 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 8.090 us | 18.850 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 5.620 us | 13.581 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 4.440 us | 51.821 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 6.241 us | 127.501 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 422 | 1.867 ms | 189.923 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 242 | 10.000 us | 27.850 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.340 us | 23.370 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 7 | 340.000 ns | 101.682 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 81 | 11.940 us | 82.281 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 785 | 10.845 ms | 406.925 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 6.350 us | 13.440 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.800 us | 49.751 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 1 | 50.000 ns | 139.892 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 6.441 us | 92.971 us |

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

