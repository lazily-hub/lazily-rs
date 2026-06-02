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

`ThreadSafeContext` intentionally keeps a mutex-first graph lock. In-flight
recompute waiters use per-slot sidecar `Condvar`s so they can park while the
compute owner runs user code, and a completion only wakes waiters for that
finished slot. Broader RwLock, sharded-lock, or CAS variants should wait for
lock wait/hold benchmark evidence and a Loom or Shuttle safety model for stale
in-flight completion, invalidation during compute, effect scheduling/disposal,
and re-entrant callbacks.

## Benchmark Results

<!-- benchmark-results:start -->
Generated for package `lazily` version `0.4.0`.

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
| cached_reads | context | 11.821 ns | 11.790 ns - 11.855 ns |
| cached_reads | thread_safe_context | 443.433 ns | 442.876 ns - 444.042 ns |
| cold_first_get | context | 118.722 ns | 112.861 ns - 124.304 ns |
| cold_first_get | thread_safe_context | 1.024 us | 1.018 us - 1.031 us |
| dependency_fan_out | context / 32 | 4.601 us | 4.251 us - 5.016 us |
| dependency_fan_out | context / 256 | 38.038 us | 35.217 us - 41.051 us |
| dependency_fan_out | thread_safe_context / 32 | 38.097 us | 37.261 us - 39.006 us |
| dependency_fan_out | thread_safe_context / 256 | 300.021 us | 293.755 us - 307.090 us |
| memo_equality_suppression | context | 4.237 us | 3.836 us - 4.676 us |
| memo_equality_suppression | thread_safe_context | 12.840 us | 12.431 us - 13.303 us |
| effect_flushing | context | 87.148 ns | 87.004 ns - 87.284 ns |
| effect_flushing | thread_safe_context | 1.313 us | 1.311 us - 1.316 us |
| batch_storms | context / 64 | 3.544 us | 3.534 us - 3.554 us |
| batch_storms | thread_safe_context / 64 | 34.675 us | 34.589 us - 34.773 us |
| thread_safe_contention | same_slot_write_read / 1 | 213.739 us | 211.997 us - 215.444 us |
| thread_safe_contention | same_slot_write_read / 2 | 744.580 us | 689.032 us - 804.875 us |
| thread_safe_contention | same_slot_write_read / 4 | 1.690 ms | 1.587 ms - 1.792 ms |
| thread_safe_contention | same_slot_write_read / 8 | 4.918 ms | 4.411 ms - 5.482 ms |
| thread_safe_contention | same_slot_write_read / 16 | 22.373 ms | 21.831 ms - 22.902 ms |
| thread_safe_contention | independent_slots / 1 | 213.313 us | 211.829 us - 214.979 us |
| thread_safe_contention | independent_slots / 2 | 626.134 us | 619.611 us - 633.291 us |
| thread_safe_contention | independent_slots / 4 | 1.349 ms | 1.309 ms - 1.387 ms |
| thread_safe_contention | independent_slots / 8 | 3.602 ms | 3.403 ms - 3.800 ms |
| thread_safe_contention | independent_slots / 16 | 11.870 ms | 11.622 ms - 12.090 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 214.405 us | 212.851 us - 215.862 us |
| thread_safe_contention | read_mostly_waiters / 2 | 451.517 us | 442.250 us - 461.633 us |
| thread_safe_contention | read_mostly_waiters / 4 | 861.143 us | 847.559 us - 875.730 us |
| thread_safe_contention | read_mostly_waiters / 8 | 1.930 ms | 1.903 ms - 1.957 ms |
| thread_safe_contention | read_mostly_waiters / 16 | 6.648 ms | 6.141 ms - 7.113 ms |
| thread_safe_contention | batched_write_bursts / 1 | 469.484 us | 466.929 us - 472.140 us |
| thread_safe_contention | batched_write_bursts / 2 | 1.593 ms | 1.558 ms - 1.630 ms |
| thread_safe_contention | batched_write_bursts / 4 | 4.593 ms | 4.431 ms - 4.801 ms |
| thread_safe_contention | batched_write_bursts / 8 | 19.200 ms | 16.486 ms - 22.172 ms |
| thread_safe_contention | batched_write_bursts / 16 | 89.676 ms | 88.602 ms - 90.831 ms |
| profile_instrumentation | context_snapshot | 420.258 ns | 415.909 ns - 428.197 ns |
| profile_instrumentation | thread_safe_snapshot | 301.715 us | 299.212 us - 304.037 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 17 | 273.652 us | 20.980 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 11.560 us | 119.991 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 26 | 0 | 1 | 0 | 0 | 0 | 367 | 288.152 us | 211.011 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 44 | 0 | 1 | 0 | 0 | 0 | 779 | 2.403 ms | 393.834 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 72 | 0 | 1 | 0 | 0 | 0 | 1662 | 10.203 ms | 736.349 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 127 | 0 | 1 | 0 | 0 | 0 | 3775 | 60.650 ms | 1.896 ms |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 181 | 5.470 us | 51.410 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 358 | 143.701 us | 113.031 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 737 | 1.216 ms | 245.350 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 1465 | 5.937 ms | 518.336 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 2897 | 28.010 ms | 1.106 ms |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 5.680 us | 53.581 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 287 | 82.621 us | 80.151 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 24 | 0 | 1 | 0 | 0 | 0 | 451 | 671.056 us | 130.540 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 26 | 0 | 1 | 0 | 0 | 0 | 845 | 3.245 ms | 242.262 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 28 | 0 | 1 | 0 | 0 | 0 | 1630 | 18.154 ms | 561.676 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 429 | 12.730 us | 150.421 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 17 | 0 | 8 | 0 | 0 | 0 | 967 | 539.514 us | 345.704 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 17 | 0 | 16 | 0 | 0 | 0 | 2407 | 4.655 ms | 871.177 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 17 | 0 | 32 | 0 | 0 | 0 | 6823 | 32.103 ms | 2.396 ms |
| thread_safe_contention_batched_write_bursts_16 | 65 | 17 | 0 | 64 | 0 | 0 | 0 | 21799 | 214.832 ms | 6.819 ms |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 2.220 us | 2.510 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 84 | 4.680 us | 27.730 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 17 | 960.000 ns | 18.000 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 1.760 us | 38.751 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.940 us | 33.000 us |
| thread_safe_contention_same_slot_write_read_2 | other | 61 | 43.940 us | 3.870 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 157 | 82.220 us | 52.371 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 26 | 21.030 us | 27.120 us |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 40.171 us | 62.540 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 66 | 33.190 us | 54.380 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 25 | 67.601 us | 10.730 us |
| thread_safe_contention_same_slot_write_read_4 | other | 123 | 245.872 us | 7.520 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 303 | 831.117 us | 89.431 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 44 | 87.270 us | 29.550 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 188.984 us | 103.902 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 138 | 326.353 us | 115.331 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 107 | 723.416 us | 48.100 us |
| thread_safe_contention_same_slot_write_read_8 | other | 241 | 1.062 ms | 8.920 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 587 | 2.712 ms | 165.861 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 72 | 320.670 us | 49.890 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 789.485 us | 186.894 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 282 | 1.145 ms | 188.254 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 352 | 4.174 ms | 136.530 us |
| thread_safe_contention_same_slot_write_read_16 | other | 435 | 4.667 ms | 16.950 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 1154 | 9.888 ms | 324.212 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 127 | 1.116 ms | 87.320 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 2.545 ms | 369.534 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 612 | 4.923 ms | 611.644 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 1191 | 37.512 ms | 486.324 us |
| thread_safe_contention_independent_slots_1 | other | 34 | 1.040 us | 2.140 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 83 | 2.490 us | 13.570 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 16 | 490.000 ns | 8.440 us |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 16 | 560.000 ns | 12.940 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 890.000 ns | 14.320 us |
| thread_safe_contention_independent_slots_2 | other | 60 | 27.340 us | 3.090 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 167 | 59.480 us | 29.680 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 33 | 12.571 us | 18.421 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 32 | 19.910 us | 28.280 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 24.400 us | 33.560 us |
| thread_safe_contention_independent_slots_4 | other | 137 | 221.822 us | 7.100 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 335 | 656.405 us | 72.210 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 67 | 90.520 us | 38.770 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 64 | 95.210 us | 57.880 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 152.152 us | 69.390 us |
| thread_safe_contention_independent_slots_8 | other | 261 | 1.331 ms | 14.320 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 671 | 3.303 ms | 154.282 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 135 | 612.173 us | 80.792 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 128 | 250.901 us | 123.380 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 439.783 us | 145.562 us |
| thread_safe_contention_independent_slots_16 | other | 485 | 5.687 ms | 32.210 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 1343 | 14.547 ms | 307.452 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 271 | 3.080 ms | 168.422 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 256 | 1.493 ms | 264.102 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 3.203 ms | 333.807 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 1.040 us | 1.480 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 84 | 2.570 us | 13.230 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 17 | 520.000 ns | 9.870 us |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 590.000 ns | 13.220 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 960.000 ns | 15.781 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 7.730 us | 1.381 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 154 | 46.411 us | 27.300 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 23 | 3.010 us | 12.480 us |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 3.250 us | 13.620 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 50 | 14.780 us | 23.860 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 8 | 7.440 us | 1.510 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 39.770 us | 1.400 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 283 | 426.672 us | 61.790 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 24 | 29.091 us | 13.760 us |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 17.260 us | 15.210 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 64 | 63.082 us | 31.000 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 28 | 95.181 us | 7.380 us |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 59.101 us | 1.331 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 541 | 1.765 ms | 125.490 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 26 | 66.421 us | 15.280 us |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 75.640 us | 16.490 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 106 | 279.873 us | 50.921 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 120 | 999.155 us | 32.750 us |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 315.353 us | 1.810 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 1055 | 8.225 ms | 262.492 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 28 | 375.183 us | 19.460 us |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 85.131 us | 18.980 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 180 | 999.054 us | 138.582 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 315 | 8.154 ms | 120.352 us |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.500 us | 4.200 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 179 | 5.180 us | 27.860 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 64 | 1.850 us | 33.260 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.320 us | 65.291 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 880.000 ns | 19.810 us |
| thread_safe_contention_batched_write_bursts_2 | other | 114 | 26.080 us | 6.570 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 491 | 238.352 us | 86.811 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 136 | 6.720 us | 72.400 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 144 | 76.671 us | 136.292 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 50 | 11.920 us | 36.311 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 32 | 179.771 us | 7.320 us |
| thread_safe_contention_batched_write_bursts_4 | other | 194 | 274.590 us | 12.980 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 1491 | 2.246 ms | 321.194 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 272 | 44.440 us | 151.340 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 272 | 593.744 us | 296.912 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 82 | 110.341 us | 64.860 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 96 | 1.386 ms | 23.891 us |
| thread_safe_contention_batched_write_bursts_8 | other | 354 | 1.742 ms | 32.360 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 5027 | 18.558 ms | 1.152 ms |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 544 | 275.663 us | 319.353 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 528 | 3.673 ms | 672.473 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 146 | 409.157 us | 152.131 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 224 | 7.445 ms | 67.650 us |
| thread_safe_contention_batched_write_bursts_16 | other | 674 | 7.367 ms | 72.631 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 18243 | 144.627 ms | 4.170 ms |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 1088 | 1.306 ms | 620.805 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1040 | 18.058 ms | 1.419 ms |
| thread_safe_contention_batched_write_bursts_16 | publish | 274 | 1.135 ms | 363.752 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 480 | 42.339 ms | 171.991 us |

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
