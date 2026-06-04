/**
 * `SignalingRoom` Durable Object (#yxjw).
 *
 * One Durable Object instance per session id (see `index.ts` routing). Cloudflare
 * guarantees a single DO instance per id globally, which gives the session a
 * single-threaded coordination point for the roster without any external lock —
 * the "internet-scale" property comes from sharding sessions across many DO
 * instances, not from one giant server.
 *
 * This adapter is intentionally thin: it accepts WebSockets, wraps each in a
 * {@link PeerConnection}, and delegates all routing to {@link RoomCore}. The DO
 * stays resident while a session has active sockets, so `RoomCore`'s in-memory
 * roster is valid for the session's lifetime.
 */

import type { Env } from "./env.js";
import { RoomCore, type PeerConnection } from "./room-core.js";
import { SignalingPermissions } from "./permissions.js";
import {
  decodeClientFrame,
  encodeServerMessage,
  type ServerMessage,
} from "./protocol.js";

class WebSocketPeer implements PeerConnection {
  constructor(private readonly ws: WebSocket) {}
  send(message: ServerMessage): void {
    this.ws.send(encodeServerMessage(message));
  }
  close(code?: number, reason?: string): void {
    this.ws.close(code, reason);
  }
}

export class SignalingRoom {
  private readonly room: RoomCore;
  private readonly conns = new Map<WebSocket, WebSocketPeer>();

  constructor(_state: DurableObjectState, env: Env) {
    const mode = env.SIGNALING_MODE === "allowlist" ? "allowlist" : "open";
    this.room = new RoomCore(new SignalingPermissions(mode));
  }

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("expected a WebSocket upgrade", { status: 426 });
    }

    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];
    server.accept();

    const peer = new WebSocketPeer(server);
    this.conns.set(server, peer);

    server.addEventListener("message", (event: MessageEvent) => {
      const data =
        typeof event.data === "string"
          ? event.data
          : new TextDecoder().decode(event.data as ArrayBuffer);
      const message = decodeClientFrame(data);
      if (message === null) {
        peer.send({
          type: "error",
          code: "bad_message",
          message: "could not parse signaling frame",
        });
        return;
      }
      this.room.handleMessage(peer, message);
    });

    const drop = () => {
      const tracked = this.conns.get(server);
      if (tracked !== undefined) {
        this.room.handleClose(tracked);
        this.conns.delete(server);
      }
    };
    server.addEventListener("close", drop);
    server.addEventListener("error", drop);

    return new Response(null, { status: 101, webSocket: client });
  }
}
