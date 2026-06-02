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

Regression budgets enforced by `python3 scripts/update-benchmark-results.py --check`:

| Profile | Max lock acquisitions | Site lock budgets |
|---|---:|---|
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 192 | set_cell_invalidation<=0, dependency_edge<=16, get_refresh<=32, publish<=32 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 900 | other<=800, set_cell_invalidation<=16, dependency_edge<=64, get_refresh<=2, publish<=2 |
| thread_safe_contention_same_slot_write_read_16 | 1600 | get_refresh<=700, publish<=450, in_flight_wait<=500, set_cell_invalidation<=32 |
| thread_safe_contention_independent_slots_16 | 1500 | other<=160, get_refresh<=700, publish<=700, dependency_edge<=16, set_cell_invalidation<=64 |
| thread_safe_contention_read_mostly_waiters_16 | 256 | get_refresh<=128, publish<=64, in_flight_wait<=64 |
| thread_safe_contention_batched_write_bursts_16 | 950 | other<=800, get_refresh<=128, dependency_edge<=64, set_cell_invalidation<=16, publish<=64, in_flight_wait<=64 |
| thread_safe_effect_contention_queue_coalescing_16 | 1800 | other<=900, dependency_edge<=900, set_cell_invalidation<=16, get_refresh<=64, publish<=0 |
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
| thread_safe_contention | same_slot_write_read / 16 | 17.665 ms | 22.097 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 3.026 ms | 3.288 ms | 10 |
| thread_safe_contention | independent_slots / 16 | 6.327 ms | 6.445 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 427.714 us | 509.889 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 796.461 us | 817.666 us | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.001 ms | 2.372 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 6.861 ms | 7.508 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 2.077 ms | 2.266 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.936 ms | 3.644 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 946.542 us | 1.410 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 1.754 ms | 2.066 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.605 ms | 1.995 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 4.528 ms | 4.757 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.908 ns | 11.810 ns - 12.080 ns |
| cached_reads | thread_safe_context | 55.969 ns | 55.848 ns - 56.099 ns |
| cold_first_get | context | 146.907 ns | 137.362 ns - 155.819 ns |
| cold_first_get | thread_safe_context | 1.267 us | 1.206 us - 1.331 us |
| dependency_fan_out | context / 32 | 5.465 us | 5.092 us - 5.810 us |
| dependency_fan_out | context / 256 | 45.240 us | 41.785 us - 48.239 us |
| dependency_fan_out | thread_safe_context / 32 | 41.209 us | 38.713 us - 43.560 us |
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
| thread_safe_effect_contention | queue_coalescing / 8 | 2.090 ms | 2.054 ms - 2.138 ms |
| thread_safe_effect_contention | queue_coalescing / 16 | 3.007 ms | 2.911 ms - 3.161 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 993.525 us | 944.959 us - 1.087 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 1.793 ms | 1.748 ms - 1.861 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.656 ms | 1.568 ms - 1.759 ms |
| thread_safe_effect_contention | batch_flush / 16 | 4.549 ms | 4.492 ms - 4.611 ms |
| profile_instrumentation | context_snapshot | 415.850 ns | 413.149 ns - 418.249 ns |
| profile_instrumentation | thread_safe_snapshot | 300.404 us | 299.514 us - 301.263 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 12 | 2.990 us | 28.880 us |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 290.000 ns | 3.190 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 270.000 ns | 3.280 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 280.000 ns | 2.980 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 270.000 ns | 3.090 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 280.000 ns | 3.530 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 9 | 450.000 ns | 8.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 18 | 530.000 ns | 7.401 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 36 | 1.250 us | 18.091 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 72 | 2.300 us | 27.340 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 144 | 4.630 us | 58.521 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 98 | 3.490 us | 93.831 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 103 | 22.890 us | 67.991 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 191 | 235.701 us | 116.701 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 361 | 1.229 ms | 203.762 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 716 | 7.833 ms | 457.133 us |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 3.070 us | 35.980 us |
| thread_safe_contention_same_slot_write_read_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 118 | 39.230 us | 66.880 us |
| thread_safe_contention_same_slot_write_read_4 | 2 | 40 | 0 | 1 | 0 | 0 | 0 | 298 | 396.252 us | 164.220 us |
| thread_safe_contention_same_slot_write_read_8 | 2 | 65 | 0 | 1 | 0 | 0 | 0 | 645 | 1.450 ms | 333.122 us |
| thread_safe_contention_same_slot_write_read_16 | 2 | 142 | 0 | 1 | 0 | 0 | 0 | 1440 | 4.099 ms | 586.191 us |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 69 | 2.750 us | 30.330 us |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 142 | 31.460 us | 65.900 us |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 297 | 490.572 us | 144.750 us |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 589 | 3.280 ms | 319.401 us |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 1206 | 20.701 ms | 733.506 us |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 73 | 3.090 us | 31.600 us |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 81 | 5.470 us | 33.230 us |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 113 | 67.751 us | 43.971 us |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 138 | 325.101 us | 63.400 us |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 18 | 0 | 1 | 0 | 0 | 0 | 115 | 460.536 us | 57.980 us |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 158 | 5.750 us | 122.972 us |
| thread_safe_contention_batched_write_bursts_2 | 9 | 13 | 0 | 8 | 0 | 0 | 0 | 217 | 203.451 us | 223.782 us |
| thread_safe_contention_batched_write_bursts_4 | 17 | 7 | 0 | 16 | 0 | 0 | 0 | 254 | 453.304 us | 247.473 us |
| thread_safe_contention_batched_write_bursts_8 | 33 | 10 | 0 | 32 | 0 | 0 | 0 | 457 | 1.820 ms | 463.824 us |
| thread_safe_contention_batched_write_bursts_16 | 65 | 3 | 0 | 64 | 0 | 0 | 0 | 724 | 6.596 ms | 462.583 us |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 480 | 448 | 15 | 1 | 1379 | 2.033 ms | 1.125 ms |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 448 | 384 | 8 | 1 | 1531 | 7.897 ms | 1.155 ms |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 152 | 152 | 20 | 1 | 559 | 940.393 us | 440.555 us |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 288 | 288 | 19 | 1 | 1040 | 7.861 ms | 916.386 us |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 35 | 2 | 5 | 1 | 655 | 1.984 ms | 342.062 us |
| thread_safe_effect_contention_batch_flush_16 | 66 | 4 | 0 | 67 | 2 | 5 | 1 | 1267 | 14.464 ms | 692.437 us |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 140.000 ns | 720.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 60.000 ns | 390.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 1.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 2 | 60.000 ns | 780.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 120.000 ns | 590.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 30.000 ns | 1.640 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 2 | 60.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 130.000 ns | 570.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 70.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 30.000 ns | 1.350 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 2 | 50.000 ns | 700.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 120.000 ns | 570.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 60.000 ns | 370.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 30.000 ns | 1.420 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 2 | 60.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 120.000 ns | 710.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 70.000 ns | 520.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 30.000 ns | 1.390 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 2 | 60.000 ns | 910.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 220.000 ns | 1.880 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 140.000 ns | 2.020 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 30.000 ns | 2.650 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 2 | 60.000 ns | 2.120 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 230.000 ns | 2.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 120.000 ns | 740.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 2.731 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 4 | 120.000 ns | 1.490 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 580.000 ns | 5.570 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 330.000 ns | 2.450 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 110.000 ns | 6.000 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 8 | 230.000 ns | 4.071 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 970.000 ns | 8.020 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 610.000 ns | 2.850 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 230.000 ns | 10.850 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 16 | 490.000 ns | 5.620 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.010 us | 18.670 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.170 us | 6.480 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 490.000 ns | 21.371 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 32 | 960.000 ns | 12.000 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 2.700 us | 20.780 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 130.000 ns | 1.330 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 110.000 ns | 6.020 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 490.000 ns | 63.751 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 2 | 60.000 ns | 1.950 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 88 | 22.280 us | 33.171 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 60.000 ns | 360.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 260.000 ns | 11.250 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 3 | 230.000 ns | 22.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 2 | 60.000 ns | 740.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 168 | 234.981 us | 70.771 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 530.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 490.000 ns | 22.500 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 3 | 100.000 ns | 22.090 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 2 | 50.000 ns | 810.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 324 | 1.228 ms | 141.601 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 60.000 ns | 380.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 960.000 ns | 44.721 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 1 | 20.000 ns | 16.300 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 2 | 60.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 646 | 7.830 ms | 306.791 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 60.000 ns | 350.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 1.940 us | 91.301 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 2 | 150.000 ns | 57.951 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 2 | 50.000 ns | 740.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 110.000 ns | 790.000 ns |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 34 | 1.710 us | 9.800 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 1.340 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 34 | 1.210 us | 24.050 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 100.000 ns | 590.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 64 | 29.860 us | 23.490 us |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 30.000 ns | 1.350 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 35 | 9.240 us | 41.450 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 14 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 6 | 190.000 ns | 690.000 ns |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 150 | 285.352 us | 60.000 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 30.000 ns | 1.380 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 1 | 50.000 ns | 4.860 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 87 | 110.630 us | 97.290 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 53 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 10 | 1.280 us | 1.130 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 297 | 998.608 us | 116.982 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 1.410 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 3 | 6.910 us | 14.880 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 162 | 443.421 us | 198.720 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 172 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 4 | 140.000 ns | 910.000 ns |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 661 | 2.829 ms | 232.241 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 1.600 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 373 | 1.269 ms | 351.440 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 401 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 130.000 ns | 1.110 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 32 | 1.420 us | 7.960 us |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 1.880 us |
| thread_safe_contention_independent_slots_1 | publish | 32 | 1.170 us | 19.380 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 230.000 ns | 1.900 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 66 | 28.060 us | 18.330 us |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 50.000 ns | 2.520 us |
| thread_safe_contention_independent_slots_2 | publish | 66 | 3.120 us | 43.150 us |
| thread_safe_contention_independent_slots_4 | other | 22 | 690.000 ns | 4.600 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 134 | 245.591 us | 39.040 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 120.000 ns | 5.190 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 3 | 2.580 us | 7.520 us |
| thread_safe_contention_independent_slots_4 | publish | 134 | 241.591 us | 88.400 us |
| thread_safe_contention_independent_slots_8 | other | 38 | 1.160 us | 8.720 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 270 | 1.435 ms | 87.750 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 250.000 ns | 10.990 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 3 | 39.250 us | 13.020 us |
| thread_safe_contention_independent_slots_8 | publish | 270 | 1.805 ms | 198.921 us |
| thread_safe_contention_independent_slots_16 | other | 92 | 5.850 us | 16.801 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 542 | 8.625 ms | 191.840 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 490.000 ns | 22.250 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 14 | 637.255 us | 53.431 us |
| thread_safe_contention_independent_slots_16 | publish | 542 | 11.433 ms | 449.184 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 110.000 ns | 900.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 34 | 1.720 us | 8.760 us |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 40.000 ns | 1.330 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 34 | 1.220 us | 20.610 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 110.000 ns | 630.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 38 | 3.690 us | 9.580 us |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 30.000 ns | 1.330 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 36 | 1.640 us | 21.690 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 120.000 ns | 620.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 60 | 53.241 us | 16.501 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 30.000 ns | 1.340 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 38 | 14.360 us | 25.510 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 10 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 110.000 ns | 630.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 70 | 205.440 us | 25.220 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 1.350 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 42 | 119.511 us | 36.200 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 120.000 ns | 650.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 63 | 426.506 us | 27.410 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 20.000 ns | 1.540 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 38 | 33.890 us | 28.380 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 2.930 us | 17.780 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 32 | 1.000 us | 14.021 us |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 160.000 ns | 5.940 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 620.000 ns | 60.010 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 32 | 1.040 us | 25.221 us |
| thread_safe_contention_batched_write_bursts_2 | other | 118 | 105.031 us | 49.841 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 38 | 78.350 us | 31.260 us |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 250.000 ns | 11.400 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 18 | 10.410 us | 97.130 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 26 | 9.410 us | 34.151 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 9 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 180 | 331.314 us | 93.361 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 20 | 36.010 us | 25.280 us |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 490.000 ns | 22.640 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 9 | 16.540 us | 86.332 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 17 | 68.950 us | 19.860 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 12 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 345 | 1.734 ms | 167.721 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 26 | 34.830 us | 77.610 us |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 940.000 ns | 46.021 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 12 | 2.320 us | 127.081 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 22 | 48.031 us | 45.391 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 20 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 646 | 6.594 ms | 292.513 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 6 | 210.000 ns | 18.680 us |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 1.870 us | 88.090 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 2 | 70.000 ns | 43.100 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 6 | 220.000 ns | 20.200 us |
| thread_safe_effect_contention_queue_coalescing_8 | other | 432 | 1.800 ms | 182.501 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 928 | 224.452 us | 833.855 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 19 | 8.900 us | 108.732 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 692 | 7.866 ms | 338.382 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 832 | 30.511 us | 755.816 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 7 | 270.000 ns | 60.391 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 194 | 248.630 us | 34.371 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 304 | 82.920 us | 278.083 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 61 | 608.843 us | 128.101 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 304 | 3.808 ms | 47.280 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 576 | 434.933 us | 530.523 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 160 | 3.617 ms | 338.583 us |
| thread_safe_effect_contention_batch_flush_8 | other | 603 | 1.968 ms | 213.922 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 6 | 660.000 ns | 11.000 us |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 37 | 1.150 us | 54.300 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 3 | 110.000 ns | 44.120 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 6 | 13.660 us | 18.720 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1180 | 14.461 ms | 464.134 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 6 | 290.000 ns | 19.150 us |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 69 | 2.070 us | 104.191 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 4 | 750.000 ns | 76.181 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 8 | 280.000 ns | 28.781 us |

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
