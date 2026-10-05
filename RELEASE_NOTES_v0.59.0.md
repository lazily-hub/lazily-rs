# lazily-rs v0.59.0 — durable owner, shared wire model, strict decoding

This release adds a durable-owner tier, moves the receipt and delta wire types
onto code generated from the shared `lazily-spec` wire model, and makes wire
decoding strict. It also relicenses the crate under Apache-2.0.

## Behaviour changes

- **Strict decoding refuses unknown keys.** The generated receipt types
  (`CausalReceipt`, `CausalReceipts`) and the generated delta types (`Delta` and
  every `DeltaOp` body) now use `deny_unknown_fields`. Payloads that carry
  extra keys were silently ignored before and are now decode errors. On
  receipts, `reason` and `payload_hash` are required-nullable: a missing key is
  an error rather than defaulting to `None`, as `schemas/receipts.json`
  requires. Encoding is byte-identical to v0.58.0 in json, json-intern, msgpack
  and postcard.
- **`ShmBlobRef` with the default backend is now written in Postcard.**
  Previously `backend: shm` was skipped in every codec, so a Postcard
  descriptor with the default backend was one field short and could not be
  decoded back (`DeserializeUnexpectedEnd`). The default is now omitted only by
  human-readable serializers, matching `DeltaOp::NodeAdd`'s key rule. JSON and
  msgpack bytes are unchanged. Postcard could never decode the old form, so no
  stored frame or peer can depend on it.
- **License is now Apache-2.0** (was MIT) for `lazily`; `LICENSE` and `NOTICE`
  files ship with the crate.

## New

- **Queue ops on the wire.** `DeltaOp::QueuePush`, `QueuePop` and `QueueClose`
  are ordinary delta ops, as `protocol.md` specifies; a schema-valid `Delta`
  carrying one was previously rejected. They are appended after `EdgeRemove`
  (postcard tags 7/8/9; existing tags unchanged). Read filtering is node-scoped,
  `QueuePush` payloads spill like `CellSet`, and the bridge hub refuses queue
  ops inbound (no queue adapter yet).
- **Generated wire types.** `ReceiptOutcome`, `CausalReceipt`,
  `CausalReceipts`, `ReceiptMessage`, `DeltaOp`, `Delta`, `IpcValue` and
  `NodeState` are generated into `src/generated/` from `lazily-spec`'s
  `schemas/receipts.json` and `schemas/delta.json`. `NodeId`, `NodeKey` and
  `ShmBlobRef` remain hand-written.
- **Durable owner tier.**
  - Durable owner core and durable reconciliation (always available).
  - `durable-postgres`: reference Postgres durable-owner host.
  - `durable-jetstream`: pull-based NATS JetStream transport over the Postgres
    authority (broker delivery never authorizes state).
  - `durable-client`: lightweight durable client with NATS publish/observe
    semantics and no owner authority.
- **Consumer simulation testkit** (`sim_consumer`): runs one generated action
  history through an in-memory reducer and selected real adapters, comparing
  canonical observations after every action.

## Dependencies and build

- The protobuf codegen toolchain (`prost-build`, `protoc-bin-vendored`) is now
  an optional build-dependency behind the `protobuf` feature. The default
  dependency graph shrinks from 45 crates to 9; `--features protobuf` is
  unchanged.
- Dependency license admission is enforced in CI against
  `docs/dependency-license-inventory.json`.

## Conformance

- `receipts/causal_receipts.json` is now replayed (157/162 fixtures opened);
  the coverage table marks Causal receipts as shipped.
