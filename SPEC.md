# lazily-rs Specification

Rust library for lazy evaluation with context-aware dependency tracking and cache invalidation. Counterpart to lazily-zig and lazily-py.

## Core Concepts

### Context

Container for all slots and cells. Owns all allocations via interior mutability (`RefCell`).

```rust
pub struct Context {
    nodes: RefCell<HashMap<SlotId, Node>>,
    next_id: RefCell<u64>,
}
```

**API:**

| Method | Purpose |
|--------|---------|
| `Context::new()` | Create a new context |
| `ctx.slot(\|ctx\| T)` | Create a lazily-computed slot |
| `ctx.get(&slot)` | Get value (computes if unset) |
| `ctx.cell(value)` | Create a mutable cell |
| `ctx.get_cell(&cell)` | Get cell value |
| `ctx.set_cell(&cell, value)` | Update cell (clears dependents if changed) |
| `ctx.effect(\|ctx\| { ... })` | Run an effect immediately and rerun it after tracked dependencies invalidate |
| `ctx.is_set(&slot)` | Check if slot has cached value |
| `slot.clear(&ctx)` | Clear cached value and cascade to dependents |
| `cell.clear_dependents(&ctx)` | Clear downstream slots without changing cell value |
| `effect.dispose(&ctx)` | Dispose an effect, unsubscribe dependencies, and run cleanup |
| `effect.is_active(&ctx)` | Check whether an effect is still registered |

### Slot

Lazily-computed cached value with dependency tracking. A Slot is either **unset** or **set** with a value produced by its compute function.

```rust
struct SlotNode {
    value: Option<Box<dyn Any>>,
    compute: Box<dyn Fn(&Context) -> Box<dyn Any>>,
    dependencies: HashSet<SlotId>,
    dependents: HashSet<SlotId>,
}
```

**Semantics:**

- **Activation:** First `ctx.get()` calls the compute function, caches the result
- **Clearing:** Removes the cached value and clears all dependent slots recursively
- **Dependencies:** If Slot B accesses Slot A during computation, B depends on A. If A clears, B clears automatically
- **Immutable by default:** Once set, a Slot's value doesn't change — only clear + recompute
- **Dynamic:** Dependencies re-discovered on each recomputation (no stale subscriptions)

### Cell

Mutable value container. Changing a Cell's value clears all dependent Slots.

```rust
struct CellNode {
    value: Box<dyn Any>,
    dependents: HashSet<SlotId>,
}
```

**Semantics:**

- `ctx.set_cell()` compares old and new via `PartialEq`
- If unchanged, no invalidation occurs (no-op)
- If changed, all dependent Slots are recursively cleared

### Effect

Side-effect callback that automatically tracks dependencies. Effects run
immediately on creation, then rerun after any Cell or Slot read during the last
run is invalidated.

```rust
struct EffectNode {
    run: Box<dyn Fn(&Context) -> Option<Box<dyn FnOnce()>>>,
    dependencies: HashSet<SlotId>,
    cleanup: Option<Box<dyn FnOnce()>>,
}
```

**Semantics:**

- **Immediate activation:** `ctx.effect()` runs the callback once during creation
- **Auto-tracking:** Any Slot or Cell accessed during the callback becomes a dependency
- **Scheduling:** Dependency invalidation schedules the effect, then the context flushes scheduled effects after the invalidation pass
- **Coalescing:** An effect scheduled through multiple dependency paths in the same invalidation pass runs once
- **Cleanup:** Returning a cleanup closure runs it before the next rerun and on disposal
- **Disposal:** `effect.dispose(&ctx)` unsubscribes from dependencies, removes pending scheduled work, and prevents future reruns

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
5. When a dependency clears or a Cell changes, slot dependents clear recursively and effect dependents are scheduled

## Invalidation Semantics

- `ctx.set_cell()` → if value changed (PartialEq) → clear all dependent slots
- `slot.clear(&ctx)` → remove cached value → cascade clear to all dependents
- `cell.clear_dependents(&ctx)` → clear all dependent slots without changing cell value
- Slot clearing → remove cached value → cascade clear to all dependents
- Cleared slots recompute on next `ctx.get()` access
- Effects rerun after the invalidation pass if any tracked dependency invalidated
- Effect cleanup runs before rerun and on disposal

## Design Goals

- **Lazy evaluation:** Values computed only when first accessed
- **Fine-grained reactivity:** Only affected dependents recompute
- **Effects:** Side effects are scheduled from the same dependency graph as slots
- **Zero external dependencies:** Pure Rust, no crates
- **Single-threaded:** `RefCell` interior mutability (no Mutex overhead)

## Differences from lazily-zig

| Aspect | lazily-zig | lazily-rs |
|--------|-----------|-----------|
| Context | Explicit allocator | Owned allocations |
| Slot creation | `comptime` function pointers | Closures (`Box<dyn Fn>`) |
| Storage modes | `.direct` / `.indirect` | Unified via generics |
| FFI | Built-in `StringView` | Via `#[no_mangle]` + `extern "C"` |
| Thread safety | Mutex by default | Single-threaded (`RefCell`) |

## Differences from lazily-py

| Aspect | lazily-py | lazily-rs |
|--------|----------|-----------|
| Context | Plain `dict` | Typed `Context` struct |
| Slot keys | Object identity | `SlotId` (u64) |
| Cell equality | `!=` operator | `PartialEq` trait |
| Context resolvers | `resolve_ctx` functions | Direct context passing |
| Dependencies | Zero | Zero (pure Rust) |
