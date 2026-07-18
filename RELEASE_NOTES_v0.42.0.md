# lazily-rs v0.42.0 — Phase 3 wire-format optimizations (`#lzperfaudit`)

Implements the spec-ratified Phase 3 wire-format wins from the lazily-spec
`#lzperfaudit` audit: frontier suppression, base64 byte-array codec, and batch
string-intern table. These are additive, backward-compatible wire optimizations
that reduce wire size and encode/decode cost on the CRDT anti-entropy and IPC
paths.

## New features

### `#lzspecfrontiersuppress` — optional CrdtSync frontier

The stamp frontier advertisement is now optional. A frame shipping only ops
whose frontier hasn't changed omits the field entirely (backward-compatible
serde `skip_serializing_if`). New `CrdtSync::ops_only` constructor and
`is_frontier_suppressed` predicate. Measured: **−42% wire**, **−37% encode**,
**−35% decode** on an 8-peer frame.

### `#lzspecbase64` — `json-base64` capability codec

New `json-base64` cargo feature. `IpcMessage::encode_json_base64` /
`decode_json_base64` encode `Inline`/`Payload` byte arrays as base64 strings
instead of JSON integer arrays — **42–67% wire reduction** and **~27% faster
decode** (depending on payload size), matching the spec's capability-gated
`json-base64` feature flag.

### `#lzspecintern` — batch string-intern table

New `IpcMessage::encode_json_intern` / `decode_json_intern` deduplicate repeated
`type_tag` strings within a Snapshot/Delta/CrdtSync batch into a sidecar
`intern.strings` table, replacing each tag with a small integer id. Wire savings
grow with the node-to-tag ratio (5% at 256 nodes / 4 tags; more at scale).

## Spec ratifications (lazily-spec)

This release accompanies spec ratification of all seven achievable Phase 3
items in lazily-spec (`#lzspecgcdefer`, `#lzspecobserverclarify`,
`#lzspecdemanddriven`, `#lzspecfrontiersuppress`, `#lzspecbase64`,
`#lzspecintern`, `#lzspecdeltacrdt`). The revision engine
(`#lzspecrevisionengine`) remains gated on the `get_equiv_push` Lean formal pin.

## Verification

fmt / clippy / build / tests green (197 lib + 50 IPC + 6 schema-compliance +
conformance + crdt-plane). New `benches/wire_optimizations.rs` benchmarks
confirm the wire and round-trip wins above.
