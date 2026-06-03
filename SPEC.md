# lazily-rs Specification

Rust library for lazy evaluation with context-aware dependency tracking and cache invalidation. Counterpart to lazily-zig and lazily-py.

## Core Concepts

### Context

Container for all slots and cells. Owns all allocations via interior mutability
using a single `RefCell<ContextInner>`.

```rust
struct ContextInner {
    nodes: Vec<Option<Node>>,
    next_id: u64,
    free_ids: Vec<u64>,
    pending_effects: VecDeque<SlotId>,
    scheduled_effects: HashSet<SlotId>,
    flushing_effects: bool,
    batch_depth: usize,
    batched_cells: HashSet<SlotId>,
    batched_cell_clears: HashSet<SlotId>,
    batched_slots: HashSet<SlotId>,
}

pub struct Context {
    inner: RefCell<ContextInner>,
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
| `cell.set(&ctx, value)` | Handle method alias for `ctx.set_cell(&cell, value)` |
| `ctx.batch(\|ctx\| { ... })` | Defer changed-cell dirty marking and explicit clears until the outermost batch exits |
| `ctx.effect(\|ctx\| { ... })` | Run an effect immediately and rerun it after tracked dependencies invalidate |
| `ctx.is_set(&slot)` | Check if slot has a cached, fresh value |
| `slot.clear(&ctx)` | Clear cached value and cascade to dependents |
| `cell.clear_dependents(&ctx)` | Clear downstream slots without changing cell value |
| `effect.dispose(&ctx)` | Dispose an effect, unsubscribe dependencies, and run cleanup |
| `effect.is_active(&ctx)` | Check whether an effect is still registered |

`Context` stores nodes in a slot-id-indexed `Vec<Option<Node>>` rather than a
hash map. `SlotId` values are allocated sequentially; effect disposal returns the
ID to a free list for reuse, preventing unbounded `Vec` growth from transient
effects while keeping lookups contiguous and hash-free.

Dependency and dependent edges use `SmallVec<[SlotId; 4]>` rather than `HashSet<SlotId>`.
For the typical 1-3 dependency fan-out, SmallVec stores edges inline without heap
allocation, avoids hash computation overhead, and eliminates the temporary
`Vec<SlotId>` allocations that were previously required in every refresh/clear/dirty
hot path (replaced with `SmallVec::clone()` or `std::mem::take`). Sets that require
true dedup semantics (scheduled effects, batch queues, tracking frames) remain as
`HashSet<SlotId>`.

The single-threaded `Context` consolidates all mutable state behind one
`RefCell<ContextInner>` instead of ten separate `RefCell` fields. This reduces
borrow-check overhead from 4-6 flag modifications per `get()` to 1 and eliminates
the risk of re-entrant borrows across fields. Slot and effect compute closures are
stored as `Rc<dyn Fn>` so they can be cloned cheaply without `unsafe` pointer copies.

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
    compute: Rc<dyn Fn(&Context) -> Box<dyn Any>>,
    equals: Option<Box<dyn Fn(&dyn Any, &dyn Any) -> bool>>,
    dependencies: SmallVec<[SlotId; 4]>,
    dependents: SmallVec<[SlotId; 4]>,
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
    dependents: SmallVec<[SlotId; 4]>,
}
```

**Semantics:**

- `ctx.set_cell()` and `cell.set(&ctx, value)` compare old and new via `PartialEq`
- If unchanged, no invalidation occurs (no-op)
- If changed, dependent Slots are marked dirty while cached values are preserved for memo validation

### Effect

Side-effect callback that automatically tracks dependencies. Effects run
immediately on creation, then rerun after any Cell or Slot read during the last
run is invalidated.

