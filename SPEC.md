# lazily-rs Specification

Rust library for lazy evaluation with context-aware dependency tracking and cache invalidation. Counterpart to lazily-zig and lazily-py.

## Core Concepts

### Context

Container for all slots and cells. Owns all allocations via interior mutability (`RefCell`).

```rust
pub struct Context {
    nodes: RefCell<Vec<Option<Node>>>,
    next_id: RefCell<u64>,
    pending_effects: RefCell<VecDeque<SlotId>>,
    scheduled_effects: RefCell<HashSet<SlotId>>,
    flushing_effects: RefCell<bool>,
    batch_depth: RefCell<usize>,
    batched_cells: RefCell<HashSet<SlotId>>,
    batched_cell_clears: RefCell<HashSet<SlotId>>,
    batched_slots: RefCell<HashSet<SlotId>>,
}
```

**API:**

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
| `effect.dispose(&ctx)` | Dispose an effect, unsubscribe dependencies, and run cleanup |
| `effect.is_active(&ctx)` | Check whether an effect is still registered |

`Context` stores nodes in a slot-id-indexed `Vec<Option<Node>>` rather than a
hash map. `SlotId` values are monotonically allocated and are not reused; effect
disposal leaves a vacant entry so existing handles and dependency ids remain
stable while lookups stay contiguous and hash-free.

### ThreadSafeContext

Mutex-backed counterpart to `Context` for sharing one reactive graph across OS
threads. It mirrors the core local-context API and requires thread-safe values
and callbacks.

```rust
pub struct ThreadSafeContext {
    inner: Arc<ThreadSafeInner>,
}
```

**API:**

| Method | Purpose |
|--------|---------|
| `ThreadSafeContext::new()` | Create a new thread-safe context |
| `ctx.computed(\|ctx\| T)` | Create a `Send + Sync` derived lazily-computed value |
| `ctx.slot(\|ctx\| T)` | Create a `Send + Sync` lazily-computed slot |
| `ctx.memo(\|ctx\| T)` | Create a `Send + Sync` lazily-computed slot with a `PartialEq` memoization guard |
| `ctx.get(&slot)` | Get value from any thread (computes if unset) |
| `ctx.cell(value)` | Create a mutable `Send + Sync` cell |
| `ctx.get_cell(&cell)` | Get cell value from any thread |
| `ctx.set_cell(&cell, value)` | Update cell and invalidate dependents across threads |
| `ctx.batch(\|ctx\| { ... })` | Defer invalidation until the outermost shared batch exits |
| `ctx.effect(\|ctx\| { ... })` | Run a `Send + Sync` effect immediately and rerun it after tracked dependencies invalidate |
| `ctx.clear(&slot)` | Clear cached value and cascade to dependents |
| `ctx.clear_cell_dependents(&cell)` | Clear downstream slots without changing cell value |
| `ctx.dispose_effect(&effect)` | Dispose an effect, unsubscribe dependencies, and run cleanup |
| `ctx.is_effect_active(&effect)` | Check whether an effect is still registered |

### Slot

Lazily-computed cached value with dependency tracking. A Slot is **fresh**, **dirty**, or **unset**; dirty slots may retain a previous cached value for memo validation.
`ctx.memo()` creates a Slot whose values implement `PartialEq` so dirty caches can be compared against recomputed values.

```rust
struct SlotNode {
    value: Option<Box<dyn Any>>,
    compute: Box<dyn Fn(&Context) -> Box<dyn Any>>,
    equals: Option<Box<dyn Fn(&dyn Any, &dyn Any) -> bool>>,
    dependencies: HashSet<SlotId>,
    dependents: HashSet<SlotId>,
    dirty: bool,
    force_recompute: bool,
}
```

**Semantics:**

