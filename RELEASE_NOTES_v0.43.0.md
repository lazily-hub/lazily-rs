# lazily-rs v0.43.0 — revision (pull) invalidation engine (`#lzspecrevisionengine`)

Implements the revision engine — an alternative invalidation strategy that
gives O(1) writes (no dependent cone walk) at the cost of O(changed-subpath)
reads with value early-cutoff. Observable values are provably identical to the
default push engine, formally pinned by `get_equiv_push` (lazily-formal
`RevisionEngine.lean`).

## New API

- **`Context::with_revision_engine()`** — creates a Context using the revision
  (pull) engine instead of the default push (dirty-walk) engine. Per-Context
  choice; never mixed within one graph.
- In revision mode, a cell write bumps a global revision counter (O(1)) instead
  of walking the dependent cone marking dirty (O(cone)). Slot staleness is
  detected lazily on read via `verified_at < revision`.
- Effects are notified via a revision-mode flush scan (O(effects) per flush, not
  O(cone) per write).
- The memo-equality guard and `PartialEq` write guard behave identically under
  both engines.

## Formal pin (`get_equiv_push`)

`LazilyFormal.RevisionEngine.get_equiv_push` — proves that for any cell write,
the value observed by revision-`get` equals the value observed by push-`get`.
Extends the glitch-free / memo-equal lemmas already in the reactive graph model.

## Verification

- 7 dedicated revision-engine tests (basic cell/slot, diamond, memo guard, deep
  chain, batch, push-parity, high-fanout) — all green.
- Full existing test suite (197 lib + 13 integration + 50 IPC + 6 schema) passes
  unchanged — the revision engine is a drop-in substitution.
- Crossover benchmarks show revision is 1.08–1.5× faster on write-heavy
  high-fan-out workloads.
