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
slot. Per-slot dependency summaries let cell-only dirty refreshes claim the
SlotId-partitioned recompute sidecar and skip the graph-locked `get_refresh`
dependency scan before the final publish mutation. Changed-cell and slot-value
invalidation build an explicit frontier plan, then apply dirty flags, revisions,
and effect scheduling in one graph-mutex mutation boundary. Sharded-lock or CAS
variants should wait for lock wait/hold benchmark evidence and a Loom or
Shuttle safety model for stale in-flight completion, invalidation during
compute, effect scheduling/disposal, and re-entrant callbacks. A lock-free
versioned optimistic read path is deferred
until cached values can be retained independently of graph-protected
erased-value storage.

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
| thread_safe_contention_same_slot_write_read_16 | 1000 | get_refresh<=160, publish<=256, in_flight_wait<=700, set_cell_invalidation<=32 |
| thread_safe_contention_independent_slots_16 | 700 | other<=160, get_refresh<=64, publish<=320, dependency_edge<=16, set_cell_invalidation<=64 |
| thread_safe_contention_read_mostly_waiters_16 | 256 | get_refresh<=128, publish<=64, in_flight_wait<=64 |
| thread_safe_contention_batched_write_bursts_16 | 950 | other<=800, get_refresh<=128, dependency_edge<=64, set_cell_invalidation<=16, publish<=64, in_flight_wait<=64 |
| thread_safe_effect_contention_queue_coalescing_16 | 2600 | other<=900, dependency_edge<=1600, set_cell_invalidation<=16, get_refresh<=64, publish<=0 |
| thread_safe_effect_contention_cleanup_execution_16 | 1300 | other<=400, dependency_edge<=700, set_cell_invalidation<=256, get_refresh<=0, publish<=0 |
| thread_safe_effect_contention_batch_flush_16 | 1500 | other<=1300, get_refresh<=32, dependency_edge<=96, set_cell_invalidation<=16, publish<=32 |

Budgets use deterministic lock acquisition counts instead of elapsed wait/hold time.

Synchronization strategy adoption gate:

| Strategy | Status | Required throughput evidence | Required p50/p95 latency evidence | Lock-site and safety gate |
|---|---|---|---|---|
| current_std_mutex_condvar | baseline | thread_safe_contention and thread_safe_effect_contention at 8/16 workers | p50/p95 latency for same-slot, read-mostly, batch, and effect-heavy cases | must stay within current lock-site budgets and Loom safety coverage |
| narrower_condvar_wakeups | adopted for per-slot recompute waiters | same-slot write/read and read-mostly waiter throughput at 8/16 workers | p50/p95 latency for waiter wakeup handoff and stale-completion retry | must not regress effect queue, cleanup, or batch flush budgets |
| parking_lot_style_parking | candidate only | same contention matrix measured against current_std_mutex_condvar | p50/p95 latency for parking/unparking under 8/16 workers | requires no worse lock-site budgets plus a deadlock/starvation model |
| targeted_cas | candidate only | fresh cached reads and independent-slot throughput at 8/16 workers | p50/p95 latency for revision validation fallback and publish races | requires unchanged effect/batch/disposal budgets plus Loom/Shuttle proof |

Candidates do not replace the current strategy before the same run reports throughput, p50/p95 latency, and lock-site budgets for the required 8/16-worker cases.

Required latency evidence uses Criterion sample per-iteration timing.