```rust
struct EffectNode {
    run: Rc<dyn Fn(&Context) -> Option<Box<dyn FnOnce()>>>,
    dependencies: SmallVec<[SlotId; 4]>,
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
- **Thread-safe local batching:** same-thread `ThreadSafeContext` batch writes buffer changed cells and clears in a thread-local batch frame, then merge that frame into the graph-owned batch queue at batch exit; cross-thread writes during another active batch still fall back to the graph-owned queue
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

- Uses one context-level `Mutex` synchronization primitive for graph state before introducing finer-grained graph locks
- `ThreadSafeState` stores nodes in a slot-id-indexed `Vec<Option<ThreadSafeNode>>` matching the single-threaded `Context`, eliminating hash-map lookup overhead on every node access. Slot IDs are reused via a free list on effect disposal.
- Fresh cached slot reads use a per-slot read-mostly cached-value sidecar; dependency-edge changes, invalidation frontier application, batch queues, effect queues, and disposal remain graph mutex mutations
- Read-mostly cached slot access is versioned optimistically: the getter loads a per-slot atomic cache revision before cloning the retained cached `Arc`, then validates the revision and dirty/force flags again after the clone. Any concurrent invalidation, clear, or value publish changes the revision and forces the getter onto the graph-validated refresh path.
- Each thread-safe slot also owns a per-slot recompute/value-publish sidecar for the cached-value visibility flags, in-flight bit, waiter `Condvar`, and revision used to reject stale callback results. Graph-state dirty/revision fields remain mirrored under the context mutex for dependency-frontier traversal and tests.
- Each thread-safe slot sidecar mirrors a per-slot dependency summary: the
  current dependency ids plus a slot-dependency count. A cell-only dirty refresh
  may claim the SlotId-partitioned recompute sidecar, snapshot old dependencies,
  and skip the graph-locked `get_refresh` dependency scan. The owner still takes
  the final `publish` graph mutation to diff dynamic dependencies, publish the
  value, notify dependents, and reject stale in-flight revisions.
- Cells and slots mirror their dependent frontiers into per-node sidecars keyed
  by `SlotId`. Changed-cell invalidation may use these sidecars without taking
  the context graph mutex only when no callback is actively discovering
  dependencies, the context is not inside a batch, and the discovered frontier
  contains slots only. The per-slot cache revision acts as the dirty epoch for
  sidecar publication, and instrumentation records each epoch advance so
  parallel writers can publish version state without the graph mutex while
  cached reads still reject mid-read invalidation races.
  Effect scheduling, batching, dynamic dependency discovery, disposal, and any
  frontier that reaches an effect fall back to the graph mutex.
- Do not hold the graph lock while running user compute callbacks, effect callbacks, or cleanup closures
- Re-acquire the lock only to publish computed values, dependency edges, invalidation state, and pending effect work
- Slot refresh must avoid helper-level lock churn: a fresh cached get should clone the value through the per-slot fast path without taking a `get_refresh` graph lock or recursively validating unchanged dependencies, dependency refresh should not take a separate node-kind probe lock before recursively validating a dependency, cell dependencies should not be probed as refreshable slots, clean dirty flags should be folded into the refresh decision lock, and recompute must diff old/new dependency sets at publish so unchanged edges stay subscribed while only stale edges are removed
- Recompute dependency tracking must skip graph-lock edge registration for dependencies already present in the slot's previous dependency set, while still eagerly registering newly discovered dependencies during the callback so concurrent invalidation can mark the in-flight result stale
- Effect rerun dependency tracking must also preserve unchanged edges: dependencies already present on the effect remain subscribed through the rerun, newly discovered dependencies are registered during tracking, and stale dependencies are removed in one post-callback graph mutation before the next cleanup is stored
- Re-entrant user code must be able to call back into the same context without deadlocking
- Concurrent first access and dirty same-slot contention share one in-flight computation for the current slot revision; waiters check the per-slot recompute sidecar before the `get_refresh`/`publish` graph-lock path, park on that slot's notification primitive, then return the published cache or retry if an invalidation makes the in-flight result stale
- Recompute waiters observe the per-slot in-flight/revision state while holding the sidecar mutex, then park on the same sidecar `Condvar`. Finishers publish value and dirty-state sidecar updates before clearing the in-flight bit and notifying, so a stale in-flight completion cannot be missed.
- Recompute notifications are scoped to the slot that finished. A completion for one in-flight slot must not wake waiters parked behind another in-flight slot.
- Per-slot recompute wakeups use a waiter-counted handoff instead of `notify_all`: the finisher calls `notify_one` when waiters exist, and each awakened waiter notifies the next parked waiter after observing the completed sidecar state. This drains all waiters without a completion-wide wakeup stampede.
- Optimistic cached reads fall back whenever the context is dirty, forced to recompute, or racing with a sidecar revision change. They do not publish dependency edges, flush effects, observe batch-local unflushed invalidations as fresh, or replace graph-locked refresh for ambiguous callback/dependency states.
- If an upstream invalidation happens while a slot callback is running, the in-flight stale result is not published as fresh; the getter retries until it can return a value that matches the latest dependency state
- Batch exit, effect scheduling, disposal, and explicit clears must each have a single atomic graph mutation boundary and one coalesced effect flush per outermost invalidation pass
- The outermost thread-safe batch exit must collect dependents for all changed cells and apply one coalesced frontier invalidation, so a shared dependent reached through many changed cells is marked dirty and advances revision once per batch flush
- Thread-safe invalidation uses an explicit `InvalidationPlan` computed from a
  frontier work queue under the graph mutex instead of recursive dependent
  walks. Changed-cell and slot-value-change roots snapshot dependent frontiers,
  coalesce duplicate slot ids in one invalidation pass, preserve direct
  changed-value `force_recompute` upgrades when a slot is reached through both
  direct and downstream paths, snapshot hard-clear frontiers for explicit
  slot/cell clears, then apply dirty, clear, revision, and effect-scheduling
  mutations at the same graph mutation boundary. The plan shape is partitionable
  for future bounded worker traversal, but this prototype keeps snapshot and application under the context mutex until benchmark and model-checking
  evidence proves a parallel apply path safe.

Lock strategy evaluation:

- Keep one context-level graph synchronization primitive until benchmark instrumentation shows a finer-grained design improves the relevant workload without trading off other contention cases
- `ThreadSafeContext` uses read-mostly per-slot cached-value sidecars for fresh
  cached reads, a per-slot recompute/value-publish sidecar for in-flight
  same-slot waiters, a per-slot dependency summary for cell-only dirty refresh
  routing, and per-node dependent frontier sidecars for slot-only changed-cell
  invalidation; same-thread batches use thread-local batch frames to coalesce
  changed-cell queueing before the graph-owned batch flush; dependency graph
  mutations still require the context mutex
- The read-mostly cached-value sidecar is an optimistic validation path, not a
  lock-free graph replacement. Its atomic cache revision rejects mid-read
  invalidation and mid-read publish races, then falls back to the existing
  graph refresh path.
- `ThreadSafeContext` uses per-slot sidecar recompute `Condvar`s for in-flight waiters. Those Condvars guard only per-slot in-flight/revision/cache-visibility state, use waiter-counted `notify_one` handoff wakeups to avoid broad `notify_all` contention, and must not mutate dependency graph state independently of the context mutex
- The read-mostly prototype is benchmark-gated by the 1/2/4/8/16-worker `same_slot_write_read`, `independent_slots`, `read_mostly_waiters`, and `batched_write_bursts` matrix after the `#lazybatch1` and `#lazybatch2` invalidation/read-churn fixes
- The dependent-frontier sidecar prototype is benchmark-gated by
  `thread_safe_contention / independent_slots` and
  `set_cell_invalidation / independent_slot_contention` at 8 and 16 workers. It
  should reduce `set_cell_invalidation` graph-lock acquisitions for independent
  slot-only roots without changing effect, batch, or dynamic-dependency
  semantics.
