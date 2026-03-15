# lazily-rs Specification

Rust library for lazy evaluation with context-aware dependency tracking and cache invalidation. Counterpart to lazily-zig and lazily-py.

## Core Concepts

### Context

Container for all slots and their cached values. Owns all allocations.

```rust
pub struct Context {
    cache: HashMap<TypeId, Box<dyn Any>>,
}

impl Context {
    pub fn new() -> Self;
    pub fn get_slot<T: 'static>(&self, key: TypeId) -> Option<&Slot<T>>;
}
```

### Slot

Lazily-computed cached value with dependency tracking. A Slot is either **unset** or **set** with a value produced by its activating function.

```rust
pub struct Slot<T> {
    value: Option<T>,
    compute: Box<dyn Fn(&mut Context) -> T>,
    subscribers: Vec<Box<dyn FnMut(&mut Context)>>,
    parents: Vec<SlotId>,
}
```

**Semantics:**

- **Activation:** First access calls the compute function, caches the result
- **Clearing:** `slot.clear()` removes the cached value and clears all dependent slots
- **Dependencies:** If Slot B accesses Slot A during computation, B depends on A. If A clears, B clears automatically
- **Immutable by default:** Once set, a Slot's value doesn't change — only clear + recompute

**API:**

| Method | Purpose |
|--------|---------|
| `Slot::new(compute_fn)` | Create a new unset slot |
| `slot.get(&mut ctx)` | Get value (compute if unset) |
| `slot.clear(&mut ctx)` | Clear value and cascade to dependents |
| `slot.is_set()` | Check if value is cached |

### Cell

Mutable value container. Changing a Cell's value clears all dependent Slots.

```rust
pub struct Cell<T> {
    value: T,
    subscribers: Vec<Box<dyn FnMut(&mut Context)>>,
}
```

**API:**

| Method | Purpose |
|--------|---------|
| `Cell::new(initial)` | Create with initial value |
| `cell.get()` | Read current value |
| `cell.set(value, &mut ctx)` | Update value, clear dependents if changed |

## Dependency Tracking

Uses a thread-local tracking stack (mirroring lazily-zig's `TrackingFrame` approach).

1. When a Slot computes, it pushes a frame onto the tracking stack
2. Any nested slot/cell access sees the parent frame
3. The child registers the parent as a dependent
4. When a dependency clears or a Cell changes, all dependents clear recursively

## Invalidation Semantics

- `Cell.set()` → if value changed → clear all dependent slots
- `Slot.clear()` → remove cached value → cascade clear to all dependents
- Cleared slots recompute on next `get()` access

## Design Goals

- **Lazy evaluation:** Values computed only when first accessed
- **Fine-grained reactivity:** Only affected dependents recompute
- **Zero-cost abstractions:** Compile-time type resolution where possible
- **Thread safety:** Optional via feature flag (default: single-threaded)
- **No runtime closures limitation:** Unlike Zig, Rust has full closure support

## Differences from lazily-zig

| Aspect | lazily-zig | lazily-rs |
|--------|-----------|-----------|
| Context | Explicit allocator | Owned allocations |
| Slot creation | `comptime` function pointers | Closures (`Box<dyn Fn>`) |
| Storage modes | `.direct` / `.indirect` | Unified via generics |
| FFI | Built-in `StringView` | Via `#[no_mangle]` + `extern "C"` |
| Thread safety | Mutex by default | Feature flag |

## Differences from lazily-py

| Aspect | lazily-py | lazily-rs |
|--------|----------|-----------|
| Context | Plain `dict` | Typed `Context` struct |
| Slot keys | Object identity | `TypeId` |
| Cell equality | `!=` operator | `PartialEq` trait |
| Context resolvers | `resolve_ctx` functions | Trait-based |
| Dependencies | Zero | Zero (pure Rust) |
