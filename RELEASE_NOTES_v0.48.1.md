# lazily-rs v0.48.1 — unified zero-copy reads

`Context::get_rc`, `Compute::get_rc`, and `ComputeOps::get_rc` now accept both
`Source<T>` and `Computed<T>`. This closes the remaining source-first migration
gap: callers no longer need the deprecated `get_cell_rc` API for zero-copy
source reads.
