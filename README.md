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
| thread_safe_contention | same_slot_write_read / 8 | 2.134 ms | 2.377 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.692 ms | 7.432 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 783.994 us | 831.301 us | 10 |
| thread_safe_contention | independent_slots / 16 | 1.838 ms | 2.001 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 548.960 us | 568.782 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.006 ms | 1.158 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.711 ms | 2.907 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 2.677 ms | 2.908 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 840.178 us | 963.316 us | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 1.960 ms | 2.037 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.054 ms | 1.150 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.601 ms | 2.732 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.450 ms | 1.614 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 3.624 ms | 3.856 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.619 ms | 3.766 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.850 ms | 6.739 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.082 ms | 2.279 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.261 ms | 5.080 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 552.728 us | 582.234 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.110 ms | 1.147 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.030 ms | 1.137 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.507 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 8.098 ns | 8.073 ns - 8.123 ns |
| cached_reads | thread_safe_context | 72.388 ns | 72.067 ns - 72.798 ns |
| cold_first_get | context | 72.907 ns | 67.766 ns - 78.522 ns |
| cold_first_get | thread_safe_context | 978.978 ns | 967.140 ns - 990.870 ns |
| dependency_fan_out | context / 32 | 3.470 us | 3.218 us - 3.699 us |
| dependency_fan_out | context / 256 | 44.866 us | 43.663 us - 46.107 us |
| dependency_fan_out | thread_safe_context / 32 | 23.466 us | 22.156 us - 24.859 us |
| dependency_fan_out | thread_safe_context / 256 | 195.239 us | 183.739 us - 207.588 us |
| set_cell_invalidation | high_fan_out / 512 | 189.858 us | 153.803 us - 224.137 us |
| set_cell_invalidation | same_slot_contention / 1 | 47.660 us | 46.093 us - 49.427 us |
| set_cell_invalidation | same_slot_contention / 2 | 103.644 us | 100.505 us - 106.830 us |
| set_cell_invalidation | same_slot_contention / 4 | 174.193 us | 168.562 us - 179.656 us |
| set_cell_invalidation | same_slot_contention / 8 | 346.684 us | 337.007 us - 355.421 us |
| set_cell_invalidation | same_slot_contention / 16 | 761.008 us | 748.684 us - 771.037 us |
| set_cell_invalidation | independent_slot_contention / 1 | 43.201 us | 42.465 us - 43.940 us |
| set_cell_invalidation | independent_slot_contention / 2 | 71.180 us | 68.490 us - 73.811 us |
| set_cell_invalidation | independent_slot_contention / 4 | 125.927 us | 122.975 us - 129.418 us |
| set_cell_invalidation | independent_slot_contention / 8 | 251.644 us | 249.553 us - 253.583 us |
| set_cell_invalidation | independent_slot_contention / 16 | 548.481 us | 537.290 us - 562.040 us |
| set_cell_invalidation | batched_write_bursts / 1 | 157.995 us | 155.812 us - 160.077 us |
| set_cell_invalidation | batched_write_bursts / 2 | 157.724 us | 151.241 us - 164.578 us |
| set_cell_invalidation | batched_write_bursts / 4 | 423.545 us | 413.132 us - 435.150 us |
| set_cell_invalidation | batched_write_bursts / 8 | 800.111 us | 774.674 us - 822.627 us |
| set_cell_invalidation | batched_write_bursts / 16 | 1.911 ms | 1.837 ms - 1.991 ms |
| memo_equality_suppression | context | 2.134 us | 1.867 us - 2.521 us |
| memo_equality_suppression | thread_safe_context | 36.478 us | 35.697 us - 37.316 us |
| effect_flushing | context | 48.039 ns | 47.878 ns - 48.291 ns |
| effect_flushing | thread_safe_context | 945.817 ns | 944.274 ns - 947.658 ns |
| batch_storms | context / 64 | 2.702 us | 2.688 us - 2.717 us |
| batch_storms | thread_safe_context / 64 | 8.897 us | 8.879 us - 8.914 us |
| thread_safe_contention | same_slot_write_read / 1 | 109.209 us | 108.469 us - 109.916 us |
| thread_safe_contention | same_slot_write_read / 2 | 318.505 us | 309.529 us - 328.761 us |
| thread_safe_contention | same_slot_write_read / 4 | 709.244 us | 677.785 us - 753.481 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.116 ms | 2.016 ms - 2.210 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.675 ms | 6.315 ms - 7.001 ms |
| thread_safe_contention | independent_slots / 1 | 107.925 us | 106.496 us - 109.339 us |
| thread_safe_contention | independent_slots / 2 | 194.165 us | 184.699 us - 203.986 us |
| thread_safe_contention | independent_slots / 4 | 367.296 us | 359.324 us - 376.280 us |
| thread_safe_contention | independent_slots / 8 | 780.957 us | 757.999 us - 802.508 us |
| thread_safe_contention | independent_slots / 16 | 1.871 ms | 1.823 ms - 1.921 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 108.671 us | 108.295 us - 109.031 us |
| thread_safe_contention | read_mostly_waiters / 2 | 155.218 us | 151.854 us - 159.080 us |
| thread_safe_contention | read_mostly_waiters / 4 | 250.229 us | 246.292 us - 254.618 us |
| thread_safe_contention | read_mostly_waiters / 8 | 552.543 us | 545.203 us - 559.713 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.027 ms | 1.003 ms - 1.061 ms |
| thread_safe_contention | batched_write_bursts / 1 | 245.995 us | 239.444 us - 254.044 us |
| thread_safe_contention | batched_write_bursts / 2 | 583.761 us | 571.768 us - 597.808 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.370 ms | 1.354 ms - 1.387 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.703 ms | 2.630 ms - 2.777 ms |
| thread_safe_contention | batched_write_bursts / 16 | 2.659 ms | 2.540 ms - 2.763 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 848.411 us | 811.332 us - 887.583 us |
| thread_safe_effect_contention | queue_coalescing / 16 | 1.956 ms | 1.912 ms - 1.996 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.046 ms | 1.005 ms - 1.082 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.612 ms | 2.551 ms - 2.672 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.493 ms | 1.438 ms - 1.546 ms |
| thread_safe_effect_contention | batch_flush / 16 | 3.643 ms | 3.552 ms - 3.735 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.628 ms | 3.598 ms - 3.665 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 5.931 ms | 5.781 ms - 6.136 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 2.119 ms | 2.048 ms - 2.189 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 4.323 ms | 4.176 ms - 4.520 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 558.593 us | 552.279 us - 566.048 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.116 ms | 1.104 ms - 1.129 ms |
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 1.570 us | 19.780 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 300.000 ns | 2.460 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 1.360 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 240.000 ns | 1.300 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 1.650 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 2.250 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 370.000 ns | 5.410 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 520.000 ns | 2.620 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.300 us | 16.630 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.160 us | 12.890 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.410 us | 24.300 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.280 us | 117.460 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 126 | 96.780 us | 133.181 us | 0 | 0 | 0 | 11 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 187 | 388.482 us | 141.131 us | 0 | 0 | 0 | 2 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 372 | 2.104 ms | 280.224 us | 0 | 0 | 0 | 5 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 11.748 ms | 493.086 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.090 us | 23.030 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 28 | 0 | 1 | 0 | 0 | 0 | 54 | 1.800 us | 34.060 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 51 | 0 | 1 | 0 | 0 | 0 | 159 | 6.580 us | 88.011 us | 64 | 64 | 0 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 94 | 0 | 1 | 0 | 0 | 0 | 358 | 300.155 us | 333.563 us | 123 | 123 | 5 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 188 | 0 | 1 | 0 | 0 | 0 | 708 | 172.301 us | 532.330 us | 250 | 250 | 6 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 980.000 ns | 18.290 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 3.260 us | 31.810 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 121 | 255.503 us | 115.671 us | 54 | 54 | 9 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 253 | 2.185 ms | 277.503 us | 104 | 104 | 23 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 453 | 17.587 ms | 630.195 us | 230 | 230 | 25 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.320 us | 25.190 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 28 | 1.590 us | 32.870 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 44 | 2.170 us | 35.311 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 68 | 7.050 us | 32.370 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 109 | 214.260 us | 58.690 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.170 us | 112.270 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 204 | 108.272 us | 203.340 us | 0 | 0 | 0 | 23 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 19 | 0 | 16 | 0 | 0 | 0 | 281 | 540.226 us | 286.992 us | 0 | 0 | 0 | 18 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 13 | 0 | 32 | 0 | 0 | 0 | 414 | 2.632 ms | 404.313 us | 0 | 0 | 0 | 12 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 11 | 0 | 64 | 0 | 0 | 0 | 759 | 9.049 ms | 571.693 us | 0 | 0 | 0 | 11 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 5 | 1 | 387 | 2.141 ms | 333.831 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 6 | 1 | 747 | 17.983 ms | 717.665 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 28 | 1 | 399 | 4.181 ms | 500.874 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 29 | 1 | 688 | 12.030 ms | 723.664 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 6 | 0 | 33 | 0 | 11 | 1 | 663 | 3.773 ms | 420.995 us | 0 | 0 | 0 | 5 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 6 | 0 | 65 | 0 | 11 | 1 | 1271 | 20.108 ms | 786.268 us | 0 | 0 | 0 | 5 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 553 | 0 | 64 | 0 | 50 | 1 | 1092 | 32.957 ms | 9.181 ms | 34 | 1088 | 94 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 552 | 0 | 64 | 0 | 50 | 1 | 1123 | 50.958 ms | 9.360 ms | 146 | 4672 | 110 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.070 us | 80.360 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 8.370 us | 91.272 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 13.140 us | 267.611 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 23.452 us | 297.011 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 91 | 0 | 65 | 0 | 13 | 1 | 955 | 3.338 ms | 857.844 us | 0 | 0 | 0 | 138 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 140 | 0 | 129 | 0 | 7 | 1 | 1447 | 10.945 ms | 1.014 ms | 0 | 0 | 0 | 155 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 140.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 240.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 50.000 ns | 300.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 260.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 40.000 ns | 280.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 110.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 170.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 610.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 40.000 ns | 290.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 230.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 40.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 120.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 40.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 200.000 ns | 1.480 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 110.000 ns | 1.180 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.620 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 30.000 ns | 1.130 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 240.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 130.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 1.180 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 80.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 630.000 ns | 6.090 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 370.000 ns | 3.860 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 130.000 ns | 4.420 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 170.000 ns | 2.260 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 990.000 ns | 3.140 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 620.000 ns | 2.000 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 250.000 ns | 5.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 300.000 ns | 2.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.150 us | 6.050 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.150 us | 3.990 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 490.000 ns | 9.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 620.000 ns | 5.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 3.260 us | 26.820 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 110.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 170.000 ns | 3.210 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 700.000 ns | 84.820 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 40.000 ns | 1.240 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 104 | 93.300 us | 42.951 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 5.500 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 11 | 3.150 us | 83.930 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 166 | 387.332 us | 85.441 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 70.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 660.000 ns | 9.910 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 2 | 380.000 ns | 44.870 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 332 | 2.102 ms | 199.224 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 120.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.310 us | 23.490 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 270.000 ns | 54.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 40.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 11.745 ms | 378.745 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 130.000 ns | 1.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.660 us | 46.151 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 180.000 ns | 65.850 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 40.000 ns | 1.240 us |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 120.000 ns | 1.420 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 1.350 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 1.720 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 810.000 ns | 18.540 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 140.000 ns | 310.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 90.000 ns | 410.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 70.000 ns | 770.000 ns |
| thread_safe_contention_same_slot_write_read_2 | publish | 28 | 1.500 us | 32.570 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 250.000 ns | 1.710 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 22 | 1.850 us | 10.910 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 50.000 ns | 2.020 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 51 | 4.430 us | 73.371 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 81 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 12 | 46.980 us | 2.020 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 46 | 46.550 us | 35.360 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 1.610 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 5 | 101.433 us | 58.931 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 94 | 105.152 us | 235.642 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 200 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 16 | 770.000 ns | 2.840 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 81 | 35.380 us | 37.580 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 50.000 ns | 2.050 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 6 | 72.660 us | 53.410 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 188 | 63.441 us | 436.450 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 416 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 120.000 ns | 1.240 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 70.000 ns | 490.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 1.670 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 760.000 ns | 14.890 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 250.000 ns | 770.000 ns |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 130.000 ns | 380.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 40.000 ns | 1.130 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 2.840 us | 29.530 us |
| thread_safe_contention_independent_slots_4 | other | 33 | 21.401 us | 3.810 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 270.000 ns | 900.000 ns |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 140.000 ns | 3.250 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 9 | 40.860 us | 31.100 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 192.832 us | 76.611 us |
| thread_safe_contention_independent_slots_8 | other | 71 | 139.220 us | 7.470 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 570.000 ns | 1.530 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 270.000 ns | 4.980 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 23 | 329.742 us | 74.311 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.716 ms | 189.212 us |
| thread_safe_contention_independent_slots_16 | other | 109 | 2.014 ms | 12.590 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.200 us | 3.790 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 480.000 ns | 11.730 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 25 | 946.038 us | 129.101 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 14.624 ms | 472.984 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 240.000 ns | 1.970 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 170.000 ns | 1.330 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 50.000 ns | 2.130 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 860.000 ns | 19.760 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 180.000 ns | 1.400 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 4 | 510.000 ns | 4.440 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 50.000 ns | 1.880 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 850.000 ns | 25.150 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 170.000 ns | 810.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 750.000 ns | 3.980 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 40.000 ns | 1.310 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 1.210 us | 29.211 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 160.000 ns | 970.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 16 | 5.900 us | 4.980 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 50.000 ns | 1.360 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 940.000 ns | 25.060 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 30 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 130.000 ns | 1.190 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 41 | 208.890 us | 15.610 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 50.000 ns | 1.600 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 5.190 us | 40.290 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 46 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.750 us | 19.290 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 240.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 2.460 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 580.000 ns | 71.260 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 630.000 ns | 19.020 us |
| thread_safe_contention_batched_write_bursts_2 | other | 127 | 85.081 us | 41.140 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 100.000 ns | 480.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 330.000 ns | 6.370 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 23 | 5.650 us | 121.390 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 17.111 us | 33.960 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 198 | 519.226 us | 94.511 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 110.000 ns | 630.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 630.000 ns | 13.140 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 18 | 1.700 us | 137.901 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 19 | 18.560 us | 40.810 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 28 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 346 | 2.601 ms | 200.851 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 120.000 ns | 570.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.370 us | 20.860 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 12 | 27.930 us | 125.331 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 13 | 1.280 us | 56.701 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 665 | 9.000 ms | 340.251 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 120.000 ns | 1.030 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.400 us | 45.630 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 12 | 46.151 us | 104.502 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 11 | 510.000 ns | 80.280 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 349 | 2.131 ms | 229.761 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.110 us | 18.390 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 6 | 8.860 us | 85.680 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 678 | 17.979 ms | 558.294 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 3.200 us | 63.610 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 5 | 570.000 ns | 95.761 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 256 | 1.982 ms | 94.182 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 710.000 ns | 15.810 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 2.199 ms | 390.882 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 401 | 4.205 ms | 118.050 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.380 us | 24.390 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 7.824 ms | 581.224 us |
| thread_safe_effect_contention_batch_flush_8 | other | 617 | 3.771 ms | 288.733 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 70.000 ns | 860.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.350 us | 22.531 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 5 | 230.000 ns | 60.771 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 6 | 310.000 ns | 48.100 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1193 | 20.105 ms | 571.167 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 80.000 ns | 730.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.220 us | 51.760 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 5 | 190.000 ns | 100.831 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 6 | 240.000 ns | 61.780 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 317 | 14.440 us | 245.402 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.910 us | 8.760 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 3.210 us | 58.590 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 94 | 26.205 ms | 7.814 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 553 | 6.731 ms | 1.054 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 333 | 2.380 ms | 211.901 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 1.920 us | 6.330 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.300 us | 45.441 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 110 | 43.154 ms | 8.112 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 552 | 5.421 ms | 984.464 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 2.060 us | 6.880 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 2.230 us | 6.850 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 990.000 ns | 24.970 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.790 us | 41.660 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 2.310 us | 8.830 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 2.190 us | 6.831 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 1.010 us | 27.210 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.860 us | 48.401 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 4.540 us | 17.970 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 3.010 us | 15.240 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 2.600 us | 49.450 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.990 us | 184.951 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 9.501 us | 27.830 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.290 us | 17.630 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 4.431 us | 82.410 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 5.230 us | 169.141 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 433 | 3.228 ms | 227.891 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 358 | 15.980 us | 44.050 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 2.270 us | 39.940 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 8 | 840.000 ns | 370.673 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 91 | 91.011 us | 175.290 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 798 | 10.892 ms | 406.452 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 376 | 12.570 us | 41.910 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 4.570 us | 84.010 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 4 | 150.000 ns | 254.572 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 140 | 35.660 us | 226.980 us |

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
