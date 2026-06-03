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

## Development

Run the local CI-equivalent suite with:

```bash
make check
```

The Makefile also exposes focused targets such as `make test-tokio`,
`make test-loom`, `make benchmark-check`, and `make benchmark-update`.

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
and effect scheduling in one graph-mutex mutation boundary; slot-only
changed-cell frontiers may publish dirty state through per-node sidecars with
cache-revision dirty epochs instead. The `thread_safe_graph_propagation` benchmarks
compare fan-out eager validation, fan-out/fan-in lazy dirty epoch publication,
and fan-in batched flush behavior with lock attribution, effect queue pushes,
dependency-edge counters, sidecar dirty marks, sidecar fallbacks, and dirty
epoch advances. Sharded-lock or CAS variants should wait for lock wait/hold
benchmark evidence and a Loom or Shuttle safety model for stale in-flight
completion, invalidation during compute, dynamic dependency cleanup/disposal,
effect scheduling/disposal, and re-entrant callbacks. A lock-free versioned
optimistic read path is deferred
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
| thread_safe_contention_read_mostly_waiters_16 | 256 | get_refresh<=128, publish<=64, in_flight_wait<=96 |
| thread_safe_contention_batched_write_bursts_16 | 950 | other<=800, get_refresh<=128, dependency_edge<=64, set_cell_invalidation<=16, publish<=64, in_flight_wait<=64 |
| thread_safe_effect_contention_queue_coalescing_16 | 2600 | other<=900, dependency_edge<=1600, set_cell_invalidation<=16, get_refresh<=64, publish<=0 |
| thread_safe_effect_contention_cleanup_execution_16 | 1300 | other<=450, dependency_edge<=700, set_cell_invalidation<=256, get_refresh<=0, publish<=0 |
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
| thread_safe_contention | same_slot_write_read / 8 | 2.291 ms | 2.387 ms | 10 |
| thread_safe_contention | same_slot_write_read / 16 | 6.525 ms | 7.099 ms | 10 |
| thread_safe_contention | independent_slots / 8 | 769.231 us | 938.785 us | 10 |
| thread_safe_contention | independent_slots / 16 | 1.943 ms | 2.164 ms | 10 |
| thread_safe_contention | read_mostly_waiters / 8 | 622.294 us | 644.583 us | 10 |
| thread_safe_contention | read_mostly_waiters / 16 | 1.167 ms | 1.339 ms | 10 |
| thread_safe_contention | batched_write_bursts / 8 | 2.810 ms | 2.971 ms | 10 |
| thread_safe_contention | batched_write_bursts / 16 | 2.601 ms | 2.890 ms | 10 |
| thread_safe_effect_contention | queue_coalescing / 8 | 899.030 us | 963.410 us | 10 |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.048 ms | 2.212 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.086 ms | 1.182 ms | 10 |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.406 ms | 2.751 ms | 10 |
| thread_safe_effect_contention | batch_flush / 8 | 1.449 ms | 1.730 ms | 10 |
| thread_safe_effect_contention | batch_flush / 16 | 3.756 ms | 4.060 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.815 ms | 3.915 ms | 10 |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.222 ms | 6.333 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.895 ms | 2.015 ms | 10 |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.580 ms | 3.876 ms | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 500.460 us | 542.983 us | 10 |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.020 ms | 1.144 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.030 ms | 1.137 ms | 10 |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.507 ms | 10 |

Criterion estimates are local mean wall-clock time per iteration.

