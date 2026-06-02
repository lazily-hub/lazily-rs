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
recompute waiters use a sidecar `Condvar` so they can park while the compute
owner runs user code. Broader RwLock, sharded-lock, or CAS variants should wait
for lock wait/hold benchmark evidence and a Loom or Shuttle safety model for
stale in-flight completion, invalidation during compute, effect
scheduling/disposal, and re-entrant callbacks.

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
| cached_reads | context | 13.098 ns | 12.796 ns - 13.414 ns |
| cached_reads | thread_safe_context | 456.397 ns | 450.001 ns - 465.561 ns |
| cold_first_get | context | 269.105 ns | 230.039 ns - 312.784 ns |
| cold_first_get | thread_safe_context | 1.198 us | 1.142 us - 1.270 us |
| dependency_fan_out | context / 32 | 5.761 us | 5.144 us - 6.344 us |
| dependency_fan_out | context / 256 | 47.694 us | 42.337 us - 54.401 us |
| dependency_fan_out | thread_safe_context / 32 | 41.208 us | 39.642 us - 42.772 us |
| dependency_fan_out | thread_safe_context / 256 | 334.240 us | 321.810 us - 345.811 us |
| memo_equality_suppression | context | 5.752 us | 5.238 us - 6.198 us |
| memo_equality_suppression | thread_safe_context | 19.460 us | 16.772 us - 22.238 us |
| effect_flushing | context | 90.798 ns | 90.564 ns - 91.038 ns |
| effect_flushing | thread_safe_context | 1.342 us | 1.337 us - 1.349 us |
| batch_storms | context / 64 | 3.635 us | 3.619 us - 3.652 us |
| batch_storms | thread_safe_context / 64 | 35.430 us | 35.349 us - 35.514 us |
| thread_safe_contention | same_slot_write_read / 1 | 235.519 us | 226.345 us - 247.931 us |
| thread_safe_contention | same_slot_write_read / 2 | 718.239 us | 705.804 us - 729.785 us |
| thread_safe_contention | same_slot_write_read / 4 | 1.985 ms | 1.940 ms - 2.039 ms |
| thread_safe_contention | same_slot_write_read / 8 | 6.146 ms | 5.843 ms - 6.422 ms |
| thread_safe_contention | same_slot_write_read / 16 | 22.205 ms | 21.739 ms - 22.629 ms |
| thread_safe_contention | independent_slots / 1 | 222.697 us | 220.766 us - 224.360 us |
| thread_safe_contention | independent_slots / 2 | 686.092 us | 680.750 us - 691.460 us |
| thread_safe_contention | independent_slots / 4 | 1.595 ms | 1.571 ms - 1.622 ms |
| thread_safe_contention | independent_slots / 8 | 4.327 ms | 4.183 ms - 4.464 ms |
| thread_safe_contention | independent_slots / 16 | 12.663 ms | 12.498 ms - 12.806 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 222.122 us | 221.309 us - 222.991 us |
| thread_safe_contention | read_mostly_waiters / 2 | 463.681 us | 458.749 us - 468.887 us |
| thread_safe_contention | read_mostly_waiters / 4 | 931.749 us | 915.997 us - 946.018 us |
| thread_safe_contention | read_mostly_waiters / 8 | 2.275 ms | 2.249 ms - 2.306 ms |
| thread_safe_contention | read_mostly_waiters / 16 | 6.627 ms | 6.518 ms - 6.738 ms |
| thread_safe_contention | batched_write_bursts / 1 | 522.491 us | 513.476 us - 533.146 us |
| thread_safe_contention | batched_write_bursts / 2 | 1.859 ms | 1.844 ms - 1.877 ms |
| thread_safe_contention | batched_write_bursts / 4 | 5.509 ms | 5.323 ms - 5.695 ms |
| thread_safe_contention | batched_write_bursts / 8 | 22.959 ms | 20.678 ms - 25.290 ms |
| thread_safe_contention | batched_write_bursts / 16 | 94.855 ms | 93.763 ms - 95.740 ms |
| profile_instrumentation | context_snapshot | 418.120 ns | 416.020 ns - 420.297 ns |
| profile_instrumentation | thread_safe_snapshot | 297.190 us | 296.178 us - 298.175 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 17 | 279.273 us | 14.811 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 5.940 us | 60.521 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 340 | 154.621 us | 112.501 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 40 | 0 | 1 | 0 | 0 | 0 | 750 | 1.772 ms | 321.935 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 76 | 0 | 1 | 0 | 0 | 0 | 1708 | 10.955 ms | 748.665 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 129 | 0 | 1 | 0 | 0 | 0 | 4037 | 69.674 ms | 1.995 ms |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 181 | 5.400 us | 50.360 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 356 | 137.800 us | 108.610 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 738 | 1.259 ms | 255.292 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 1455 | 6.710 ms | 564.294 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 2878 | 31.757 ms | 1.165 ms |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 5.380 us | 52.131 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 284 | 81.301 us | 78.080 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 21 | 0 | 1 | 0 | 0 | 0 | 457 | 688.926 us | 126.491 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 773 | 4.237 ms | 292.190 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 27 | 0 | 1 | 0 | 0 | 0 | 1419 | 10.400 ms | 360.353 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 429 | 12.220 us | 150.202 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 17 | 0 | 8 | 0 | 0 | 0 | 967 | 543.985 us | 349.782 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 18 | 0 | 16 | 0 | 0 | 0 | 2446 | 5.659 ms | 988.959 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 18 | 0 | 32 | 0 | 0 | 0 | 6898 | 36.586 ms | 2.572 ms |
| thread_safe_contention_batched_write_bursts_16 | 65 | 19 | 0 | 64 | 0 | 0 | 0 | 22094 | 241.945 ms | 7.291 ms |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_contention_same_slot_write_read_1 | other | 36 | 1.050 us | 1.790 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 84 | 2.360 us | 14.790 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 17 | 500.000 ns | 9.200 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 1.070 us | 17.520 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 960.000 ns | 17.221 us |
| thread_safe_contention_same_slot_write_read_2 | other | 53 | 28.710 us | 2.250 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 150 | 36.750 us | 28.390 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 19 | 4.830 us | 10.961 us |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 25.850 us | 37.240 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 54 | 13.200 us | 26.510 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 32 | 45.281 us | 7.150 us |
| thread_safe_contention_same_slot_write_read_4 | other | 125 | 228.421 us | 5.910 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 299 | 586.095 us | 81.431 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 40 | 74.440 us | 24.860 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 150.712 us | 96.881 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 127 | 217.980 us | 85.493 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 95 | 514.846 us | 27.360 us |
| thread_safe_contention_same_slot_write_read_8 | other | 245 | 942.938 us | 9.250 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 591 | 2.851 ms | 166.792 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 76 | 412.973 us | 53.200 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 822.965 us | 192.132 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 290 | 1.066 ms | 210.030 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 378 | 4.859 ms | 117.261 us |
| thread_safe_contention_same_slot_write_read_16 | other | 472 | 4.597 ms | 18.661 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 1156 | 11.202 ms | 333.060 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 129 | 975.439 us | 85.214 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 2.482 ms | 372.250 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 647 | 6.298 ms | 723.514 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 1377 | 44.119 ms | 462.466 us |
| thread_safe_contention_independent_slots_1 | other | 34 | 990.000 ns | 1.690 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 83 | 2.420 us | 13.380 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 16 | 450.000 ns | 8.610 us |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 16 | 630.000 ns | 12.440 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 910.000 ns | 14.240 us |
| thread_safe_contention_independent_slots_2 | other | 58 | 22.980 us | 3.540 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 167 | 61.300 us | 29.580 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 33 | 7.950 us | 18.150 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 32 | 17.760 us | 27.340 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 27.810 us | 30.000 us |
| thread_safe_contention_independent_slots_4 | other | 138 | 283.752 us | 7.480 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 335 | 686.799 us | 77.282 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 67 | 119.180 us | 39.940 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 64 | 34.440 us | 60.400 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 134.391 us | 70.190 us |
| thread_safe_contention_independent_slots_8 | other | 251 | 1.361 ms | 14.180 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 671 | 3.459 ms | 168.191 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 135 | 679.075 us | 86.452 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 128 | 270.071 us | 132.550 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 941.328 us | 162.921 us |
| thread_safe_contention_independent_slots_16 | other | 466 | 6.401 ms | 37.500 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 1343 | 15.955 ms | 331.413 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 271 | 4.026 ms | 172.152 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 256 | 2.058 ms | 284.994 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 3.317 ms | 338.895 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 36 | 1.050 us | 1.570 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 84 | 2.430 us | 13.300 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 17 | 450.000 ns | 9.561 us |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 500.000 ns | 13.070 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 950.000 ns | 14.630 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 36 | 7.760 us | 1.420 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 154 | 50.751 us | 27.240 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 23 | 1.150 us | 12.440 us |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 3.320 us | 13.680 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 50 | 14.190 us | 22.410 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 5 | 4.130 us | 890.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 36 | 42.751 us | 1.330 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 280 | 392.283 us | 59.831 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 21 | 27.570 us | 11.980 us |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 16.700 us | 15.380 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 63 | 78.481 us | 28.290 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 41 | 131.141 us | 9.680 us |
| thread_safe_contention_read_mostly_waiters_8 | other | 36 | 173.473 us | 1.930 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 538 | 2.547 ms | 162.390 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 23 | 63.192 us | 15.960 us |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 24.430 us | 22.840 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 81 | 221.443 us | 63.200 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 79 | 1.208 ms | 25.870 us |
| thread_safe_contention_read_mostly_waiters_16 | other | 36 | 65.261 us | 1.570 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 1054 | 6.894 ms | 222.132 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 27 | 62.582 us | 15.550 us |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 161.432 us | 17.030 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 131 | 465.483 us | 64.301 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 155 | 2.751 ms | 39.770 us |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.090 us | 3.980 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 179 | 5.160 us | 28.060 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 64 | 1.860 us | 33.640 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.230 us | 64.782 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 880.000 ns | 19.740 us |
| thread_safe_contention_batched_write_bursts_2 | other | 114 | 24.721 us | 7.170 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 491 | 242.853 us | 88.421 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 136 | 8.440 us | 73.170 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 144 | 78.801 us | 138.021 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 50 | 9.130 us | 35.950 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 32 | 180.040 us | 7.050 us |
| thread_safe_contention_batched_write_bursts_4 | other | 196 | 424.282 us | 18.810 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 1507 | 2.651 ms | 350.325 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 288 | 124.352 us | 165.362 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 273 | 765.366 us | 345.602 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 84 | 59.701 us | 81.920 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 98 | 1.634 ms | 26.940 us |
| thread_safe_contention_batched_write_bursts_8 | other | 356 | 1.728 ms | 40.660 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 5059 | 21.106 ms | 1.219 ms |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 576 | 433.024 us | 338.452 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 529 | 4.195 ms | 733.065 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 148 | 338.542 us | 172.652 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 230 | 8.786 ms | 67.961 us |
| thread_safe_contention_batched_write_bursts_16 | other | 678 | 8.656 ms | 79.060 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 18371 | 159.662 ms | 4.496 ms |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 1216 | 1.764 ms | 691.235 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1042 | 18.212 ms | 1.471 ms |
| thread_safe_contention_batched_write_bursts_16 | publish | 279 | 1.472 ms | 387.894 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 508 | 52.178 ms | 165.810 us |

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
