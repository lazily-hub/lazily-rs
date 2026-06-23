# lazily v0.12.2

Patch release over v0.12.1. Last published: crates.io `0.12.1`.
Tag `v0.12.2` points at this release commit on `main`.

## Highlights

Supersedes v0.12.1 with the same state-machine API plus the benchmark metadata
version update required by the default spec-compliance test suite.

## Fixed

- **Benchmark report version metadata.** `BENCHMARKS.md` now identifies package
  version `0.12.2`, matching `Cargo.toml`, so
  `benchmark_report_harness::readme_benchmark_results_track_package_version`
  passes under `cargo test --locked`.

## Verification

- `cargo test --locked`
- `cargo publish --dry-run`

## Publish checklist

1. `cargo publish` (dry-run verified clean from a clean worktree).
2. `gh release create v0.12.2 --notes-file RELEASE_NOTES_v0.12.2.md --title "lazily v0.12.2"`.
