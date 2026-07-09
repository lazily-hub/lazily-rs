# lazily v0.22.2

Patch release over v0.22.1. No API changes. Removes the cache-hit read overhead
on single-threaded `Context` computed/memo slots (`#lzslotfastpath`).

## Highlights

**Perf: `Context::refresh_slot` clean-cache-hit fast path.** When a slot holds a
value and is neither dirty nor force-recompute, `refresh_slot` now early-returns
instead of running the full dependency-refresh walk. On the cache-hit path this
skips:

- the `enter_refresh` cycle-guard `borrow_mut` and its `RefreshGuard`-drop
  `borrow_mut`,
- the `slot.dependencies` `Vec` clone (a heap allocation),
- a per-dependency `is_slot_node` shared borrow,
- the `needs_recompute` shared borrow,
- the `clear_slot_dirty_flags` `borrow_mut`.

…collapsing ~5–6 `RefCell` borrows plus a heap clone down to a single shared
borrow that only checks `value.is_some() && !dirty && !force_recompute`.

### Controlled A/B evidence

Same-session `--save-baseline before_slot` A/B (criterion statistical
comparison):

| case | change | p-value |
|---|---|---|
| `typed_cache_reads/context_slot` | **−58.9%** | p=0.00 |
| `cached_reads/context` | **−51.6%** | p=0.00 |
| `typed_cache_reads/context_cell` | −2.1% | p=0.37 (not significant) |

`typed_cache_reads/context_slot` drops from **~11.8 ns to ~4.7 ns**, now within
~1.5 ns of `typed_cache_reads/context_cell` (~3.0 ns). The cell path is
correctly unchanged, confirming no collateral effect.

### Correctness

The fast-path predicate `value.is_some() && !dirty && !force_recompute` is
provably a clean cache hit: `mark_slot_dirty` is always called with
`force_recompute=true` from `invalidate_dependent_from_changed_value` (for both
cell- and slot-driven changes), so any upstream change since the last compute
sets `dirty=true` (and `force_recompute=true`), which bypasses the fast path and
runs the existing dependency-refresh + recompute walk. A brand-new (never
computed) slot has `value == None`, so it also bypasses the fast path.

### Correction to the prior framing

The v0.22.1 release notes and the preceding analysis attributed the slot-read
overhead to `Rc<dyn Any>` type-erasure downcast. That was wrong: `CellNode` also
stores `Rc<dyn Any>` (`context.rs:96`) and downcasts on every read, yet reads at
~3.7 ns. The real cost was `refresh_slot`'s redundant work on clean reads, which
this release removes.
