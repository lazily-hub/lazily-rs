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
slot. Changed-cell and slot-value invalidation build an explicit frontier plan,
then apply dirty flags, revisions, and effect scheduling in one graph-mutex
mutation boundary. Sharded-lock or CAS variants should wait for lock wait/hold
benchmark evidence and a Loom or Shuttle safety model for stale in-flight
completion, invalidation during compute, effect scheduling/disposal, and
re-entrant callbacks. A lock-free versioned optimistic read path is deferred
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
| thread_safe_contention_same_slot_write_read_16 | 1600 | get_refresh<=700, publish<=450, in_flight_wait<=500, set_cell_invalidation<=32 |
| thread_safe_contention_independent_slots_16 | 1500 | other<=160, get_refresh<=700, publish<=700, dependency_edge<=16, set_cell_invalidation<=64 |
| thread_safe_contention_read_mostly_waiters_16 | 256 | get_refresh<=128, publish<=64, in_flight_wait<=64 |
| thread_safe_contention_batched_write_bursts_16 | 950 | other<=800, get_refresh<=128, dependency_edge<=64, set_cell_invalidation<=16, publish<=64, in_flight_wait<=64 |
| thread_safe_effect_contention_queue_coalescing_16 | 2600 | other<=900, dependency_edge<=1600, set_cell_invalidation<=16, get_refresh<=64, publish<=0 |
| thread_safe_effect_contention_cleanup_execution_16 | 1300 | other<=400, dependency_edge<=700, set_cell_invalidation<=256, get_refresh<=0, publish<=0 |
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

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 5.671 ms | 5.993 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 17.665 ms | 22.097 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 3.026 ms | 3.288 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 5.096 ms | 5.340 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 427.714 us | 509.889 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 796.461 us | 817.666 us | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.001 ms | 2.372 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 2.734 ms | 2.818 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 2.077 ms | 2.266 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.936 ms | 3.644 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 946.542 us | 1.410 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 1.754 ms | 2.066 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.605 ms | 1.995 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.224 ms | 4.257 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.884 ns | 11.838 ns - 11.929 ns |
| cached_reads | thread_safe_context | 56.525 ns | 56.071 ns - 57.277 ns |
| cold_first_get | context | 119.939 ns | 113.819 ns - 127.974 ns |
| cold_first_get | thread_safe_context | 1.252 us | 1.216 us - 1.286 us |
| dependency_fan_out | context / 32 | 4.760 us | 4.408 us - 5.123 us |
| dependency_fan_out | context / 256 | 45.240 us | 41.785 us - 48.239 us |
| dependency_fan_out | thread_safe_context / 32 | 36.972 us | 35.610 us - 38.402 us |
| dependency_fan_out | thread_safe_context / 256 | 332.443 us | 310.295 us - 353.822 us |
| set_cell_invalidation | high_fan_out / 512 | 265.796 us | 230.721 us - 294.348 us |
| set_cell_invalidation | same_slot_contention / 1 | 47.326 us | 46.935 us - 47.705 us |
| set_cell_invalidation | same_slot_contention / 2 | 139.539 us | 137.591 us - 141.207 us |
| set_cell_invalidation | same_slot_contention / 4 | 244.445 us | 241.737 us - 247.492 us |
| set_cell_invalidation | same_slot_contention / 8 | 491.318 us | 483.876 us - 497.616 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.109 ms | 1.096 ms - 1.125 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 48.128 us | 46.630 us - 49.522 us |
| set_cell_invalidation | independent_slot_contention / 2 | 87.369 us | 84.954 us - 90.302 us |
| set_cell_invalidation | independent_slot_contention / 4 | 159.753 us | 153.005 us - 169.502 us |
| set_cell_invalidation | independent_slot_contention / 8 | 322.765 us | 320.520 us - 325.708 us |
| set_cell_invalidation | independent_slot_contention / 16 | 684.035 us | 678.829 us - 689.449 us |
| set_cell_invalidation | batched_write_bursts / 1 | 207.688 us | 182.312 us - 239.360 us |
| set_cell_invalidation | batched_write_bursts / 2 | 206.201 us | 201.105 us - 211.635 us |
| set_cell_invalidation | batched_write_bursts / 4 | 499.164 us | 489.809 us - 508.988 us |
| set_cell_invalidation | batched_write_bursts / 8 | 871.485 us | 831.943 us - 918.413 us |
| set_cell_invalidation | batched_write_bursts / 16 | 2.502 ms | 2.434 ms - 2.582 ms |
| memo_equality_suppression | context | 5.916 us | 5.041 us - 6.833 us |
| memo_equality_suppression | thread_safe_context | 51.942 us | 49.383 us - 54.550 us |
| effect_flushing | context | 89.830 ns | 89.309 ns - 90.664 ns |
| effect_flushing | thread_safe_context | 1.414 us | 1.401 us - 1.432 us |
| batch_storms | context / 64 | 3.679 us | 3.632 us - 3.742 us |
| batch_storms | thread_safe_context / 64 | 34.530 us | 34.501 us - 34.562 us |
| thread_safe_contention | same_slot_write_read / 1 | 185.915 us | 183.019 us - 189.096 us |
| thread_safe_contention | same_slot_write_read / 2 | 624.101 us | 612.928 us - 634.271 us |
| thread_safe_contention | same_slot_write_read / 4 | 1.680 ms | 1.659 ms - 1.700 ms |
| thread_safe_contention | same_slot_write_read / 8 | 5.542 ms | 5.245 ms - 5.766 ms |
| thread_safe_contention | same_slot_write_read / 16 | 18.263 ms | 17.281 ms - 19.472 ms |
| thread_safe_contention | independent_slots / 1 | 194.652 us | 193.351 us - 196.639 us |
| thread_safe_contention | independent_slots / 2 | 617.864 us | 605.152 us - 629.988 us |
| thread_safe_contention | independent_slots / 4 | 1.271 ms | 1.247 ms - 1.302 ms |
| thread_safe_contention | independent_slots / 8 | 3.019 ms | 2.861 ms - 3.162 ms |
| thread_safe_contention | independent_slots / 16 | 5.119 ms | 5.051 ms - 5.186 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 196.274 us | 193.446 us - 199.489 us |
| thread_safe_contention | read_mostly_waiters / 2 | 212.806 us | 210.307 us - 216.165 us |
| thread_safe_contention | read_mostly_waiters / 4 | 282.537 us | 274.366 us - 297.184 us |
| thread_safe_contention | read_mostly_waiters / 8 | 441.589 us | 427.876 us - 459.952 us |
| thread_safe_contention | read_mostly_waiters / 16 | 796.127 us | 789.325 us - 802.938 us |
| thread_safe_contention | batched_write_bursts / 1 | 368.647 us | 367.201 us - 370.193 us |
| thread_safe_contention | batched_write_bursts / 2 | 724.050 us | 583.208 us - 880.374 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.170 ms | 1.090 ms - 1.247 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.065 ms | 1.982 ms - 2.157 ms |
| thread_safe_contention | batched_write_bursts / 16 | 2.730 ms | 2.683 ms - 2.771 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 2.090 ms | 2.054 ms - 2.138 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.007 ms | 2.911 ms - 3.161 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 993.525 us | 944.959 us - 1.087 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 1.793 ms | 1.748 ms - 1.861 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.656 ms | 1.568 ms - 1.759 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.212 ms | 4.190 ms - 4.233 ms |
| profile_instrumentation | context_snapshot | 415.850 ns | 413.149 ns - 418.249 ns |
| profile_instrumentation | thread_safe_snapshot | 300.404 us | 299.514 us - 301.263 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 12 | 1.750 us | 11.330 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 450.000 ns | 6.750 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 440.000 ns | 6.930 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 270.000 ns | 2.870 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 420.000 ns | 6.470 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 340.000 ns | 2.870 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 270.000 ns | 3.600 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 18 | 510.000 ns | 6.490 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 36 | 1.180 us | 14.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 72 | 2.240 us | 29.051 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 144 | 4.250 us | 51.340 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 98 | 3.240 us | 89.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 112 | 54.540 us | 93.301 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 194 | 276.302 us | 140.960 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 373 | 1.247 ms | 269.824 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 719 | 10.840 ms | 553.305 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 3.110 us | 37.240 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 20 | 0 | 1 | 0 | 0 | 0 | 130 | 37.640 us | 76.081 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 41 | 0 | 1 | 0 | 0 | 0 | 313 | 230.913 us | 155.920 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 59 | 0 | 1 | 0 | 0 | 0 | 564 | 827.994 us | 210.582 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 153 | 0 | 1 | 0 | 0 | 0 | 1436 | 3.515 ms | 750.365 us |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 69 | 2.450 us | 30.900 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 142 | 5.030 us | 61.412 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 303 | 681.657 us | 168.943 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 592 | 3.854 ms | 350.213 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 1224 | 17.387 ms | 732.518 us |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 2.620 us | 32.830 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 80 | 4.880 us | 33.090 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 99 | 53.900 us | 40.560 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 94 | 8.580 us | 38.520 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 138 | 389.736 us | 71.490 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 158 | 4.870 us | 123.512 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 32 | 0 | 8 | 0 | 0 | 0 | 314 | 9.530 us | 268.395 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 17 | 0 | 16 | 0 | 0 | 0 | 339 | 781.946 us | 374.201 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 6 | 0 | 32 | 0 | 0 | 0 | 394 | 1.637 ms | 326.233 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 747 | 7.189 ms | 587.645 us |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 320 | 288 | 10 | 1 | 1015 | 1.656 ms | 851.319 us |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 448 | 384 | 7 | 1 | 1539 | 6.342 ms | 1.165 ms |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 144 | 144 | 19 | 1 | 600 | 1.299 ms | 457.656 us |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 288 | 288 | 19 | 1 | 1176 | 6.097 ms | 942.360 us |
| thread_safe_effect_contention_batch_flush_8 | 34 | 2 | 0 | 34 | 1 | 3 | 1 | 639 | 2.927 ms | 305.124 us |
| thread_safe_effect_contention_batch_flush_16 | 66 | 2 | 0 | 66 | 1 | 3 | 1 | 1247 | 11.023 ms | 571.183 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 230.000 ns | 1.570 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 130.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.900 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 2 | 60.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 240.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 120.000 ns | 1.430 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 2.150 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 2 | 60.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 120.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 2 | 60.000 ns | 740.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 210.000 ns | 1.260 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 130.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 2.030 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 2 | 50.000 ns | 1.890 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 190.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.220 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 2 | 60.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 140.000 ns | 830.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 60.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.410 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 2 | 40.000 ns | 860.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 240.000 ns | 1.800 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 100.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 2.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 4 | 110.000 ns | 1.310 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 530.000 ns | 4.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 310.000 ns | 1.740 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 100.000 ns | 5.280 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 8 | 240.000 ns | 3.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.050 us | 8.060 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 520.000 ns | 3.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 230.000 ns | 11.081 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 16 | 440.000 ns | 6.220 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.880 us | 13.490 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 960.000 ns | 5.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 490.000 ns | 20.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 32 | 920.000 ns | 12.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.540 us | 20.630 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 5.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 450.000 ns | 63.030 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 2 | 60.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 94 | 53.920 us | 40.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 10.790 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 6 | 280.000 ns | 40.561 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 2 | 60.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 170 | 272.592 us | 73.570 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 450.000 ns | 22.210 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 4 | 3.120 us | 44.170 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 2 | 60.000 ns | 660.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 332 | 1.246 ms | 142.002 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 930.000 ns | 43.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 5 | 300.000 ns | 82.832 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 2 | 60.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 10.837 ms | 350.373 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 140.000 ns | 1.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.840 us | 101.571 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 550.000 ns | 98.301 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 2 | 60.000 ns | 2.020 us |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 140.000 ns | 1.130 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 34 | 1.790 us | 9.850 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 2.030 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.150 us | 24.230 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 110.000 ns | 630.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 64 | 24.530 us | 21.370 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 1.280 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 45 | 12.970 us | 52.801 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 6 | 200.000 ns | 940.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 143 | 130.401 us | 50.490 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 1.820 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 1 | 50.000 ns | 3.800 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 96 | 100.232 us | 98.870 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 66 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 6 | 200.000 ns | 780.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 284 | 612.881 us | 88.520 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 1.570 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 1 | 50.000 ns | 4.000 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 143 | 214.833 us | 115.712 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 129 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 10 | 24.400 us | 1.140 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 666 | 2.554 ms | 272.253 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 1.320 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 3 | 1.030 us | 16.500 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 377 | 935.636 us | 459.152 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 379 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 130.000 ns | 1.150 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 32 | 1.240 us | 7.750 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 1.980 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 1.050 us | 20.020 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 230.000 ns | 2.670 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 66 | 2.530 us | 15.720 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 2.780 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 2.210 us | 40.242 us |
| thread_safe_contention_independent_slots_4 | other | 26 | 2.020 us | 5.120 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 134 | 320.982 us | 39.630 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 110.000 ns | 4.880 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 5 | 11.731 us | 19.240 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 346.814 us | 100.073 us |
| thread_safe_contention_independent_slots_8 | other | 40 | 1.530 us | 8.700 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 270 | 1.987 ms | 89.201 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 230.000 ns | 10.730 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 4 | 10.730 us | 17.390 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 1.855 ms | 224.192 us |
| thread_safe_contention_independent_slots_16 | other | 103 | 287.832 us | 21.511 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 542 | 7.983 ms | 176.642 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 470.000 ns | 23.750 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 21 | 312.943 us | 77.030 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 8.803 ms | 433.585 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 120.000 ns | 1.140 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 34 | 1.300 us | 8.370 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 1.940 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.160 us | 21.380 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 110.000 ns | 540.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 38 | 3.100 us | 9.400 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 1.220 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 35 | 1.640 us | 21.930 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 130.000 ns | 520.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 49 | 31.920 us | 13.260 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 1.220 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 38 | 21.820 us | 25.560 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 560.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 44 | 5.990 us | 11.140 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 1.200 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 38 | 2.450 us | 25.620 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 550.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 69 | 249.194 us | 27.770 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 1.260 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 40 | 140.392 us | 41.910 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 24 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.330 us | 18.410 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 32 | 960.000 ns | 13.560 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 6.650 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 500.000 ns | 59.070 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 960.000 ns | 25.822 us |
| thread_safe_contention_batched_write_bursts_2 | other | 146 | 4.520 us | 34.940 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 64 | 1.920 us | 44.211 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 11.031 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 32 | 970.000 ns | 110.942 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 64 | 1.880 us | 67.271 us |
| thread_safe_contention_batched_write_bursts_4 | other | 198 | 533.696 us | 91.460 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 42 | 127.560 us | 55.210 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 480.000 ns | 22.320 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 18 | 7.150 us | 151.240 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 39 | 113.060 us | 53.971 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 26 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 332 | 1.635 ms | 165.642 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 10 | 370.000 ns | 20.890 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.210 us | 60.121 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 5 | 140.000 ns | 52.320 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 12 | 430.000 ns | 27.260 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 652 | 7.108 ms | 322.803 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 14 | 45.960 us | 58.260 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.910 us | 89.281 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 5 | 340.000 ns | 75.931 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 10 | 32.300 us | 41.370 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 395 | 1.522 ms | 178.842 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 608 | 103.191 us | 578.306 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 12 | 30.570 us | 94.171 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 696 | 6.257 ms | 315.384 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 832 | 77.080 us | 750.665 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 11 | 8.230 us | 98.921 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 220 | 384.533 us | 37.670 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 288 | 91.061 us | 250.814 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 92 | 823.678 us | 169.172 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 372 | 1.681 ms | 43.900 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 576 | 276.253 us | 492.967 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 228 | 4.140 ms | 405.493 us |
| thread_safe_effect_contention_batch_flush_8 | other | 595 | 2.926 ms | 216.162 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 4 | 110.000 ns | 6.050 us |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 35 | 1.030 us | 47.851 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 1 | 40.000 ns | 24.151 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 4 | 130.000 ns | 10.910 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1171 | 11.021 ms | 402.051 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 4 | 120.000 ns | 10.150 us |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 67 | 2.110 us | 98.171 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 1 | 30.000 ns | 46.361 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 4 | 130.000 ns | 14.450 us |

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
