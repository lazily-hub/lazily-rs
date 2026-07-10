# lazily v0.25.0

Minor release over v0.24.0. Adds the cross-process zero-copy transport
(`#lzzcpy`) — pluggable blob backends (`BlobBackend` / shm / arrow) that let
large IPC payloads cross the process boundary as descriptors, not copies.

## Highlights

**New: zero-copy blob-backend transport.** A `Snapshot` / `Delta` / `CrdtSync`
message carrying a large payload (Arrow record-batch, image, serialized
sub-document) no longer copies those bytes through the wire codec. The producer
**spills** the payload to a blob backend and ships a small `ShmBlobRef`
descriptor; the receiver **resolves** the descriptor against the same backend
and reads the bytes in place — no copy, no checksum recompute.

The `BlobBackend` trait (`src/transport.rs`) is the adapter seam. Three backends
ship:

- `InProcessBackend` — wraps `ShmBlobArena` for the in-process / FFI-host case
  (single address space, e.g. an editor plugin).
- `ArrowBackend` — holds Apache Arrow IPC stream bytes. The descriptor's bytes
  *are* an Arrow IPC stream a columnar consumer imports as an `Array` /
  `RecordBatch` zero-copy (bring your own `arrow` crate around the resolved
  `&[u8]`).
- `ShmBackend` (POSIX `shm_open` + `mmap`, behind the `shm` feature, Linux) —
  the cross-process backend with a lock-free atomic bump allocator. Validated
  by a `fork()` cross-process smoke test.

`spill_message(&mut msg, &mut backend, threshold)` replaces large `Inline` /
`Payload` sites with `SharedBlob` descriptors across all `IpcMessage` variants.
`BlobRouter` is the receiver-side multi-backend resolver — it routes by the
descriptor's `backend` discriminator so a `shm` descriptor resolves against the
shm backend and an `arrow` descriptor against the arrow backend.

Implements `lazily-spec/docs/zero-copy-transport.md`. Backed by the Lean formal
model `lazily-formal/LazilyFormal/ZeroCopyTransport.lean` — proves
spill-then-resolve identity, backend isolation, ABA generation safety, and
checksum integrity for **any** backend satisfying the contract.

## Wire discriminator

The `ShmBlobRef` descriptor gained an optional `backend` field
(`BlobBackendKind::Shm` | `Arrow` | `InProcess`). It defaults to `Shm` and is
omitted on the wire (self-describing codecs) when `Shm`, so every legacy
descriptor validates unchanged — the transport is a strict superset of the
pre-existing shared-memory blob path. The conformance fixture
`delta_zero_copy_arrow.json` pins the `backend: "arrow"` form.

## Fixes

- Updated two stale `spec_compliance` instrumentation tests that asserted the
  removed per-node sidecar-frontier invalidation path (v0.24.0,
  `#lzstateinvalidation`). All invalidation now goes through the single
  state-locked DFS; the vestigial `sidecar_invalidation_frontiers` /
  `sidecar_dirty_marks` / `sidecar_invalidation_fallbacks` counters are now
  asserted as zero, and `SetCellInvalidation` lock acquisitions are asserted as
  the expected positive counts.
