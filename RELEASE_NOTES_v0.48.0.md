# lazily-rs v0.48.0 — source API across every context

The Cell-kernel `source` / `get` / `set` vocabulary now covers the local,
thread-safe, and async execution models consistently.

## Breaking API alignment

- `ThreadSafeContext::source` is now the primary source constructor;
  `source_copy` is the inline-copy fast-path constructor. `cell` and `cell_copy`
  remain deprecated compatibility aliases.
- `AsyncContext::source` and `AsyncTeardownScope::source` replace their `cell`
  constructors. Async source reads and writes use the existing unified `get`
  and `set` methods.
- Async handles are now `AsyncSource<T>` and `AsyncComputed<T>`.
  `AsyncCellHandle<T>` and `AsyncSlotHandle<T>` remain deprecated type aliases
  for migration.
- `SyncReactiveGraph` and `AsyncReactiveGraph` construct with `source` and expose
  generic `get` / `set` operations through `Read` / `Write`. Async derived reads
  that drive computation are named `get_async`.
- All teardown scope flavors expose `source`, so scoped and unscoped
  construction use the same vocabulary.

## Migration

| Before | Now |
|---|---|
| `ctx.cell(value)` | `ctx.source(value)` |
| `thread_safe.cell_copy(value)` | `thread_safe.source_copy(value)` |
| `ctx.get_cell(&source)` | `ctx.get(&source)` |
| `ctx.set_cell(&source, value)` | `ctx.set(&source, value)` |
| `AsyncCellHandle<T>` | `AsyncSource<T>` |
| `AsyncSlotHandle<T>` | `AsyncComputed<T>` |
| `AsyncReactiveGraph::get(&computed)` | `AsyncReactiveGraph::get_async(&computed)` |

The normative `lazily-spec` async surface and the `lazily-formal` reactive and
thread-safe theorem names were updated in lockstep (`setSource`,
`recomputeComputed`).
