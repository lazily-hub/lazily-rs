# lazily-rs

Lazy reactive primitives library for Rust.

## Architecture

- `src/context.rs` — `Context` struct, dependency graph, thread-local tracking stack
- `src/slot.rs` — `SlotHandle<T>` (lightweight `Copy` id into Context)
- `src/cell.rs` — `CellHandle<T>` (lightweight `Copy` id into Context)
- `tests/integration.rs` — 13 integration tests
- `tests/spec_compliance.rs` — 36 spec compliance tests

## Key Design Decisions

- **Lazy, not eager:** Slots clear on invalidation but only recompute on access
- **PartialEq guard:** `Cell.set()` only invalidates when value actually changes
- **Dynamic dependencies:** Edges re-discovered on each recomputation (no stale subscriptions)
- **RefCell interior mutability:** Single-threaded by design

## Commands

```bash
cargo test          # Run all tests (49 total)
cargo clippy        # Lint
cargo build         # Build
```

## Related Projects

- `lazily-zig` — Zig counterpart with FFI, thread-safe mutex
- `lazily-py` — Python counterpart with context-as-dict model