- **Activation:** First `ctx.get()` calls the compute function, caches the result
- **Computed alias:** `ctx.computed()` creates the same Slot as `ctx.slot()` for derived-value ergonomics
- **Invalidation:** Marks the cached value dirty and marks downstream slots dirty without discarding their cached values
- **Clearing:** Explicit `slot.clear(&ctx)` removes the cached value and clears all dependent slots recursively
- **Memo guard:** Dirty `ctx.memo()` slots compare recomputed values with the previous cache via `PartialEq`; equal values make downstream dirty slots fresh without recomputing them
- **Dependencies:** If Slot B accesses Slot A during computation, B depends on A. If A clears, B clears automatically; if A's value changes after dirty validation, B is forced stale
- **Immutable by default:** Once set, a Slot's value doesn't change — only clear + recompute
- **Dynamic:** Dependencies re-discovered on each recomputation (no stale subscriptions)

### Cell

Mutable value container. Changing a Cell's value marks dependent Slots dirty.

```rust
struct CellNode {
    value: Box<dyn Any>,
    dependents: HashSet<SlotId>,
}
```

**Semantics:**

- `ctx.set_cell()` compares old and new via `PartialEq`
- If unchanged, no invalidation occurs (no-op)
- If changed, dependent Slots are marked dirty while cached values are preserved for memo validation

### Effect

Side-effect callback that automatically tracks dependencies. Effects run
immediately on creation, then rerun after any Cell or Slot read during the last
run is invalidated.

```rust
struct EffectNode {
    run: Box<dyn Fn(&Context) -> Option<Box<dyn FnOnce()>>>,
    dependencies: HashSet<SlotId>,
    cleanup: Option<Box<dyn FnOnce()>>,
    force_run: bool,
}
```

**Semantics:**

- **Immediate activation:** `ctx.effect()` runs the callback once during creation
- **Auto-tracking:** Any Slot or Cell accessed during the callback becomes a dependency
- **Scheduling:** Dependency invalidation schedules the effect, then the context flushes scheduled effects after the invalidation pass
- **Coalescing:** An effect scheduled through multiple dependency paths in the same invalidation pass runs once
- **Memo guard:** Effects scheduled by dirty slot dependencies first validate those slots and skip cleanup/rerun when values are unchanged
- **Cleanup:** Returning a cleanup closure runs it before the next rerun and on disposal
- **Disposal:** `effect.dispose(&ctx)` unsubscribes from dependencies, removes pending scheduled work, and prevents future reruns

### Batch

Write-coalescing boundary for multiple cell updates or explicit slot clears.

```rust
ctx.batch(|ctx| {
    ctx.set_cell(&a, 1);
    ctx.set_cell(&b, 2);
});
```

**Semantics:**

- **Outermost boundary:** Nested batches flush only when the outermost batch exits
- **Changed cells:** `ctx.set_cell()` still updates the cell value immediately, but dependent dirty marking is queued until batch exit
- **Explicit clears:** `slot.clear(&ctx)` and `cell.clear_dependents(&ctx)` are queued until batch exit
- **Coalescing:** Repeated updates to the same cell or clears of the same slot queue one invalidation root
- **Effect flushing:** Effects scheduled by batched invalidation rerun after the batch invalidation pass and coalesce duplicate schedules
- **Reads during a batch:** Direct `ctx.get_cell()` reads see the latest cell value immediately; dependent slot reads keep their pre-batch cached value until dirty marking flushes at batch exit

### SlotId