| Group | Case | p50 | p95 | Samples |
|---|---|---:|---:|---:|
| thread_safe_contention | same_slot_write_read / 8 | 5.671 ms | 5.993 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 7.010 ms | 7.180 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 3.026 ms | 3.288 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 1.996 ms | 2.113 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 427.714 us | 509.889 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 796.461 us | 817.666 us | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.001 ms | 2.372 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 2.734 ms | 2.818 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 2.077 ms | 2.266 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.936 ms | 3.644 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 946.542 us | 1.410 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 1.754 ms | 2.066 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.605 ms | 1.995 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.224 ms | 4.257 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.884 ns | 11.838 ns - 11.929 ns |
| cached_reads | thread_safe_context | 56.525 ns | 56.071 ns - 57.277 ns |
| cold_first_get | context | 119.939 ns | 113.819 ns - 127.974 ns |
| cold_first_get | thread_safe_context | 1.252 us | 1.216 us - 1.286 us |
| dependency_fan_out | context / 32 | 4.760 us | 4.408 us - 5.123 us |
| dependency_fan_out | context / 256 | 45.240 us | 41.785 us - 48.239 us |
| dependency_fan_out | thread_safe_context / 32 | 36.972 us | 35.610 us - 38.402 us |
| dependency_fan_out | thread_safe_context / 256 | 332.443 us | 310.295 us - 353.822 us |
| set_cell_invalidation | high_fan_out / 512 | 265.796 us | 230.721 us - 294.348 us |
| set_cell_invalidation | same_slot_contention / 1 | 47.326 us | 46.935 us - 47.705 us |
| set_cell_invalidation | same_slot_contention / 2 | 139.539 us | 137.591 us - 141.207 us |
| set_cell_invalidation | same_slot_contention / 4 | 244.445 us | 241.737 us - 247.492 us |
| set_cell_invalidation | same_slot_contention / 8 | 491.318 us | 483.876 us - 497.616 us |
| set_cell_invalidation | same_slot_contention / 16 | 1.109 ms | 1.096 ms - 1.125 ms |
| set_cell_invalidation | independent_slot_contention / 1 | 48.128 us | 46.630 us - 49.522 us |
| set_cell_invalidation | independent_slot_contention / 2 | 87.369 us | 84.954 us - 90.302 us |
| set_cell_invalidation | independent_slot_contention / 4 | 159.753 us | 153.005 us - 169.502 us |
| set_cell_invalidation | independent_slot_contention / 8 | 322.765 us | 320.520 us - 325.708 us |
| set_cell_invalidation | independent_slot_contention / 16 | 684.035 us | 678.829 us - 689.449 us |
| set_cell_invalidation | batched_write_bursts / 1 | 207.688 us | 182.312 us - 239.360 us |
| set_cell_invalidation | batched_write_bursts / 2 | 206.201 us | 201.105 us - 211.635 us |
| set_cell_invalidation | batched_write_bursts / 4 | 499.164 us | 489.809 us - 508.988 us |
| set_cell_invalidation | batched_write_bursts / 8 | 871.485 us | 831.943 us - 918.413 us |
| set_cell_invalidation | batched_write_bursts / 16 | 2.502 ms | 2.434 ms - 2.582 ms |
| memo_equality_suppression | context | 5.916 us | 5.041 us - 6.833 us |
| memo_equality_suppression | thread_safe_context | 51.942 us | 49.383 us - 54.550 us |
| effect_flushing | context | 89.830 ns | 89.309 ns - 90.664 ns |
| effect_flushing | thread_safe_context | 1.414 us | 1.401 us - 1.432 us |
| batch_storms | context / 64 | 3.679 us | 3.632 us - 3.742 us |
| batch_storms | thread_safe_context / 64 | 34.530 us | 34.501 us - 34.562 us |
| thread_safe_contention | same_slot_write_read / 1 | 185.915 us | 183.019 us - 189.096 us |
| thread_safe_contention | same_slot_write_read / 2 | 624.101 us | 612.928 us - 634.271 us |
| thread_safe_contention | same_slot_write_read / 4 | 1.680 ms | 1.659 ms - 1.700 ms |
| thread_safe_contention | same_slot_write_read / 8 | 5.542 ms | 5.245 ms - 5.766 ms |
| thread_safe_contention | same_slot_write_read / 16 | 7.023 ms | 6.979 ms - 7.075 ms |
| thread_safe_contention | independent_slots / 1 | 194.652 us | 193.351 us - 196.639 us |
| thread_safe_contention | independent_slots / 2 | 617.864 us | 605.152 us - 629.988 us |
| thread_safe_contention | independent_slots / 4 | 1.271 ms | 1.247 ms - 1.302 ms |
| thread_safe_contention | independent_slots / 8 | 3.019 ms | 2.861 ms - 3.162 ms |
| thread_safe_contention | independent_slots / 16 | 2.009 ms | 1.965 ms - 2.049 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 196.274 us | 193.446 us - 199.489 us |
| thread_safe_contention | read_mostly_waiters / 2 | 212.806 us | 210.307 us - 216.165 us |
| thread_safe_contention | read_mostly_waiters / 4 | 282.537 us | 274.366 us - 297.184 us |
| thread_safe_contention | read_mostly_waiters / 8 | 441.589 us | 427.876 us - 459.952 us |
| thread_safe_contention | read_mostly_waiters / 16 | 796.127 us | 789.325 us - 802.938 us |
| thread_safe_contention | batched_write_bursts / 1 | 368.647 us | 367.201 us - 370.193 us |
| thread_safe_contention | batched_write_bursts / 2 | 724.050 us | 583.208 us - 880.374 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.170 ms | 1.090 ms - 1.247 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.065 ms | 1.982 ms - 2.157 ms |
| thread_safe_contention | batched_write_bursts / 16 | 2.730 ms | 2.683 ms - 2.771 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 2.090 ms | 2.054 ms - 2.138 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.007 ms | 2.911 ms - 3.161 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 993.525 us | 944.959 us - 1.087 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 1.793 ms | 1.748 ms - 1.861 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.656 ms | 1.568 ms - 1.759 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.212 ms | 4.190 ms - 4.233 ms |
| profile_instrumentation | context_snapshot | 415.850 ns | 413.149 ns - 418.249 ns |
| profile_instrumentation | thread_safe_snapshot | 300.404 us | 299.514 us - 301.263 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 14.400 us | 21.030 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 270.000 ns | 3.190 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 280.000 ns | 2.990 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 280.000 ns | 3.110 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 250.000 ns | 3.130 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 330.000 ns | 4.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 3.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 520.000 ns | 7.070 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.070 us | 13.680 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.090 us | 27.920 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.210 us | 53.361 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.020 us | 95.841 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 102 | 18.530 us | 78.360 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 187 | 249.650 us | 130.041 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 1.881 ms | 286.803 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 7.793 ms | 489.203 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 990.000 ns | 25.730 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 25 | 0 | 1 | 0 | 0 | 0 | 53 | 5.470 us | 49.411 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 42 | 0 | 1 | 0 | 0 | 0 | 165 | 10.860 us | 100.650 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 90 | 0 | 1 | 0 | 0 | 0 | 374 | 16.530 us | 153.062 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 198 | 0 | 1 | 0 | 0 | 0 | 762 | 25.221 us | 287.232 us |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 1.010 us | 19.770 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 4.360 us | 43.300 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 137 | 380.174 us | 143.611 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 239 | 1.987 ms | 270.141 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 510 | 8.160 ms | 496.193 us |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 930.000 ns | 21.150 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 31 | 1.310 us | 22.210 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 43 | 5.290 us | 31.710 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 80 | 26.141 us | 40.761 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 79 | 35.510 us | 40.340 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.340 us | 109.450 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 22 | 0 | 8 | 0 | 0 | 0 | 195 | 78.102 us | 192.582 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 8 | 0 | 16 | 0 | 0 | 0 | 209 | 204.683 us | 170.242 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 8 | 0 | 32 | 0 | 0 | 0 | 394 | 1.493 ms | 322.461 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 2 | 0 | 64 | 0 | 0 | 0 | 713 | 6.654 ms | 455.373 us |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 128 | 96 | 4 | 1 | 581 | 1.372 ms | 437.245 us |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 850 | 5.925 ms | 554.487 us |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 152 | 152 | 20 | 1 | 581 | 5.418 ms | 572.415 us |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 288 | 288 | 19 | 1 | 1004 | 4.443 ms | 865.936 us |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 35 | 2 | 5 | 1 | 646 | 3.581 ms | 368.893 us |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 66 | 1 | 3 | 1 | 1246 | 15.487 ms | 648.627 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 110.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 80.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 1.630 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 40.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 550.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 80.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 1.570 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 40.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 120.000 ns | 520.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 80.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 1.750 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 40.000 ns | 490.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 420.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.700 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 40.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 130.000 ns | 800.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 130.000 ns | 1.120 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 40.000 ns | 800.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 120.000 ns | 750.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 90.000 ns | 570.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 1.730 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 40.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 230.000 ns | 1.770 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 140.000 ns | 770.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 70.000 ns | 3.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 80.000 ns | 990.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 450.000 ns | 3.660 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 340.000 ns | 1.380 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 140.000 ns | 6.700 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 140.000 ns | 1.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 890.000 ns | 6.720 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 610.000 ns | 2.710 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 280.000 ns | 14.590 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 310.000 ns | 3.900 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 1.890 us | 13.410 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.190 us | 5.481 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 550.000 ns | 26.820 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 580.000 ns | 7.650 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 3.270 us | 21.380 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 60.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 7.000 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 520.000 ns | 66.581 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 30.000 ns | 510.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 88 | 17.820 us | 35.560 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 240.000 ns | 14.880 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 3 | 360.000 ns | 27.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 30.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 166 | 248.920 us | 71.681 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 90.000 ns | 330.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 530.000 ns | 29.620 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 2 | 70.000 ns | 27.930 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 1.880 ms | 164.052 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 340.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.150 us | 59.591 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 140.000 ns | 62.340 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 20.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 7.791 ms | 305.472 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 70.000 ns | 500.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.950 us | 120.161 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 90.000 ns | 62.450 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 30.000 ns | 620.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 130.000 ns | 1.140 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 130.000 ns | 1.030 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 30.000 ns | 2.520 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 700.000 ns | 21.040 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 110.000 ns | 540.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 4 | 3.640 us | 2.990 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 1.590 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 25 | 1.690 us | 44.291 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 4 | 120.000 ns | 660.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 26 | 8.670 us | 12.020 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 1.690 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 42 | 2.040 us | 86.280 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 92 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 4 | 120.000 ns | 1.190 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 50 | 10.220 us | 21.822 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 2.680 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 90 | 6.150 us | 127.370 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 229 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 6 | 210.000 ns | 760.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 108 | 8.721 us | 32.930 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 1.640 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 1 | 70.000 ns | 6.230 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 198 | 16.180 us | 245.672 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 448 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 150.000 ns | 710.000 ns |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 380.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 40.000 ns | 1.610 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 740.000 ns | 17.070 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 240.000 ns | 1.990 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 140.000 ns | 720.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 80.000 ns | 3.150 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 3.900 us | 37.440 us |
| thread_safe_contention_independent_slots_4 | other | 43 | 75.451 us | 6.030 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 340.000 ns | 1.360 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 140.000 ns | 6.510 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 15 | 57.961 us | 42.760 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 246.282 us | 86.951 us |
| thread_safe_contention_independent_slots_8 | other | 63 | 81.211 us | 10.040 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 590.000 ns | 2.760 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 250.000 ns | 13.930 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 17 | 230.072 us | 61.400 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.674 ms | 182.011 us |
| thread_safe_contention_independent_slots_16 | other | 144 | 338.652 us | 22.990 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.270 us | 5.770 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 570.000 ns | 30.350 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 47 | 1.435 ms | 122.292 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 6.385 ms | 314.791 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 130.000 ns | 1.520 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 90.000 ns | 410.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 1.900 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 680.000 ns | 17.320 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 120.000 ns | 560.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 6 | 450.000 ns | 2.230 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 1.590 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 700.000 ns | 17.830 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 120.000 ns | 590.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 12 | 4.100 us | 4.350 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 1.670 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 1.040 us | 25.100 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 180.000 ns | 660.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 26 | 24.831 us | 11.970 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 30.000 ns | 1.820 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 1.100 us | 26.311 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 32 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 640.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 15 | 30.550 us | 8.200 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 40.000 ns | 1.700 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 18 | 4.800 us | 29.800 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 41 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.950 us | 18.470 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 380.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 8.420 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 520.000 ns | 60.510 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 650.000 ns | 21.670 us |
| thread_safe_contention_batched_write_bursts_2 | other | 124 | 67.371 us | 38.960 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 350.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 320.000 ns | 16.130 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 21 | 1.810 us | 106.652 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 22 | 8.521 us | 30.490 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 18 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 176 | 203.413 us | 73.051 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 360.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 640.000 ns | 31.881 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 7 | 230.000 ns | 41.000 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 8 | 320.000 ns | 23.950 us |
| thread_safe_contention_batched_write_bursts_8 | other | 336 | 1.490 ms | 149.220 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 80.000 ns | 370.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 970.000 ns | 63.131 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 7 | 1.510 us | 82.710 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 8 | 320.000 ns | 27.030 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 644 | 6.652 ms | 280.682 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 90.000 ns | 350.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.950 us | 125.271 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 1 | 30.000 ns | 39.010 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 2 | 60.000 ns | 10.060 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 352 | 1.301 ms | 148.923 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 224 | 68.540 us | 229.742 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 5 | 2.260 us | 58.580 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 657 | 5.919 ms | 281.014 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 192 | 5.580 us | 224.213 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 1 | 30.000 ns | 49.260 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 205 | 3.918 ms | 42.040 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 304 | 938.938 us | 356.643 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 72 | 561.454 us | 173.732 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 286 | 1.073 ms | 41.530 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 576 | 375.462 us | 551.303 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 142 | 2.994 ms | 273.103 us |
| thread_safe_effect_contention_batch_flush_8 | other | 602 | 3.579 ms | 226.823 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 50.000 ns | 500.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 37 | 1.180 us | 84.000 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 50.000 ns | 39.520 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 120.000 ns | 18.050 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1172 | 15.485 ms | 433.995 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 60.000 ns | 910.000 ns |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 67 | 2.050 us | 133.491 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 70.000 ns | 65.051 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 120.000 ns | 15.180 us |

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
