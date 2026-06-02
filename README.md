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

`ThreadSafeContext` intentionally keeps a mutex-first graph lock. RwLock,
sharded-lock, Condvar, or CAS variants should wait for lock wait/hold benchmark
evidence and a Loom or Shuttle safety model for stale in-flight completion,
invalidation during compute, effect scheduling/disposal, and re-entrant
callbacks.

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
| cached_reads | context | 11.631 ns | 11.593 ns - 11.676 ns |
| cached_reads | thread_safe_context | 451.647 ns | 450.160 ns - 453.187 ns |
| cold_first_get | context | 124.483 ns | 111.164 ns - 142.822 ns |
| cold_first_get | thread_safe_context | 988.475 ns | 973.876 ns - 1.012 us |
| dependency_fan_out | context / 32 | 4.994 us | 4.534 us - 5.584 us |
| dependency_fan_out | context / 256 | 40.692 us | 36.637 us - 45.534 us |
| dependency_fan_out | thread_safe_context / 32 | 36.430 us | 35.622 us - 37.243 us |
| dependency_fan_out | thread_safe_context / 256 | 281.637 us | 276.779 us - 287.619 us |
| memo_equality_suppression | context | 5.818 us | 4.948 us - 7.153 us |
| memo_equality_suppression | thread_safe_context | 15.311 us | 14.363 us - 16.297 us |
| effect_flushing | context | 88.871 ns | 88.457 ns - 89.384 ns |
| effect_flushing | thread_safe_context | 1.321 us | 1.314 us - 1.331 us |
| batch_storms | context / 64 | 3.548 us | 3.538 us - 3.559 us |
| batch_storms | thread_safe_context / 64 | 34.496 us | 34.366 us - 34.642 us |
| thread_safe_contention | 1 | 204.506 us | 203.526 us - 205.574 us |
| thread_safe_contention | 2 | 676.346 us | 662.158 us - 690.328 us |
| thread_safe_contention | 4 | 1.811 ms | 1.737 ms - 1.889 ms |
| thread_safe_contention | 8 | 5.761 ms | 5.171 ms - 6.337 ms |
| thread_safe_contention | 16 | 23.379 ms | 22.850 ms - 24.025 ms |
| profile_instrumentation | context_snapshot | 412.814 ns | 411.664 ns - 414.282 ns |
| profile_instrumentation | thread_safe_snapshot | 294.596 us | 293.471 us - 295.593 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 669 | 35.220 us | 119.802 us |
| thread_safe_contention_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 5.640 us | 64.920 us |
| thread_safe_contention_2 | 2 | 19 | 0 | 1 | 0 | 0 | 0 | 370 | 126.431 us | 105.131 us |
| thread_safe_contention_4 | 2 | 33 | 0 | 1 | 0 | 0 | 0 | 736 | 1.349 ms | 257.201 us |
| thread_safe_contention_8 | 2 | 96 | 0 | 1 | 0 | 0 | 0 | 2996 | 13.724 ms | 905.799 us |
| thread_safe_contention_16 | 2 | 190 | 0 | 1 | 0 | 0 | 0 | 10055 | 85.850 ms | 2.350 ms |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_contention_1 | other | 36 | 1.030 us | 5.800 us |
| thread_safe_contention_1 | get_refresh | 84 | 2.410 us | 14.650 us |
| thread_safe_contention_1 | dependency_edge | 17 | 510.000 ns | 9.670 us |
| thread_safe_contention_1 | set_cell_invalidation | 16 | 720.000 ns | 17.070 us |
| thread_safe_contention_1 | publish | 34 | 970.000 ns | 17.730 us |
| thread_safe_contention_2 | other | 56 | 24.670 us | 2.430 us |
| thread_safe_contention_2 | get_refresh | 150 | 37.851 us | 28.380 us |
| thread_safe_contention_2 | dependency_edge | 19 | 9.370 us | 11.160 us |
| thread_safe_contention_2 | set_cell_invalidation | 32 | 16.710 us | 32.750 us |
| thread_safe_contention_2 | publish | 52 | 14.290 us | 20.460 us |
| thread_safe_contention_2 | in_flight_wait | 61 | 23.540 us | 9.951 us |
| thread_safe_contention_4 | other | 131 | 222.872 us | 4.440 us |
| thread_safe_contention_4 | get_refresh | 292 | 476.931 us | 76.661 us |
| thread_safe_contention_4 | dependency_edge | 33 | 73.721 us | 21.890 us |
| thread_safe_contention_4 | set_cell_invalidation | 64 | 162.600 us | 77.470 us |
| thread_safe_contention_4 | publish | 108 | 95.961 us | 45.230 us |
| thread_safe_contention_4 | in_flight_wait | 108 | 317.344 us | 31.510 us |
| thread_safe_contention_8 | other | 248 | 996.984 us | 8.450 us |
| thread_safe_contention_8 | get_refresh | 611 | 2.951 ms | 171.000 us |
| thread_safe_contention_8 | dependency_edge | 96 | 485.425 us | 66.442 us |
| thread_safe_contention_8 | set_cell_invalidation | 128 | 819.427 us | 193.542 us |
| thread_safe_contention_8 | publish | 318 | 869.607 us | 132.753 us |
| thread_safe_contention_8 | in_flight_wait | 1595 | 7.602 ms | 333.612 us |
| thread_safe_contention_16 | other | 488 | 5.015 ms | 15.310 us |
| thread_safe_contention_16 | get_refresh | 1217 | 9.843 ms | 298.592 us |
| thread_safe_contention_16 | dependency_edge | 190 | 1.881 ms | 118.890 us |
| thread_safe_contention_16 | set_cell_invalidation | 256 | 1.792 ms | 306.062 us |
| thread_safe_contention_16 | publish | 672 | 3.780 ms | 245.851 us |
| thread_safe_contention_16 | in_flight_wait | 7232 | 63.540 ms | 1.365 ms |

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