Unique identifier for reactive nodes. Lightweight `Copy` type wrapping a `u64`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(u64);
```

Both `SlotHandle<T>` and `CellHandle<T>` wrap a `SlotId` with `PhantomData<T>` for type safety.

## Dependency Tracking

Uses a thread-local tracking stack (mirroring lazily-zig's `TrackingFrame` approach).

1. When a Slot computes, it pushes a frame onto the tracking stack
2. When an Effect runs, it also pushes a frame onto the tracking stack
3. Any nested slot/cell access sees the parent frame
4. The child registers the parent as a dependent
5. When a dependency clears, slot dependents hard-clear recursively; when a Cell changes, slot dependents are marked dirty and effect dependents are scheduled

## Threading and Concurrency Contract

### Current `Context`

`Context` is intentionally local to one OS thread. It owns `RefCell` graph state,
cached values as `Box<dyn Any>`, compute callbacks as `Box<dyn Fn(&Context)>`,
effect callbacks as `Box<dyn Fn(&Context)>`, and cleanups as `Box<dyn FnOnce()>`.
Those storage choices avoid synchronization overhead for the common single-threaded
path and make `Context` neither `Send` nor `Sync`.

Current guarantees:

- Independent `Context` instances may be used on different OS threads
- A single `Context` must not be moved into, shared with, or accessed from another thread
- `SlotHandle<T>` and `CellHandle<T>` are lightweight ids and are `Send + Sync` when `T` is `Send + Sync`, but they are only meaningful with their owning context
- `EffectHandle` is a lightweight id; effect execution and cleanup remain tied to the owning context thread
- Dependency tracking is thread-local; a compute/effect callback cannot split work onto another thread and expect nested reads there to attach to the original tracking frame

### `ThreadSafeContext`

Thread-safe support is explicit rather than a silent change to `Context`.
`ThreadSafeContext` mirrors the existing `Context` methods while preserving the
single-threaded fast path.

API bounds:

| Method family | Additional bounds |
|---------------|-------------------|
| `cell`, `get_cell`, `set_cell` | `T: PartialEq + Clone + Send + Sync + 'static` |
| `slot`, `computed` | `T: Clone + Send + Sync + 'static`; compute closure `Fn(&ThreadSafeContext) -> T + Send + Sync + 'static` |
| `memo` | `T: PartialEq + Clone + Send + Sync + 'static`; compute closure `Send + Sync + 'static` |
| `effect` | effect callback `Fn(&ThreadSafeContext) -> R + Send + Sync + 'static`; cleanup `FnOnce() + Send + 'static` |
| handles | remain id-only and copyable; usable from any thread only with the owning `ThreadSafeContext` |

Locking model:

- Uses one context-level synchronization primitive for graph state, matching lazily-zig's mutex-first design before introducing finer-grained locks
- Do not hold the graph lock while running user compute callbacks, effect callbacks, or cleanup closures
- Re-acquire the lock only to publish computed values, dependency edges, invalidation state, and pending effect work
- Slot refresh must avoid helper-level lock churn: a fresh cached get should clone the value under one `get_refresh` lock without recursively validating unchanged dependencies, dependency refresh should not take a separate node-kind probe lock before recursively validating a dependency, clean dirty flags should be folded into the refresh decision lock, and recompute must diff old/new dependency sets at publish so unchanged edges stay subscribed while only stale edges are removed
- Re-entrant user code must be able to call back into the same context without deadlocking
- Concurrent first access shares one in-flight computation for the current slot revision; waiters park on that slot's recompute notification primitive, then return the published cache or retry if an invalidation makes the in-flight result stale
- Recompute notifications are scoped to the slot that finished. A completion for one in-flight slot must not wake waiters parked behind another in-flight slot.
- If an upstream invalidation happens while a slot callback is running, the in-flight stale result is not published as fresh; the getter retries until it can return a value that matches the latest dependency state
- Batch exit, effect scheduling, disposal, and explicit clears must each have a single atomic graph mutation boundary and one coalesced effect flush per outermost invalidation pass
- Thread-safe invalidation uses an explicit frontier work queue under the graph
  mutex instead of recursive dependent walks. Changed-cell and slot-value-change
  roots snapshot dependent frontiers, coalesce duplicate slot ids in one
  invalidation pass, preserve direct changed-value `force_recompute` upgrades
  when a slot is reached through both direct and downstream paths, and then
  apply dirty/revision/effect scheduling mutations at the same graph mutation
  boundary. The frontier shape is partitionable for future bounded worker
  traversal, but this prototype keeps application under the context mutex until
  benchmark and model-checking evidence proves a parallel apply path safe.

Lock strategy evaluation:

- Keep the context-level `Mutex` as the graph synchronization primitive until benchmark instrumentation shows lock wait/hold time dominates the relevant workload
- `ThreadSafeContext` may use per-slot sidecar recompute `Condvar`s for in-flight waiters after attribution shows the spin-yield wait loop is material; those Condvars must not guard graph state independently of the context mutex
- The current short contention sample after in-flight dedup improved 1-2 worker runs, was neutral around 4 workers, and regressed at 8-16 workers; this is not enough evidence to adopt `RwLock`, sharded locks, or targeted CAS
- Versioned optimistic reads are deferred for the current erased-value storage. The mutex protects both slot metadata and the cached `Box<dyn Any>` value; a lock-free read path would need independently retained value snapshots plus atomic dirty/revision validation. Any such path must prove that a `get` starting after a cross-thread invalidation cannot return the pre-invalidation cached value.
- Any future `RwLock`, sharding, or CAS path must include a Loom or Shuttle safety model covering concurrent first get, stale in-flight completion, invalidation during compute, effect scheduling/disposal, and re-entrant callbacks before it can replace the mutex-first design
- The current sidecar `Condvar` waiter path is covered by `cargo test --features loom --test thread_safe_loom`, which models concurrent first get, scoped slot notification, stale in-flight completion and retry, invalidation during compute, effect scheduling/disposal races, and re-entrant callback graph access
- A lock-strategy change must preserve the rule that user compute/effect/cleanup callbacks never run while holding graph-state locks

Tokio integration is scoped in two stages:

1. Synchronous thread-safe sharing first: `ThreadSafeContext` should work inside
   `tokio::spawn` and `tokio::task::spawn_blocking` when all captured values and
   callbacks satisfy the `Send + Sync` bounds above. This is exposed behind the
   optional `tokio` feature with async tests and the `tokio_sync` example; it
   must not introduce async compute/effect semantics.
2. True async computations/effects are separate future work. They need explicit
   semantics for in-flight future deduplication, cancellation, dependency tracking
   across `.await`, stale future completion, cleanup ordering, and `Send` versus
   `LocalSet` futures.

### Future async computations/effects design

True async support must be a new explicit async context surface, not an overload
of `Context` or `ThreadSafeContext`. A future implementation should introduce an
`AsyncContext` API behind a separate feature once these semantics are covered by
tests.

API sketch:

```rust
pub struct AsyncContext { /* shared async graph state */ }
pub struct AsyncSlotHandle<T> { /* id + marker */ }
pub struct AsyncEffectHandle { /* id */ }

impl AsyncContext {
    pub fn async_computed<T, F, Fut>(&self, compute: F) -> AsyncSlotHandle<T>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static;

    pub async fn get_async<T>(&self, slot: &AsyncSlotHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static;

    pub fn async_effect<F, Fut, C, CleanupFut>(&self, effect: F) -> AsyncEffectHandle
    where
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<C>> + Send + 'static,
        C: FnOnce() -> CleanupFut + Send + 'static,
        CleanupFut: Future<Output = ()> + Send + 'static;
}
```

Required semantics:

- **In-flight future deduplication:** each async slot has one published cache and
  at most one in-flight computation for the current slot revision. Concurrent
  `get_async` callers await the same in-flight result instead of spawning
  duplicate futures.
- **Cancellation:** dropping one waiter does not cancel the shared in-flight
  computation while other waiters still need it. Explicit `clear`, dependency
  invalidation, or context disposal may mark the in-flight revision canceled and
  abort the task if the runtime provides an abort handle. User futures and
  cleanup futures must be cancellation-safe because aborting drops them at an
  `.await` boundary.
- **Dependency tracking across `.await`:** do not depend on thread-local stack
  state surviving across suspension or executor thread migration. Async compute
  and effect callbacks receive an `AsyncComputeContext`; every `get_async` and
  `get_cell` through that context records dependencies against the active async
  node explicitly.
- **Stale future completion:** an async computation records the slot revision it
  started from. When it completes, the graph publishes its value only if the
  slot revision is still current. Stale completions are discarded and waiting
  callers retry or await the newer in-flight computation.
- **Effect cleanup ordering:** async effect reruns are serialized per effect.
  Before a rerun or disposal publishes the next effect state, the previous
  cleanup future must run outside the graph lock and complete before the next
  effect body starts. Disposal removes pending reruns before awaiting cleanup.