| Group | Case | Mean | 95% CI |
|---|---|---:|---:|
| cached_reads | context | 11.728 ns | 11.681 ns - 11.785 ns |
| cached_reads | thread_safe_context | 73.141 ns | 72.747 ns - 73.554 ns |
| cold_first_get | context | 124.060 ns | 107.656 ns - 151.572 ns |
| cold_first_get | thread_safe_context | 1.082 us | 1.058 us - 1.108 us |
| dependency_fan_out | context / 32 | 4.814 us | 4.363 us - 5.348 us |
| dependency_fan_out | context / 256 | 37.428 us | 34.000 us - 41.543 us |
| dependency_fan_out | thread_safe_context / 32 | 33.600 us | 29.220 us - 38.091 us |
| dependency_fan_out | thread_safe_context / 256 | 287.949 us | 243.368 us - 334.962 us |
| set_cell_invalidation | high_fan_out / 512 | 289.384 us | 264.914 us - 323.842 us |
| set_cell_invalidation | same_slot_contention / 1 | 43.883 us | 43.139 us - 44.590 us |
| set_cell_invalidation | same_slot_contention / 2 | 92.194 us | 89.596 us - 95.104 us |
| set_cell_invalidation | same_slot_contention / 4 | 182.203 us | 176.112 us - 189.264 us |
| set_cell_invalidation | same_slot_contention / 8 | 394.250 us | 381.822 us - 408.447 us |
| set_cell_invalidation | same_slot_contention / 16 | 870.283 us | 857.965 us - 883.967 us |
| set_cell_invalidation | independent_slot_contention / 1 | 43.780 us | 42.880 us - 44.537 us |
| set_cell_invalidation | independent_slot_contention / 2 | 79.176 us | 77.272 us - 81.054 us |
| set_cell_invalidation | independent_slot_contention / 4 | 129.498 us | 125.978 us - 133.095 us |
| set_cell_invalidation | independent_slot_contention / 8 | 290.808 us | 283.505 us - 299.703 us |
| set_cell_invalidation | independent_slot_contention / 16 | 587.167 us | 571.047 us - 603.865 us |
| set_cell_invalidation | batched_write_bursts / 1 | 160.885 us | 158.939 us - 163.481 us |
| set_cell_invalidation | batched_write_bursts / 2 | 163.799 us | 159.756 us - 169.194 us |
| set_cell_invalidation | batched_write_bursts / 4 | 466.013 us | 456.860 us - 476.333 us |
| set_cell_invalidation | batched_write_bursts / 8 | 809.706 us | 793.443 us - 825.990 us |
| set_cell_invalidation | batched_write_bursts / 16 | 1.918 ms | 1.834 ms - 2.006 ms |
| memo_equality_suppression | context | 4.201 us | 3.753 us - 4.738 us |
| memo_equality_suppression | thread_safe_context | 42.818 us | 41.838 us - 44.000 us |
| effect_flushing | context | 86.758 ns | 86.688 ns - 86.829 ns |
| effect_flushing | thread_safe_context | 1.011 us | 1.004 us - 1.022 us |
| batch_storms | context / 64 | 3.552 us | 3.540 us - 3.564 us |
| batch_storms | thread_safe_context / 64 | 9.384 us | 9.348 us - 9.422 us |
| thread_safe_contention | same_slot_write_read / 1 | 110.393 us | 109.004 us - 111.574 us |
| thread_safe_contention | same_slot_write_read / 2 | 297.831 us | 286.788 us - 309.513 us |
| thread_safe_contention | same_slot_write_read / 4 | 747.294 us | 704.401 us - 793.703 us |
| thread_safe_contention | same_slot_write_read / 8 | 2.269 ms | 2.207 ms - 2.325 ms |
| thread_safe_contention | same_slot_write_read / 16 | 6.373 ms | 6.034 ms - 6.685 ms |
| thread_safe_contention | independent_slots / 1 | 109.264 us | 108.313 us - 110.297 us |
| thread_safe_contention | independent_slots / 2 | 225.835 us | 216.564 us - 234.556 us |
| thread_safe_contention | independent_slots / 4 | 404.920 us | 382.741 us - 425.448 us |
| thread_safe_contention | independent_slots / 8 | 793.649 us | 751.169 us - 841.424 us |
| thread_safe_contention | independent_slots / 16 | 1.964 ms | 1.886 ms - 2.044 ms |
| thread_safe_contention | read_mostly_waiters / 1 | 109.944 us | 108.913 us - 110.927 us |
| thread_safe_contention | read_mostly_waiters / 2 | 163.445 us | 159.474 us - 167.403 us |
| thread_safe_contention | read_mostly_waiters / 4 | 266.166 us | 256.585 us - 276.063 us |
| thread_safe_contention | read_mostly_waiters / 8 | 600.116 us | 574.779 us - 622.448 us |
| thread_safe_contention | read_mostly_waiters / 16 | 1.172 ms | 1.122 ms - 1.224 ms |
| thread_safe_contention | batched_write_bursts / 1 | 247.918 us | 246.351 us - 249.185 us |
| thread_safe_contention | batched_write_bursts / 2 | 644.367 us | 598.583 us - 694.251 us |
| thread_safe_contention | batched_write_bursts / 4 | 1.469 ms | 1.453 ms - 1.486 ms |
| thread_safe_contention | batched_write_bursts / 8 | 2.808 ms | 2.750 ms - 2.866 ms |
| thread_safe_contention | batched_write_bursts / 16 | 2.480 ms | 2.290 ms - 2.656 ms |
| thread_safe_effect_contention | queue_coalescing / 8 | 881.070 us | 839.237 us - 918.092 us |
| thread_safe_effect_contention | queue_coalescing / 16 | 2.083 ms | 2.036 ms - 2.131 ms |
| thread_safe_effect_contention | cleanup_execution / 8 | 1.058 ms | 992.515 us - 1.109 ms |
| thread_safe_effect_contention | cleanup_execution / 16 | 2.434 ms | 2.335 ms - 2.543 ms |
| thread_safe_effect_contention | batch_flush / 8 | 1.473 ms | 1.404 ms - 1.546 ms |
| thread_safe_effect_contention | batch_flush / 16 | 3.745 ms | 3.631 ms - 3.854 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 8 | 3.835 ms | 3.794 ms - 3.875 ms |
| thread_safe_graph_propagation | fan_out_eager_validation / 16 | 6.195 ms | 6.110 ms - 6.271 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 8 | 1.881 ms | 1.827 ms - 1.932 ms |
| thread_safe_graph_propagation | fan_out_lazy_dirty_epochs / 16 | 3.552 ms | 3.427 ms - 3.679 ms |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 8 | 509.248 us | 499.185 us - 520.752 us |
| thread_safe_graph_propagation | fan_in_lazy_dirty_epochs / 16 | 1.034 ms | 1.013 ms - 1.062 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 8 | 1.043 ms | 1.023 ms - 1.069 ms |
| thread_safe_graph_propagation | fan_in_batched_flush / 16 | 1.466 ms | 1.448 ms - 1.481 ms |
| profile_instrumentation | context_snapshot | 421.918 ns | 416.285 ns - 428.322 ns |
| profile_instrumentation | thread_safe_snapshot | 305.223 us | 300.698 us - 312.876 us |

