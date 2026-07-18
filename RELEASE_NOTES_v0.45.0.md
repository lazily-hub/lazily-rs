# lazily-rs v0.45.0 — clone-free thread-safe reads + pooled async trackers

Two `#lzperfaudit` substrate items, plus a build fix for the v0.44.0 release.

## New API

- **`ThreadSafeContext::get_arc(&slot) -> Arc<T>`** (`#lzrsgetarc`) — the
  `Send + Sync` analog of `Context::get_rc`. `get` deep-clones the value on
  every read; `get_arc` hands out the `Arc` the node already stores, so the read
  costs a refcount bump instead of an allocation plus a copy. Requires only
  `T: Send + Sync + 'static` — no `Clone` bound.

  It is **not** a universal replacement for `get`. `get_arc` trades a `T::clone`
  for an atomic refcount bump, so it only pays off once cloning `T` costs more
  than the bump (`typed_cache_reads`, instrumented build):

  | `typed_cache_reads` case | `get` | `get_arc` |
  |---|---|---|
  | `usize` slot | 55.1 ns | 63.2 ns (**+15%** — don't) |
  | 384-byte `String` slot | 72.6 ns | 63.1 ns (**−13%**) |

  The `get_arc` cost is flat in the size of `T` while `get` grows with it, so
  the margin widens for larger values. **Prefer `get` for scalars, `get_arc` for
  values with heap buffers.**

  Like `get`, it reads through the cached-read sidecar (which already stores the
  value behind an `Arc`) rather than taking the state lock; only inline-storage
  slots — small `Copy` values, where you want `get` regardless — fall back to
  the locked node read.

## Performance

- **Pooled async dependency trackers** (`#lzrsdeppool`) — every async slot and
  effect spawn minted a fresh `Arc<Mutex<HashSet<SlotId>>>` and cloned the
  tracked set back out on completion. Trackers now come from a free-list on the
  context (cap 32), reusing the `Arc` *and* the set's table capacity, and
  extraction moved from `.clone()` to `mem::take`.

  Recycling is gated on `Arc::get_mut`, so a run that stashed its
  `AsyncComputeContext` somewhere outliving itself keeps its tracker out of the
  pool — a later spawn can never share a tracker with a live writer.

  This is an allocation-side change only. `async_invalidation_throughput` shows
  no significant movement: the async path is dominated by tokio scheduling
  (~95 µs for a batch that costs the sync context ~6.4 µs), which is orders of
  magnitude above the allocations removed.

## Fixes

- **`--all-features` compiles again** — the v0.44.0 `#lzspecdeltacrdt` variant
  landed without updating the `IpcMessage` matches in `bridge.rs` and
  `webrtc_transport.rs` (E0004). CI had been red since. Both sites treat
  `DeltaSinceRequest` as the control frame it is: it carries no node content and
  grants its sender no write authority.

## Docs

- `BENCHMARKS.md` regenerated. It had drifted to `version 0.40.1` against a
  `0.44.0` package — v0.41 through v0.44 never refreshed it — which had been
  failing the `#lzbenchver` gate in `make check`.

## Not done

- **`#lzrsasyncsmallany`** (inline small-value storage for `AsyncContext`) was
  evaluated and **rejected for now**. The allocation it removes is ~20 ns on a
  write path measured at ~95 µs, so there is no perf case; on memory it is a
  wash-to-negative (small scalars save a heap box, but every large-valued node
  grows by the inline envelope). Revisit only with a workload that shows async
  allocation pressure actually mattering.
