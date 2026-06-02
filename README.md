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
| cached_reads | context | 11.953 ns | 11.933 ns - 11.979 ns |
| cached_reads | thread_safe_context | 449.374 ns | 447.502 ns - 451.446 ns |
| cold_first_get | context | 310.969 ns | 263.774 ns - 357.679 ns |
| cold_first_get | thread_safe_context | 1.101 us | 1.081 us - 1.120 us |
| dependency_fan_out | context / 32 | 7.588 us | 6.814 us - 8.439 us |
| dependency_fan_out | context / 256 | 55.629 us | 47.959 us - 65.181 us |
| dependency_fan_out | thread_safe_context / 32 | 43.200 us | 41.005 us - 45.307 us |
| dependency_fan_out | thread_safe_context / 256 | 345.613 us | 329.280 us - 360.974 us |
| memo_equality_suppression | context | 7.607 us | 6.741 us - 8.425 us |
| memo_equality_suppression | thread_safe_context | 21.728 us | 19.204 us - 24.113 us |
| effect_flushing | context | 89.082 ns | 88.810 ns - 89.375 ns |
| effect_flushing | thread_safe_context | 1.338 us | 1.334 us - 1.343 us |
| batch_storms | context / 64 | 3.594 us | 3.580 us - 3.610 us |
| batch_storms | thread_safe_context / 64 | 35.191 us | 35.142 us - 35.242 us |
| thread_safe_contention | 1 | 228.176 us | 226.095 us - 230.670 us |
| thread_safe_contention | 2 | 719.032 us | 711.340 us - 726.104 us |
| thread_safe_contention | 4 | 1.911 ms | 1.864 ms - 1.956 ms |
| thread_safe_contention | 8 | 5.432 ms | 5.066 ms - 5.788 ms |
| thread_safe_contention | 16 | 23.213 ms | 22.041 ms - 24.098 ms |
| profile_instrumentation | context_snapshot | 419.272 ns | 417.834 ns - 420.710 ns |
| profile_instrumentation | thread_safe_snapshot | 315.940 us | 314.490 us - 317.679 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 17 | 290.172 us | 24.720 us |
| thread_safe_contention_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 187 | 5.840 us | 71.090 us |
| thread_safe_contention_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 333 | 145.161 us | 106.901 us |
| thread_safe_contention_4 | 2 | 43 | 0 | 1 | 0 | 0 | 0 | 763 | 1.658 ms | 301.732 us |
| thread_safe_contention_8 | 2 | 71 | 0 | 1 | 0 | 0 | 0 | 1704 | 10.062 ms | 720.597 us |
| thread_safe_contention_16 | 2 | 137 | 0 | 1 | 0 | 0 | 0 | 4150 | 89.186 ms | 2.563 ms |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_contention_1 | other | 36 | 1.220 us | 10.000 us |
| thread_safe_contention_1 | get_refresh | 84 | 2.510 us | 15.760 us |
| thread_safe_contention_1 | dependency_edge | 17 | 530.000 ns | 9.750 us |
| thread_safe_contention_1 | set_cell_invalidation | 16 | 570.000 ns | 16.800 us |
| thread_safe_contention_1 | publish | 34 | 1.010 us | 18.780 us |
| thread_safe_contention_2 | other | 54 | 21.931 us | 2.270 us |
| thread_safe_contention_2 | get_refresh | 148 | 46.060 us | 28.930 us |
| thread_safe_contention_2 | dependency_edge | 17 | 4.380 us | 10.310 us |
| thread_safe_contention_2 | set_cell_invalidation | 32 | 19.420 us | 34.141 us |
| thread_safe_contention_2 | publish | 50 | 10.850 us | 24.290 us |
| thread_safe_contention_2 | in_flight_wait | 32 | 42.520 us | 6.960 us |
| thread_safe_contention_4 | other | 128 | 195.650 us | 5.390 us |
| thread_safe_contention_4 | get_refresh | 302 | 598.366 us | 82.440 us |
| thread_safe_contention_4 | dependency_edge | 43 | 58.551 us | 29.001 us |
| thread_safe_contention_4 | set_cell_invalidation | 64 | 139.770 us | 80.710 us |
| thread_safe_contention_4 | publish | 132 | 175.082 us | 75.580 us |
| thread_safe_contention_4 | in_flight_wait | 94 | 490.373 us | 28.611 us |
| thread_safe_contention_8 | other | 253 | 960.465 us | 10.170 us |
| thread_safe_contention_8 | get_refresh | 586 | 2.672 ms | 165.663 us |
| thread_safe_contention_8 | dependency_edge | 71 | 286.121 us | 49.520 us |
| thread_safe_contention_8 | set_cell_invalidation | 128 | 518.262 us | 176.810 us |
| thread_safe_contention_8 | publish | 281 | 742.573 us | 201.734 us |
| thread_safe_contention_8 | in_flight_wait | 385 | 4.883 ms | 116.700 us |
| thread_safe_contention_16 | other | 477 | 5.638 ms | 24.560 us |
| thread_safe_contention_16 | get_refresh | 1164 | 12.939 ms | 428.700 us |
| thread_safe_contention_16 | dependency_edge | 137 | 1.672 ms | 100.400 us |
| thread_safe_contention_16 | set_cell_invalidation | 256 | 3.408 ms | 417.553 us |
| thread_safe_contention_16 | publish | 647 | 6.144 ms | 1.013 ms |
| thread_safe_contention_16 | in_flight_wait | 1469 | 59.384 ms | 579.268 us |

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