Instrumentation snapshots are single local profile runs captured by
`examples/instrumentation_profile.rs`.

| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold | Sidecar frontiers | Sidecar dirty marks | Sidecar fallbacks | Dirty epochs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| context_memo_effect | 4 | 3 | 0 | 4 | 1 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_fan_out_32 | 33 | 64 | 0 | 64 | 32 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| context_batch_storm_64 | 65 | 0 | 0 | 128 | 64 | 2 | 1 | 0 | 0.000 ns | 0.000 ns | 0 | 0 | 0 | 0 |
| thread_safe_first_get_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 11 | 14.180 us | 18.270 us | 0 | 0 | 0 | 0 |
| thread_safe_set_cell_invalidation_high_fan_out_512 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 ns | 0.000 ns | 1 | 512 | 0 | 512 |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 320.000 ns | 4.980 us | 16 | 16 | 0 | 16 |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 280.000 ns | 4.120 us | 32 | 32 | 0 | 32 |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 310.000 ns | 4.210 us | 64 | 64 | 0 | 64 |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 320.000 ns | 3.840 us | 128 | 128 | 0 | 128 |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 290.000 ns | 4.010 us | 256 | 256 | 0 | 256 |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 8 | 350.000 ns | 5.100 us | 15 | 15 | 0 | 15 |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | 4 | 2 | 0 | 2 | 0 | 0 | 0 | 16 | 590.000 ns | 9.041 us | 31 | 31 | 0 | 31 |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | 8 | 4 | 0 | 4 | 0 | 0 | 0 | 32 | 1.180 us | 17.010 us | 63 | 63 | 0 | 63 |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | 16 | 8 | 0 | 8 | 0 | 0 | 0 | 64 | 2.460 us | 34.260 us | 127 | 127 | 0 | 127 |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | 32 | 16 | 0 | 16 | 0 | 0 | 0 | 128 | 4.760 us | 66.190 us | 255 | 255 | 0 | 255 |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | 5 | 1 | 0 | 4 | 0 | 0 | 0 | 97 | 4.640 us | 137.292 us | 0 | 0 | 0 | 15 |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | 9 | 1 | 0 | 8 | 0 | 0 | 0 | 105 | 29.931 us | 90.411 us | 0 | 0 | 0 | 4 |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | 17 | 1 | 0 | 16 | 0 | 0 | 0 | 187 | 296.742 us | 157.981 us | 0 | 0 | 0 | 2 |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | 33 | 1 | 0 | 32 | 0 | 0 | 0 | 366 | 1.679 ms | 308.831 us | 0 | 0 | 0 | 3 |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | 65 | 1 | 0 | 64 | 0 | 0 | 0 | 718 | 9.719 ms | 604.522 us | 0 | 0 | 0 | 3 |
| thread_safe_contention_same_slot_write_read_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 1.040 us | 31.661 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_same_slot_write_read_2 | 2 | 21 | 0 | 1 | 0 | 0 | 0 | 47 | 1.270 us | 36.900 us | 32 | 32 | 0 | 32 |
| thread_safe_contention_same_slot_write_read_4 | 2 | 50 | 0 | 1 | 0 | 0 | 0 | 170 | 23.791 us | 98.151 us | 63 | 63 | 1 | 64 |
| thread_safe_contention_same_slot_write_read_8 | 2 | 98 | 0 | 1 | 0 | 0 | 0 | 415 | 76.263 us | 217.652 us | 126 | 126 | 2 | 128 |
| thread_safe_contention_same_slot_write_read_16 | 2 | 214 | 0 | 1 | 0 | 0 | 0 | 806 | 55.520 us | 383.706 us | 254 | 254 | 2 | 256 |
| thread_safe_contention_independent_slots_1 | 2 | 16 | 0 | 1 | 0 | 0 | 0 | 23 | 960.000 ns | 26.180 us | 15 | 15 | 0 | 15 |
| thread_safe_contention_independent_slots_2 | 4 | 33 | 0 | 2 | 0 | 0 | 0 | 47 | 11.800 us | 57.350 us | 31 | 31 | 0 | 31 |
| thread_safe_contention_independent_slots_4 | 8 | 67 | 0 | 4 | 0 | 0 | 0 | 131 | 274.031 us | 149.371 us | 51 | 51 | 12 | 63 |
| thread_safe_contention_independent_slots_8 | 16 | 135 | 0 | 8 | 0 | 0 | 0 | 270 | 2.438 ms | 342.362 us | 100 | 100 | 27 | 127 |
| thread_safe_contention_independent_slots_16 | 32 | 271 | 0 | 16 | 0 | 0 | 0 | 512 | 12.861 ms | 681.443 us | 208 | 208 | 47 | 255 |
| thread_safe_contention_read_mostly_waiters_1 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 24 | 960.000 ns | 25.540 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_2 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 26 | 1.070 us | 25.370 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_4 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 45 | 3.330 us | 32.220 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_8 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 80 | 34.760 us | 41.880 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_read_mostly_waiters_16 | 2 | 17 | 0 | 1 | 0 | 0 | 0 | 98 | 123.203 us | 58.601 us | 16 | 16 | 0 | 16 |
| thread_safe_contention_batched_write_bursts_1 | 5 | 16 | 0 | 4 | 0 | 0 | 0 | 112 | 4.880 us | 130.900 us | 0 | 0 | 0 | 15 |
| thread_safe_contention_batched_write_bursts_2 | 9 | 23 | 0 | 8 | 0 | 0 | 0 | 202 | 102.230 us | 252.621 us | 0 | 0 | 0 | 22 |
| thread_safe_contention_batched_write_bursts_4 | 17 | 13 | 0 | 16 | 0 | 0 | 0 | 248 | 591.186 us | 353.864 us | 0 | 0 | 0 | 12 |
| thread_safe_contention_batched_write_bursts_8 | 33 | 9 | 0 | 32 | 0 | 0 | 0 | 407 | 2.193 ms | 459.554 us | 0 | 0 | 0 | 8 |
| thread_safe_contention_batched_write_bursts_16 | 65 | 5 | 0 | 64 | 0 | 0 | 0 | 728 | 8.911 ms | 634.625 us | 0 | 0 | 0 | 4 |
| thread_safe_effect_contention_queue_coalescing_8 | 33 | 0 | 0 | 32 | 0 | 8 | 1 | 411 | 1.731 ms | 363.483 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_queue_coalescing_16 | 65 | 0 | 0 | 64 | 0 | 3 | 1 | 730 | 9.163 ms | 593.713 us | 0 | 0 | 0 | 0 |
| thread_safe_effect_contention_cleanup_execution_8 | 9 | 0 | 0 | 8 | 8 | 33 | 1 | 412 | 3.221 ms | 450.655 us | 0 | 0 | 127 | 0 |
| thread_safe_effect_contention_cleanup_execution_16 | 17 | 0 | 0 | 16 | 16 | 29 | 1 | 692 | 12.150 ms | 800.017 us | 0 | 0 | 255 | 0 |
| thread_safe_effect_contention_batch_flush_8 | 34 | 3 | 0 | 33 | 0 | 5 | 1 | 639 | 3.080 ms | 392.315 us | 0 | 0 | 0 | 2 |
| thread_safe_effect_contention_batch_flush_16 | 66 | 3 | 0 | 65 | 0 | 3 | 1 | 1242 | 14.619 ms | 748.326 us | 0 | 0 | 0 | 2 |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | 34 | 552 | 0 | 64 | 0 | 50 | 1 | 1113 | 32.844 ms | 10.674 ms | 23 | 736 | 105 | 4096 |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | 34 | 550 | 0 | 64 | 0 | 50 | 1 | 1209 | 88.062 ms | 13.181 ms | 102 | 3264 | 154 | 8192 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.170 us | 149.300 us | 128 | 4096 | 0 | 4096 |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | 33 | 64 | 0 | 32 | 0 | 0 | 0 | 226 | 7.210 us | 148.871 us | 256 | 8192 | 0 | 8192 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | 65 | 66 | 0 | 64 | 0 | 0 | 0 | 328 | 10.190 us | 282.182 us | 508 | 540 | 0 | 572 |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | 129 | 130 | 0 | 128 | 0 | 0 | 0 | 648 | 19.850 us | 560.884 us | 1020 | 1084 | 0 | 1148 |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | 66 | 137 | 0 | 65 | 0 | 25 | 1 | 1395 | 4.379 ms | 1.401 ms | 0 | 0 | 0 | 247 |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | 130 | 135 | 0 | 129 | 0 | 5 | 1 | 1315 | 6.931 ms | 1.121 ms | 0 | 0 | 0 | 154 |