- High-parallel graph propagation profiles gate the lazy-invalidation path with
  `thread_safe_graph_propagation` at 8 and 16 workers. The matrix compares
  fan-out eager validation, fan-out lazy dirty epoch publication, fan-in lazy
  dirty epoch publication, and fan-in batched flush behavior using throughput,
  p50/p95 latency, lock attribution, effect queue pushes, dependency-edge
  counters, sidecar dirty marks, sidecar fallbacks, and dirty epoch advances.
- The local batch-frame prototype is benchmark-gated by `set_cell_invalidation /
  batched_write_bursts` and `thread_safe_contention / batched_write_bursts` at 8
  and 16 workers. It should reduce per-write graph queueing during same-thread
  batches while preserving one coalesced dirty/effect flush at the outermost
  batch exit.
- Effect-heavy contention profiles gate any queue or batch synchronization
  change. `thread_safe_effect_contention` isolates effect queue coalescing,
  cleanup execution, and nested batch flush behavior at 8 and 16 workers with
  deterministic lock-site budgets before a sharded graph-lock design can be
  considered.
- Synchronization strategy comparison is release-gated by one fixed evidence
  table. The current `std::sync` mutex/Condvar path is the baseline; narrower
  Condvar wakeups are adopted only for per-slot recompute waiters; parking_lot
  style parking and targeted CAS remain candidates. A candidate must report
  throughput plus p50/p95 latency for the required 8/16-worker contention and
  effect-heavy cases, stay within lock-site budgets, and carry Loom/Shuttle
  proof for stale completion, effect scheduling/disposal, batch flush, and
  re-entrant callbacks before release.
