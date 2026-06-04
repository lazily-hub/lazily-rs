/**
 * Per-peer signaling permission boundary (#yxjw, mirroring #39c5).
 *
 * The Rust `lazily::distributed::PeerPermissions` gates which `RemoteOp`s a peer
 * may perform against a shared graph. The signaling server applies the same
 * idea one layer out: which *signaling* ops a peer may perform against a
 * session — may it join at all, and which other peers may it send
 * offer/answer/ice/relay frames to. The local data plane still re-checks
 * `RemoteOp` on the Rust side; this is the discovery-layer gate.
 *
 * Two admission modes:
 * - `"open"` — any peer may join and signal any other joined peer (the default
 *   for trusted/LAN sessions and the common discovery case).
 * - `"allowlist"` — **default-deny**: a peer may only join when explicitly
 *   granted, and may only signal target peers explicitly allowed for it.
 */

import type { DirectedKind, PeerId } from "./protocol.js";

export type SignalingMode = "open" | "allowlist";

/** A signaling op subject to permission gating. */
export type SignalOp =
  | { kind: "join" }
  | { kind: DirectedKind; to: PeerId };

interface PeerGrants {
  /** May this peer join the session. */
  join: boolean;
  /** Target peers this peer may send directed frames to. */
  targets: Set<PeerId>;
}

/**
 * Per-peer signaling allowlist. In `"allowlist"` mode this is default-deny and
 * mirrors `PeerPermissions`; in `"open"` mode every check passes.
 */
export class SignalingPermissions {
  private readonly mode: SignalingMode;
  private readonly peers = new Map<PeerId, PeerGrants>();

  constructor(mode: SignalingMode = "open") {
    this.mode = mode;
  }

  private grants(peer: PeerId): PeerGrants {
    let g = this.peers.get(peer);
    if (g === undefined) {
      g = { join: false, targets: new Set() };
      this.peers.set(peer, g);
    }
    return g;
  }

  /** Grant `peer` permission to join the session. */
  allowJoin(peer: PeerId): void {
    this.grants(peer).join = true;
  }

  /** Grant `peer` permission to send directed frames to `target`. */
  allowSignal(peer: PeerId, target: PeerId): void {
    this.grants(peer).targets.add(target);
  }

  /** Grant `peer` join plus directed-signal access to every peer in `targets`. */
  allowMany(peer: PeerId, targets: Iterable<PeerId>): void {
    const g = this.grants(peer);
    g.join = true;
    for (const t of targets) g.targets.add(t);
  }

  /** Remove every grant for `peer` (e.g. on permanent eviction). */
  revokePeer(peer: PeerId): boolean {
    return this.peers.delete(peer);
  }

  /** Whether `peer` may perform `op`. Default-deny in `"allowlist"` mode. */
  isAllowed(peer: PeerId, op: SignalOp): boolean {
    if (this.mode === "open") return true;
    const g = this.peers.get(peer);
    if (g === undefined) return false;
    return op.kind === "join" ? g.join : g.targets.has(op.to);
  }

  /** Fail-closed check returning a `permission_denied`-style verdict. */
  check(peer: PeerId, op: SignalOp): { ok: true } | { ok: false } {
    return this.isAllowed(peer, op) ? { ok: true } : { ok: false };
  }
}
