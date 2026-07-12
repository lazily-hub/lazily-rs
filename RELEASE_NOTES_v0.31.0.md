# lazily-rs v0.31.0 — reliable sync (`#lzsync`)

Reference implementation of the reliable-sync protocol
(`lazily-spec` § Reliable Sync, backstop `lazily-formal` `ReliableSync.lean`).

## New (`src/reliable_sync.rs`, feature `ipc`)

- **`ResyncCoordinator`** — receiver-side decision function (`Apply` /
  `RequestSnapshot { from_epoch }` / `Ignore`), multi-epoch-span aware, with a
  single-request-per-gap `resyncing` state.
- **`DurableOutbox`** trait + **`InMemoryOutbox`** — at-least-once contract:
  append-before-send, `ack_through` retention, `replay_from(cursor)`. Combined
  with the coordinator's idempotent `Ignore` of already-applied deltas, delivery
  is at-least-once with exactly-once effect.
- **`OrSet`** (observed-remove, add-wins) + **`WireLwwRegister`** (WireStamp-keyed
  LWW) — the liveness cells that ride the CrdtSync plane.
- **Control frames as `IpcMessage` variants** — `ResyncRequest` / `OutboxAck`
  (FFI message kinds 4/5), the reverse direction of the same bidirectional plane
  as `Snapshot`/`Delta`/`CrdtSync`: one codec/framing/demux/FFI path, in-band
  ordering with the deltas. Round-trip through `json` + `msgpack`.
- **`Delta::span()`** + multi-epoch-span docs (`epoch >= base_epoch + 1`).

## Conformance

`tests/reliable_sync_conformance.rs` replays all five
`lazily-spec/conformance/reliable-sync/` fixtures (multi-epoch delta,
gap→resync→converge, idempotent re-delivery, outbox crash-replay, OR-set/LWW
liveness) + a reference file-backed outbox + the control-frame codec round-trip.
New `make test-reliable-sync-conformance` target.

## Verification

`cargo publish --dry-run` clean. fmt / clippy / build / tests / lean formal green.
(The pre-existing `benchmark-check` missing-p50/p95-rows baseline is unchanged and
unrelated to this release.)