- Benchmark watch items from generated README deltas must be confirmed with a
  controlled A/B rerun before tuning. The rerun should use the same benchmark
  filter on the same host/toolchain when possible, isolate baseline and current
  code in clean worktrees or Criterion baselines, and record whether the signal
  reproduces. If confidence intervals overlap or Criterion reports no
  statistically significant change, document the watch item and avoid
  speculative synchronization changes.
- Edge storage A/B benchmarking procedure: the `vec_edges` feature flag
  switches `EdgeVec` from `SmallVec<[SlotId; 4]>` (default) to `Vec<SlotId>`.
  To compare:
  1. `cargo bench --bench context -- dependency_fan_out,set_cell_invalidation/high_fan_out --save-baseline smallvec`
  2. `cargo bench --bench context --features vec_edges -- dependency_fan_out,set_cell_invalidation/high_fan_out --baseline smallvec`
  3. Compare Criterion output — if confidence intervals overlap, keep SmallVec;
     if Vec is faster at the tested fan-out widths, reconsider the default.
  The comparison must run on the same host/toolchain with no other workload.
- Fully lock-free cached reads are deferred for the current erased-value storage. The current versioned optimistic path still clones through the retained sidecar `Arc` snapshot and uses atomic dirty/revision validation to ensure a `get` starting after a completed cross-thread invalidation cannot return the pre-invalidation cached value.
- Any future sharding or CAS path must include a Loom or Shuttle safety model covering concurrent first get, stale in-flight completion, invalidation during compute, effect scheduling/disposal, and re-entrant callbacks before it can replace the single-graph-lock design
- The current sidecar `Mutex`/`Condvar` waiter path, optimistic cached-read fallback, and explicit invalidation-plan safety envelope are covered by `cargo test --features loom --test thread_safe_loom`, which models concurrent first get, scoped slot notification, waiter-counted handoff wakeup draining, stale in-flight completion and retry, read-mostly waiter handoff, mid-read optimistic validation fallback, invalidation during compute, fast-frontier fallback while dependency discovery is active, dynamic dependency switch/disposal cleanup, effect scheduling/disposal races, re-entrant callback graph access, duplicate diamond paths marking each frontier slot once, effect enqueue coalescing, and nested batch invalidation flushing only at the outermost boundary
- A lock-strategy change must preserve the rule that user compute/effect/cleanup callbacks never run while holding graph-state locks

Sharded/versioned storage evaluation:

- After the frontier invalidation and read-churn prototypes, the
  `thread_safe_contention` benchmark and instrumentation rows are the gate for
  storage changes. The read-mostly cached-value sidecar is limited to fresh
  cached slot reads; write-side graph mutation remains serialized so the same
  benchmark matrix can show whether read-side wins trade off same-slot writes,
  independent slots, or batched aggregate writes.
- Do not replace the single graph lock with sharded storage in the current
  implementation. Sharding may help independent roots and slots, but it does
  not remove serialization for same-root writes, shared aggregate slots, effect
  queues, batch-depth accounting, disposal, or dynamic dependency edge changes.
  A shard design must first define shard ownership for dependency edges that
  cross shards, a deterministic merge/apply order for dirty/revision/effect
  mutations, and a Loom or Shuttle model for deadlock-free multi-shard
  invalidation and disposal.
