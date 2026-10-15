# lazily-rs v0.60.0 — Lazily Core facade and qualification evidence

This release adds `lazily::core`, the facade over the candidate Lazily Core
`core-v1` profile, and the evidence that qualifies the crate against it. No
existing API changes; everything here is additive.

## Added

- **`lazily::core`** (`src/core.rs`) re-exports the Core profile's symbols.
  `api/core-v1.tsv` is the committed snapshot, and `tests/core_api_compat.rs`
  derives the expected surface from the spec profile so a drift between the
  crate and `lazily-spec/profiles/core-v1.json` fails the build.
- **Core qualification evidence.** `scripts/core-conformance.json` declares the
  binding, `scripts/generate-core-report.py` replays the Core fixtures, and
  `scripts/prove-core-c-abi.py` proves the C-ABI boundary.
- **Packaged artifact consumer.** `make core-artifact-consumer-check` builds a
  throwaway crate against the packaged `lazily` and runs it.

## CI

Every Core gate is a named, reach-pinned, unconditional CI step, and the
activation guard refuses a step-level `if:` or `continue-on-error:` on any of
them unless that exact value is pinned.
