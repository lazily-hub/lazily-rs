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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 15 | 12.661 us | 14.450 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 110.000 ns | 1.069 ms |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 59 | 2.740 us | 45.930 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 104 | 143.711 us | 99.631 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 186 | 736.626 us | 185.281 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 337 | 2.402 ms | 332.971 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 580 | 11.215 ms | 648.805 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 56 | 1.810 us | 34.661 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 114 | 69.251 us | 70.841 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 230 | 331.375 us | 147.780 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 419 | 2.441 ms | 313.332 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 760 | 10.483 ms | 641.506 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 166 | 5.390 us | 96.940 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 236 | 106.151 us | 91.601 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 460 | 924.186 us | 217.241 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 905 | 4.033 ms | 385.721 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 1804 | 17.160 ms | 777.892 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 138 | 4.350 us | 66.951 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 29 | 0 | 1 | 0 | 0 | 0 | 285 | 150.061 us | 139.452 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 57 | 0 | 1 | 0 | 0 | 0 | 664 | 1.218 ms | 325.673 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 93 | 0 | 1 | 0 | 0 | 0 | 1476 | 6.366 ms | 726.643 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 151 | 0 | 1 | 0 | 0 | 0 | 3404 | 44.997 ms | 2.448 ms |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 131 | 4.231 us | 61.240 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 269 | 170.662 us | 132.871 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 545 | 1.218 ms | 293.823 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 1092 | 7.356 ms | 660.377 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 2145 | 27.025 ms | 1.231 ms |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 138 | 4.380 us | 64.710 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 143 | 6.490 us | 65.570 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 154 | 52.860 us | 71.550 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 20 | 0 | 1 | 0 | 0 | 0 | 177 | 164.470 us | 81.330 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 256 | 1.223 ms | 134.191 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 286 | 8.911 us | 140.431 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 7 | 0 | 8 | 0 | 0 | 0 | 336 | 170.331 us | 151.591 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 2 | 0 | 16 | 0 | 0 | 0 | 477 | 789.896 us | 187.362 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 4 | 0 | 32 | 0 | 0 | 0 | 1023 | 4.233 ms | 437.764 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 4 | 0 | 64 | 0 | 0 | 0 | 2011 | 16.804 ms | 821.536 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 80.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 30.000 ns | 1.069 ms |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 38 | 1.680 us | 3.880 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 690.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 910.000 ns | 40.280 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 2 | 60.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 67 | 121.711 us | 5.650 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 50.000 ns | 1.110 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 21.850 us | 90.001 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 2 | 60.000 ns | 2.020 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 117 | 590.626 us | 6.680 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 60.000 ns | 400.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 800.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 145.850 us | 176.691 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 2 | 60.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 204 | 1.485 ms | 9.120 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 820.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 917.067 us | 321.861 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 2 | 50.000 ns | 810.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 319 | 7.463 ms | 13.840 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 3.752 ms | 633.005 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 2 | 60.000 ns | 820.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 35 | 1.160 us | 1.720 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 60.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 701.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 16 | 500.000 ns | 30.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 2 | 60.000 ns | 1.060 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 72 | 2.310 us | 3.750 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 150.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 1.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 32 | 66.601 us | 63.591 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 4 | 130.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 146 | 91.441 us | 8.850 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 320.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 120.000 ns | 2.970 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 64 | 239.254 us | 130.170 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 8 | 240.000 ns | 3.840 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 251 | 1.310 ms | 18.060 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 510.000 ns | 2.630 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 230.000 ns | 6.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 128 | 1.130 ms | 281.862 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 16 | 450.000 ns | 4.770 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 424 | 6.418 ms | 31.371 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 940.000 ns | 5.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 460.000 ns | 12.240 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 256 | 4.063 ms | 582.655 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 32 | 950.000 ns | 9.990 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 78 | 2.770 us | 4.540 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 410.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 3.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.380 us | 88.070 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 2 | 60.000 ns | 830.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 94 | 47.991 us | 7.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 6.440 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 130 | 57.820 us | 77.101 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 2 | 50.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 182 | 236.914 us | 15.050 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 60.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 480.000 ns | 13.510 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 258 | 686.662 us | 187.361 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 2 | 70.000 ns | 930.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 356 | 857.736 us | 25.970 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 900.000 ns | 27.810 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 513 | 3.175 ms | 330.831 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 2 | 60.000 ns | 780.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 710 | 4.422 ms | 61.100 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 130.000 ns | 960.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.850 us | 54.250 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1026 | 12.736 ms | 659.602 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 2 | 60.000 ns | 1.980 us |
| thread_safe_contention_same_slot_write_read_1 | other | 53 | 1.700 us | 4.200 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 34 | 1.110 us | 8.570 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 750.000 ns |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 500.000 ns | 32.441 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.010 us | 20.990 us |
| thread_safe_contention_same_slot_write_read_2 | other | 97 | 30.941 us | 7.940 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 66 | 52.810 us | 18.051 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 700.000 ns |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 28.970 us | 69.611 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 68 | 21.680 us | 37.630 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 21 | 15.630 us | 5.520 us |
| thread_safe_contention_same_slot_write_read_4 | other | 189 | 101.601 us | 17.491 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 128 | 408.951 us | 40.950 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 20.000 ns | 750.000 ns |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 166.640 us | 147.060 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 166 | 172.871 us | 87.170 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 116 | 367.542 us | 32.252 us |
| thread_safe_contention_same_slot_write_read_8 | other | 352 | 443.533 us | 31.151 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 257 | 1.717 ms | 84.820 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 30.000 ns | 740.000 ns |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 1.030 ms | 301.921 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 338 | 742.366 us | 185.531 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 400 | 2.433 ms | 122.480 us |
| thread_safe_contention_same_slot_write_read_16 | other | 621 | 3.565 ms | 72.320 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 517 | 8.035 ms | 189.700 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 30.000 ns | 720.000 ns |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 5.232 ms | 727.196 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 672 | 6.354 ms | 820.578 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 1337 | 21.811 ms | 637.233 us |
| thread_safe_contention_independent_slots_1 | other | 50 | 1.721 us | 4.010 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 32 | 970.000 ns | 7.580 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 730.000 ns |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 16 | 540.000 ns | 30.700 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 970.000 ns | 18.220 us |
| thread_safe_contention_independent_slots_2 | other | 103 | 45.080 us | 9.260 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 66 | 53.051 us | 17.040 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 1.360 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 32 | 32.010 us | 65.651 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 40.471 us | 39.560 us |
| thread_safe_contention_independent_slots_4 | other | 209 | 236.013 us | 21.841 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 134 | 533.484 us | 40.761 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 2.730 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 64 | 278.590 us | 140.761 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 169.391 us | 87.730 us |
| thread_safe_contention_independent_slots_8 | other | 416 | 1.786 ms | 51.950 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 270 | 2.179 ms | 85.661 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 240.000 ns | 6.210 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 128 | 1.209 ms | 297.715 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 2.183 ms | 218.841 us |
| thread_safe_contention_independent_slots_16 | other | 789 | 6.559 ms | 91.190 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 542 | 9.422 ms | 153.500 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 480.000 ns | 12.590 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 256 | 4.074 ms | 578.164 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 6.970 ms | 395.873 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 53 | 1.690 us | 3.920 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 34 | 1.070 us | 8.110 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 690.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 540.000 ns | 32.920 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.050 us | 19.070 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 53 | 2.830 us | 3.810 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 36 | 1.390 us | 8.720 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 690.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 490.000 ns | 32.130 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 35 | 1.410 us | 19.810 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 340.000 ns | 410.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 54 | 6.520 us | 4.060 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 40 | 8.660 us | 10.280 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 710.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 500.000 ns | 32.230 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 38 | 20.300 us | 23.020 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 5 | 16.850 us | 1.250 us |
| thread_safe_contention_read_mostly_waiters_8 | other | 56 | 18.030 us | 4.740 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 47 | 65.110 us | 13.690 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 720.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 10.810 us | 32.880 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 45 | 43.900 us | 26.190 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 12 | 26.590 us | 3.110 us |
| thread_safe_contention_read_mostly_waiters_16 | other | 54 | 94.970 us | 6.720 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 83 | 691.961 us | 33.610 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 690.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 11.930 us | 35.990 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 58 | 139.340 us | 40.370 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 44 | 284.690 us | 16.811 us |
| thread_safe_contention_batched_write_bursts_1 | other | 138 | 4.561 us | 13.560 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 32 | 950.000 ns | 13.660 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 3.060 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.370 us | 86.451 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 920.000 ns | 23.700 us |
| thread_safe_contention_batched_write_bursts_2 | other | 152 | 76.441 us | 15.790 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 16 | 6.830 us | 11.150 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 230.000 ns | 6.600 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 135 | 71.150 us | 103.901 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 17 | 5.880 us | 12.430 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 8 | 9.800 us | 1.720 us |
| thread_safe_contention_batched_write_bursts_4 | other | 196 | 246.481 us | 16.860 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 4 | 140.000 ns | 3.000 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 480.000 ns | 13.360 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 257 | 542.645 us | 150.062 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 4 | 150.000 ns | 4.080 us |
| thread_safe_contention_batched_write_bursts_8 | other | 456 | 1.628 ms | 47.260 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 8 | 1.760 us | 15.170 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 950.000 ns | 27.290 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 515 | 2.595 ms | 334.264 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 9 | 280.000 ns | 13.010 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 3 | 6.640 us | 770.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 904 | 4.802 ms | 86.280 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 8 | 250.000 ns | 27.100 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.910 us | 55.340 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1027 | 11.999 ms | 623.316 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 8 | 230.000 ns | 29.500 us |

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
