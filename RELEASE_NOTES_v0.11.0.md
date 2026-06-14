# lazily v0.11.0

Pre-staged for the operator publish (#12b1). Last published: crates.io `0.10.3`.
Tag `v0.11.0` now points at HEAD (`5ce3eed`).

## Highlights — networked WebRTC transport

This minor lands a full WebRTC DataChannel transport stack on top of the
v0.10.x reactive core: a sans-IO str0m backend, a real-socket networked
backend, a WebSocket fallback, signaling, and the glue that drives a complete
handshake end to end.

## Features

- **#lzwebrtcwire** — wire `SignalingClient` to `Str0mNet`. New
  `webrtc_signaling` module (`offer_to_peer` / `answer_next_offer`) owns the full
  SDP offer/answer + trickled-ICE handshake over `SignalingClient`, pumping
  frames into `accept_answer` / `add_remote_candidate` until the data channel
  opens. Integration test brokers two real `SignalingClient` WebSocket peers
  through an in-process #yxjw-protocol loopback relay and proves a
  permission-filtered `Snapshot` crosses the negotiated channel.
- **#lzwebrtcnet** — networked str0m `DataChannel` backend (`Str0mNet`) over a
  real UDP socket with the str0m DTLS/SCTP/ICE driver.
- **#97xn** — multi-channel reactive bridge hub.
- **#akp3** — WebSocket `DataChannel` backend (in-process loopback over a real
  WS handshake).
- **#webrtcbackend** — concrete sans-IO str0m `DataChannel` backend.
- **#webrtc2 / #webrtc3** — WebRTC `DataChannel` IPC transport abstraction,
  loopback integration tests, and Criterion benchmarks.

## CI / tests

- **#lzspecconf** — IPC conformance run against the canonical lazily-spec
  fixtures.
- **#k03k / #lzasync** — deterministic async resolve-loop window coverage.

## Remaining (operator-gated)

- Live two-host / NAT validation of `Str0mNet` through the deployed #yxjw
  Cloudflare Worker (`#lzwebrtcnet-e2e`, part of #h6qb) — cannot be done in CI.

## Publish checklist (#12b1)

1. `cargo publish` (dry-run already verified clean: 61 files, 219 KiB compressed).
2. `gh release create v0.11.0 --notes-file RELEASE_NOTES_v0.11.0.md --title "lazily v0.11.0"`.
3. Rotate the crates.io token if expired before step 1.
