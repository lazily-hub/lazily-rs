# Contributing to lazily-rs

**Minimum supported Rust version (MSRV): 1.88** — declared via `rust-version`
in `Cargo.toml`. The crate uses `let_chains` (stabilized in 1.88) pervasively.

## Verify a change

Run the local CI-equivalent suite with:

```bash
make check
```

The Makefile also exposes focused targets such as `make test-tokio`,
`make test-loom`, `make benchmark-evidence`, `make benchmark-check`, and
`make benchmark-update`.

`make check` measures the benchmark budgets rather than assuming them: it runs
`benchmark-evidence` (a reduced-sample Criterion pass over the budgeted groups,
plus the deterministic instrumentation profile) and then `benchmark-check`,
which fails if that evidence is absent or if it measured a different source tree
than the one checked out. There is no "budgets skipped" path that still exits 0.

## Core qualification

[`scripts/core-conformance.json`](scripts/core-conformance.json) declares the
machine-readable Core profile entry point. Its shell-free `report_command`
replays the 16 required reactive-graph fixtures through `Context`,
`ThreadSafeContext`, and `AsyncContext`, then writes the requested report and
fresh assertion-consumption and negative-control evidence beneath the verifier's
report root.

The report is intentionally partial for async `failed_compute` and
`feedback_drain_bound`; the single-threaded and thread-safe flavors still run
those fixtures. The fixtureless C ABI proof builds the crate's `cdylib`, loads
it as a native shared library, and invokes the exported channel constructor,
length, and destructor symbols.
