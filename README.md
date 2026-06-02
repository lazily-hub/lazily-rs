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
finished slot. Fresh cached gets clone the cached value under one graph lock
without recursively validating unchanged dependencies. Broader RwLock,
sharded-lock, or CAS variants should wait for lock wait/hold benchmark evidence
and a Loom or Shuttle safety model for stale in-flight completion, invalidation
during compute, effect scheduling/disposal, and re-entrant callbacks. A
lock-free versioned optimistic read path is deferred until cached values can be
retained independently of the mutex-protected erased-value storage.

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
| cached_reads | context | 11.539 ns | 11.507 ns - 11.572 ns |
| cached_reads | thread_safe_context | 108.430 ns | 108.128 ns - 108.767 ns |
| cold_first_get | context | 111.137 ns | 107.964 ns - 114.579 ns |
| cold_first_get | thread_safe_context | 1.043 us | 1.037 us - 1.051 us |
| dependency_fan_out | context / 32 | 5.034 us | 4.572 us - 5.701 us |
| dependency_fan_out | context / 256 | 37.165 us | 33.933 us - 41.657 us |
| dependency_fan_out | thread_safe_context / 32 | 37.770 us | 37.279 us - 38.316 us |
| dependency_fan_out | thread_safe_context / 256 | 303.666 us | 298.118 us - 309.302 us |
| set_cell_invalidation | high_fan_out / 512 | 200.697 us | 159.531 us - 247.363 us |
| set_cell_invalidation | same_slot_contention / 1 | 87.070 us | 85.688 us - 88.584 us |
| set_cell_invalidation | same_slot_contention / 2 | 233.063 us | 228.504 us - 237.591 us |
| set_cell_invalidation | same_slot_contention / 4 | 458.064 us | 439.022 us - 475.339 us |
| set_cell_invalidation | same_slot_contention / 8 | 1.043 ms | 994.161 us - 1.095 ms |
| set_cell_invalidation | same_slot_contention / 16 | 2.377 ms | 2.272 ms - 2.489 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 83.257 us | 81.103 us - 85.258 us |
| set_cell_invalidation | independent_slot_contention / 2 | 216.920 us | 213.765 us - 220.476 us |
| set_cell_invalidation | independent_slot_contention / 4 | 436.898 us | 423.757 us - 449.882 us |
| set_cell_invalidation | independent_slot_contention / 8 | 995.519 us | 956.797 us - 1.034 ms |
| set_cell_invalidation | independent_slot_contention / 16 | 2.466 ms | 2.410 ms - 2.520 ms |
| set_cell_invalidation | batched_write_bursts / 1 | 226.884 us | 225.378 us - 228.459 us |
| set_cell_invalidation | batched_write_bursts / 2 | 452.789 us | 425.137 us - 482.300 us |
| set_cell_invalidation | batched_write_bursts / 4 | 962.385 us | 896.799 us - 1.031 ms |
| set_cell_invalidation | batched_write_bursts / 8 | 2.428 ms | 2.289 ms - 2.548 ms |
| set_cell_invalidation | batched_write_bursts / 16 | 7.147 ms | 6.875 ms - 7.354 ms |
| memo_equality_suppression | context | 4.664 us | 4.184 us - 5.344 us |
| memo_equality_suppression | thread_safe_context | 14.395 us | 13.533 us - 15.326 us |
| effect_flushing | context | 87.949 ns | 87.665 ns - 88.220 ns |
| effect_flushing | thread_safe_context | 1.310 us | 1.308 us - 1.312 us |
| batch_storms | context / 64 | 3.540 us | 3.525 us - 3.558 us |
| batch_storms | thread_safe_context / 64 | 34.530 us | 34.501 us - 34.562 us |
| thread_safe_contention | same_slot_write_read / 1 | 212.584 us | 210.972 us - 214.085 us |
| thread_safe_contention | same_slot_write_read / 2 | 691.897 us | 680.689 us - 705.398 us |
| thread_safe_contention | same_slot_write_read / 4 | 2.308 ms | 2.127 ms - 2.499 ms |
| thread_safe_contention | same_slot_write_read / 8 | 8.115 ms | 7.206 ms - 9.078 ms |
| thread_safe_contention | same_slot_write_read / 16 | 39.306 ms | 38.520 ms - 40.085 ms |
| thread_safe_contention | independent_slots / 1 | 216.053 us | 213.457 us - 219.868 us |
| thread_safe_contention | independent_slots / 2 | 644.221 us | 635.228 us - 653.876 us |
| thread_safe_contention | independent_slots / 4 | 1.507 ms | 1.443 ms - 1.580 ms |
| thread_safe_contention | independent_slots / 8 | 3.673 ms | 3.522 ms - 3.821 ms |
| thread_safe_contention | independent_slots / 16 | 11.738 ms | 11.435 ms - 12.008 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 214.971 us | 214.226 us - 215.644 us |
| thread_safe_contention | read_mostly_waiters / 2 | 316.049 us | 313.553 us - 318.465 us |
| thread_safe_contention | read_mostly_waiters / 4 | 513.081 us | 510.400 us - 515.575 us |
| thread_safe_contention | read_mostly_waiters / 8 | 1.093 ms | 1.070 ms - 1.115 ms |
| thread_safe_contention | read_mostly_waiters / 16 | 3.133 ms | 3.065 ms - 3.200 ms |
| thread_safe_contention | batched_write_bursts / 1 | 475.724 us | 472.921 us - 478.362 us |
| thread_safe_contention | batched_write_bursts / 2 | 832.325 us | 793.809 us - 865.497 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.481 ms | 1.418 ms - 1.543 ms |
| thread_safe_contention | batched_write_bursts / 8 | 3.248 ms | 2.894 ms - 3.600 ms |
| thread_safe_contention | batched_write_bursts / 16 | 7.920 ms | 7.798 ms - 8.041 ms |
| profile_instrumentation | context_snapshot | 414.270 ns | 412.262 ns - 416.413 ns |
| profile_instrumentation | thread_safe_snapshot | 302.045 us | 299.812 us - 303.927 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 17 | 275.122 us | 22.190 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 130.000 ns | 1.086 ms |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 60 | 2.480 us | 42.291 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 108 | 166.742 us | 166.842 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 164 | 676.795 us | 218.602 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 396 | 1.585 ms | 288.262 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 698 | 10.444 ms | 642.304 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 57 | 2.150 us | 36.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 116 | 88.440 us | 73.130 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 234 | 317.862 us | 149.802 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 469 | 1.507 ms | 298.873 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 878 | 7.966 ms | 629.426 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 167 | 5.390 us | 144.081 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 249 | 206.492 us | 165.862 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 464 | 894.186 us | 234.943 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 906 | 3.742 ms | 426.585 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 1802 | 18.639 ms | 925.557 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 7.080 us | 81.030 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 27 | 0 | 1 | 0 | 0 | 0 | 422 | 393.354 us | 216.702 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 50 | 0 | 1 | 0 | 0 | 0 | 938 | 2.382 ms | 464.596 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 99 | 0 | 1 | 0 | 0 | 0 | 2328 | 15.365 ms | 1.157 ms |
| thread_safe_contention_same_slot_write_read_16 | 2 | 198 | 0 | 1 | 0 | 0 | 0 | 6282 | 118.436 ms | 3.598 ms |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 178 | 6.181 us | 73.240 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 365 | 237.003 us | 162.170 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 738 | 1.514 ms | 344.394 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 1474 | 8.742 ms | 761.575 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 2911 | 40.846 ms | 1.595 ms |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 7.370 us | 80.012 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 235 | 115.911 us | 107.851 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 315 | 421.133 us | 131.601 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 22 | 0 | 1 | 0 | 0 | 0 | 472 | 1.839 ms | 181.500 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 889 | 9.366 ms | 349.341 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 423 | 14.360 us | 236.352 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 3 | 0 | 8 | 0 | 0 | 0 | 325 | 192.702 us | 151.821 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 3 | 0 | 16 | 0 | 0 | 0 | 695 | 1.495 ms | 349.063 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 3 | 0 | 32 | 0 | 0 | 0 | 1237 | 4.578 ms | 545.254 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 2453 | 21.335 ms | 1.100 ms |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_high_fan_out_512 | other | 2 | 90.000 ns | 180.000 ns |
| thread_safe_set_cell_invalidation_high_fan_out_512 | set_cell_invalidation | 1 | 40.000 ns | 1.086 ms |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 38 | 1.590 us | 2.070 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 3 | 120.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 820.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | set_cell_invalidation | 16 | 650.000 ns | 38.201 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 2 | 80.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 70 | 35.350 us | 4.690 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 3 | 120.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 740.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | set_cell_invalidation | 32 | 131.152 us | 160.182 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 2 | 80.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 94 | 400.024 us | 4.500 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 3 | 90.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | set_cell_invalidation | 64 | 276.581 us | 212.202 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 2 | 60.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 262 | 573.764 us | 7.170 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 3 | 100.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 40.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | set_cell_invalidation | 128 | 1.011 ms | 279.232 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 2 | 80.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 436 | 6.166 ms | 13.600 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 3 | 110.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 750.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | set_cell_invalidation | 256 | 4.278 ms | 626.684 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 2 | 80.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 35 | 1.450 us | 2.340 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 3 | 90.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 850.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | set_cell_invalidation | 16 | 530.000 ns | 31.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 2 | 50.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 72 | 3.060 us | 3.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 6 | 230.000 ns | 1.140 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 80.000 ns | 1.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | set_cell_invalidation | 32 | 84.920 us | 65.810 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 4 | 150.000 ns | 1.200 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 146 | 5.940 us | 7.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 12 | 450.000 ns | 2.281 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 160.000 ns | 3.020 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | set_cell_invalidation | 64 | 310.992 us | 134.211 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 8 | 320.000 ns | 2.600 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 293 | 416.954 us | 14.311 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 24 | 690.000 ns | 4.000 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 290.000 ns | 5.870 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | set_cell_invalidation | 128 | 1.089 ms | 270.332 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 16 | 500.000 ns | 4.360 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 526 | 3.788 ms | 28.240 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 48 | 1.890 us | 8.930 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 660.000 ns | 12.291 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | set_cell_invalidation | 256 | 4.174 ms | 570.495 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 32 | 1.250 us | 9.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 78 | 2.680 us | 4.290 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 3 | 100.000 ns | 540.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 150.000 ns | 3.210 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.390 us | 135.391 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 2 | 70.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 102 | 69.510 us | 7.390 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 3 | 110.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 320.000 ns | 6.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 134 | 136.472 us | 150.642 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 2 | 80.000 ns | 580.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 184 | 204.952 us | 15.160 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 3 | 110.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 640.000 ns | 14.191 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 259 | 688.414 us | 204.422 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 2 | 70.000 ns | 610.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 356 | 705.713 us | 27.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 3 | 110.000 ns | 560.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.080 us | 27.830 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 513 | 3.035 ms | 370.265 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 2 | 70.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 708 | 5.231 ms | 59.270 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 3 | 110.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.270 us | 55.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 1025 | 13.406 ms | 809.917 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 2 | 70.000 ns | 750.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 53 | 2.110 us | 4.360 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 67 | 2.360 us | 12.570 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 17 | 660.000 ns | 9.380 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 690.000 ns | 34.500 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.260 us | 20.220 us |
| thread_safe_contention_same_slot_write_read_2 | other | 95 | 29.202 us | 8.380 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 152 | 144.431 us | 37.850 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 27 | 30.080 us | 18.980 us |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 43.020 us | 80.371 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 74 | 16.120 us | 55.140 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 42 | 130.501 us | 15.981 us |
| thread_safe_contention_same_slot_write_read_4 | other | 182 | 121.960 us | 18.620 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 322 | 696.806 us | 81.982 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 50 | 128.140 us | 31.920 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 257.521 us | 161.520 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 170 | 249.622 us | 115.142 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 150 | 927.558 us | 55.412 us |
| thread_safe_contention_same_slot_write_read_8 | other | 355 | 660.716 us | 34.921 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 737 | 3.226 ms | 184.440 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 99 | 487.465 us | 61.511 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 1.114 ms | 335.842 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 419 | 2.007 ms | 320.042 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 590 | 7.870 ms | 220.672 us |
| thread_safe_contention_same_slot_write_read_16 | other | 708 | 3.320 ms | 66.181 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 1681 | 15.085 ms | 455.721 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 198 | 2.063 ms | 134.062 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 4.886 ms | 737.076 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 1030 | 9.759 ms | 1.212 ms |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 2409 | 83.323 ms | 993.317 us |
| thread_safe_contention_independent_slots_1 | other | 50 | 1.950 us | 4.750 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 64 | 2.080 us | 11.220 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 16 | 520.000 ns | 8.790 us |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 16 | 570.000 ns | 31.580 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 1.061 us | 16.900 us |
| thread_safe_contention_independent_slots_2 | other | 103 | 31.490 us | 9.700 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 131 | 118.372 us | 26.880 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 33 | 32.340 us | 19.600 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 32 | 17.671 us | 68.280 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 37.130 us | 37.710 us |
| thread_safe_contention_independent_slots_4 | other | 208 | 186.131 us | 20.420 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 265 | 853.299 us | 64.140 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 67 | 294.502 us | 41.761 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 64 | 116.080 us | 139.823 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 63.700 us | 78.250 us |
| thread_safe_contention_independent_slots_8 | other | 408 | 1.796 ms | 45.990 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 533 | 3.666 ms | 136.012 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 135 | 1.380 ms | 86.230 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 128 | 519.470 us | 300.212 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 1.379 ms | 193.131 us |
| thread_safe_contention_independent_slots_16 | other | 773 | 9.014 ms | 94.241 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 1069 | 15.007 ms | 262.993 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 271 | 6.419 ms | 178.341 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 256 | 3.661 ms | 630.187 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 6.745 ms | 428.804 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 53 | 2.170 us | 4.791 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 67 | 2.440 us | 12.401 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 17 | 700.000 ns | 9.290 us |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 750.000 ns | 35.210 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.310 us | 18.320 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 53 | 5.370 us | 4.680 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 101 | 70.360 us | 22.920 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 17 | 5.910 us | 14.140 us |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 4.250 us | 38.660 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 39 | 7.610 us | 23.721 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 9 | 22.411 us | 3.730 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 55 | 19.340 us | 5.510 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 157 | 251.692 us | 38.190 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 19 | 13.140 us | 11.790 us |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 7.310 us | 37.460 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 49 | 42.201 us | 31.971 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 19 | 87.450 us | 6.680 us |
| thread_safe_contention_read_mostly_waiters_8 | other | 58 | 63.470 us | 6.650 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 263 | 964.670 us | 66.120 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 22 | 35.670 us | 13.330 us |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 32.651 us | 37.840 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 63 | 158.223 us | 40.810 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 50 | 583.893 us | 16.750 us |
| thread_safe_contention_read_mostly_waiters_16 | other | 59 | 172.501 us | 7.370 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 520 | 3.470 ms | 137.120 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 23 | 80.500 us | 14.920 us |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 7.240 us | 42.261 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 110 | 633.606 us | 95.460 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 161 | 5.002 ms | 52.210 us |
| thread_safe_contention_batched_write_bursts_1 | other | 138 | 5.090 us | 14.470 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 109 | 3.650 us | 18.680 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 64 | 2.160 us | 33.921 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.470 us | 141.181 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 990.000 ns | 28.100 us |
| thread_safe_contention_batched_write_bursts_2 | other | 110 | 61.900 us | 11.060 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 55 | 12.250 us | 11.330 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 24 | 890.000 ns | 15.510 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 130 | 117.452 us | 107.771 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 6 | 210.000 ns | 6.150 us |
| thread_safe_contention_batched_write_bursts_4 | other | 216 | 411.964 us | 22.281 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 157 | 283.161 us | 37.810 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 48 | 10.080 us | 31.480 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 259 | 653.204 us | 241.761 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 9 | 26.750 us | 11.111 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 6 | 109.493 us | 4.620 us |
| thread_safe_contention_batched_write_bursts_8 | other | 422 | 1.793 ms | 46.321 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 199 | 358.093 us | 43.270 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 96 | 3.530 us | 64.480 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 514 | 2.423 ms | 374.782 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 6 | 210.000 ns | 16.401 us |
| thread_safe_contention_batched_write_bursts_16 | other | 838 | 5.624 ms | 88.040 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 391 | 1.902 ms | 87.532 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 192 | 7.050 us | 123.521 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1026 | 13.802 ms | 781.677 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 6 | 230.000 ns | 19.300 us |

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