- Do not treat versioned optimistic reads as an invalidation optimization.
  Versioned reads target fresh cached `get` latency, while the isolated
  `set_cell_invalidation` profiles attribute invalidation pressure to
  write-side graph mutation. The current prototype keeps independently retained
  sidecar `Arc` snapshots plus atomic dirty/revision validation so a `get`
  starting after a cross-thread invalidation cannot return the pre-invalidation
  cached value.
- The next storage experiment, if pursued, should be a benchmark-gated
  prototype rather than a replacement: shard independent `ThreadSafeState`
  mutation by stable node id, keep effect queue and batch flush as one
  deterministic merge boundary, and require the isolated `thread_safe_effect_contention`
  profiles, `set_cell_invalidation` matrix, and Loom/Shuttle coverage to improve
  before adopting it.

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
- **Zero mandatory runtime dependencies:** The default library surface uses only the Rust standard library and `smallvec`; Tokio is optional and Criterion is dev-only for benchmarks
- **Single-threaded fast path:** `Context` uses a single `RefCell<ContextInner>` with no mutex overhead and no `unsafe` code
- **Contiguous local storage:** Both `Context` and `ThreadSafeContext` index nodes directly by `SlotId` in a `Vec<Option<Node>>` to avoid hash-map lookup and churn; slot IDs are reused via a free list to prevent unbounded growth from transient effects
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
- `ThreadSafeContext` `set_cell` invalidation isolation, split into:
  - high fan-out changed-cell invalidation without dependent reads
  - same-root/same-slot write contention without dependent reads
  - independent per-worker roots and slots without dependent reads
  - batched write bursts over per-worker cell groups without dependent reads
- `ThreadSafeContext` contention at 1, 2, 4, 8, and 16 workers, split into:
  - same-root/same-slot write plus read contention
  - independent per-worker roots and computed slots
  - read-mostly waiters with one writer and many readers
  - batched write bursts over per-worker cell groups
- `ThreadSafeContext` effect-heavy contention at 8 and 16 workers, split into:
  - effect queue coalescing across batched worker writes
  - effect cleanup execution during concurrent cell updates
  - nested batch flushes that schedule computed-effect dependencies
- `ThreadSafeContext` synchronization model checking with the optional `loom` feature

The optional `instrumentation` feature adds `instrumentation_snapshot()` and
`reset_instrumentation()` to both context types and exports
`InstrumentationSnapshot`. The snapshot records:

- Reactive node allocation events as a stable allocation proxy
- Slot recompute callback starts
- Duplicate speculative `ThreadSafeContext` recomputes that lose publication races; this should remain zero when in-flight deduplication is effective
- Dependency edges added and removed
- Effect queue pushes and maximum pending queue depth
- `ThreadSafeContext` lock/coordination acquisitions plus total wait and hold nanoseconds

`ThreadSafeContext::lock_profile_snapshot()` returns per-operation lock and
coordination counters for the thread-safe path. The buckets are intentionally
high-level: unattributed/other work, `get` refresh, dependency edge add/remove,
`set_cell` invalidation, recompute publication, and in-flight recompute waiting.
For the per-slot sidecar recompute `Condvar`s, the in-flight wait bucket records
the parked wait and reacquire boundary. The bucket acquisition counts must sum
to the aggregate `lock_acquisitions` counter so profile consumers can attribute
contention without losing the stable summary fields.

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
each required benchmark scenario above, p50/p95 Criterion sample latency rows
for the required 8/16-worker same-slot, independent-slot, read-mostly,
batched-write, and effect-heavy cases, and instrumentation rows covering
recomputes, duplicate speculative recomputes, dependency edge churn, effect
queue depth, node allocations, lock wait/hold time, and per-operation
`ThreadSafeContext` lock attribution for every 1/2/4/8/16-worker contention
matrix profile and every `set_cell` invalidation isolation profile. The generated
section must also publish regression budgets for the slow contention profiles and
their lock-site acquisition totals. `--check` verifies that the README section is
already current without rewriting it and fails when required p50/p95 latency
rows are missing or any instrumentation profile exceeds its lock-acquisition
budget; `--no-run` reuses existing Criterion estimate files for a report-only
refresh after a manual baseline comparison run while refreshing the
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
