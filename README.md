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
during compute, effect scheduling/disposal, and re-entrant callbacks.

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
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 17 | 280.612 us | 24.982 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 6.570 us | 61.680 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 23 | 0 | 1 | 0 | 0 | 0 | 383 | 404.683 us | 228.963 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 56 | 0 | 1 | 0 | 0 | 0 | 905 | 2.608 ms | 429.043 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 99 | 0 | 1 | 0 | 0 | 0 | 2275 | 14.611 ms | 1.058 ms |
| thread_safe_contention_same_slot_write_read_16 | 2 | 188 | 0 | 1 | 0 | 0 | 0 | 5949 | 96.826 ms | 2.944 ms |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 178 | 5.950 us | 52.611 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 351 | 130.122 us | 110.211 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 728 | 1.578 ms | 282.732 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 1465 | 6.133 ms | 548.628 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 2897 | 27.972 ms | 1.098 ms |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 6.680 us | 56.820 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 210 | 32.360 us | 63.971 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 374 | 476.934 us | 107.302 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 21 | 0 | 1 | 0 | 0 | 0 | 499 | 1.508 ms | 147.122 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 24 | 0 | 1 | 0 | 0 | 0 | 865 | 6.270 ms | 259.101 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 423 | 13.140 us | 161.271 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 12 | 0 | 8 | 0 | 0 | 0 | 664 | 536.134 us | 312.754 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 5 | 0 | 16 | 0 | 0 | 0 | 787 | 1.185 ms | 309.771 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 2 | 0 | 32 | 0 | 0 | 0 | 1134 | 3.667 ms | 426.252 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 2 | 0 | 64 | 0 | 0 | 0 | 2254 | 17.656 ms | 842.418 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_contention_same_slot_write_read_1 | other | 53 | 1.760 us | 4.280 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 67 | 2.140 us | 12.580 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 17 | 540.000 ns | 9.460 us |
| thread_safe_contention_same_slot_write_read_1 | set_cell_invalidation | 16 | 1.090 us | 17.770 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.040 us | 17.590 us |
| thread_safe_contention_same_slot_write_read_2 | other | 91 | 55.960 us | 11.500 us |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 140 | 117.320 us | 52.610 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 23 | 14.530 us | 21.580 us |
| thread_safe_contention_same_slot_write_read_2 | set_cell_invalidation | 32 | 62.921 us | 64.141 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 65 | 38.380 us | 62.192 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 32 | 115.572 us | 16.940 us |
| thread_safe_contention_same_slot_write_read_4 | other | 183 | 375.334 us | 21.290 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 304 | 652.965 us | 85.631 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 56 | 106.001 us | 39.770 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 64 | 269.842 us | 94.340 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 167 | 181.910 us | 132.712 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 131 | 1.022 ms | 55.300 us |
| thread_safe_contention_same_slot_write_read_8 | other | 346 | 1.061 ms | 37.120 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 752 | 3.358 ms | 221.551 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 99 | 521.026 us | 73.441 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 128 | 1.042 ms | 202.152 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 403 | 1.527 ms | 310.161 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 547 | 7.101 ms | 213.553 us |
| thread_safe_contention_same_slot_write_read_16 | other | 679 | 3.952 ms | 66.491 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 1666 | 14.308 ms | 486.630 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 188 | 2.083 ms | 132.571 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 256 | 3.865 ms | 386.042 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 991 | 8.514 ms | 980.280 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 2169 | 64.104 ms | 892.161 us |
| thread_safe_contention_independent_slots_1 | other | 50 | 1.690 us | 5.690 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 64 | 2.050 us | 11.170 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 16 | 550.000 ns | 8.370 us |
| thread_safe_contention_independent_slots_1 | set_cell_invalidation | 16 | 620.000 ns | 12.830 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 1.040 us | 14.551 us |
| thread_safe_contention_independent_slots_2 | other | 89 | 33.930 us | 8.130 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 131 | 43.902 us | 25.251 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 33 | 6.310 us | 17.750 us |
| thread_safe_contention_independent_slots_2 | set_cell_invalidation | 32 | 21.460 us | 27.860 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 24.520 us | 31.220 us |
| thread_safe_contention_independent_slots_4 | other | 198 | 426.112 us | 21.960 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 265 | 638.466 us | 65.941 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 67 | 241.235 us | 44.100 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 64 | 106.230 us | 68.170 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 165.592 us | 82.561 us |
| thread_safe_contention_independent_slots_8 | other | 399 | 1.901 ms | 43.090 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 533 | 2.840 ms | 139.472 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 135 | 749.657 us | 87.422 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 128 | 253.863 us | 129.123 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 389.242 us | 149.521 us |
| thread_safe_contention_independent_slots_16 | other | 759 | 9.025 ms | 87.452 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 1069 | 11.721 ms | 255.912 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 271 | 2.793 ms | 162.860 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 256 | 1.599 ms | 269.066 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 2.833 ms | 322.862 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 53 | 1.870 us | 4.160 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 67 | 2.310 us | 12.340 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 17 | 630.000 ns | 8.890 us |
| thread_safe_contention_read_mostly_waiters_1 | set_cell_invalidation | 16 | 780.000 ns | 15.700 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.090 us | 15.730 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 54 | 1.750 us | 4.150 us |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 86 | 20.190 us | 17.420 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 18 | 4.150 us | 10.040 us |
| thread_safe_contention_read_mostly_waiters_2 | set_cell_invalidation | 16 | 1.240 us | 14.511 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 36 | 5.030 us | 17.850 us |
| thread_safe_contention_read_mostly_waiters_4 | other | 55 | 60.581 us | 5.190 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 184 | 220.500 us | 39.862 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 19 | 7.920 us | 10.550 us |
| thread_safe_contention_read_mostly_waiters_4 | set_cell_invalidation | 16 | 8.120 us | 14.460 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 61 | 78.703 us | 27.700 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 39 | 101.110 us | 9.540 us |
| thread_safe_contention_read_mostly_waiters_8 | other | 57 | 74.921 us | 5.550 us |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 272 | 624.565 us | 60.902 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 21 | 21.731 us | 12.390 us |
| thread_safe_contention_read_mostly_waiters_8 | set_cell_invalidation | 16 | 29.450 us | 16.090 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 69 | 195.233 us | 33.930 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 64 | 561.630 us | 18.260 us |
| thread_safe_contention_read_mostly_waiters_16 | other | 60 | 274.762 us | 7.060 us |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 496 | 2.668 ms | 115.130 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 24 | 110.861 us | 14.920 us |
| thread_safe_contention_read_mostly_waiters_16 | set_cell_invalidation | 16 | 71.580 us | 16.750 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 111 | 601.134 us | 62.780 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 158 | 2.544 ms | 42.461 us |
| thread_safe_contention_batched_write_bursts_1 | other | 138 | 4.530 us | 13.770 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 109 | 3.410 us | 18.930 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 64 | 1.970 us | 37.380 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 80 | 2.270 us | 70.060 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 960.000 ns | 21.131 us |
| thread_safe_contention_batched_write_bursts_2 | other | 206 | 129.630 us | 26.990 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 175 | 130.621 us | 44.771 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 96 | 26.620 us | 56.090 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 142 | 140.202 us | 153.112 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 30 | 4.900 us | 25.791 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 15 | 104.161 us | 6.000 us |
| thread_safe_contention_batched_write_bursts_4 | other | 254 | 375.092 us | 27.090 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 175 | 89.820 us | 35.611 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 80 | 2.420 us | 45.870 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 262 | 653.076 us | 186.790 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 12 | 1.560 us | 13.470 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 4 | 62.720 us | 940.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 388 | 1.331 ms | 36.240 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 165 | 169.462 us | 37.470 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 64 | 1.890 us | 43.560 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 513 | 2.164 ms | 302.852 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 4 | 110.000 ns | 6.130 us |
| thread_safe_contention_batched_write_bursts_16 | other | 772 | 4.377 ms | 73.760 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 325 | 895.206 us | 64.492 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 128 | 3.660 us | 85.931 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1025 | 12.379 ms | 608.115 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 4 | 110.000 ns | 10.120 us |

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
