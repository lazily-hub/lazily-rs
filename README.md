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
| cached_reads | context | 11.477 ns | 11.462 ns - 11.492 ns |
| cached_reads | thread_safe_context | 541.954 ns | 535.156 ns - 552.626 ns |
| cold_first_get | context | 148.229 ns | 129.799 ns - 170.864 ns |
| cold_first_get | thread_safe_context | 939.592 ns | 932.338 ns - 946.645 ns |
| dependency_fan_out | context / 32 | 5.302 us | 4.893 us - 5.679 us |
| dependency_fan_out | context / 256 | 48.500 us | 43.916 us - 53.555 us |
| dependency_fan_out | thread_safe_context / 32 | 39.433 us | 38.457 us - 40.423 us |
| dependency_fan_out | thread_safe_context / 256 | 415.214 us | 386.182 us - 443.927 us |
| memo_equality_suppression | context | 6.858 us | 6.013 us - 7.678 us |
| memo_equality_suppression | thread_safe_context | 23.413 us | 21.970 us - 24.837 us |
| effect_flushing | context | 90.136 ns | 89.843 ns - 90.462 ns |
| effect_flushing | thread_safe_context | 1.339 us | 1.316 us - 1.366 us |
| batch_storms | context / 64 | 3.856 us | 3.628 us - 4.138 us |
| batch_storms | thread_safe_context / 64 | 33.552 us | 33.506 us - 33.599 us |
| thread_safe_contention | 1 | 224.896 us | 217.414 us - 237.908 us |
| thread_safe_contention | 2 | 814.329 us | 789.289 us - 845.507 us |
| thread_safe_contention | 4 | 2.114 ms | 2.047 ms - 2.178 ms |
| thread_safe_contention | 8 | 7.167 ms | 6.765 ms - 7.623 ms |
| thread_safe_contention | 16 | 26.407 ms | 24.743 ms - 27.761 ms |
| profile_instrumentation | context_snapshot | 432.221 ns | 417.167 ns - 457.023 ns |
| profile_instrumentation | thread_safe_snapshot | 348.653 us | 311.290 us - 395.873 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 658 | 30.610 us | 112.941 us |
| thread_safe_contention_1 | 2 | 17 | 0 | 17 | 16 | 0 | 0 | 203 | 6.170 us | 65.331 us |
| thread_safe_contention_2 | 2 | 19 | 0 | 19 | 18 | 0 | 0 | 381 | 139.101 us | 107.951 us |
| thread_safe_contention_4 | 2 | 49 | 0 | 49 | 48 | 0 | 0 | 1294 | 3.317 ms | 481.921 us |
| thread_safe_contention_8 | 2 | 87 | 0 | 87 | 86 | 0 | 0 | 3151 | 14.890 ms | 1.042 ms |
| thread_safe_contention_16 | 2 | 141 | 0 | 141 | 140 | 0 | 0 | 10082 | 99.083 ms | 2.668 ms |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_contention_1 | other | 53 | 1.670 us | 6.041 us |
| thread_safe_contention_1 | get_refresh | 67 | 1.940 us | 12.120 us |
| thread_safe_contention_1 | dependency_edge | 33 | 970.000 ns | 18.840 us |
| thread_safe_contention_1 | set_cell_invalidation | 16 | 600.000 ns | 16.900 us |
| thread_safe_contention_1 | publish | 34 | 990.000 ns | 11.430 us |
| thread_safe_contention_2 | other | 87 | 32.761 us | 8.120 us |
| thread_safe_contention_2 | get_refresh | 115 | 31.300 us | 21.670 us |
| thread_safe_contention_2 | dependency_edge | 37 | 15.150 us | 19.930 us |
| thread_safe_contention_2 | set_cell_invalidation | 32 | 19.540 us | 33.051 us |
| thread_safe_contention_2 | publish | 53 | 15.690 us | 15.760 us |
| thread_safe_contention_2 | in_flight_wait | 57 | 24.660 us | 9.420 us |
| thread_safe_contention_4 | other | 194 | 511.286 us | 23.940 us |
| thread_safe_contention_4 | get_refresh | 225 | 613.017 us | 73.430 us |
| thread_safe_contention_4 | dependency_edge | 97 | 290.411 us | 75.481 us |
| thread_safe_contention_4 | set_cell_invalidation | 64 | 199.140 us | 108.650 us |
| thread_safe_contention_4 | publish | 148 | 267.671 us | 71.490 us |
| thread_safe_contention_4 | in_flight_wait | 566 | 1.435 ms | 128.930 us |
| thread_safe_contention_8 | other | 379 | 2.026 ms | 56.450 us |
| thread_safe_contention_8 | get_refresh | 397 | 1.451 ms | 119.340 us |
| thread_safe_contention_8 | dependency_edge | 173 | 998.424 us | 155.981 us |
| thread_safe_contention_8 | set_cell_invalidation | 128 | 687.187 us | 183.641 us |
| thread_safe_contention_8 | publish | 294 | 849.755 us | 138.461 us |
| thread_safe_contention_8 | in_flight_wait | 1780 | 8.878 ms | 387.803 us |
| thread_safe_contention_16 | other | 726 | 7.000 ms | 94.830 us |
| thread_safe_contention_16 | get_refresh | 751 | 6.361 ms | 221.452 us |
| thread_safe_contention_16 | dependency_edge | 281 | 3.490 ms | 206.080 us |
| thread_safe_contention_16 | set_cell_invalidation | 256 | 3.125 ms | 357.223 us |
| thread_safe_contention_16 | publish | 552 | 3.182 ms | 219.422 us |
| thread_safe_contention_16 | in_flight_wait | 7516 | 75.925 ms | 1.569 ms |

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
