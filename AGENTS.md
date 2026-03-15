# lazily-rs

Lazy reactive primitives library for Rust.

## Key Design Decisions

- **Lazy, not eager:** Slots clear on invalidation but only recompute on access
- **PartialEq guard:** `Cell.set()` only invalidates when value actually changes
- **Dynamic dependencies:** Edges re-discovered on each recomputation (no stale subscriptions)
- **RefCell interior mutability:** Single-threaded by design

## Commands

```bash
cargo test          # Run all tests
cargo clippy        # Lint
cargo build         # Build
```

## Related Projects

- `lazily-zig` — Zig counterpart with FFI, thread-safe mutex
- `lazily-py` — Python counterpart with context-as-dict model
