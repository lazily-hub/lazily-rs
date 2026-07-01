# lazily v0.15.0

Minor release over v0.14.0. Last published: crates.io `0.14.0`.
Tag `v0.15.0` points at this release commit on `main`.

## Highlights

Adds **capability negotiation** to the IPC layer — the compatibility handshake
every non-local `lazily-ipc` session exchanges before any `Snapshot` or `Delta`
flows (protocol.md § Capability Negotiation). No default-feature surface
changed.

## Added

- **`CapabilityHandshake` (under the `ipc` feature).** The frame exchanged at
  session open: `{ protocol_id, protocol_major_version, codec, max_frame_size,
  fragmentation_supported, ordered_reliable, peer_id, session_id, features }`.
  `is_compatible_with` implements the fail-closed contract: peers that disagree
  on `protocol_id`, `protocol_major_version`, `codec`, or `ordered_reliable`
  are rejected before any graph state is applied. Feature advertisement
  (`shared-blob`, `signaling-relay`, …) is caller-driven via `has_feature`.
- **`PROTOCOL_ID` / `PROTOCOL_MAJOR_VERSION` constants** — the canonical
  `"lazily-ipc"` / `1` values the handshake validates against.
- **Builder API.** `CapabilityHandshake::new(peer_id, session_id)` fills the
  protocol defaults (JSON codec, 1 MiB frame size, ordered-reliable);
  `.with_codec(…)` / `.with_max_frame_size(…)` / `.with_features(…)` /
  `.with_fragmentation(…)` configure the rest.

## Verification

- `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features
  -D warnings` clean.
- `cargo test --locked --features ipc --test ipc` (30/30) and `--test
  conformance` (10/10) pass.
- `cargo publish --dry-run` clean.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.15.0 --notes-file RELEASE_NOTES_v0.15.0.md --title "lazily v0.15.0"`.
