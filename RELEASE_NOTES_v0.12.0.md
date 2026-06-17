# lazily v0.12.0

Minor release over v0.11.1. Last published: crates.io `0.11.1`.
Tag `v0.12.0` points at this release commit on `main`.

## Highlights

Correctness fixes to the networked str0m `DataChannel` backend (`Str0mNet`,
behind the opt-in `webrtc-str0m` feature). Two bugs that silently lost data are
fixed; one of them adds a new public error variant (the single breaking surface
in this release). The core reactive primitives (`Context` / `ThreadSafeContext`
/ `AsyncContext`, slots, cells, effects, signals) are unchanged.

## Breaking (scoped to the `webrtc-str0m` feature)

- **New `Str0mNetError::Backpressure` variant (`#lzstr0mframe`).** The
  `Str0mNetError` enum is not `#[non_exhaustive]`, so adding a variant is a
  breaking change for downstream code with an exhaustive `match` on it. The
  variant is returned by `Str0mNetChannel::send_frame` once the driver's
  outbound queue reaches `MAX_PENDING_FRAMES` (1024), so callers apply flow
  control instead of growing memory without bound. Callers matching
  `Str0mNetError` exhaustively must add a `Backpressure` arm (yield and retry
  `send_frame`). Only the `webrtc-str0m` feature surface is affected; no other
  public API changed.

## Fixes

- **#lzstr0mframe** — silent frame loss under SCTP backpressure. The driver's
  flush loop used `if ch.write(...).is_err() { break; }`, which discarded the
  `bool` returned by `Channel::write`. str0m returns `Ok(false)` when the SCTP
  send buffer is full (backpressure); the old code treated that identically to
  `Err`, **silently dropping every frame that exceeded the SCTP send window** —
  violating the ordered/reliable DataChannel invariant `WebRtcSink` /
  `WebRtcSource` rely on. Post-fix the `Ok(false)` and `Err` paths both
  **re-queue the frame and yield**, letting the next `poll_output` / `recv_from`
  cycle drain the window (for `Ok(false)`) or detect a dead `Rtc` on the next
  `is_alive()` check (for `Err`). Combined with the new bounded queue +
  `Backpressure` variant, sustained bursts can no longer exhaust memory or lose
  frames. Regression test `burst_of_frames_arrives_in_order_under_backpressure`
  bursts 100 × 8 KiB frames through one channel and asserts all 100 arrive in
  order, retrying on `Backpressure` as needed.
- **#lzstr0mpolldrive** — surface driver I/O errors. The driver's UDP transmit
  path was `let _ = socket.send_to(...)`, discarding every error: `ENOBUFS`
  (send-buffer pressure), `ECONNREFUSED` (ICMP port-unreachable, peer down),
  `ENETUNREACH` / `EHOSTUNREACH` (route flap), `EBADF` (socket closed). The
  corresponding ICE/DTLS/SCTP packet was silently lost and the handshake / data
  path stalled without diagnostics. Post-fix, `WouldBlock` / `Interrupted`
  (retryable on a blocking socket) `continue` the drain loop — str0m re-emits the
  `Transmit` on a later `poll_output`; any other error breaks the driver
  (`'outer`), surfacing `Closed` so the caller re-signals. The read-timeout cap
  is also documented as a command-poll interval (`COMMAND_POLL_INTERVAL`, 15 ms),
  not a str0m timing parameter.

## Verification

- `cargo build --all-features`, `cargo clippy --all-features --all-targets -- -D warnings`,
  `cargo fmt --check` clean.
- `cargo test --features "signaling-client webrtc-str0m" --test str0m_net --test webrtc_signaling --test webrtc_transport`
  — 7/7 pass, including `burst_of_frames_arrives_in_order_under_backpressure`.
- Full default + feature test suites pass (107 spec/integration tests + loom +
  stress + tokio + async + ffi + ipc + websocket).

### Pre-existing, unrelated: `benchmark-check`

`make check` ends in `benchmark-check`, which is **red at this commit AND was red
at tag v0.11.1** (already shipped): stale `thread_safe` instrumentation
lock-acquisition budgets that pre-date this release's content. The four overshoots
are in `thread_safe` contention benchmarks (unrelated module); `src/str0m_net.rs`
is the only source touched by v0.12.0 and cannot affect those counts. Tracked
separately in `tasks/software/lazily-rs.md` as `#lzbenchbudget`.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.12.0 --notes-file RELEASE_NOTES_v0.12.0.md --title "lazily v0.12.0"`.
3. Rotate the crates.io token if expired before step 1.
