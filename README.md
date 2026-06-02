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
ctx.set_cell(&counter, 5);

// Slot recomputes lazily on next access
assert_eq!(ctx.get(&doubled), 10);

// Effects run immediately and then after tracked dependencies change
let effect = ctx.effect(move |ctx| {
    println!("counter = {}", ctx.get_cell(&counter));
});

ctx.set_cell(&counter, 6); // schedules and runs the effect once
effect.dispose(&ctx); // unsubscribes and prevents future reruns

// Batch writes coalesce invalidation and effect reruns.
ctx.batch(|ctx| {
    ctx.set_cell(&counter, 7);
    ctx.set_cell(&counter, 8);
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

A `CellHandle<T>` holds a mutable value. `ctx.set_cell()` compares old and new values via `PartialEq` — if unchanged, no invalidation occurs. If changed, all dependent Slots are recursively marked dirty.

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
- **PartialEq guard:** `Cell.set()` only invalidates when value actually changes
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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 14 | 13.450 us | 16.030 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 130.000 ns | 1.121 ms |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 59 | 2.570 us | 46.371 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 106 | 74.501 us | 90.330 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 192 | 453.532 us | 195.761 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 349 | 2.275 ms | 330.142 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 730 | 7.986 ms | 600.654 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.940 us | 37.511 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 114 | 56.360 us | 74.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 230 | 322.312 us | 174.881 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 385 | 2.703 ms | 409.501 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 822 | 8.439 ms | 688.054 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 166 | 5.750 us | 107.241 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 236 | 137.130 us | 100.170 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 457 | 707.113 us | 188.121 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 905 | 3.319 ms | 372.663 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 1804 | 15.250 ms | 746.505 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 138 | 4.730 us | 79.361 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 21 | 0 | 1 | 0 | 0 | 0 | 254 | 155.550 us | 154.691 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 476 | 1.066 ms | 304.333 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 38 | 0 | 1 | 0 | 0 | 0 | 903 | 4.715 ms | 594.502 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 48 | 0 | 1 | 0 | 0 | 0 | 1891 | 20.270 ms | 1.171 ms |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 131 | 4.570 us | 67.850 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 269 | 157.171 us | 142.981 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 545 | 1.567 ms | 345.173 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 1094 | 7.081 ms | 654.524 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 2196 | 29.456 ms | 1.295 ms |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 138 | 4.570 us | 68.981 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 139 | 5.590 us | 75.340 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 167 | 70.492 us | 80.750 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 204 | 248.110 us | 93.341 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 332 | 1.451 ms | 150.651 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 286 | 9.530 us | 150.521 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 3 | 0 | 8 | 0 | 0 | 0 | 266 | 98.970 us | 109.641 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 2 | 0 | 16 | 0 | 0 | 0 | 483 | 775.385 us | 204.891 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 4 | 0 | 32 | 0 | 0 | 0 | 1019 | 3.851 ms | 423.705 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 2 | 0 | 64 | 0 | 0 | 0 | 1869 | 16.522 ms | 746.983 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 90.000 ns | 210.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 40.000 ns | 1.120 ms |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 38 | 1.490 us | 2.370 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 50.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 890.000 ns | 42.071 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 2 | 80.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 69 | 16.130 us | 3.020 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 70.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 58.211 us | 85.380 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 2 | 60.000 ns | 860.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 123 | 124.891 us | 13.170 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 160.000 ns | 1.080 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 990.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 328.381 us | 178.371 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 2 | 60.000 ns | 2.150 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 216 | 1.319 ms | 9.040 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 520.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 955.824 us | 318.732 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 2 | 60.000 ns | 1.150 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 469 | 4.011 ms | 15.430 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 810.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 3.975 ms | 582.064 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 2 | 60.000 ns | 1.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 35 | 1.260 us | 2.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 60.000 ns | 880.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 16 | 530.000 ns | 31.431 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 2 | 60.000 ns | 2.110 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 72 | 8.700 us | 3.760 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 50.000 ns | 1.410 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 32 | 47.370 us | 67.330 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 4 | 120.000 ns | 1.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 146 | 98.411 us | 8.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 300.000 ns | 1.640 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 2.850 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 64 | 223.251 us | 158.361 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 8 | 230.000 ns | 3.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 217 | 1.543 ms | 14.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 620.000 ns | 2.910 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 240.000 ns | 5.780 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 128 | 1.159 ms | 379.881 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 16 | 470.000 ns | 6.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 486 | 4.706 ms | 30.550 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.360 us | 6.201 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 480.000 ns | 12.070 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 256 | 3.730 ms | 626.153 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 32 | 960.000 ns | 13.080 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 78 | 2.930 us | 5.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 100.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 120.000 ns | 3.180 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.540 us | 97.011 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 2 | 60.000 ns | 1.320 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 94 | 14.580 us | 7.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 6.430 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 130 | 122.200 us | 85.400 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 2 | 50.000 ns | 870.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 180 | 117.510 us | 19.110 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 110.000 ns | 680.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 480.000 ns | 13.480 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 257 | 588.943 us | 153.351 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 2 | 70.000 ns | 1.500 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 356 | 691.664 us | 29.701 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 120.000 ns | 910.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 940.000 ns | 27.800 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 513 | 2.627 ms | 312.192 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 2 | 60.000 ns | 2.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 710 | 4.285 ms | 55.590 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.900 us | 55.450 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1026 | 10.963 ms | 633.635 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 2 | 60.000 ns | 1.300 us |
| thread_safe_contention_same_slot_write_read_1 | other | 53 | 1.800 us | 4.640 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 34 | 1.140 us | 9.440 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 750.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 720.000 ns | 39.641 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.040 us | 24.890 us |
| thread_safe_contention_same_slot_write_read_2 | other | 89 | 24.950 us | 6.210 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 66 | 55.620 us | 20.390 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 710.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 44.030 us | 90.121 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 50 | 30.920 us | 37.260 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 16 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 155 | 70.001 us | 12.451 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 140 | 532.567 us | 51.721 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 720.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 304.750 us | 172.631 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 64 | 158.170 us | 66.810 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 52 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 277 | 364.504 us | 20.680 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 239 | 2.183 ms | 86.240 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 830.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 1.618 ms | 359.102 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 111 | 549.894 us | 127.650 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 147 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 531 | 960.886 us | 32.791 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 492 | 9.396 ms | 166.531 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 50.000 ns | 1.050 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 7.581 ms | 716.612 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 182 | 2.333 ms | 253.943 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 429 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 50 | 1.830 us | 4.860 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 32 | 990.000 ns | 8.050 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 40.000 ns | 1.100 us |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 16 | 720.000 ns | 32.470 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 990.000 ns | 21.370 us |
| thread_safe_contention_independent_slots_2 | other | 103 | 27.900 us | 9.240 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 66 | 48.020 us | 17.580 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 60.000 ns | 1.480 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 32 | 37.601 us | 70.000 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 43.590 us | 44.681 us |
| thread_safe_contention_independent_slots_4 | other | 209 | 223.642 us | 23.260 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 134 | 550.212 us | 41.720 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 130.000 ns | 3.050 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 64 | 267.023 us | 172.622 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 525.642 us | 104.521 us |
| thread_safe_contention_independent_slots_8 | other | 418 | 784.318 us | 43.561 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 270 | 2.197 ms | 84.881 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 250.000 ns | 6.060 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 128 | 1.282 ms | 302.822 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 2.817 ms | 217.200 us |
| thread_safe_contention_independent_slots_16 | other | 840 | 4.077 ms | 86.190 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 542 | 11.540 ms | 164.151 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 480.000 ns | 11.990 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 256 | 3.904 ms | 602.273 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 9.935 ms | 430.104 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 53 | 1.770 us | 4.680 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 34 | 1.060 us | 8.450 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 830.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 660.000 ns | 33.581 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.050 us | 21.440 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 53 | 2.630 us | 4.100 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 34 | 1.150 us | 8.440 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 740.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 700.000 ns | 34.710 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 34 | 1.080 us | 27.350 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 54 | 16.621 us | 4.840 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 48 | 41.051 us | 13.640 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 730.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 680.000 ns | 34.990 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 39 | 12.110 us | 26.550 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 53 | 37.940 us | 5.140 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 77 | 188.590 us | 24.711 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 740.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 6.070 us | 35.390 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 37 | 15.480 us | 27.360 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 20 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 55 | 138.601 us | 5.620 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 143 | 1.144 ms | 52.820 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 40.000 ns | 750.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 4.050 us | 41.261 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 44 | 164.160 us | 50.200 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 73 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 138 | 4.710 us | 13.840 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 32 | 1.030 us | 14.111 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 3.230 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.660 us | 93.010 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 1.020 us | 26.330 us |
| thread_safe_contention_batched_write_bursts_2 | other | 112 | 19.650 us | 10.800 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 8 | 3.680 us | 5.220 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 6.761 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 131 | 66.270 us | 80.910 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 6 | 9.140 us | 5.950 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 198 | 269.961 us | 17.660 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 6 | 2.500 us | 6.110 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 490.000 ns | 13.640 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 258 | 490.284 us | 162.801 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 4 | 12.150 us | 4.680 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 1 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 456 | 1.553 ms | 43.931 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 8 | 280.000 ns | 14.620 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 940.000 ns | 27.800 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 515 | 2.297 ms | 319.174 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 8 | 260.000 ns | 18.180 us |
| thread_safe_contention_batched_write_bursts_16 | other | 772 | 6.378 ms | 67.260 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 4 | 160.000 ns | 9.920 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.930 us | 55.281 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1025 | 10.141 ms | 601.522 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 4 | 130.000 ns | 13.000 us |

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