- **`Send` versus `LocalSet`:** the default async context should require
  `Send + Sync + 'static` values, callbacks, futures, and cleanup futures so it
  can run on a multithreaded Tokio runtime. A separate `LocalAsyncContext` may
  support `!Send` futures on `tokio::task::LocalSet`, but its handles must not be
  interchangeable with the `Send` async context.

Implementation notes:

- Async graph locks must never be held while polling user futures or cleanup
  futures.
- Nested async slot reads should register dependencies on the awaiting parent
  before awaiting the child result.
- Batching semantics should remain synchronous at the graph mutation boundary:
  invalidations schedule async reruns only after the outermost batch exits.
- The sync `tokio` feature is not enough to enable this API; true async support
  should use a separate feature flag so downstream users do not accidentally
  accept the larger semantic surface.

## Invalidation Semantics

- `ctx.set_cell()` → if value changed (PartialEq) → mark all dependent slots dirty
- `slot.clear(&ctx)` → remove cached value → cascade clear to all dependents
- `cell.clear_dependents(&ctx)` → clear all dependent slots without changing cell value
- `ctx.batch()` → queue changed cells and explicit slot/cell clears, then flush queued roots when the outermost batch exits
- Slot invalidation → preserve cached value as dirty → validate/recompute on next `ctx.get()` access
- Thread-safe slot invalidation walks an explicit coalesced frontier, so diamond
  paths mark each reachable slot at most once per invalidation pass unless a
  later direct changed-value path upgrades that slot to forced recompute
- If a dirty `ctx.memo()` slot recomputes to an equal value, downstream dirty slots become fresh without recomputing
- Slot clearing → remove cached value → hard-clear dependents recursively
- Effects rerun after the invalidation pass if any tracked dependency invalidated
- Effects scheduled only by dirty slot dependencies skip rerun if those slots validate unchanged
- Effect cleanup runs before rerun and on disposal

## Design Goals

- **Lazy evaluation:** Values computed only when first accessed or when dirty caches are validated
- **Ergonomic derived values:** `ctx.computed()` is the preferred spelling for ordinary derived slots
- **Fine-grained reactivity:** Only affected dependents recompute
- **Memoized invalidation:** Equal intermediate `ctx.memo()` recomputation suppresses downstream recomputation/effect reruns
- **Effects:** Side effects are scheduled from the same dependency graph as slots
- **Batching:** Multiple writes can share one invalidation/effect flush boundary
- **Zero mandatory runtime dependencies:** The default library surface uses only the Rust standard library; Tokio is optional and Criterion is dev-only for benchmarks
- **Single-threaded fast path:** `Context` uses `RefCell` interior mutability with no mutex overhead
- **Contiguous local storage:** `Context` indexes nodes directly by `SlotId` to avoid hash-map lookup and churn in the single-threaded fast path
- **Explicit thread-safe path:** `ThreadSafeContext` uses a context-level lock and `Send + Sync` bounds for shared reactive graphs
- **Performance tracking:** Criterion benchmarks cover both `Context` and `ThreadSafeContext` for cached reads, cold first access, dependency fan-out, memo equality suppression, effect flushing, and batch storms; `ThreadSafeContext` also tracks a 1/2/4/8/16-worker contention matrix that separates hot shared-slot writes, independent per-worker slots, read-mostly waiters, and batched write bursts.
- **Benchmark instrumentation:** The optional `instrumentation` feature exposes lightweight counters for recompute starts, duplicate speculative thread-safe computes, dependency edge churn, effect queue depth, reactive node allocations, aggregate `ThreadSafeContext` lock wait/hold timing, and per-operation thread-safe lock attribution.

## Performance Benchmarks

The benchmark suite is a development-only surface under `benches/context.rs`.
It must compile with `cargo bench --no-run` and should remain focused on public
API behavior rather than private graph internals.

Required benchmark scenarios:

- Cached reads after the slot has already been computed
- Cold first `get` including graph construction and initial dependency capture
- Dependency fan-out invalidation followed by dependent reads
- Memo equality suppression where an equal intermediate value prevents downstream recomputation
- Effect flushing after dependency mutation
- Batch storms that coalesce many writes into one invalidation/effect flush boundary
- `ThreadSafeContext` contention at 1, 2, 4, 8, and 16 workers, split into:
  - same-root/same-slot write plus read contention
  - independent per-worker roots and computed slots
  - read-mostly waiters with one writer and many readers
  - batched write bursts over per-worker cell groups
- `ThreadSafeContext` synchronization model checking with the optional `loom` feature

The optional `instrumentation` feature adds `instrumentation_snapshot()` and
`reset_instrumentation()` to both context types and exports
`InstrumentationSnapshot`. The snapshot records:

- Reactive node allocation events as a stable allocation proxy
- Slot recompute callback starts
- Duplicate speculative `ThreadSafeContext` recomputes that lose publication races; this should remain zero when in-flight deduplication is effective
- Dependency edges added and removed
- Effect queue pushes and maximum pending queue depth
- `ThreadSafeContext` graph-lock acquisitions plus total wait and hold nanoseconds

`ThreadSafeContext::lock_profile_snapshot()` returns per-operation graph-lock
counters for the thread-safe path. The buckets are intentionally high-level:
unattributed/other work, `get` refresh, dependency edge add/remove, `set_cell`
invalidation, recompute publication, and in-flight recompute waiting. For the
per-slot sidecar recompute `Condvar`s, the in-flight wait bucket records the
parked wait and reacquire boundary. The bucket acquisition counts must sum to the aggregate
`lock_acquisitions` counter so profile consumers can attribute contention
without losing the stable summary fields.

The instrumentation profile bench lives in `benches/profile.rs` and is gated
behind `required-features = ["instrumentation"]`; compile it with
`cargo bench --features instrumentation --no-run`.

The benchmark report harness lives at
`scripts/update-benchmark-results.py`. It runs
`cargo bench --features instrumentation`, reads Criterion estimate files from
`target/criterion`, captures `examples/instrumentation_profile.rs` counter
snapshots in `target/lazily-instrumentation-profile.csv`, and rewrites the
generated README section between
`<!-- benchmark-results:start -->` and `<!-- benchmark-results:end -->`.
The generated section must include the current Cargo package version, the
refresh command, the Criterion baseline comparison workflow, one timing row for
each required benchmark scenario above, and instrumentation rows covering
recomputes, duplicate speculative recomputes, dependency edge churn, effect
queue depth, node allocations, lock wait/hold time, and per-operation
`ThreadSafeContext` lock attribution for every 1/2/4/8/16-worker contention
matrix profile. `--check` verifies that the README section is already current without
rewriting it; `--no-run` reuses existing Criterion estimate files for a
report-only refresh after a manual baseline comparison run while refreshing the
instrumentation CSV unless it is also running in check mode.

## Differences from lazily-zig

| Aspect | lazily-zig | lazily-rs |
|--------|-----------|-----------|
| Context | Explicit allocator | Owned allocations |
| Slot creation | `comptime` function pointers | Closures (`Box<dyn Fn>`) |
| Storage modes | `.direct` / `.indirect` | Unified via generics |
| FFI | Built-in `StringView` | Via `#[no_mangle]` + `extern "C"` |
| Thread safety | Mutex by default; `-Dthread_safe=false` removes locking | `Context` is single-threaded (`RefCell`); `ThreadSafeContext` uses a context-level lock |

## Differences from lazily-py

| Aspect | lazily-py | lazily-rs |
|--------|----------|-----------|
| Context | Plain `dict` | Typed `Context` struct |
| Slot keys | Object identity | `SlotId` (u64) |
| Cell equality | `!=` operator | `PartialEq` trait |
| Context resolvers | `resolve_ctx` functions | Direct context passing |
| Dependencies | Zero mandatory runtime crates by default; optional Tokio support and dev-only Criterion benchmarks | Zero (pure Rust) |
