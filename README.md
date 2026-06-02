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
slot. Sharded-lock or CAS variants should wait for lock wait/hold benchmark
evidence and a Loom or Shuttle safety model for stale in-flight completion,
invalidation during compute, effect scheduling/disposal, and re-entrant
callbacks. A lock-free versioned optimistic read path is deferred until cached
values can be retained independently of graph-protected erased-value storage.

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
| thread_safe_effect_contention_queue_coalescing_16 | 1800 | other<=900, dependency_edge<=900, set_cell_invalidation<=16, get_refresh<=0, publish<=0 |
| thread_safe_effect_contention_cleanup_execution_16 | 1300 | other<=400, dependency_edge<=700, set_cell_invalidation<=220, get_refresh<=0, publish<=0 |
| thread_safe_effect_contention_batch_flush_16 | 1500 | other<=1300, get_refresh<=8, dependency_edge<=96, set_cell_invalidation<=16, publish<=8 |

Budgets use deterministic lock acquisition counts instead of elapsed wait/hold time.

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.539 ns | 11.507 ns - 11.572 ns |
| cached_reads | thread_safe_context | 108.430 ns | 108.128 ns - 108.767 ns |
| cold_first_get | context | 111.137 ns | 107.964 ns - 114.579 ns |
| cold_first_get | thread_safe_context | 1.043 us | 1.037 us - 1.051 us |
| dependency_fan_out | context / 32 | 5.034 us | 4.572 us - 5.701 us |
| dependency_fan_out | context / 256 | 37.165 us | 33.933 us - 41.657 us |
| dependency_fan_out | thread_safe_context / 32 | 37.770 us | 37.279 us - 38.316 us |
| dependency_fan_out | thread_safe_context / 256 | 303.666 us | 298.118 us - 309.302 us |
| set_cell_invalidation | high_fan_out / 512 | 180.308 us | 155.207 us - 202.906 us |
| set_cell_invalidation | same_slot_contention / 1 | 82.844 us | 82.141 us - 83.666 us |
| set_cell_invalidation | same_slot_contention / 2 | 223.737 us | 221.385 us - 226.524 us |
| set_cell_invalidation | same_slot_contention / 4 | 451.414 us | 447.015 us - 456.117 us |
| set_cell_invalidation | same_slot_contention / 8 | 904.079 us | 887.307 us - 920.383 us |
| set_cell_invalidation | same_slot_contention / 16 | 2.507 ms | 2.476 ms - 2.536 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 81.664 us | 81.194 us - 82.254 us |
| set_cell_invalidation | independent_slot_contention / 2 | 218.791 us | 218.011 us - 219.564 us |
| set_cell_invalidation | independent_slot_contention / 4 | 412.269 us | 407.578 us - 416.645 us |
| set_cell_invalidation | independent_slot_contention / 8 | 881.260 us | 865.119 us - 899.910 us |
| set_cell_invalidation | independent_slot_contention / 16 | 2.531 ms | 2.510 ms - 2.550 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 194.429 us | 193.776 us - 195.165 us |
| set_cell_invalidation | batched_write_bursts / 2 | 433.009 us | 429.215 us - 436.912 us |
| set_cell_invalidation | batched_write_bursts / 4 | 862.268 us | 830.254 us - 917.665 us |
| set_cell_invalidation | batched_write_bursts / 8 | 2.277 ms | 2.250 ms - 2.305 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 6.773 ms | 6.634 ms - 6.909 ms |
| memo_equality_suppression | context | 4.664 us | 4.184 us - 5.344 us |
| memo_equality_suppression | thread_safe_context | 14.395 us | 13.533 us - 15.326 us |
| effect_flushing | context | 87.949 ns | 87.665 ns - 88.220 ns |
| effect_flushing | thread_safe_context | 1.310 us | 1.308 us - 1.312 us |
| batch_storms | context / 64 | 3.540 us | 3.525 us - 3.558 us |
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
| thread_safe_contention | independent_slots / 16 | 5.907 ms | 5.515 ms - 6.279 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 196.274 us | 193.446 us - 199.489 us |
| thread_safe_contention | read_mostly_waiters / 2 | 212.806 us | 210.307 us - 216.165 us |
| thread_safe_contention | read_mostly_waiters / 4 | 282.537 us | 274.366 us - 297.184 us |
| thread_safe_contention | read_mostly_waiters / 8 | 441.589 us | 427.876 us - 459.952 us |
| thread_safe_contention | read_mostly_waiters / 16 | 796.127 us | 789.325 us - 802.938 us |
| thread_safe_contention | batched_write_bursts / 1 | 368.647 us | 367.201 us - 370.193 us |
| thread_safe_contention | batched_write_bursts / 2 | 724.050 us | 583.208 us - 880.374 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.170 ms | 1.090 ms - 1.247 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.065 ms | 1.982 ms - 2.157 ms |
| thread_safe_contention | batched_write_bursts / 16 | 6.983 ms | 6.792 ms - 7.174 ms |
| profile_instrumentation | context_snapshot | 415.850 ns | 413.149 ns - 418.249 ns |
| profile_instrumentation | thread_safe_snapshot | 300.404 us | 299.514 us - 301.263 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 12 | 1.310 us | 19.690 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 270.000 ns | 3.440 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 280.000 ns | 2.920 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 250.000 ns | 2.850 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 310.000 ns | 2.830 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 280.000 ns | 2.910 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 300.000 ns | 3.320 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 18 | 540.000 ns | 6.890 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 36 | 1.120 us | 12.430 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 72 | 2.370 us | 25.311 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 144 | 4.590 us | 49.251 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 98 | 3.640 us | 92.450 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 97 | 14.201 us | 61.370 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 196 | 328.853 us | 155.392 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 379 | 2.008 ms | 308.843 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 722 | 6.144 ms | 442.633 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 3.080 us | 35.191 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 127 | 23.380 us | 54.490 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 29 | 0 | 1 | 0 | 0 | 0 | 264 | 232.950 us | 110.671 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 67 | 0 | 1 | 0 | 0 | 0 | 595 | 837.544 us | 233.732 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 121 | 0 | 1 | 0 | 0 | 0 | 1295 | 2.470 ms | 450.402 us |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 69 | 2.770 us | 31.060 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 145 | 31.731 us | 65.100 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 303 | 460.422 us | 147.681 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 589 | 3.237 ms | 310.461 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 1200 | 15.259 ms | 632.486 us |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 2.510 us | 30.590 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 81 | 5.140 us | 34.630 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 109 | 47.240 us | 42.050 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 95 | 54.903 us | 38.010 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 21 | 0 | 1 | 0 | 0 | 0 | 156 | 995.660 us | 89.790 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 158 | 5.330 us | 121.331 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 14 | 0 | 8 | 0 | 0 | 0 | 250 | 165.441 us | 217.682 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 8 | 0 | 16 | 0 | 0 | 0 | 264 | 481.536 us | 234.932 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 5 | 0 | 32 | 0 | 0 | 0 | 391 | 1.834 ms | 308.252 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 724 | 6.560 ms | 488.903 us |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 96 | 64 | 3 | 1 | 511 | 1.306 ms | 355.693 us |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 384 | 320 | 6 | 1 | 1401 | 5.838 ms | 1.002 ms |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 152 | 152 | 21 | 1 | 561 | 866.556 us | 419.033 us |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 288 | 288 | 19 | 1 | 1060 | 5.427 ms | 868.316 us |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 35 | 2 | 5 | 1 | 653 | 1.896 ms | 301.891 us |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 67 | 2 | 5 | 1 | 1261 | 9.473 ms | 570.332 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 130.000 ns | 1.170 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 20.000 ns | 1.230 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 2 | 60.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 20.000 ns | 1.350 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 2 | 80.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 110.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 1.290 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 2 | 50.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 140.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 80.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 2 | 60.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 130.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.400 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 2 | 60.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 130.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 1.320 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 2 | 70.000 ns | 950.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 240.000 ns | 2.390 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 2.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 4 | 110.000 ns | 1.350 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 500.000 ns | 3.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 270.000 ns | 1.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 5.210 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 8 | 230.000 ns | 2.640 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.080 us | 6.421 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 530.000 ns | 2.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 250.000 ns | 10.800 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 16 | 510.000 ns | 5.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.990 us | 13.180 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.160 us | 5.450 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 440.000 ns | 20.091 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 32 | 1.000 us | 10.530 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.850 us | 21.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 5.230 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 540.000 ns | 65.200 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 2 | 60.000 ns | 660.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 84 | 13.831 us | 35.430 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 50.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 10.870 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 1 | 30.000 ns | 14.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 2 | 60.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 171 | 326.593 us | 77.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 540.000 ns | 21.401 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 5 | 1.560 us | 55.351 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 2 | 80.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 336 | 2.006 ms | 154.873 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 990.000 ns | 44.950 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 7 | 590.000 ns | 107.410 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 2 | 70.000 ns | 1.130 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 650 | 6.142 ms | 281.723 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.010 us | 86.600 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 4 | 180.000 ns | 72.580 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 2 | 60.000 ns | 1.280 us |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 150.000 ns | 870.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 34 | 1.890 us | 9.380 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 1.350 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.000 us | 23.591 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 130.000 ns | 710.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 68 | 17.040 us | 20.300 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 1.180 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 39 | 6.180 us | 32.300 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 6 | 1.670 us | 780.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 138 | 162.060 us | 45.751 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 1.260 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 1 | 40.000 ns | 3.890 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 70 | 69.150 us | 58.990 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 48 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 8 | 290.000 ns | 981.000 ns |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 268 | 589.433 us | 87.651 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 1.240 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 2 | 100.000 ns | 6.710 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 162 | 247.681 us | 137.150 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 154 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 10 | 350.000 ns | 1.050 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 603 | 1.803 ms | 192.112 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 1.570 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 3 | 120.000 ns | 11.190 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 316 | 665.625 us | 244.480 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 362 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 140.000 ns | 750.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 32 | 1.500 us | 8.590 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 20.000 ns | 1.270 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 1.110 us | 20.450 us |
| thread_safe_contention_independent_slots_2 | other | 10 | 370.000 ns | 2.040 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 66 | 22.040 us | 16.570 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 2.560 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 1 | 50.000 ns | 3.830 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 9.211 us | 40.100 us |
| thread_safe_contention_independent_slots_4 | other | 26 | 880.000 ns | 6.430 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 134 | 227.171 us | 37.100 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 6.250 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 5 | 220.000 ns | 11.640 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 232.031 us | 86.261 us |
| thread_safe_contention_independent_slots_8 | other | 38 | 1.310 us | 9.700 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 270 | 1.403 ms | 82.630 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 220.000 ns | 10.740 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 3 | 12.430 us | 10.200 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 1.820 ms | 197.191 us |
| thread_safe_contention_independent_slots_16 | other | 88 | 3.080 us | 19.760 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 542 | 6.804 ms | 165.852 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 480.000 ns | 21.111 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 12 | 152.371 us | 36.290 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 8.298 ms | 389.473 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 830.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 34 | 1.290 us | 8.050 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 1.420 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.060 us | 20.290 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 190.000 ns | 730.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 38 | 3.370 us | 9.860 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 1.550 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 36 | 1.540 us | 22.490 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 130.000 ns | 600.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 55 | 39.340 us | 14.800 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 1.300 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 40 | 7.740 us | 25.350 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 130.000 ns | 540.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 48 | 52.723 us | 13.350 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 1.270 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 37 | 2.010 us | 22.850 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 5 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 560.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 78 | 631.480 us | 33.730 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 1.340 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 47 | 364.030 us | 54.160 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 26 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.600 us | 18.400 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 32 | 1.010 us | 13.690 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 5.990 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 580.000 ns | 58.631 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 1.020 us | 24.620 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 61.750 us | 37.820 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 46 | 71.431 us | 35.700 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 250.000 ns | 10.810 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 1.960 us | 100.371 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 37 | 30.050 us | 32.981 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 11 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 180 | 276.750 us | 73.240 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 30 | 95.212 us | 38.051 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 560.000 ns | 22.690 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 9 | 34.171 us | 76.841 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 19 | 74.843 us | 24.110 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 330 | 1.778 ms | 152.741 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 8 | 460.000 ns | 15.510 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 940.000 ns | 45.580 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 4 | 14.050 us | 76.870 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 10 | 41.240 us | 17.551 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 7 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 646 | 6.557 ms | 320.752 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 6 | 240.000 ns | 18.820 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.010 us | 88.520 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 2 | 60.000 ns | 37.880 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 6 | 240.000 ns | 22.931 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 346 | 1.272 ms | 149.891 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 160 | 31.670 us | 161.942 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 5 | 2.110 us | 43.860 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 688 | 5.783 ms | 298.083 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 704 | 50.671 us | 643.573 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 9 | 5.180 us | 60.651 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 195 | 74.580 us | 33.350 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 304 | 77.861 us | 267.962 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 62 | 714.115 us | 117.721 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 314 | 565.643 us | 42.213 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 576 | 381.443 us | 524.872 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 170 | 4.480 ms | 301.231 us |
| thread_safe_effect_contention_batch_flush_8 | other | 602 | 1.894 ms | 196.651 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 6 | 220.000 ns | 10.850 us |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 37 | 1.150 us | 50.460 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 70.000 ns | 25.230 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 6 | 230.000 ns | 18.700 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1178 | 9.470 ms | 388.972 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 6 | 200.000 ns | 19.240 us |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 69 | 2.270 us | 92.600 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 90.000 ns | 43.990 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 6 | 220.000 ns | 25.530 us |

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
