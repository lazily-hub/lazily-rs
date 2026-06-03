# lazily

Lazy reactive primitives for Rust — Context, Slots, Cells with automatic dependency tracking and cache invalidation.

[![crates.io](https://img.shields.io/crates/v/lazily.svg)](https://crates.io/crates/lazily)

## Overview

`lazily` provides four core primitives for lazy reactive computation:

- **Context** — owns all reactive state and manages the dependency graph
- **Slot** — a lazily-computed cached value that automatically tracks dependencies
- **Cell** — a mutable value that invalidates dependent Slots when changed
- **Effect** — a side-effect callback that automatically reruns after tracked dependencies invalidate

Values are **lazy**: dependents are marked dirty on invalidation but only validated or recomputed when accessed. This contrasts with eager "signal" systems that recompute immediately.
`ctx.memo()` Slots use a memo guard: if recomputation produces the same value, downstream dirty caches and effects are left alone.
Multiple updates can be grouped with `ctx.batch(...)` so invalidation and effect reruns happen once after the outermost batch exits.

## Development

Run the local CI-equivalent suite with:

```bash
make check
```

The Makefile also exposes focused targets such as `make test-tokio`,
`make test-loom`, `make benchmark-check`, and `make benchmark-update`.

## Usage

```rust
use lazily::Context;

let ctx = Context::new();

// Create a mutable cell
let counter = ctx.cell(0i32);

// Create a derived value (automatically tracks dependencies)
let doubled = ctx.computed(|ctx| {
    let val = ctx.get_cell(&counter);
    val * 2
});

assert_eq!(ctx.get(&doubled), 0);

// Mutate the cell — dependents are marked dirty (not recomputed yet)
counter.set(&ctx, 5);

// Slot recomputes lazily on next access
assert_eq!(ctx.get(&doubled), 10);

// Effects run immediately and then after tracked dependencies change
let effect = ctx.effect(move |ctx| {
    println!("counter = {}", ctx.get_cell(&counter));
});

counter.set(&ctx, 6); // schedules and runs the effect once
effect.dispose(&ctx); // unsubscribes and prevents future reruns

// Batch writes coalesce invalidation and effect reruns.
ctx.batch(|ctx| {
    counter.set(ctx, 7);
    counter.set(ctx, 8);
});
```

## Why Lazy?

| | Lazy (Slots) | Eager (Signals) |
|---|---|---|
| **When does recomputation happen?** | On access (`get`) | Immediately on change |
| **Wasted work** | Zero — only compute what's read | Can compute values nobody uses |
| **Glitch-free** | By construction | Requires topological sorting |
| **Ordering** | Irrelevant — pull-based | Critical — push-based DAG walk |
| **Use case** | Request handling, data pipelines | UI rendering, real-time updates |

In a web server handling requests, you might have 50 computed values available but any given request only uses 5. With eager reactivity, all 50 recompute on every change. With lazy, only the 5 actually accessed compute.

## Core Concepts

### Context

`Context` owns all Slots and Cells. It manages the dependency graph and provides the API for creating, reading, and mutating reactive values. Think of it as the "world" for your reactive computations — in web frameworks, this maps to a request context, application scope, or component tree.

The current `Context` is intentionally single-threaded. It uses `RefCell` and
non-`Send` callback storage to keep the fast path allocation-only and mutex-free.
Create independent contexts per OS thread for local graphs, or use
`ThreadSafeContext` when one reactive graph must be shared across threads.

### Slot

A `SlotHandle<T>` wraps a compute function `Fn(&Context) -> T`. The result is cached after first access. Dependencies are discovered automatically via a thread-local tracking stack — any Slot or Cell accessed during computation becomes a dependency. `ctx.computed()` is the ergonomic name for a derived value; `ctx.slot()` is the same primitive. Use `ctx.memo()` when `T: PartialEq` and equal recomputations should suppress downstream work.

When a dependency is invalidated, the Slot marks its cached value dirty. It does **not** validate or recompute until `ctx.get()` is called again.
For `ctx.memo()` slots, if recomputation returns a value equal to the previous cache, downstream dirty Slots become fresh without recomputing, and scheduled effects that only depended on unchanged Slots skip cleanup/rerun.

**Dependencies are dynamic.** Every time a Slot recomputes, it re-discovers its dependencies from scratch. If your compute function has conditional branches that access different Cells depending on state, the dependency graph updates automatically. No stale subscriptions, no manual cleanup.

### Cell

A `CellHandle<T>` holds a mutable value. `cell.set(&ctx, value)` and `ctx.set_cell()` compare old and new values via `PartialEq` — if unchanged, no invalidation occurs. If changed, all dependent Slots are recursively marked dirty.

### Batch Updates

`ctx.batch(|ctx| { ... })` groups multiple cell updates and explicit slot/cell clears into one invalidation pass. Nested batches flush only when the outermost batch exits. Direct `ctx.get_cell()` reads inside the callback see the latest cell value immediately; changed-cell dependents are marked dirty after the batch, so Slot reads during the callback return their pre-batch cached value until the batch completes.

### Effect

An `EffectHandle` represents a side-effect callback registered with `ctx.effect()`. Effects run immediately, track any Slots or Cells read during that run, and rerun after those dependencies invalidate. Scheduled effect reruns are flushed after the invalidation pass, so diamond dependency paths coalesce to one rerun. Effects scheduled only by dirty Slot dependencies first validate those Slots and skip cleanup/rerun when values are unchanged.

Effects can return a cleanup closure. Cleanup runs before the next rerun and when the handle is disposed:

```rust
let effect = ctx.effect(move |ctx| {
    let value = ctx.get_cell(&counter);
    move || println!("cleanup for {value}")
});

effect.dispose(&ctx);
```

## API

| Method | Purpose |
|--------|---------|
| `Context::new()` | Create a new context |
| `ctx.computed(\|ctx\| T)` | Create a derived lazily-computed value |
| `ctx.slot(\|ctx\| T)` | Create a lazily-computed slot; synonym of `ctx.computed()` |
| `ctx.memo(\|ctx\| T)` | Create a lazily-computed slot with a `PartialEq` memoization guard |
| `ctx.get(&slot)` | Get value (computes if unset) |
| `ctx.cell(value)` | Create a mutable cell |
| `ctx.get_cell(&cell)` | Get cell value |
| `ctx.set_cell(&cell, value)` | Update cell (marks dependents dirty if changed) |
| `cell.set(&ctx, value)` | Handle method alias for `ctx.set_cell(&cell, value)` |
| `ctx.batch(\|ctx\| { ... })` | Defer changed-cell dirty marking and explicit clears until the outermost batch exits |
| `ctx.effect(\|ctx\| { ... })` | Run an effect immediately and rerun it after tracked dependencies invalidate |
| `ctx.is_set(&slot)` | Check if slot has a cached, fresh value |
| `slot.clear(&ctx)` | Clear cached value and cascade to dependents |
| `cell.clear_dependents(&ctx)` | Clear downstream slots without changing cell value |
| `effect.dispose(&ctx)` | Dispose an effect and unsubscribe dependencies |
| `effect.is_active(&ctx)` | Check whether an effect is still registered |

### ThreadSafeContext

`ThreadSafeContext` is the mutex-backed counterpart for sharing one reactive
graph across OS threads. It mirrors the core `Context` methods while requiring
`Send + Sync + 'static` values and compute/effect callbacks. The graph lock is
released before user compute callbacks, effect callbacks, or cleanup closures
run, so callbacks can re-enter the same context without deadlocking. If a slot
is invalidated while its callback is running, the stale result is discarded and
the getter retries before returning a fresh value.

## Design

- **Lazy, not eager:** Slots mark dirty on invalidation but only validate/recompute on access
- **Ergonomic aliases:** `ctx.computed()` names derived values while preserving `ctx.slot()` for low-level terminology
- **PartialEq guard:** `CellHandle::set()` only invalidates when value actually changes
- **Memo guard:** Dirty `ctx.memo()` Slots compare recomputed values and suppress downstream recomputation/effect reruns when values are equal
- **Dynamic dependencies:** Edges re-discovered on each recomputation (no stale subscriptions)
- **Batching:** Multiple writes share one invalidation/effect flush boundary
- **Effect scheduling:** Effects rerun after dependency invalidation and coalesce duplicate schedules
- Slot-id-indexed contiguous node storage for the single-threaded fast path
- Interior mutability via `RefCell` (single-threaded)
- Thread-local tracking stack for automatic dependency discovery
- Zero mandatory runtime dependencies in the default library surface
- Optional `instrumentation` feature for benchmark counters, lock timing, and thread-safe lock attribution

## Threading Roadmap

`lazily-rs` guarantees local, single-threaded `Context` graphs plus an explicit
`ThreadSafeContext` for shared graphs. `SlotHandle<T>` and `CellHandle<T>` are
`Send + Sync` when `T` is `Send + Sync`, and `EffectHandle` is also `Send + Sync`,
but handles must be used with their owning context.

Enable the optional `loom` feature to run the thread-safe synchronization model:

```bash
cargo test --features loom --test thread_safe_loom
```

Enable the optional `tokio` feature for sync-on-Tokio integration tests and the
`tokio_sync` example:

```bash
cargo test --features tokio
cargo run --example tokio_sync --features tokio
```

The feature proves `ThreadSafeContext` can be shared through `tokio::spawn` and
`tokio::task::spawn_blocking`. It does not add async computations or effects;
those need the separate `AsyncContext` design captured in `SPEC.md`, including
in-flight future deduplication, stale completion handling, cleanup ordering, and
separate `Send` versus `LocalSet` surfaces.

`ThreadSafeContext` intentionally keeps one mutex-backed graph lock while
fresh cached slot reads use a per-slot read-mostly cached-value sidecar.
Dependency edges, dirty/revision state, cached-value publication, batching, and
effect queues still mutate under the graph mutex. In-flight recompute waiters
use per-slot generation `Condvar` sidecars so they can park while the compute
owner runs user code, and a completion only wakes waiters for that finished
slot. Per-slot dependency summaries let cell-only dirty refreshes claim the
SlotId-partitioned recompute sidecar and skip the graph-locked `get_refresh`
dependency scan before the final publish mutation. Changed-cell and slot-value
invalidation build an explicit frontier plan, then apply dirty flags, revisions,
and effect scheduling in one graph-mutex mutation boundary; slot-only
changed-cell frontiers may publish dirty state through per-node sidecars with
cache-revision dirty epochs instead. The `thread_safe_graph_propagation` benchmarks
compare fan-out eager validation, fan-out/fan-in lazy dirty epoch publication,
and fan-in batched flush behavior with lock attribution, effect queue pushes,
dependency-edge counters, sidecar dirty marks, sidecar fallbacks, and dirty
epoch advances. Sharded-lock or CAS variants should wait for lock wait/hold
benchmark evidence and a Loom or Shuttle safety model for stale in-flight
completion, invalidation during compute, dynamic dependency cleanup/disposal,
effect scheduling/disposal, and re-entrant callbacks. A lock-free versioned
optimistic read path is deferred
until cached values can be retained independently of graph-protected
erased-value storage.

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
| thread_safe_contention | same_slot_write_read / 8 | 2.291 ms | 2.387 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.525 ms | 7.099 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 769.231 us | 938.785 us | 10 |
| thread_safe_contention | independent_slots / 16 | 1.943 ms | 2.164 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 622.294 us | 644.583 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.167 ms | 1.339 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.810 ms | 2.971 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 2.601 ms | 2.890 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 899.030 us | 963.410 us | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.048 ms | 2.212 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.086 ms | 1.182 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.406 ms | 2.751 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.449 ms | 1.730 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 3.756 ms | 4.060 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.815 ms | 3.915 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.222 ms | 6.333 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.895 ms | 2.015 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.580 ms | 3.876 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 500.460 us | 542.983 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.020 ms | 1.144 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.030 ms | 1.137 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.507 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.728 ns | 11.681 ns - 11.785 ns |
| cached_reads | thread_safe_context | 73.141 ns | 72.747 ns - 73.554 ns |
| cold_first_get | context | 124.060 ns | 107.656 ns - 151.572 ns |
| cold_first_get | thread_safe_context | 1.082 us | 1.058 us - 1.108 us |
| dependency_fan_out | context / 32 | 4.814 us | 4.363 us - 5.348 us |
| dependency_fan_out | context / 256 | 37.428 us | 34.000 us - 41.543 us |
| dependency_fan_out | thread_safe_context / 32 | 33.600 us | 29.220 us - 38.091 us |
| dependency_fan_out | thread_safe_context / 256 | 287.949 us | 243.368 us - 334.962 us |
| set_cell_invalidation | high_fan_out / 512 | 289.384 us | 264.914 us - 323.842 us |
| set_cell_invalidation | same_slot_contention / 1 | 43.883 us | 43.139 us - 44.590 us |
| set_cell_invalidation | same_slot_contention / 2 | 92.194 us | 89.596 us - 95.104 us |
| set_cell_invalidation | same_slot_contention / 4 | 182.203 us | 176.112 us - 189.264 us |
| set_cell_invalidation | same_slot_contention / 8 | 394.250 us | 381.822 us - 408.447 us |
| set_cell_invalidation | same_slot_contention / 16 | 870.283 us | 857.965 us - 883.967 us |
| set_cell_invalidation | independent_slot_contention / 1 | 43.780 us | 42.880 us - 44.537 us |
| set_cell_invalidation | independent_slot_contention / 2 | 79.176 us | 77.272 us - 81.054 us |
| set_cell_invalidation | independent_slot_contention / 4 | 129.498 us | 125.978 us - 133.095 us |
| set_cell_invalidation | independent_slot_contention / 8 | 290.808 us | 283.505 us - 299.703 us |
| set_cell_invalidation | independent_slot_contention / 16 | 587.167 us | 571.047 us - 603.865 us |
| set_cell_invalidation | batched_write_bursts / 1 | 160.885 us | 158.939 us - 163.481 us |
| set_cell_invalidation | batched_write_bursts / 2 | 163.799 us | 159.756 us - 169.194 us |
| set_cell_invalidation | batched_write_bursts / 4 | 466.013 us | 456.860 us - 476.333 us |
| set_cell_invalidation | batched_write_bursts / 8 | 809.706 us | 793.443 us - 825.990 us |
| set_cell_invalidation | batched_write_bursts / 16 | 1.918 ms | 1.834 ms - 2.006 ms |
| memo_equality_suppression | context | 4.201 us | 3.753 us - 4.738 us |
| memo_equality_suppression | thread_safe_context | 42.818 us | 41.838 us - 44.000 us |
| effect_flushing | context | 86.758 ns | 86.688 ns - 86.829 ns |
| effect_flushing | thread_safe_context | 1.011 us | 1.004 us - 1.022 us |
| batch_storms | context / 64 | 3.552 us | 3.540 us - 3.564 us |
| batch_storms | thread_safe_context / 64 | 9.384 us | 9.348 us - 9.422 us |
| thread_safe_contention | same_slot_write_read / 1 | 110.393 us | 109.004 us - 111.574 us |
| thread_safe_contention | same_slot_write_read / 2 | 297.831 us | 286.788 us - 309.513 us |
| thread_safe_contention | same_slot_write_read / 4 | 747.294 us | 704.401 us - 793.703 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.269 ms | 2.207 ms - 2.325 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.373 ms | 6.034 ms - 6.685 ms |
| thread_safe_contention | independent_slots / 1 | 109.264 us | 108.313 us - 110.297 us |
| thread_safe_contention | independent_slots / 2 | 225.835 us | 216.564 us - 234.556 us |
| thread_safe_contention | independent_slots / 4 | 404.920 us | 382.741 us - 425.448 us |
| thread_safe_contention | independent_slots / 8 | 793.649 us | 751.169 us - 841.424 us |
| thread_safe_contention | independent_slots / 16 | 1.964 ms | 1.886 ms - 2.044 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 109.944 us | 108.913 us - 110.927 us |
| thread_safe_contention | read_mostly_waiters / 2 | 163.445 us | 159.474 us - 167.403 us |
| thread_safe_contention | read_mostly_waiters / 4 | 266.166 us | 256.585 us - 276.063 us |
| thread_safe_contention | read_mostly_waiters / 8 | 600.116 us | 574.779 us - 622.448 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.172 ms | 1.122 ms - 1.224 ms |
| thread_safe_contention | batched_write_bursts / 1 | 247.918 us | 246.351 us - 249.185 us |
| thread_safe_contention | batched_write_bursts / 2 | 644.367 us | 598.583 us - 694.251 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.469 ms | 1.453 ms - 1.486 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.808 ms | 2.750 ms - 2.866 ms |
| thread_safe_contention | batched_write_bursts / 16 | 2.480 ms | 2.290 ms - 2.656 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 881.070 us | 839.237 us - 918.092 us |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.083 ms | 2.036 ms - 2.131 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.058 ms | 992.515 us - 1.109 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.434 ms | 2.335 ms - 2.543 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.473 ms | 1.404 ms - 1.546 ms |
| thread_safe_effect_contention | batch_flush / 16 | 3.745 ms | 3.631 ms - 3.854 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.835 ms | 3.794 ms - 3.875 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.195 ms | 6.110 ms - 6.271 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.881 ms | 1.827 ms - 1.932 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.552 ms | 3.427 ms - 3.679 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 509.248 us | 499.185 us - 520.752 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.034 ms | 1.013 ms - 1.062 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.043 ms | 1.023 ms - 1.069 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.448 ms - 1.481 ms |
| profile_instrumentation | context_snapshot | 421.918 ns | 416.285 ns - 428.322 ns |
| profile_instrumentation | thread_safe_snapshot | 305.223 us | 300.698 us - 312.876 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 3.280 us | 10.230 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 4.830 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 220.000 ns | 3.080 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 2.970 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 3.260 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 230.000 ns | 3.110 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 4.040 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 480.000 ns | 7.560 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.020 us | 14.510 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 1.960 us | 28.921 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 3.910 us | 56.291 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 3.590 us | 89.810 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 105 | 31.760 us | 79.801 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 187 | 225.571 us | 128.711 us | 0 | 0 | 0 | 2 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 363 | 1.337 ms | 241.902 us | 0 | 0 | 0 | 2 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 6.450 ms | 489.195 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 970.000 ns | 22.910 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 44 | 1.050 us | 26.730 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 45 | 0 | 1 | 0 | 0 | 0 | 159 | 4.320 us | 73.010 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 89 | 0 | 1 | 0 | 0 | 0 | 384 | 6.230 us | 128.380 us | 128 | 128 | 0 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 207 | 0 | 1 | 0 | 0 | 0 | 769 | 14.830 us | 286.782 us | 256 | 256 | 0 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 1.450 us | 30.770 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 53 | 9.210 us | 48.831 us | 29 | 29 | 2 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 133 | 205.621 us | 118.930 us | 50 | 50 | 13 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 261 | 1.619 ms | 248.831 us | 102 | 102 | 25 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 477 | 7.096 ms | 449.544 us | 223 | 223 | 32 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 910.000 ns | 19.780 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 27 | 1.070 us | 21.140 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 47 | 3.211 us | 25.670 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 46 | 1.220 us | 24.350 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 80 | 27.081 us | 38.221 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.350 us | 112.081 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 197 | 66.830 us | 185.461 us | 0 | 0 | 0 | 21 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 11 | 0 | 16 | 0 | 0 | 0 | 241 | 454.541 us | 255.961 us | 0 | 0 | 0 | 10 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 11 | 0 | 32 | 0 | 0 | 0 | 420 | 1.543 ms | 379.411 us | 0 | 0 | 0 | 10 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 6 | 0 | 64 | 0 | 0 | 0 | 733 | 6.045 ms | 516.031 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 388 | 1.277 ms | 251.332 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 5 | 1 | 742 | 6.135 ms | 486.915 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 34 | 1 | 408 | 2.568 ms | 379.793 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 31 | 1 | 692 | 9.682 ms | 658.378 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 1.938 ms | 313.421 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 65 | 0 | 7 | 1 | 1255 | 8.766 ms | 590.762 us | 0 | 0 | 0 | 3 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 554 | 0 | 64 | 0 | 50 | 1 | 1101 | 23.116 ms | 8.345 ms | 30 | 960 | 98 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 553 | 0 | 64 | 0 | 49 | 1 | 1108 | 35.158 ms | 9.028 ms | 152 | 4864 | 104 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.050 us | 143.472 us | 127 | 4064 | 0 | 4064 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.940 us | 163.481 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 19.720 us | 442.313 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 20.130 us | 523.474 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 112 | 0 | 65 | 0 | 15 | 1 | 1041 | 2.715 ms | 969.970 us | 0 | 0 | 0 | 167 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 130 | 0 | 129 | 0 | 3 | 1 | 1179 | 6.202 ms | 1.002 ms | 0 | 0 | 0 | 133 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 130.000 ns | 1.540 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.900 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 30.000 ns | 740.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 110.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 1.660 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 30.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 130.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 90.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 1.640 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 40.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.770 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 30.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 110.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.680 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 30.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 170.000 ns | 960.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 70.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 1.840 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 220.000 ns | 2.350 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 3.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 70.000 ns | 970.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 500.000 ns | 4.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 270.000 ns | 1.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 6.910 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 130.000 ns | 1.860 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 890.000 ns | 7.861 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 570.000 ns | 2.750 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 250.000 ns | 14.580 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 250.000 ns | 3.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.810 us | 16.980 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.080 us | 5.590 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 520.000 ns | 26.420 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 500.000 ns | 7.301 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.820 us | 19.200 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 7.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 560.000 ns | 62.440 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 20.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 90 | 31.160 us | 35.511 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 15.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 4 | 260.000 ns | 28.390 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 166 | 224.891 us | 69.641 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 500.000 ns | 29.150 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 2 | 70.000 ns | 29.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 30.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 326 | 1.336 ms | 147.361 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 960.000 ns | 59.761 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 2 | 70.000 ns | 33.960 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 30.000 ns | 470.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 6.448 ms | 292.853 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.870 us | 119.601 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 120.000 ns | 75.851 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 490.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 190.000 ns | 790.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 80.000 ns | 510.000 ns |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 1.610 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 670.000 ns | 20.000 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 110.000 ns | 620.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 80.000 ns | 350.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 1.760 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 19 | 830.000 ns | 24.000 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 100.000 ns | 580.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 20 | 1.190 us | 9.710 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 1.720 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 45 | 3.000 us | 61.000 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 89 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 4 | 110.000 ns | 600.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 40 | 1.940 us | 13.140 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 1.660 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 89 | 4.150 us | 112.980 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 250 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 4 | 110.000 ns | 640.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 88 | 4.180 us | 26.210 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 1.810 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 207 | 10.510 us | 258.122 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 469 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 680.000 ns | 8.830 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 120.000 ns | 1.730 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 2.910 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 630.000 ns | 17.300 us |
| thread_safe_contention_independent_slots_2 | other | 12 | 1.420 us | 2.800 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 130.000 ns | 740.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 3.550 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 2 | 1.250 us | 8.150 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 6.350 us | 33.591 us |
| thread_safe_contention_independent_slots_4 | other | 41 | 10.170 us | 5.860 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 250.000 ns | 1.340 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 6.430 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 13 | 23.070 us | 33.150 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 172.011 us | 72.150 us |
| thread_safe_contention_independent_slots_8 | other | 77 | 126.962 us | 10.960 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 650.000 ns | 2.760 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 270.000 ns | 15.050 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 25 | 270.620 us | 66.410 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.220 ms | 153.651 us |
| thread_safe_contention_independent_slots_16 | other | 126 | 148.420 us | 18.510 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.090 us | 5.350 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 510.000 ns | 29.061 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 32 | 1.227 ms | 83.662 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 5.719 ms | 312.961 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 120.000 ns | 790.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 50.000 ns | 340.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 20.000 ns | 1.630 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 720.000 ns | 17.020 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 110.000 ns | 530.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 50.000 ns | 360.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 1.640 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 880.000 ns | 18.610 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 120.000 ns | 590.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 2.170 us | 3.130 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 1.610 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 891.000 ns | 20.340 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 590.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 8 | 340.000 ns | 2.380 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 1.760 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 740.000 ns | 19.620 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 110.000 ns | 520.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 20 | 25.081 us | 8.831 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 1.810 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 1.860 us | 27.060 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 37 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.900 us | 18.030 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 7.890 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 570.000 ns | 63.841 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 710.000 ns | 21.970 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 57.090 us | 37.420 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 330.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 14.700 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.800 us | 102.950 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 7.640 us | 30.061 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 20 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 182 | 403.070 us | 85.870 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 120.000 ns | 1.240 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 500.000 ns | 31.890 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 10 | 4.300 us | 108.901 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 11 | 46.551 us | 28.060 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 20 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 342 | 1.513 ms | 151.210 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 390.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 970.000 ns | 60.870 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 10 | 16.050 us | 125.181 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 11 | 13.090 us | 41.760 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 23 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 652 | 6.043 ms | 280.651 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 360.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.920 us | 119.310 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 5 | 210.000 ns | 75.220 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 6 | 270.000 ns | 40.490 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 4 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 352 | 1.276 ms | 166.001 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 980.000 ns | 48.900 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 4 | 120.000 ns | 36.431 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 673 | 6.133 ms | 329.294 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 1.930 us | 94.481 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 5 | 310.000 ns | 63.140 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 265 | 404.093 us | 72.551 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 490.000 ns | 22.531 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 2.163 ms | 284.711 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 1.814 ms | 105.460 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 940.000 ns | 45.261 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 7.867 ms | 507.657 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 1.937 ms | 202.150 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 70.000 ns | 900.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.000 us | 65.761 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 60.000 ns | 29.190 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 140.000 ns | 15.420 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1181 | 8.763 ms | 382.661 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 80.000 ns | 390.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 1.850 us | 119.880 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 3 | 80.000 ns | 52.701 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 170.000 ns | 35.130 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 321 | 521.253 us | 218.350 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.211 us | 13.330 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.010 us | 115.251 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 98 | 17.403 ms | 6.789 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 554 | 5.188 ms | 1.209 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 323 | 1.330 ms | 218.942 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.930 us | 11.110 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 1.970 us | 112.602 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 104 | 28.699 ms | 7.505 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 553 | 5.125 ms | 1.180 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.950 us | 17.960 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.010 us | 11.090 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 940.000 ns | 64.301 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.150 us | 50.121 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.890 us | 14.660 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.030 us | 10.700 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 910.000 ns | 60.721 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 3.110 us | 77.400 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 7.670 us | 62.650 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 4.610 us | 29.271 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 4.190 us | 230.022 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 3.250 us | 120.370 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.420 us | 64.340 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.200 us | 35.211 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.990 us | 223.571 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 4.520 us | 200.352 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 439 | 2.529 ms | 185.441 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 416 | 13.030 us | 137.912 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.110 us | 113.600 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 9 | 7.430 us | 283.373 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 112 | 162.821 us | 249.644 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 786 | 6.174 ms | 315.764 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 132 | 4.380 us | 32.571 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.280 us | 226.681 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 2 | 80.000 ns | 224.242 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 130 | 19.650 us | 202.721 us |

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
