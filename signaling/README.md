# lazily-signaling (#yxjw)

Cloudflare Worker signaling server for **lazily-distributed** peer discovery.

It brokers peer discovery and signaling for the lazily-distributed CRDT cell
plane: peers join a session, learn the current roster, exchange the WebRTC
SDP/ICE needed to open direct P2P data channels, and relay opaque payloads (for
example CRDT deltas) when a direct channel is unavailable. It is a *discovery +
relay* layer — it never interprets CRDT state.

## Architecture

- **One Durable Object per session id.** `GET /session/:id` (WebSocket upgrade)
  routes to a `SignalingRoom` Durable Object keyed by `idFromName(sessionId)`.
  Cloudflare guarantees a single global DO instance per id, giving each session
  a lock-free coordination point. "Internet-scale" comes from sharding sessions
  across many DO instances, not one large server.
- **Thin DO, pure core.** `SignalingRoom` (`src/room.ts`) only wires WebSockets
  to `RoomCore` (`src/room-core.ts`), which owns the roster and routing and is
  fully unit-tested without any runtime plumbing.
- **Anti-spoofing.** The `from` on every forwarded frame is the sender
  connection's registered peer id, never a client-supplied value.
- **Permission boundary (#39c5 mirror).** `SignalingPermissions`
  (`src/permissions.ts`) gates which signaling ops each peer may perform. The
  `open` mode allows any joined peer to signal any other; `allowlist` mode is
  default-deny, mirroring Rust `lazily::distributed::PeerPermissions`.

## Wire protocol

`PeerId` matches Rust `PeerId(u64)` (serialized by `serde` as a bare number;
keep ids ≤ `Number.MAX_SAFE_INTEGER`). All frames are JSON, tagged by `type`.

| Client → Server | Server → Client |
| --- | --- |
| `join { peer, capabilities? }` | `welcome { peer, peers }` |
| `offer { to, sdp }` | `peer-joined { peer }` / `peer-left { peer }` |
| `answer { to, sdp }` | `offer`/`answer`/`ice`/`relay` (with `from`) |
| `ice { to, candidate }` | `error { code, message }` |
| `relay { to, payload }` | |
| `leave` | |

## Develop

```bash
npm install
npm run typecheck    # tsc --noEmit
npm test             # vitest (workerd runtime)
npm run check        # typecheck + test
npm run dev          # wrangler dev (local)
npm run deploy       # wrangler deploy
```

Set `SIGNALING_MODE = "allowlist"` in `wrangler.toml` (or as a binding var) to
require explicit per-peer grants (default-deny).

## Tests

- `test/protocol.test.ts` — frame parsing/validation/codec.
- `test/permissions.test.ts` — open vs allowlist (default-deny) gating.
- `test/room-core.test.ts` — join/welcome/roster/relay/leave/errors and
  permission gating, against the same `RoomCore` the DO delegates to.
- `test/worker.test.ts` — end-to-end through the real Worker + Durable Object +
  WebSocket in the workerd runtime (`@cloudflare/vitest-pool-workers`).
