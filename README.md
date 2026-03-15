# lazily

Lazy reactive signals for Rust — dependency tracking and cache invalidation.

[![crates.io](https://img.shields.io/crates/v/lazily.svg)](https://crates.io/crates/lazily)

## Overview

`lazily` provides two core primitives for lazy reactive computation:

- **Slot** — a lazily-computed cached value that automatically tracks dependencies
- **Cell** — a mutable value that invalidates dependent Slots when changed

Values are **lazy**: dependents are cleared on invalidation but only recomputed when accessed. This contrasts with eager "signal" systems that recompute immediately.

## Usage

```rust
use lazily::Context;

let ctx = Context::new();

// Create a mutable cell
let counter = ctx.cell(0i32);

// Create a derived slot (automatically tracks dependencies)
let doubled = ctx.slot(|ctx| {
    let val = ctx.get_cell(&counter);
    val * 2
});

assert_eq!(ctx.get(&doubled), 0);

// Mutate the cell — dependents are cleared (not recomputed yet)
ctx.set_cell(&counter, 5);

// Slot recomputes lazily on next access
assert_eq!(ctx.get(&doubled), 10);
```

## Core Concepts

### Slot

A `SlotHandle<T>` wraps a compute function `Fn(&Context) -> T`. The result is cached after first access. Dependencies are discovered automatically via a thread-local tracking stack — any Slot or Cell accessed during computation becomes a dependency.

When a dependency is invalidated, the Slot clears its cached value. It does **not** recompute until `ctx.get()` is called again.

### Cell

A `CellHandle<T>` holds a mutable value. `ctx.set_cell()` compares old and new values via `PartialEq` — if unchanged, no invalidation occurs. If changed, all dependent Slots are recursively cleared.

### Context

`Context` owns all Slots and Cells. It manages the dependency graph and provides the API surface for creating, reading, and mutating reactive values.

## Design

- Interior mutability via `RefCell` (single-threaded)
- Thread-local tracking stack for automatic dependency discovery
- Dynamic dependency graphs — edges re-discovered on each recomputation
- Zero external dependencies

## Related

- [lazily-zig](https://github.com/btakita/lazily-zig) — Zig implementation with FFI support
- [lazily-py](https://github.com/btakita/lazily-py) — Python implementation with context-as-dict

## License

MIT
