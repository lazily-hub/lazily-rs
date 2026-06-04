/**
 * Transport-agnostic signaling room logic (#yxjw).
 *
 * `RoomCore` owns the roster and message routing for one session. It is
 * deliberately decoupled from WebSockets and Durable Objects: it operates on an
 * abstract {@link PeerConnection} sink so it can be unit-tested directly, and
 * the {@link SignalingRoom} Durable Object is a thin adapter that wires real
 * WebSockets to it.
 *
 * Anti-spoofing: the `from` field on every forwarded frame is taken from the
 * *connection's* registered peer id, never from the sender-supplied message, so
 * a peer cannot impersonate another by lying in the payload.
 */

import {
  type ClientMessage,
  type DirectedKind,
  type ErrorCode,
  type PeerId,
  type ServerMessage,
} from "./protocol.js";
import { SignalingPermissions, type SignalOp } from "./permissions.js";

/** A connection the room can push server messages to. */
export interface PeerConnection {
  send(message: ServerMessage): void;
  close(code?: number, reason?: string): void;
}

export class RoomCore {
  private readonly byPeer = new Map<PeerId, PeerConnection>();
  private readonly byConn = new Map<PeerConnection, PeerId>();
  private readonly permissions: SignalingPermissions;

  constructor(permissions?: SignalingPermissions) {
    this.permissions = permissions ?? new SignalingPermissions("open");
  }

  /** Current roster, ascending by peer id for deterministic output. */
  roster(): PeerId[] {
    return [...this.byPeer.keys()].sort((a, b) => a - b);
  }

  /** Number of joined peers. */
  size(): number {
    return this.byPeer.size;
  }

  /** Route any client message from `conn`. */
  handleMessage(conn: PeerConnection, msg: ClientMessage): void {
    switch (msg.type) {
      case "join":
        this.handleJoin(conn, msg.peer);
        return;
      case "leave":
        this.handleLeave(conn);
        return;
      case "offer":
        this.forward(conn, "offer", msg.to, (from) => ({
          type: "offer",
          from,
          sdp: msg.sdp,
        }));
        return;
      case "answer":
        this.forward(conn, "answer", msg.to, (from) => ({
          type: "answer",
          from,
          sdp: msg.sdp,
        }));
        return;
      case "ice":
        this.forward(conn, "ice", msg.to, (from) => ({
          type: "ice",
          from,
          candidate: msg.candidate,
        }));
        return;
      case "relay":
        this.forward(conn, "relay", msg.to, (from) => ({
          type: "relay",
          from,
          payload: msg.payload,
        }));
        return;
    }
  }

  /** Remove a connection that dropped (socket close/error). */
  handleClose(conn: PeerConnection): void {
    this.handleLeave(conn);
  }

  private handleJoin(conn: PeerConnection, peer: PeerId): void {
    if (this.byConn.has(conn)) {
      conn.send(makeError("already_joined", "connection already joined"));
      return;
    }
    if (!this.checkAllowed(conn, peer, { kind: "join" })) return;
    if (this.byPeer.has(peer)) {
      conn.send(makeError("duplicate_peer", `peer ${peer} already present`));
      return;
    }

    this.byPeer.set(peer, conn);
    this.byConn.set(conn, peer);

    // Tell the newcomer the current roster (excluding itself).
    const peers = this.roster().filter((p) => p !== peer);
    conn.send({ type: "welcome", peer, peers });
    // Announce to everyone else.
    this.broadcastExcept(peer, { type: "peer-joined", peer });
  }

  private handleLeave(conn: PeerConnection): void {
    const peer = this.byConn.get(conn);
    if (peer === undefined) return;
    this.byConn.delete(conn);
    this.byPeer.delete(peer);
    this.broadcastExcept(peer, { type: "peer-left", peer });
  }

  private forward(
    conn: PeerConnection,
    kind: DirectedKind,
    to: PeerId,
    build: (from: PeerId) => ServerMessage,
  ): void {
    const from = this.byConn.get(conn);
    if (from === undefined) {
      conn.send(makeError("not_joined", "join before signaling"));
      return;
    }
    const op: SignalOp = { kind, to };
    if (!this.checkAllowed(conn, from, op)) return;
    const target = this.byPeer.get(to);
    if (target === undefined) {
      conn.send(makeError("unknown_target", `peer ${to} is not in the session`));
      return;
    }
    target.send(build(from));
  }

  private checkAllowed(
    conn: PeerConnection,
    peer: PeerId,
    op: SignalOp,
  ): boolean {
    if (this.permissions.check(peer, op).ok) return true;
    conn.send(
      makeError("permission_denied", "signaling op not allowed for peer"),
    );
    return false;
  }

  private broadcastExcept(exclude: PeerId, message: ServerMessage): void {
    for (const [peer, conn] of this.byPeer) {
      if (peer !== exclude) conn.send(message);
    }
  }
}

function makeError(code: ErrorCode, message: string): ServerMessage {
  return { type: "error", code, message };
}
