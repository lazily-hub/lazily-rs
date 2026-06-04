# lazily Benchmark Results

Generated benchmark data for the [lazily](https://crates.io/crates/lazily) reactive primitives library.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.5.1`.

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
| cached_reads | context | 8.252 ns | 8.240 ns - 8.265 ns |
| cached_reads | thread_safe_context | 64.349 ns | 64.221 ns - 64.479 ns |
| cold_first_get | context | 99.465 ns | 91.290 ns - 107.820 ns |
| cold_first_get | thread_safe_context | 1.110 us | 1.047 us - 1.174 us |
| dependency_fan_out | context / 32 | 4.227 us | 3.727 us - 4.866 us |
| dependency_fan_out | context / 256 | 50.285 us | 48.124 us - 53.128 us |
| dependency_fan_out | thread_safe_context / 32 | 22.499 us | 21.617 us - 23.436 us |
| dependency_fan_out | thread_safe_context / 256 | 164.375 us | 160.723 us - 168.326 us |
| set_cell_invalidation | high_fan_out / 512 | 104.541 us | 98.798 us - 109.418 us |
| set_cell_invalidation | same_slot_contention / 1 | 47.350 us | 45.968 us - 48.899 us |
| set_cell_invalidation | same_slot_contention / 2 | 108.846 us | 104.716 us - 113.491 us |
| set_cell_invalidation | same_slot_contention / 4 | 211.307 us | 204.051 us - 218.140 us |
| set_cell_invalidation | same_slot_contention / 8 | 545.710 us | 533.443 us - 556.715 us |
| set_cell_invalidation | same_slot_contention / 16 | 2.203 ms | 2.151 ms - 2.258 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 43.492 us | 41.716 us - 44.906 us |
| set_cell_invalidation | independent_slot_contention / 2 | 73.240 us | 71.121 us - 75.069 us |
| set_cell_invalidation | independent_slot_contention / 4 | 141.877 us | 135.753 us - 148.315 us |
| set_cell_invalidation | independent_slot_contention / 8 | 244.257 us | 241.227 us - 246.808 us |
| set_cell_invalidation | independent_slot_contention / 16 | 489.280 us | 482.721 us - 495.869 us |
| set_cell_invalidation | batched_write_bursts / 1 | 141.426 us | 139.235 us - 143.810 us |
| set_cell_invalidation | batched_write_bursts / 2 | 224.765 us | 219.578 us - 230.737 us |
| set_cell_invalidation | batched_write_bursts / 4 | 482.013 us | 472.082 us - 491.829 us |
| set_cell_invalidation | batched_write_bursts / 8 | 1.245 ms | 1.199 ms - 1.292 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 3.465 ms | 3.379 ms - 3.550 ms |
| memo_equality_suppression | context | 2.509 us | 2.182 us - 2.889 us |
| memo_equality_suppression | thread_safe_context | 33.606 us | 32.855 us - 34.334 us |
| effect_flushing | context | 50.696 ns | 50.472 ns - 50.961 ns |
| effect_flushing | thread_safe_context | 901.972 ns | 899.232 ns - 905.047 ns |
| batch_storms | context / 64 | 2.875 us | 2.861 us - 2.891 us |
| batch_storms | thread_safe_context / 64 | 6.773 us | 6.754 us - 6.796 us |
| thread_safe_contention | same_slot_write_read / 1 | 106.711 us | 105.842 us - 107.667 us |
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 13.430 us | 16.600 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 2.270 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 1.030 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 340.000 ns | 970.000 ns | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 960.000 ns | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 320.000 ns | 7.010 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 280.000 ns | 6.640 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 540.000 ns | 2.140 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.100 us | 3.860 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.100 us | 8.760 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.800 us | 16.931 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.040 us | 52.620 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 132 | 96.261 us | 83.931 us | 0 | 0 | 0 | 13 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 187 | 522.234 us | 127.251 us | 0 | 0 | 0 | 2 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 381 | 2.025 ms | 230.411 us | 0 | 0 | 0 | 8 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 715 | 9.947 ms | 437.342 us | 0 | 0 | 0 | 2 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 770.000 ns | 19.840 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 29 | 0 | 1 | 0 | 0 | 0 | 54 | 2.170 us | 36.780 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 50 | 0 | 1 | 0 | 0 | 0 | 107 | 11.790 us | 243.312 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 104 | 0 | 1 | 0 | 0 | 0 | 292 | 26.301 us | 261.430 us | 127 | 127 | 1 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 194 | 0 | 1 | 0 | 0 | 0 | 561 | 45.502 us | 509.881 us | 255 | 255 | 1 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 710.000 ns | 13.530 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 53 | 23.520 us | 37.321 us | 29 | 29 | 2 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 126 | 298.082 us | 87.570 us | 52 | 52 | 11 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 244 | 1.937 ms | 182.710 us | 107 | 107 | 20 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 482 | 9.468 ms | 383.253 us | 215 | 215 | 40 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 690.000 ns | 13.780 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 25 | 750.000 ns | 16.300 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 38 | 2.180 us | 18.980 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 66 | 64.350 us | 46.520 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 72 | 47.360 us | 53.410 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 3.850 us | 60.840 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 189 | 61.490 us | 112.520 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 46 | 0 | 16 | 0 | 0 | 0 | 415 | 330.883 us | 329.632 us | 0 | 0 | 0 | 46 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 86 | 0 | 32 | 0 | 0 | 0 | 837 | 916.537 us | 672.068 us | 0 | 0 | 0 | 91 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 2 | 0 | 64 | 0 | 0 | 0 | 713 | 9.322 ms | 436.592 us | 0 | 0 | 0 | 1 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 2 | 1 | 368 | 2.262 ms | 223.752 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 4 | 1 | 733 | 9.468 ms | 451.251 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 32 | 1 | 412 | 2.267 ms | 204.681 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 32 | 1 | 700 | 10.768 ms | 346.643 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 6 | 0 | 33 | 0 | 9 | 1 | 658 | 2.861 ms | 281.201 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 65 | 0 | 3 | 1 | 1239 | 17.646 ms | 571.403 us | 0 | 0 | 0 | 1 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 559 | 0 | 64 | 0 | 50 | 1 | 1138 | 25.043 ms | 6.080 ms | 14 | 448 | 114 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 561 | 0 | 64 | 0 | 50 | 1 | 1190 | 48.154 ms | 7.547 ms | 117 | 3744 | 139 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.840 us | 61.850 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.010 us | 59.541 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 10.970 us | 95.251 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 22.760 us | 169.223 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 71 | 0 | 65 | 0 | 5 | 1 | 673 | 2.424 ms | 398.771 us | 0 | 0 | 0 | 78 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1313 | 9.860 ms | 728.257 us | 0 | 0 | 0 | 138 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 120.000 ns | 1.360 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 20.000 ns | 310.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 130.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 80.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 20.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 220.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 80.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 20.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 110.000 ns | 200.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 40.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 190.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 80.000 ns | 2.130 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 20.000 ns | 1.530 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 140.000 ns | 2.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 1.560 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 20.000 ns | 1.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 280.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 70.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 590.000 ns | 1.270 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 300.000 ns | 660.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 110.000 ns | 1.090 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 100.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.100 us | 2.630 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 510.000 ns | 1.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 250.000 ns | 2.640 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 240.000 ns | 1.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.460 us | 4.870 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.270 us | 3.030 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 540.000 ns | 5.240 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 530.000 ns | 3.791 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 3.030 us | 16.230 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 740.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 1.720 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 810.000 ns | 33.260 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 108 | 94.631 us | 35.331 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 3.550 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 13 | 1.270 us | 44.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 40.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 166 | 514.173 us | 99.290 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 110.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 531.000 ns | 7.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 2 | 7.380 us | 19.991 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 338 | 2.023 ms | 184.111 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 90.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.180 us | 13.530 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 8 | 390.000 ns | 32.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 50.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 9.944 ms | 386.351 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 250.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.220 us | 29.461 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 100.000 ns | 20.950 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 120.000 ns | 1.280 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 120.000 ns | 1.180 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 1.040 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 500.000 ns | 16.340 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 90.000 ns | 330.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 1.130 us | 1.070 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 20.000 ns | 270.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 29 | 930.000 ns | 35.110 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 130.000 ns | 530.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 10 | 6.970 us | 5.570 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 400.000 ns |
| thread_safe_contention_same_slot_write_read_4 | publish | 50 | 4.670 us | 236.812 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 42 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 290.000 ns | 640.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 25 | 7.110 us | 5.490 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 20.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 5.281 us | 8.680 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 104 | 13.600 us | 246.310 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 155 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 7.630 us | 680.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 49 | 29.272 us | 16.500 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 340.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 120.000 ns | 3.260 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 194 | 8.450 us | 489.101 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 310 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 160.000 ns | 690.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 60.000 ns | 300.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 50.000 ns | 430.000 ns |
| thread_safe_contention_independent_slots_1 | publish | 16 | 440.000 ns | 12.110 us |
| thread_safe_contention_independent_slots_2 | other | 12 | 3.630 us | 1.170 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 120.000 ns | 360.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 70.000 ns | 570.000 ns |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 2 | 220.000 ns | 4.590 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 19.480 us | 30.631 us |
| thread_safe_contention_independent_slots_4 | other | 36 | 58.531 us | 2.600 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 330.000 ns | 710.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 150.000 ns | 1.250 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 11 | 39.370 us | 17.900 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 199.701 us | 65.110 us |
| thread_safe_contention_independent_slots_8 | other | 65 | 316.493 us | 4.700 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 630.000 ns | 1.210 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 250.000 ns | 2.270 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 20 | 205.470 us | 33.150 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.414 ms | 141.380 us |
| thread_safe_contention_independent_slots_16 | other | 123 | 1.009 ms | 8.210 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.210 us | 2.570 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 580.000 ns | 5.040 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 40 | 1.428 ms | 70.781 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 7.030 ms | 296.652 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 100.000 ns | 650.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 70.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 420.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 490.000 ns | 12.370 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 130.000 ns | 1.060 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 70.000 ns | 480.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 1.030 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 520.000 ns | 13.730 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 100.000 ns | 250.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 8 | 1.530 us | 1.210 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 330.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 520.000 ns | 17.190 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 8 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 250.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 21 | 62.590 us | 10.210 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 320.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | publish | 18 | 1.610 us | 35.740 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 22 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 17 | 37.890 us | 4.630 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 290.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 9.330 us | 48.130 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 32 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.480 us | 13.560 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 180.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 1.410 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 710.000 ns | 27.860 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 480.000 ns | 17.830 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 57.800 us | 29.280 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 160.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 3.590 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.810 us | 51.230 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 1.560 us | 28.260 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 253 | 308.333 us | 81.421 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 410.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 540.000 ns | 7.890 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 46 | 6.660 us | 125.430 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 46 | 15.270 us | 114.481 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 52 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 502 | 860.447 us | 137.050 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 12 | 12.910 us | 6.110 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.170 us | 13.811 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 91 | 14.000 us | 214.912 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 86 | 28.010 us | 300.185 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 114 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 644 | 9.320 ms | 382.962 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 100.000 ns | 330.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.331 us | 30.390 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1 | 50.000 ns | 13.480 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 2 | 60.000 ns | 9.430 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 335 | 2.261 ms | 206.102 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 900.000 ns | 9.530 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 1 | 30.000 ns | 8.120 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 666 | 9.466 ms | 414.421 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.900 us | 20.580 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 3 | 140.000 ns | 16.250 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 269 | 892.767 us | 66.620 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 540.000 ns | 10.400 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 1.374 ms | 127.661 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 413 | 4.779 ms | 101.563 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 990.000 ns | 14.900 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 5.988 ms | 230.180 us |
| thread_safe_effect_contention_batch_flush_8 | other | 612 | 2.859 ms | 224.261 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 80.000 ns | 550.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.090 us | 13.970 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 5 | 280.000 ns | 21.140 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 6 | 170.000 ns | 21.280 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1169 | 17.643 ms | 519.422 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 80.000 ns | 260.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.140 us | 30.730 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 40.000 ns | 11.640 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 2 | 70.000 ns | 9.351 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 337 | 6.505 ms | 175.000 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.840 us | 4.700 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 1.880 us | 25.270 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 114 | 13.972 ms | 5.373 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 559 | 4.562 ms | 501.714 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 362 | 6.444 ms | 191.431 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.790 us | 5.060 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.870 us | 25.680 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 139 | 35.448 ms | 6.798 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 561 | 6.258 ms | 527.133 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.120 us | 5.660 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.520 us | 4.990 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 1.040 us | 15.640 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.160 us | 35.560 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 2.000 us | 4.361 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.550 us | 4.710 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.110 us | 14.490 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.350 us | 35.980 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.010 us | 10.570 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.990 us | 8.420 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.900 us | 26.970 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.070 us | 49.291 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 9.020 us | 14.340 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 5.750 us | 12.851 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.820 us | 49.681 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 4.170 us | 92.351 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 408 | 2.407 ms | 208.170 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 126 | 5.950 us | 16.460 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.940 us | 31.850 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 3 | 3.830 us | 82.541 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 71 | 5.160 us | 59.750 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 792 | 9.675 ms | 403.224 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 10.490 us | 25.851 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.750 us | 50.270 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 3 | 161.341 us | 145.691 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 9.520 us | 103.221 us |

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

