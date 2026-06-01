# lazily-rs Specification

Rust library for lazy evaluation with context-aware dependency tracking and cache invalidation. Counterpart to lazily-zig and lazily-py.

## Core Concepts

### Context

Container for all slots and cells. Owns all allocations via interior mutability (`RefCell`).

```rust
pub struct Context {
    nodes: RefCell<HashMap<SlotId, Node>>,
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
- Re-entrant user code must be able to call back into the same context without deadlocking
- Concurrent first access may perform duplicate speculative computation, but at most one value may be published as the slot cache; later optimization can add in-flight deduplication
- Batch exit, effect scheduling, disposal, and explicit clears must each have a single atomic graph mutation boundary and one coalesced effect flush per outermost invalidation pass

Tokio integration is scoped in two stages:

1. Synchronous thread-safe sharing first: `ThreadSafeContext` should work inside
   `tokio::spawn` and `tokio::task::spawn_blocking` when all captured values and
   callbacks satisfy the `Send + Sync` bounds above.
2. True async computations/effects are separate future work. They need explicit
   semantics for in-flight future deduplication, cancellation, dependency tracking
   across `.await`, stale future completion, cleanup ordering, and `Send` versus
   `LocalSet` futures.

## Invalidation Semantics

- `ctx.set_cell()` → if value changed (PartialEq) → mark all dependent slots dirty
- `slot.clear(&ctx)` → remove cached value → cascade clear to all dependents
- `cell.clear_dependents(&ctx)` → clear all dependent slots without changing cell value
- `ctx.batch()` → queue changed cells and explicit slot/cell clears, then flush queued roots when the outermost batch exits
- Slot invalidation → preserve cached value as dirty → validate/recompute on next `ctx.get()` access
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
- **Zero external dependencies:** Pure Rust, no crates
- **Single-threaded fast path:** `Context` uses `RefCell` interior mutability with no mutex overhead
- **Explicit thread-safe path:** `ThreadSafeContext` uses a context-level lock and `Send + Sync` bounds for shared reactive graphs

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
| Dependencies | Zero | Zero (pure Rust) |
