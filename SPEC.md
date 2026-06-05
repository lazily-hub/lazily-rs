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
| `slot.get(&ctx)` | Get value (computes if unset) |
| `ctx.get(&slot)` | Context method alias for `slot.get(&ctx)` |
| `ctx.get_rc(&slot)` | Get slot value as `Rc<T>`, avoiding deep clone |
| `ctx.cell(value)` | Create a mutable cell |
| `cell.get(&ctx)` | Get cell value |
| `ctx.get_cell(&cell)` | Context method alias for `cell.get(&ctx)` |
| `ctx.get_cell_rc(&cell)` | Get cell value as `Rc<T>`, avoiding deep clone |
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
| `slot.get(&ctx)` | Get value from any thread (computes if unset) |
| `ctx.get(&slot)` | Context method alias for `slot.get(&ctx)` |
| `ctx.cell(value)` | Create a mutable `Send + Sync` cell |
| `cell.get(&ctx)` | Get cell value from any thread |
| `ctx.get_cell(&cell)` | Context method alias for `cell.get(&ctx)` |
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
- **Lock-free cached reads are implemented** (#vd5v / #lazilyperf38). The per-slot read-mostly cached-value sidecar (`ThreadSafeSlotFastPath.value`) is an `arc_swap::ArcSwapOption<Arc<dyn Any + Send + Sync>>`: `read_fresh` performs a wait-free atomic load of the published snapshot — no `RwLock` read lock — then reconstructs `&T` via the inline `type_id` (the typed-cache prerequisite) without vtable indirection. The atomic dirty/revision validation envelope is unchanged: a reader loads `cache_revision` and checks `dirty`/`force_recompute` before the load and re-checks both after the clone, so a `get` starting after a completed cross-thread invalidation cannot return the pre-invalidation cached value. (`arc-swap`'s `RefCnt` is `Sized`-only, so the stored value is `Arc<Arc<dyn Any>>` — the inner `Arc` is a sized fat pointer; the extra outer `Arc` is allocated only on the cold publish path, never on the read.) Verified by the full default suite and the `thread_safe_loom` model.

  **Contention tradeoff (rigorous isolated-worktree A/B, #xtwf).** This is a deliberate low-contention-for-high-contention trade, not a free win. Comparing `463ca71` (arc-swap) against its parent `06fd3c2` (the `parking_lot::RwLock` read) on a shared Criterion target dir, same host/toolchain:

  | benchmark | arc-swap vs RwLock |
  | --- | --- |
  | `cached_reads/thread_safe_context` (1 thread) | **+3.1%** (slower) |
  | `read_mostly_waiters/4` | **+11.9%** (slower) |
  | `read_mostly_waiters/8` | **−6.5%** (faster) |
  | `read_mostly_waiters/16` | **−28.6%** (faster) |

  arc-swap's wait-free read wins at high core counts where `RwLock` read-lock cache-line traffic dominates, but its debt-tracking load plus the `Arc<Arc<…>>` double-indirection costs a few percent when uncontended. lazily-rs `ThreadSafeContext` targets highly-parallel reactive graphs, so the 8/16-worker win is the operative case and the ≤~3% uncontended/low-contention regression is accepted (see the revised benchmark gate below). The contended numbers carry wide confidence intervals; treat the crossover (~4→8 workers) as approximate.
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

### Typed cache fast-path

`Context` and `ThreadSafeContext` store cached values as type-erased trait
objects (`Rc<dyn Any>` and `Arc<dyn Any + Send + Sync>`).  Every cached read
calls `downcast_ref::<T>()`, which invokes a virtual `type_id()` through the
trait object's vtable, compares the returned `TypeId`, and then performs a
pointer cast.  The vtable call adds indirect branching overhead to the hot
cached-read path.

The typed cache fast-path stores a `TypeId` inline in each node at creation
time.  Cached reads compare the stored `TypeId` directly (a single inline
`u64` equality check) and, on match, use an unchecked pointer cast to recover
the typed reference.  This eliminates the vtable indirection on every cached
slot and cell read for both `Context` and `ThreadSafeContext`.

Storage layout:

- `SlotNode.type_id: TypeId` — set once at `slot()` / `computed()` / `memo()`
  creation from `TypeId::of::<T>()`
- `CellNode.type_id: TypeId` — set once at `cell()` creation
- `ThreadSafeSlotFastPath.type_id: TypeId` — set once at slot creation
- `ThreadSafeCellFastPath.type_id: TypeId` — set once at cell creation

Read-path fast-path:

1. Load the node's `type_id` field (inline `u64` load, no vtable)
2. Compare with `TypeId::of::<T>()`
3. On match, cast the stored value pointer to `&T` without going through
   `dyn Any::downcast_ref`
4. On mismatch, panic with the same "type mismatch" message as before

`get_rc()` and `get_cell_rc()` follow the same pattern but clone the
reference-counted pointer instead of cloning the inner value.

The compute-function signature (`dyn Fn(&Context) -> Rc<dyn Any>` for
`Context`, `dyn Fn(&ThreadSafeContext) -> Box<ThreadSafeAny>` for
`ThreadSafeContext`) remains unchanged; the typed cache is a storage-side
optimization that does not affect the closure API.

Benchmark gate:

- `cached_reads/context` must show a measurable improvement over the
  `downcast_ref` baseline.  The current baseline is approximately 8 ns per
  cached read; the typed cache target is to reduce this by the vtable call
  overhead (typically 0.5–1.5 ns on x86-64).
- `cached_reads/thread_safe_context` (single-thread, uncontended) is a
  **bounded-regression** gate, not a no-regression gate, as of the #vd5v
  lock-free read. The `arc-swap` read sidecar may regress the uncontended
  cached read by up to ~3% in exchange for the high-contention win
  (`read_mostly_waiters` −6%/−29% at 8/16 workers; see *Lock strategy
  evaluation* → contention tradeoff). A regression beyond ~3% uncontended, or
  any regression in the 8/16-worker read-contention matrix, fails the gate and
  must be investigated. The earlier inline-`TypeId` vtable-elimination win for
  `cached_reads/context` (single-threaded `Context`) is unaffected and must
  still hold.
- The improvement must reproduce under controlled Criterion A/B comparison
  (same host, same toolchain, non-overlapping confidence intervals).

Future implications:

- Fully lock-free cached reads require typed storage so that a reader can
  reconstruct a typed reference from an atomically published pointer without
  vtable indirection.  The inline `TypeId` is a prerequisite: it proves the
  type at compile time and makes the unchecked cast sound. **Implemented**
  (#vd5v): `ThreadSafeSlotFastPath.value` is now an `arc_swap::ArcSwapOption`,
  so `read_fresh` loads the published snapshot wait-free and recovers `&T` via
  the inline `type_id` — see *Lock strategy evaluation* above.
- A future `ErasedValue` storage type could replace `Rc<dyn Any>` /
  `Arc<dyn Any>` entirely, storing the value inline for small types and
  avoiding heap allocation on compute.  The current inline-`TypeId` step
  preserves the `Rc<dyn Any>` / `Arc<dyn Any>` layout while unlocking the
  fast-path read optimization.

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

### AsyncContext

True async support is a new explicit async context surface, not an overload of
`Context` or `ThreadSafeContext`. The `AsyncContext` API lives behind a separate
`async` feature flag so downstream users do not accidentally accept the larger
semantic surface. The `async` feature depends on `tokio` for runtime primitives
(`spawn`, `JoinHandle`, notification).

#### AsyncContext type definitions

```rust
pub struct AsyncContext {
    inner: Arc<AsyncContextInner>,
}

pub struct AsyncSlotHandle<T> {
    id: SlotId,
    _marker: PhantomData<T>,
}

pub struct AsyncCellHandle<T> {
    id: SlotId,
    _marker: PhantomData<T>,
}

pub struct AsyncEffectHandle {
    id: SlotId,
}

pub struct AsyncComputeContext<'a> {
    context_id: AsyncContextId,
    node_id: SlotId,
    inner: &'a AsyncContextInner,
}
```

#### AsyncContext API surface

| Method | Signature | Purpose |
|--------|-----------|---------|
| `new` | `fn new() -> Self` | Create a new async context |
| `cell` | `fn cell<T>(&self, value: T) -> AsyncCellHandle<T>` | Create a mutable cell (`T: PartialEq + Clone + Send + Sync + 'static`) |
| `get_cell` | `fn get_cell<T>(&self, handle: &AsyncCellHandle<T>) -> T` | Get cell value (synchronous) |
| `set_cell` | `fn set_cell<T>(&self, handle: &AsyncCellHandle<T>, value: T)` | Update cell and invalidate dependents |
| `computed_async` | `fn computed_async<T, F, Fut>(&self, compute: F) -> AsyncSlotHandle<T>` | Create an async computed slot |
| `get` | `fn get<T>(&self, handle: &AsyncSlotHandle<T>) -> Option<T>` | Synchronous cached read; returns `Some(T)` if resolved, `None` otherwise. Avoids async overhead on warm paths |
| `get_async` | `async fn get_async<T>(&self, handle: &AsyncSlotHandle<T>) -> T` | Await slot value; uses `get()` fast-path for resolved slots, otherwise spawns async compute |
| `memo_async` | `fn memo_async<T, F, Fut>(&self, compute: F) -> AsyncSlotHandle<T>` | Like `computed_async` with `PartialEq` memo guard |
| `effect_async` | `fn effect_async<F, Fut, C, CleanupFut>(&self, effect: F) -> AsyncEffectHandle` | Create an async effect |
| `dispose_async_effect` | `fn dispose_async_effect(&self, handle: &AsyncEffectHandle)` | Dispose async effect and await cleanup |
| `batch` | `fn batch<F, R>(&self, run: F) -> R` | Synchronous batch boundary; schedules async reruns at batch exit |

API bounds:

| Method family | Additional bounds |
|---------------|-------------------|
| `get` | `T: Clone + Send + Sync + 'static` |
| `cell`, `get_cell`, `set_cell` | `T: PartialEq + Clone + Send + Sync + 'static` |
| `computed_async`, `memo_async` | `T: PartialEq + Clone + Send + Sync + 'static`; compute `Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static`; future `Future<Output = T> + Send + 'static` |
| `effect_async` | effect `Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static`; future `Future<Output = Option<C>> + Send + 'static`; cleanup `FnOnce() -> CleanupFut + Send + 'static`; cleanup future `Future<Output = ()> + Send + 'static` |
| handles | remain id-only and copyable; usable from any task only with the owning `AsyncContext` |

#### AsyncSlotNode state machine

Each async slot tracks its state through a finite state machine:

```rust
enum AsyncSlotState {
    Empty,
    Computing { revision: u64, handle: JoinHandle<()> },
    Resolved,
    Error,
}
```

States:

- **Empty:** no cached value, no in-flight computation. Entered on creation and
  after hard clear.
- **Computing:** a `JoinHandle` tracks the in-flight future for the current
  revision. Concurrent `get_async` callers attach waiters to the same
  in-flight result instead of spawning duplicate futures.
- **Resolved:** the cached value is fresh. The value remains until dependency
  invalidation transitions back to Computing.
- **Error:** the last computation failed. Callers receive the error or retry on
  the next `get_async`.

State transitions:

- **Empty → Computing:** first `get_async` call or dependency invalidation when
  no cached value exists.
- **Computing → Resolved:** future completes with `Ok`, and the recorded
  revision still matches the current slot revision. The value is cached.
- **Computing → Error:** future completes with `Err`, and the recorded revision
  still matches.
- **Computing → Computing (stale):** dependency invalidation advances the slot
  revision during an in-flight computation. The completing future finds its
  revision no longer matches and discards the result. A new future is spawned
  for the updated revision.
- **Resolved → Computing:** dependency invalidation marks the cached value stale
  and spawns a new computation.
- **Error → Computing:** `get_async` retry after an error.

Revision tracking ensures stale completions are discarded: an async computation
records the slot revision at start; at publish time the graph accepts the value
only if the revision is still current.

#### AsyncContext cancellation contract

1. **Waiter cancellation is safe:** dropping one `get_async` future does not
   cancel the shared in-flight computation while other waiters still need it.
   Each waiter holds a shared handle (e.g., oneshot receiver or `Shared<...>`);
   dropping the receiver does not abort the `JoinHandle`.

2. **Stale completion handling:** when dependency invalidation advances the slot
   revision during an in-flight computation, the completing future finds its
   recorded revision no longer matches and discards the result. Waiting callers
   are retried against the new revision or attached to the newly spawned future.

3. **Explicit cancellation:** `slot.clear()`, dependency invalidation, or
   context disposal may mark the in-flight revision as canceled. If the runtime
   provides an abort handle, the task is aborted. User futures must be
   cancellation-safe because aborting drops them at an `.await` boundary.

4. **Context disposal:** dropping the `AsyncContext` cancels all in-flight
   computations via their `JoinHandle::abort()` handles and awaits completion
   of all active cleanup futures before returning.

5. **Effect cleanup futures** must complete before the next effect body starts.
   Disposal removes pending reruns before awaiting cleanup.

#### AsyncContext dependency tracking

Async compute and effect callbacks do not use thread-local tracking stacks.
Instead, each callback receives an `AsyncComputeContext`:

```rust
impl<'a> AsyncComputeContext<'a> {
    pub async fn get_async<T>(&self, handle: &AsyncSlotHandle<T>) -> T;
    pub fn get_cell<T>(&self, handle: &AsyncCellHandle<T>) -> T;
}
```

- `get_async` on the compute context records the accessed slot as a dependency
  before awaiting its value.
- `get_cell` on the compute context records the accessed cell as a dependency
  synchronously.
- Dependencies are collected into a `HashSet<SlotId>` attached to the async
  node. On rerun, stale dependencies are removed and new dependencies are
  registered.
- This design survives executor thread migration and suspension/resume across
  `.await` points because the dependency set is carried by the
  `AsyncComputeContext`, not a thread-local.

#### AsyncContext async effects

```rust
fn effect_async<F, Fut, C, CleanupFut>(&self, effect: F) -> AsyncEffectHandle
where
    F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<C>> + Send + 'static,
    C: FnOnce() -> CleanupFut + Send + 'static,
    CleanupFut: Future<Output = ()> + Send + 'static;
```

- **Serialized reruns:** async effect reruns are serialized per effect. A rerun
  does not start until the previous cleanup future completes.
- **Cleanup ordering:** the cleanup future from the previous run completes before
  the next effect body starts. Disposal awaits the current cleanup before
  removing the effect node.
- **Auto-tracking:** the effect body receives an `AsyncComputeContext` and
  tracks dependencies through `get_async` and `get_cell` calls.
- **Dependency invalidation** schedules an async rerun after the current
  invalidation pass. The rerun is spawned on the runtime executor, not inline.
- **Effect disposal:** removes pending scheduled reruns, awaits the current
  cleanup future, and unsubscribes dependency edges.

#### AsyncContext batch support

- `ctx.batch()` is a synchronous boundary. Cell updates queue invalidation
  roots.
- At batch exit, queued roots trigger invalidation propagation. Async slots
  and effects are scheduled for rerun but do not execute inside the batch
  callback.
- Async reruns execute after the batch returns, on the runtime executor.
- Batching semantics remain synchronous at the graph mutation boundary:
  invalidations schedule async reruns only after the outermost batch exits.

#### AsyncContext feature flag

```toml
[features]
async = ["dep:tokio"]
```

The `async` feature depends on Tokio for `spawn`, `JoinHandle`, and runtime
primitives. It is separate from the `tokio` feature (which covers synchronous
`ThreadSafeContext` sharing inside Tokio tasks). The `async` feature implies
`tokio`.

Integration tests live in `tests/async_integration.rs` and are gated behind
`#![cfg(feature = "async")]`. Run with `make test-async` (or
`cargo test --locked --features async`). The `make check` target includes
`test-async` alongside `test`, `test-tokio`, and `test-loom`.

#### AsyncContext implementation notes

- Async graph locks must never be held while polling user futures or cleanup
  futures. Acquire the lock only to read/write graph state, then release before
  polling.
- Nested async slot reads register dependencies on the awaiting parent before
  awaiting the child result.
- The sync `tokio` feature is not enough to enable this API; true async support
  uses the separate `async` feature flag.
- The `Send` async context requires `Send + Sync + 'static` values, callbacks,
  futures, and cleanup futures. A future `LocalAsyncContext` may support `!Send`
  futures on `tokio::task::LocalSet`, but handles must not be interchangeable
  with the `Send` async context.
- In-flight future deduplication: each async slot has one published cache and
  at most one in-flight computation for the current slot revision. Concurrent
  `get_async` callers await the same in-flight result instead of spawning
  duplicate futures.
- Synchronous cached-read fast-path: `get()` returns the cached value
  synchronously when the slot is `Resolved`, avoiding async overhead. `get_async()`
  calls `get()` first; only unresolved or dirty slots enter the async spawn path.

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

## Serialization (`lazily-serde` feature gate)

The `serde` feature is declared in `Cargo.toml` but currently has no
implementation. Its purpose is to produce a serializable snapshot of context
state so the planned `lazily-ipc` (snapshot + incremental update protocol) and
`lazily-distributed` (CRDT/Raft remote graphs) layers have a stable on-the-wire
representation to build on. This section fixes the design of that feature gate
before any code lands.

### The type-erasure problem

`Context` and `ThreadSafeContext` store cached values as type-erased trait
objects (`Rc<dyn Any>` and `Arc<dyn Any + Send + Sync>`; see *Typed cache
fast-path*). `serde::Serialize` is **not object-safe** — it has a generic
`serialize<S: Serializer>` method — so a `dyn Any` cannot be serialized
directly, and the concrete type `T` is gone by the time a snapshot walks the
node `Vec`. Any serialization design must recover the ability to call a
monomorphized `Serialize`/`Deserialize` for each node's erased `T`. Two
approaches were considered.

### Approach A — trait bounds (erased-serde style)

Replace `dyn Any` cache storage with a serialize-aware trait object — a sealed
`dyn ReactiveValue: Any` whose vtable also carries an `erased_serde`-style
`erased_serialize(&self, &mut dyn Serializer)`. Every signal-creation API
(`slot`, `computed`, `memo`, `cell`) gains a `T: Serialize + DeserializeOwned`
bound under `#[cfg(feature = "serde")]`.

- **Pros:** serialization is *total* — every cached node is serializable with
  no per-node opt-in; one storage type, one source of truth.
- **Cons:**
  - The `T: Serialize` bound is **viral**: it propagates through the whole
    public API and leaks onto `Context` itself even for signals that are never
    serialized, forcing callers to satisfy it for purely local reactive state.
  - Requires either the `erased-serde` crate or a hand-rolled vtable
    equivalent — acceptable under a feature gate but still added surface.
  - **Breaks the typed cache fast-path.** That optimization recovers `&T` via
    an unchecked pointer cast keyed on an inline `TypeId`; swapping the trait
    object out from under it for a serialize-aware type would have to preserve
    the same unchecked-cast guarantee.
  - `Deserialize` is the hard half: reconstruction needs the concrete type at
    the call site, so a type-tag → constructor registry is required anyway —
    Approach A does not avoid the registry, it only adds the viral bound on top
    of it.

### Approach B — type-erased closures captured at creation (recommended)

Keep `dyn Any` storage untouched. At signal creation under the `serde` feature,
capture a monomorphized serde **vtable** — a small `&'static` struct of
function pointers — alongside the `TypeId` the typed cache fast-path already
records:

```rust
#[cfg(feature = "serde")]
pub struct SerdeVTable {
    pub type_tag: &'static str,                                  // stable cross-process key
    pub serialize:   fn(&dyn Any, &mut dyn erased_serde::Serializer)
                       -> Result<(), erased_serde::Error>,
    pub deserialize: fn(&mut dyn erased_serde::Deserializer)
                       -> Result<Rc<dyn Any>, erased_serde::Error>,
}
```

Each `SlotNode`/`CellNode` (and the `ThreadSafe*FastPath` mirrors) gains a
`#[cfg(feature = "serde")] serde_vtable: Option<&'static SerdeVTable>` field,
set once at `slot()` / `computed()` / `memo()` / `cell()` creation. The thunks
are monomorphized per `T` at the construction call site, so the concrete type
is captured exactly where it is still known; invoking `serialize` downcasts the
`&dyn Any` back to `&T` and calls the real `T: Serialize`.

- **Pros:**
  - **Composes with the typed cache fast-path** — both capture per-`T`
    metadata at creation; the read hot path is untouched and pays zero
    overhead (thunks run only at snapshot time).
  - **Opt-in per node.** A signal whose `T: !Serialize` stores `None` and is
    emitted as an `Opaque` placeholder, matching how `lazily-ipc` will snapshot
    only an explicitly shared subgraph rather than the whole context. No viral
    bound on `Context`.
  - The `deserialize` thunk doubles as the type-tag → constructor registry
    Approach A needs anyway; populating it at slot construction keeps tags and
    constructors in one place.
- **Cons:**
  - Per-node storage grows by one pointer (`Option<&'static SerdeVTable>`, 8
    bytes) — eliminated entirely when the `serde` feature is off via
    `#[cfg(feature = "serde")]` on the field.
  - Without specialization on stable Rust, "serialize if `T: Serialize`, else
    `None`" needs a sealed `MaybeSerialize<T>` autoref/marker helper or
    explicit `*_serde` constructor variants rather than a blanket impl.

### Decision

**Adopt Approach B.** The `lazily-ipc`/`lazily-distributed` roadmap serializes
an explicit allowlisted `RemoteOp` set (#39c5), not arbitrary nodes, so the
*totality* Approach A buys is unneeded — and it costs a viral bound plus a
rework of the typed cache fast-path to buy it. Approach B keeps the default
build byte-for-byte identical (field compiled out), preserves the zero-overhead
read path, and reuses the `TypeId`-at-creation pattern already proven by the
typed cache.

### Feature-gate shape

- `serde = ["dep:serde"]` (declared) gains `dep:erased-serde` under the same
  gate; both stay optional, preserving the **zero mandatory runtime
  dependency** goal.
- A sealed `MaybeSerialize<T>` helper resolves the vtable to `Some` when
  `T: Serialize + DeserializeOwned` and `None` otherwise, so existing
  constructor signatures are unchanged and non-serializable signals keep
  compiling.
- `Context::snapshot()` / `ThreadSafeContext::snapshot()` walk live nodes,
  invoke each present vtable, and emit `ContextSnapshot { nodes: Vec<NodeSnapshot
  { slot_id, type_tag, payload | Opaque }> }`. Restoration reads `type_tag`,
  looks up the `deserialize` thunk, and rebuilds typed cache entries.
- This snapshot type is the input to #ipc2 (snapshot + incremental update
  protocol) and the value substrate for #ipc3 (CRDT vs Raft).

## IPC snapshot + incremental update protocol (`lazily-ipc`)

`lazily-ipc` transmits a reactive graph's state to a remote observer and keeps
it in sync as the graph mutates. It builds directly on the `ContextSnapshot`
from the `lazily-serde` design and reuses the existing batch-flush and
cache-revision machinery as its consistency boundary rather than inventing a
new one.

### Two message kinds

- **`Snapshot`** — full graph state. Sent on connect and on resync.
- **`Delta`** — incremental change set. Sent **once per outermost batch-flush
  invalidation pass** — the single atomic graph-mutation boundary the
  thread-safe path already guarantees (see *Threading and Concurrency
  Contract*: "Batch exit ... a single atomic graph mutation boundary and one
  coalesced effect flush per outermost invalidation pass"). Coalescing is
  therefore free: one delta per flush, never one per `set_cell`.

### Epoch / versioning

A context-level monotonic `ipc_epoch: u64` advances **once per outermost batch
flush**, not per write. It is independent of the per-slot `cache_revision`
atomics (which remain the read-path dirty epoch) — `ipc_epoch` is the wire
sequence number.

- `Snapshot` carries `epoch`.
- Each `Delta` carries `{ base_epoch, epoch }` with `epoch == base_epoch + 1`.
  Deltas are strictly sequential, so a receiver detects any gap, reorder, or
  sender restart by checking `base_epoch == last_epoch`.

### Payloads

```
Snapshot { epoch: u64, nodes: Vec<NodeSnapshot>,           // NodeSnapshot from lazily-serde
           edges: Vec<(SlotId /*dependent*/, SlotId /*dependency*/)>,
           roots: Vec<SlotId> }                            // cells + source slots

Delta { base_epoch: u64, epoch: u64, ops: Vec<DeltaOp> }

DeltaOp =
  | CellSet    { slot_id, payload }           // changed-value cell write (PartialEq-guarded)
  | SlotValue  { slot_id, payload }           // a recompute published a new value
  | Invalidate { slot_id }                    // dirtied, not yet recomputed (lazy)
  | NodeAdd    { slot_id, type_tag, payload | Opaque }
  | NodeRemove { slot_id }                    // freed slot id (free-list reuse → Remove then Add)
  | EdgeAdd    { dependent, dependency }
  | EdgeRemove { dependent, dependency }
```

### Consistency invariants (inherited, not re-derived)

- **PartialEq cell guard:** an equal `set_cell` invalidates nothing, so it emits
  no `CellSet` and no downstream ops — the wire is silent exactly when the graph
  is (SPEC *Invalidation Semantics*).
- **memo equality suppression:** a dirty `memo()` that recomputes to an equal
  value emits no `SlotValue` and no downstream `Invalidate`, mirroring the
  local "downstream dirty slots become fresh without recomputing" rule.
- **Coalesced frontier:** a dependent reached through many changed cells in one
  batch appears at most once per delta — the same once-per-pass guarantee the
  thread-safe frontier already enforces.

### Lazy reconciliation

Because lazily-rs is lazy, a flush can invalidate a slot without producing a new
value. Two receiver modes:

- **Value-mirror (default for IPC):** at flush the *sender* resolves each
  invalidated allowlisted slot via `ctx.get()` so the delta carries concrete
  `SlotValue`s. The receiver stays a pure data mirror holding no compute
  closures. Trades local laziness for a value-complete wire image.
- **Mirror-lazy:** the sender emits bare `Invalidate` and the receiver keeps a
  stale marker, recomputing only on its own read. This requires the compute
  closures to be replicated too and is therefore **deferred to
  `lazily-distributed`** (#ipc3), not `lazily-ipc`.

### Resync / gap handling

The receiver tracks `last_epoch`. On a `Delta` whose `base_epoch != last_epoch`
(gap, reorder, or sender restart) it discards the delta and requests a
`Snapshot`; the sender replies with a fresh `Snapshot { epoch }` and resumes
deltas from there. Messages are length-prefixed and tagged `Snapshot` / `Delta`
via `serde`/`erased-serde`; the protocol is transport-agnostic (unix socket,
pipe, WebSocket — the last feeds the #yxjw signaling server).

### Permission boundary (forward link to #39c5)

Only nodes on the per-peer allowlist (#39c5 `RemoteOp`) are serialized into a
snapshot or delta; non-allowlisted nodes are **omitted entirely** — not even as
`Opaque` — so a peer cannot infer their existence. The allowlist is applied at
snapshot/delta *construction*, before serialization, so the filter is the same
on the full and incremental paths.

#### Implemented (#39c5)

The permission policy layer ships behind the `distributed` feature in
`src/distributed.rs`:

- `NodeId` / `PeerId` — wire-stable identifiers (decoupled from the internal
  `SlotId`), `serde`-derived under the `serde` feature.
- `OpKind` (`Read` / `Write` / `TriggerEffect`) and `RemoteOp { kind, node }` —
  the gated, serializable unit a peer requests; the three kinds are gated
  **independently** (a read grant never implies write or effect-trigger).
- `PeerPermissions` — **default-deny** per-peer allowlist with `allow`,
  `allow_many`, `revoke` (prunes empty peer entries), `revoke_peer`,
  `is_allowed`, and a fail-closed `check` → `Result<(), PermissionDenied>`.
- `filter_readable(peer, nodes)` enforces the **omission** invariant above:
  non-readable nodes are dropped from the result entirely, preserving input
  order, so it can be applied at snapshot/delta construction before
  serialization.

`PeerPermissions` is local server-side state and is intentionally **not**
serializable; only the wire-facing `RemoteOp` family is. Higher layers
(`lazily-ipc` snapshot/delta construction, the `lazily-distributed` CRDT cell
plane, and the single-writer effect authority) gate every remote request
through `check` and build observable subgraphs through `filter_readable`.

### Feature gate

A new `ipc = ["serde"]` feature adds the pure-protocol `Snapshot`/`Delta` types
plus a transport-agnostic `IpcSink` / `IpcSource` trait pair. No transport
dependency enters the core crate. The `Delta`/`ipc_epoch` model is a
single-writer linear log; whether multi-writer needs CRDT merge or Raft
consensus on top of that log is exactly the #ipc3 question.

## Multi-writer coordination: CRDT vs Raft (`lazily-distributed`)

`lazily-ipc` (above) is a **single-writer** linear log: one authority mutates
the graph and streams `Delta`s stamped by a monotonic `ipc_epoch`.
`lazily-distributed` asks the harder question — when **multiple peers** may
write the same shared reactive graph, what coordination model orders those
writes: a CRDT (conflict-free replicated data types, eventual consistency) or
Raft (leader-ordered consensus, strong consistency)?

### The reactive structure collapses most of the question

The key observation is that **not all graph state is writable**. lazily-rs has
exactly two node kinds with respect to authorship:

- **Cells / source slots** — externally writable. These are the *only* state a
  peer can directly set.
- **Derived slots** (`computed` / `memo`) — pure deterministic functions of
  their dependencies. They are never written; they *recompute*. Their values,
  and the dynamic dependency topology discovered during recompute, are a
  **deterministic view** of the cell state — *provided every peer runs
  identical compute closures* (the closure-replication prerequisite already
  flagged as `lazily-ipc`'s mirror-lazy mode).

So coordination is only needed on the small **cell plane**. The entire derived
graph — typically the large majority of nodes, plus all edges and the effect
schedule — converges automatically once the cells converge and recompute runs.
This is the same property the local engine already relies on: derived state is a
function, not a source of truth.

### The two models on the cell plane

| Aspect | CRDT (cell-plane registers) | Raft (leader-ordered log) |
|--------|------------------------------|---------------------------|
| Consistency | Eventual; peers converge after delivery | Strong; one total order, every peer identical |
| Availability | Local-first — peers read/write while partitioned | Minority partition cannot write; needs quorum |
| Write latency | Local (no round-trip) | Quorum round-trip to leader per write |
| Offline peers | Native (merge on reconnect) | Not supported (writes need quorum) |
| P2P / WAN fit | Direct (no leader); fits #yxjw signaling | Awkward — leader election over WAN, quorum cost |
| Conflict model | Per-cell merge (LWW / MV register) | None — serialized, last in order wins by fiat |
| Extends #ipc2 | Per-peer `Delta`s + causal stamps, merged | One global Raft-replicated `Delta` log |
| Cost | Every writable cell must *be* a CRDT | Election, log replication, quorum machinery |

### Recommendation — CRDT cell plane (HLC-stamped registers), not Raft

Adopt a **CRDT layer on the cell plane only**, with derived slots recomputing
deterministically on each peer. Concretely:

- Each replicated cell is a **register CRDT keyed by a hybrid logical clock
  (HLC)** — wall-clock for human-meaningful ordering, logical counter for
  causal tiebreak. Two flavors, chosen per cell via a trait:
  - **LWW-register** (last-write-wins) — default; "current value" semantics that
    most reactive cells want. Silently drops the losing concurrent write.
  - **MV-register** (multi-value) — surfaces concurrent writes as a set for the
    compute layer (or app) to resolve, when dropping a write is unacceptable.
  - Additive cells can opt into a **PN-counter** instead of a register.
- The local **PartialEq invalidation guard** still applies — *after* merge: a
  merge that yields an equal value invalidates nothing, exactly as a local equal
  `set_cell` does. **memo equality suppression** likewise holds post-merge, so
  convergent peers do the same downstream work.
- `lazily-ipc`'s `Delta` generalizes from one monotonic `ipc_epoch` to
  **per-peer causal stamps**: each peer keeps its own sequence; cross-peer order
  comes from the HLC/dot metadata carried on each `CellSet`. Delivery can be
  out-of-order; merge is commutative/associative/idempotent so gaps self-heal
  without the snapshot-resync that the single-writer log needed.

Raft is the wrong default because the `lazily-distributed` roadmap is
explicitly **availability-first** — P2P signaling (#yxjw) and offline peers —
and Raft trades exactly that away for a global total order that the reactive
model does not need: derived state is already deterministic, and the writable
surface is small.

### The narrow exception — irreversible effects need an authority

CRDT convergence is correct for *state*. It is **not** sufficient for **effects
that perform irreversible external actions** (send an email, charge a card,
fire a webhook): convergence may run the same effect on every peer, or run it
twice as merges arrive. Pure state can converge; an external side effect cannot
be merged.

For that narrow class, gate the effect behind a **single-writer effect
authority** — a designated peer (or a small Raft group owning *only the
effect-intent log*, not the whole graph) decides when an irreversible effect
fires, at-most-once. This is a hybrid: **CRDT for the state plane, a
single-writer/Raft authority for the irreversible-effect plane**, with the
#39c5 `RemoteOp` allowlist already gating which remote writes and effects a peer
may trigger at all. The large reactive core stays leaderless and local-first;
only the small irreversible-effect tail pays for consensus.

### Open prototype gates (deferred to implementation)

- HLC skew bounds and the LWW-vs-MV default per cell category.
- Whether closure replication (required for peers to recompute derived slots) is
  shipped as serialized compute descriptors or restricted to a shared compiled
  graph — this gates how much of the derived plane can live remotely at all.
- Delta-state vs op-based CRDT encoding on the wire, reusing `lazily-serde`.

## Internet-scale peer discovery: signaling server (`#yxjw`)

The CRDT cell plane (above) is leaderless and local-first, but peers still have
to *find* each other and open transport before any `Delta` can flow. On a LAN
that is mDNS or a known address; across the internet it needs a rendezvous
point. `#yxjw` is that rendezvous: a small **Cloudflare Worker signaling
server** that brokers peer discovery and relays the WebRTC SDP/ICE handshake so
peers can establish **direct P2P data channels**, falling back to server relay
of opaque payloads when a direct channel cannot be formed. It is strictly a
*discovery + relay* layer — it never parses or merges CRDT state, so it stays
trivially horizontally scalable and never becomes the consistency authority the
CRDT design deliberately avoids.

### Why a Cloudflare Worker + Durable Objects

- **One Durable Object per session id.** A WebSocket upgrade to
  `GET /session/:id` routes to a `SignalingRoom` DO keyed by
  `idFromName(sessionId)`. Cloudflare guarantees a single global instance per
  id, so each session gets a lock-free single-threaded coordination point for
  its roster with no external store. Scale is achieved by **sharding sessions
  across DO instances**, not by growing one server — which matches the
  availability-first, P2P posture of the CRDT recommendation (it "fits #yxjw
  signaling").
- **Edge-local.** Workers run close to peers worldwide, minimizing handshake
  latency; the DO migrates to wherever its session is most active.

### Roles and protocol

The server tracks a per-session **roster** of connected peers and forwards three
classes of frame. `PeerId` is the same `u64` as Rust `PeerId` (serialized by
`serde` as a bare JSON number; ids must stay ≤ `Number.MAX_SAFE_INTEGER`).

- Client → server: `join { peer, capabilities? }`, `offer { to, sdp }`,
  `answer { to, sdp }`, `ice { to, candidate }`, `relay { to, payload }`,
  `leave`.
- Server → client: `welcome { peer, peers }` (roster on join),
  `peer-joined`/`peer-left`, forwarded `offer`/`answer`/`ice`/`relay` stamped
  with the real `from`, and `error { code, message }`.

**Anti-spoofing:** the `from` on every forwarded frame is the *sender
connection's* registered peer id, never a client-supplied field, so a peer
cannot impersonate another.

### Permission boundary (reuses #39c5)

Admission and relay are gated by `SignalingPermissions`, the discovery-layer
mirror of `lazily::distributed::PeerPermissions`:

- `open` mode — any peer may join and signal any other joined peer (trusted /
  LAN / common discovery case).
- `allowlist` mode — **default-deny**: a peer may join only when explicitly
  granted, and may send directed frames only to explicitly allowed targets,
  exactly as #39c5 gates `RemoteOp`. This is the discovery-layer half of the
  same boundary; the Rust data plane still re-checks every `RemoteOp` locally.

### Reconnect / resync

The roster lives in the DO; it is authoritative. A peer that drops simply
re-`join`s and receives a fresh `welcome` roster — no snapshot epoch is needed
at this layer because signaling carries no CRDT state (the data plane's
HLC/dot-stamped `Delta`s self-heal independently per the CRDT design).

### Implemented (#yxjw)

Ships as a standalone TypeScript Worker under `signaling/` (its own Node
toolchain; not part of the Rust crate build):

- `src/protocol.ts` — wire types + untrusted-frame validation/codec.
- `src/permissions.ts` — `SignalingPermissions` (`open` / default-deny
  `allowlist`).
- `src/room-core.ts` — transport-agnostic `RoomCore`: roster, routing,
  anti-spoofing, permission gating.
- `src/room.ts` — `SignalingRoom` Durable Object (thin WebSocket adapter).
- `src/index.ts` — Worker entry: `/health` + `/session/:id` routing.
- `test/` — 24 vitest tests (protocol/permissions/room-core units plus an
  end-to-end Worker + DO + WebSocket test in the workerd runtime).

### Open gates (deferred)

- TURN/relay fallback policy when both peers are behind symmetric NAT (today the
  server can relay payloads, but a dedicated TURN allocation is out of scope).
- Authenticated admission tokens feeding the `allowlist` grants from an external
  identity source rather than static configuration.
- Capacity/back-pressure limits per session DO.

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