ThreadSafe lock attribution for contention profiles:

| Profile | Site | Lock acquisitions | Lock wait | Lock hold |
|---|---|---:|---:|---:|
| thread_safe_set_cell_invalidation_same_slot_contention_1 | other | 4 | 170.000 ns | 890.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | get_refresh | 2 | 70.000 ns | 1.080 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 2.140 us |
| thread_safe_set_cell_invalidation_same_slot_contention_1 | publish | 1 | 40.000 ns | 870.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | other | 4 | 130.000 ns | 650.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | get_refresh | 2 | 70.000 ns | 760.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | dependency_edge | 1 | 40.000 ns | 2.090 us |
| thread_safe_set_cell_invalidation_same_slot_contention_2 | publish | 1 | 40.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | other | 4 | 150.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | get_refresh | 2 | 80.000 ns | 620.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | dependency_edge | 1 | 40.000 ns | 2.230 us |
| thread_safe_set_cell_invalidation_same_slot_contention_4 | publish | 1 | 40.000 ns | 730.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | other | 4 | 140.000 ns | 670.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | get_refresh | 2 | 80.000 ns | 460.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | dependency_edge | 1 | 50.000 ns | 2.110 us |
| thread_safe_set_cell_invalidation_same_slot_contention_8 | publish | 1 | 50.000 ns | 600.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | other | 4 | 140.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | get_refresh | 2 | 60.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | dependency_edge | 1 | 40.000 ns | 2.300 us |
| thread_safe_set_cell_invalidation_same_slot_contention_16 | publish | 1 | 50.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | other | 4 | 150.000 ns | 1.010 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | get_refresh | 2 | 110.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | dependency_edge | 1 | 40.000 ns | 2.610 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_1 | publish | 1 | 50.000 ns | 840.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | other | 8 | 290.000 ns | 2.460 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | get_refresh | 4 | 160.000 ns | 980.000 ns |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | dependency_edge | 2 | 60.000 ns | 4.351 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_2 | publish | 2 | 80.000 ns | 1.250 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | other | 16 | 570.000 ns | 4.230 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | get_refresh | 8 | 310.000 ns | 1.720 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | dependency_edge | 4 | 150.000 ns | 8.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_4 | publish | 4 | 150.000 ns | 2.370 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | other | 32 | 1.220 us | 8.540 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | get_refresh | 16 | 640.000 ns | 3.440 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | dependency_edge | 8 | 320.000 ns | 17.690 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_8 | publish | 8 | 280.000 ns | 4.590 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | other | 64 | 2.430 us | 17.470 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | get_refresh | 32 | 1.140 us | 6.720 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | dependency_edge | 16 | 560.000 ns | 32.940 us |
| thread_safe_set_cell_invalidation_independent_slot_contention_16 | publish | 16 | 630.000 ns | 9.060 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | other | 74 | 3.680 us | 31.140 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | get_refresh | 2 | 80.000 ns | 480.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | dependency_edge | 4 | 140.000 ns | 8.850 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | set_cell_invalidation | 16 | 700.000 ns | 96.162 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_1 | publish | 1 | 40.000 ns | 660.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | other | 90 | 29.211 us | 40.961 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | get_refresh | 2 | 70.000 ns | 450.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | dependency_edge | 8 | 310.000 ns | 19.120 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | set_cell_invalidation | 4 | 300.000 ns | 29.240 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_2 | publish | 1 | 40.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | other | 166 | 295.972 us | 92.401 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | get_refresh | 2 | 60.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | dependency_edge | 16 | 580.000 ns | 38.470 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | set_cell_invalidation | 2 | 90.000 ns | 26.040 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_4 | publish | 1 | 40.000 ns | 640.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | other | 328 | 1.677 ms | 180.711 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | get_refresh | 2 | 70.000 ns | 430.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | dependency_edge | 32 | 1.210 us | 75.670 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | set_cell_invalidation | 3 | 160.000 ns | 51.390 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_8 | publish | 1 | 40.000 ns | 630.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | other | 648 | 9.716 ms | 378.350 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 440.000 ns |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | dependency_edge | 64 | 2.450 us | 151.002 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | set_cell_invalidation | 3 | 170.000 ns | 74.080 us |
| thread_safe_set_cell_invalidation_batched_write_bursts_16 | publish | 1 | 40.000 ns | 650.000 ns |
| thread_safe_contention_same_slot_write_read_1 | other | 4 | 130.000 ns | 1.520 us |
| thread_safe_contention_same_slot_write_read_1 | get_refresh | 2 | 150.000 ns | 1.270 us |
| thread_safe_contention_same_slot_write_read_1 | dependency_edge | 1 | 40.000 ns | 3.300 us |
| thread_safe_contention_same_slot_write_read_1 | publish | 17 | 720.000 ns | 25.571 us |
| thread_safe_contention_same_slot_write_read_2 | other | 4 | 140.000 ns | 750.000 ns |
| thread_safe_contention_same_slot_write_read_2 | get_refresh | 2 | 70.000 ns | 440.000 ns |
| thread_safe_contention_same_slot_write_read_2 | dependency_edge | 1 | 40.000 ns | 2.110 us |
| thread_safe_contention_same_slot_write_read_2 | publish | 21 | 1.020 us | 33.600 us |
| thread_safe_contention_same_slot_write_read_2 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_4 | other | 6 | 4.940 us | 1.030 us |
| thread_safe_contention_same_slot_write_read_4 | get_refresh | 22 | 1.470 us | 11.080 us |
| thread_safe_contention_same_slot_write_read_4 | dependency_edge | 1 | 40.000 ns | 2.150 us |
| thread_safe_contention_same_slot_write_read_4 | set_cell_invalidation | 1 | 770.000 ns | 12.331 us |
| thread_safe_contention_same_slot_write_read_4 | publish | 50 | 16.571 us | 71.560 us |
| thread_safe_contention_same_slot_write_read_4 | in_flight_wait | 90 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_8 | other | 8 | 14.950 us | 1.350 us |
| thread_safe_contention_same_slot_write_read_8 | get_refresh | 58 | 28.241 us | 27.660 us |
| thread_safe_contention_same_slot_write_read_8 | dependency_edge | 1 | 40.000 ns | 2.120 us |
| thread_safe_contention_same_slot_write_read_8 | set_cell_invalidation | 2 | 2.940 us | 22.330 us |
| thread_safe_contention_same_slot_write_read_8 | publish | 98 | 30.092 us | 164.192 us |
| thread_safe_contention_same_slot_write_read_8 | in_flight_wait | 248 | 0.000 ns | 0.000 ns |
| thread_safe_contention_same_slot_write_read_16 | other | 8 | 320.000 ns | 2.340 us |
| thread_safe_contention_same_slot_write_read_16 | get_refresh | 106 | 10.650 us | 45.010 us |
| thread_safe_contention_same_slot_write_read_16 | dependency_edge | 1 | 40.000 ns | 3.420 us |
| thread_safe_contention_same_slot_write_read_16 | set_cell_invalidation | 2 | 12.780 us | 12.050 us |
| thread_safe_contention_same_slot_write_read_16 | publish | 214 | 31.730 us | 320.886 us |
| thread_safe_contention_same_slot_write_read_16 | in_flight_wait | 475 | 0.000 ns | 0.000 ns |
| thread_safe_contention_independent_slots_1 | other | 4 | 170.000 ns | 1.940 us |
| thread_safe_contention_independent_slots_1 | get_refresh | 2 | 80.000 ns | 590.000 ns |
| thread_safe_contention_independent_slots_1 | dependency_edge | 1 | 30.000 ns | 3.920 us |
| thread_safe_contention_independent_slots_1 | publish | 16 | 680.000 ns | 19.730 us |
| thread_safe_contention_independent_slots_2 | other | 8 | 300.000 ns | 2.930 us |
| thread_safe_contention_independent_slots_2 | get_refresh | 4 | 120.000 ns | 890.000 ns |
| thread_safe_contention_independent_slots_2 | dependency_edge | 2 | 80.000 ns | 4.070 us |
| thread_safe_contention_independent_slots_2 | publish | 33 | 11.300 us | 49.460 us |
| thread_safe_contention_independent_slots_4 | other | 40 | 14.760 us | 6.430 us |
| thread_safe_contention_independent_slots_4 | get_refresh | 8 | 320.000 ns | 1.690 us |
| thread_safe_contention_independent_slots_4 | dependency_edge | 4 | 130.000 ns | 8.360 us |
| thread_safe_contention_independent_slots_4 | set_cell_invalidation | 12 | 26.170 us | 40.600 us |
| thread_safe_contention_independent_slots_4 | publish | 67 | 232.651 us | 92.291 us |
| thread_safe_contention_independent_slots_8 | other | 84 | 159.621 us | 15.081 us |
| thread_safe_contention_independent_slots_8 | get_refresh | 16 | 650.000 ns | 3.690 us |
| thread_safe_contention_independent_slots_8 | dependency_edge | 8 | 300.000 ns | 17.730 us |
| thread_safe_contention_independent_slots_8 | set_cell_invalidation | 27 | 472.212 us | 93.730 us |
| thread_safe_contention_independent_slots_8 | publish | 135 | 1.805 ms | 212.131 us |
| thread_safe_contention_independent_slots_16 | other | 146 | 1.358 ms | 27.490 us |
| thread_safe_contention_independent_slots_16 | get_refresh | 32 | 1.210 us | 7.070 us |
| thread_safe_contention_independent_slots_16 | dependency_edge | 16 | 550.000 ns | 36.141 us |
| thread_safe_contention_independent_slots_16 | set_cell_invalidation | 47 | 1.891 ms | 172.141 us |
| thread_safe_contention_independent_slots_16 | publish | 271 | 9.609 ms | 438.601 us |
| thread_safe_contention_read_mostly_waiters_1 | other | 4 | 150.000 ns | 1.170 us |
| thread_safe_contention_read_mostly_waiters_1 | get_refresh | 2 | 110.000 ns | 800.000 ns |
| thread_safe_contention_read_mostly_waiters_1 | dependency_edge | 1 | 30.000 ns | 2.530 us |
| thread_safe_contention_read_mostly_waiters_1 | publish | 17 | 670.000 ns | 21.040 us |
| thread_safe_contention_read_mostly_waiters_2 | other | 4 | 170.000 ns | 670.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | get_refresh | 2 | 80.000 ns | 440.000 ns |
| thread_safe_contention_read_mostly_waiters_2 | dependency_edge | 1 | 40.000 ns | 2.000 us |
| thread_safe_contention_read_mostly_waiters_2 | publish | 17 | 780.000 ns | 22.260 us |
| thread_safe_contention_read_mostly_waiters_2 | in_flight_wait | 2 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_4 | other | 4 | 130.000 ns | 1.040 us |
| thread_safe_contention_read_mostly_waiters_4 | get_refresh | 10 | 2.320 us | 4.160 us |
| thread_safe_contention_read_mostly_waiters_4 | dependency_edge | 1 | 40.000 ns | 2.350 us |
| thread_safe_contention_read_mostly_waiters_4 | publish | 17 | 840.000 ns | 24.670 us |
| thread_safe_contention_read_mostly_waiters_4 | in_flight_wait | 13 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | other | 4 | 150.000 ns | 670.000 ns |
| thread_safe_contention_read_mostly_waiters_8 | get_refresh | 28 | 31.600 us | 12.910 us |
| thread_safe_contention_read_mostly_waiters_8 | dependency_edge | 1 | 40.000 ns | 2.140 us |
| thread_safe_contention_read_mostly_waiters_8 | publish | 17 | 2.970 us | 26.160 us |
| thread_safe_contention_read_mostly_waiters_8 | in_flight_wait | 30 | 0.000 ns | 0.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | other | 4 | 150.000 ns | 750.000 ns |
| thread_safe_contention_read_mostly_waiters_16 | get_refresh | 31 | 113.222 us | 20.630 us |
| thread_safe_contention_read_mostly_waiters_16 | dependency_edge | 1 | 30.000 ns | 2.090 us |
| thread_safe_contention_read_mostly_waiters_16 | publish | 17 | 9.801 us | 35.131 us |
| thread_safe_contention_read_mostly_waiters_16 | in_flight_wait | 45 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_1 | other | 74 | 3.310 us | 22.180 us |
| thread_safe_contention_batched_write_bursts_1 | get_refresh | 2 | 70.000 ns | 480.000 ns |
| thread_safe_contention_batched_write_bursts_1 | dependency_edge | 4 | 130.000 ns | 9.010 us |
| thread_safe_contention_batched_write_bursts_1 | set_cell_invalidation | 16 | 690.000 ns | 71.730 us |
| thread_safe_contention_batched_write_bursts_1 | publish | 16 | 680.000 ns | 27.500 us |
| thread_safe_contention_batched_write_bursts_2 | other | 126 | 83.300 us | 48.740 us |
| thread_safe_contention_batched_write_bursts_2 | get_refresh | 2 | 80.000 ns | 470.000 ns |
| thread_safe_contention_batched_write_bursts_2 | dependency_edge | 8 | 290.000 ns | 18.770 us |
| thread_safe_contention_batched_write_bursts_2 | set_cell_invalidation | 22 | 2.100 us | 144.220 us |
| thread_safe_contention_batched_write_bursts_2 | publish | 23 | 16.460 us | 40.421 us |
| thread_safe_contention_batched_write_bursts_2 | in_flight_wait | 21 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_4 | other | 186 | 546.606 us | 113.741 us |
| thread_safe_contention_batched_write_bursts_4 | get_refresh | 2 | 80.000 ns | 730.000 ns |
| thread_safe_contention_batched_write_bursts_4 | dependency_edge | 16 | 640.000 ns | 38.101 us |
| thread_safe_contention_batched_write_bursts_4 | set_cell_invalidation | 12 | 1.120 us | 157.672 us |
| thread_safe_contention_batched_write_bursts_4 | publish | 13 | 42.740 us | 43.620 us |
| thread_safe_contention_batched_write_bursts_4 | in_flight_wait | 19 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_8 | other | 340 | 2.134 ms | 205.111 us |
| thread_safe_contention_batched_write_bursts_8 | get_refresh | 2 | 100.000 ns | 490.000 ns |
| thread_safe_contention_batched_write_bursts_8 | dependency_edge | 32 | 1.210 us | 77.831 us |
| thread_safe_contention_batched_write_bursts_8 | set_cell_invalidation | 9 | 39.340 us | 137.251 us |
| thread_safe_contention_batched_write_bursts_8 | publish | 9 | 18.890 us | 38.871 us |
| thread_safe_contention_batched_write_bursts_8 | in_flight_wait | 15 | 0.000 ns | 0.000 ns |
| thread_safe_contention_batched_write_bursts_16 | other | 650 | 8.907 ms | 379.804 us |
| thread_safe_contention_batched_write_bursts_16 | get_refresh | 2 | 80.000 ns | 730.000 ns |
| thread_safe_contention_batched_write_bursts_16 | dependency_edge | 64 | 2.470 us | 150.151 us |
| thread_safe_contention_batched_write_bursts_16 | set_cell_invalidation | 4 | 290.000 ns | 65.450 us |
| thread_safe_contention_batched_write_bursts_16 | publish | 5 | 240.000 ns | 38.490 us |
| thread_safe_contention_batched_write_bursts_16 | in_flight_wait | 3 | 0.000 ns | 0.000 ns |
| thread_safe_effect_contention_queue_coalescing_8 | other | 371 | 1.729 ms | 242.113 us |
| thread_safe_effect_contention_queue_coalescing_8 | dependency_edge | 32 | 1.160 us | 61.260 us |
| thread_safe_effect_contention_queue_coalescing_8 | set_cell_invalidation | 8 | 311.000 ns | 60.110 us |
| thread_safe_effect_contention_queue_coalescing_16 | other | 662 | 9.160 ms | 397.642 us |
| thread_safe_effect_contention_queue_coalescing_16 | dependency_edge | 64 | 2.270 us | 123.670 us |
| thread_safe_effect_contention_queue_coalescing_16 | set_cell_invalidation | 4 | 210.000 ns | 72.401 us |
| thread_safe_effect_contention_cleanup_execution_8 | other | 269 | 476.233 us | 92.921 us |
| thread_safe_effect_contention_cleanup_execution_8 | dependency_edge | 16 | 590.000 ns | 23.940 us |
| thread_safe_effect_contention_cleanup_execution_8 | set_cell_invalidation | 127 | 2.744 ms | 333.794 us |
| thread_safe_effect_contention_cleanup_execution_16 | other | 405 | 2.532 ms | 126.300 us |
| thread_safe_effect_contention_cleanup_execution_16 | dependency_edge | 32 | 1.180 us | 46.101 us |
| thread_safe_effect_contention_cleanup_execution_16 | set_cell_invalidation | 255 | 9.617 ms | 627.616 us |
| thread_safe_effect_contention_batch_flush_8 | other | 599 | 3.078 ms | 256.393 us |
| thread_safe_effect_contention_batch_flush_8 | get_refresh | 2 | 70.000 ns | 860.000 ns |
| thread_safe_effect_contention_batch_flush_8 | dependency_edge | 33 | 1.250 us | 78.581 us |
| thread_safe_effect_contention_batch_flush_8 | set_cell_invalidation | 2 | 80.000 ns | 35.811 us |
| thread_safe_effect_contention_batch_flush_8 | publish | 3 | 160.000 ns | 20.670 us |
| thread_safe_effect_contention_batch_flush_16 | other | 1170 | 14.616 ms | 505.795 us |
| thread_safe_effect_contention_batch_flush_16 | get_refresh | 2 | 70.000 ns | 1.020 us |
| thread_safe_effect_contention_batch_flush_16 | dependency_edge | 65 | 2.460 us | 159.451 us |
| thread_safe_effect_contention_batch_flush_16 | set_cell_invalidation | 2 | 390.000 ns | 64.170 us |
| thread_safe_effect_contention_batch_flush_16 | publish | 3 | 170.000 ns | 17.890 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | other | 328 | 13.900 us | 251.054 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | get_refresh | 64 | 2.350 us | 13.750 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | dependency_edge | 64 | 2.500 us | 153.221 us |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | set_cell_invalidation | 105 | 25.178 ms | 8.876 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_8 | publish | 552 | 7.647 ms | 1.380 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | other | 377 | 15.040 us | 227.980 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | get_refresh | 64 | 2.070 us | 12.920 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | dependency_edge | 64 | 2.310 us | 131.261 us |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | set_cell_invalidation | 154 | 77.390 ms | 11.600 ms |
| thread_safe_graph_propagation_fan_out_eager_validation_16 | publish | 550 | 10.653 ms | 1.209 ms |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | other | 66 | 1.990 us | 19.910 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | get_refresh | 64 | 1.990 us | 11.740 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | dependency_edge | 32 | 980.000 ns | 65.020 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_8 | publish | 64 | 2.210 us | 52.630 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | other | 66 | 1.950 us | 19.080 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | get_refresh | 64 | 1.960 us | 11.400 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | dependency_edge | 32 | 980.000 ns | 63.711 us |
| thread_safe_graph_propagation_fan_out_lazy_dirty_epochs_16 | publish | 64 | 2.320 us | 54.680 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | other | 130 | 3.760 us | 34.471 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | get_refresh | 68 | 2.410 us | 20.020 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | dependency_edge | 64 | 1.880 us | 119.130 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_8 | publish | 66 | 2.140 us | 108.561 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | other | 258 | 7.590 us | 76.991 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | get_refresh | 132 | 4.150 us | 42.660 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | dependency_edge | 128 | 3.790 us | 225.161 us |
| thread_safe_graph_propagation_fan_in_lazy_dirty_epochs_16 | publish | 130 | 4.320 us | 216.072 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | other | 471 | 3.996 ms | 240.782 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | get_refresh | 706 | 35.970 us | 231.991 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | dependency_edge | 65 | 1.870 us | 116.871 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | set_cell_invalidation | 16 | 3.310 us | 474.164 us |
| thread_safe_graph_propagation_fan_in_batched_flush_8 | publish | 137 | 341.653 us | 337.423 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | other | 793 | 6.862 ms | 358.343 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | get_refresh | 254 | 8.920 us | 74.980 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | dependency_edge | 129 | 3.620 us | 224.533 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | set_cell_invalidation | 4 | 4.170 us | 242.931 us |
| thread_safe_graph_propagation_fan_in_batched_flush_16 | publish | 135 | 52.780 us | 220.233 us |

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
