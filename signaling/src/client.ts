/**
 * Client for the lazily-distributed signaling endpoint (#s0fc).
 *
 * Lets any TypeScript/JavaScript project depend on the #yxjw signaling Worker
 * for peer discovery: connect to a session, learn the roster, and exchange the
 * WebRTC SDP/ICE handshake (or relay opaque payloads). It is the TS counterpart
 * of the Rust `lazily::SignalingClient`; both speak the identical wire protocol
 * defined in `protocol.ts` / `SPEC.md`.
 *
 * Works against any `WebSocket`-like socket (browser, Node ≥ 22 global
 * `WebSocket`, or an injected socket) so it is testable without a network.
 */

import {
  type ClientMessage,
  type PeerId,
  type ServerMessage,
  decodeServerFrame,
  encodeClientMessage,
} from "./protocol.js";

/** Minimal socket surface the client needs (a `WebSocket` subset). */
export interface SocketLike {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  addEventListener(type: "open" | "close" | "error", listener: () => void): void;
  removeEventListener(type: string, listener: (...args: never[]) => void): void;
  readyState?: number;
}

export type ServerMessageHandler = (message: ServerMessage) => void;

export interface SignalingClientOptions {
  /** Capabilities advertised to other peers in the `join`. */
  capabilities?: string[];
  /** Socket factory; defaults to the global `WebSocket`. */
  createWebSocket?: (url: string) => SocketLike;
}

const OPEN = 1;

/** A connected signaling-session client. */
export class SignalingClient {
  private constructor(
    private readonly socket: SocketLike,
    readonly peer: PeerId,
  ) {}

  /**
   * Open a WebSocket to `{baseUrl}/session/{session}` and join as `peer`.
   * Resolves once the socket is open and the `join` frame is sent.
   */
  static connect(
    baseUrl: string,
    session: string,
    peer: PeerId,
    options: SignalingClientOptions = {},
  ): Promise<SignalingClient> {
    const url = `${baseUrl.replace(/\/$/, "")}/session/${session}`;
    const factory =
      options.createWebSocket ??
      ((u: string) => new WebSocket(u) as unknown as SocketLike);
    const socket = factory(url);

    return new Promise((resolve, reject) => {
      const onError = () => reject(new Error(`signaling connection to ${url} failed`));
      const finish = () => {
        socket.removeEventListener("error", onError as never);
        resolve(SignalingClient.attach(socket, peer, options));
      };
      if (socket.readyState === OPEN) {
        finish();
        return;
      }
      socket.addEventListener("open", finish);
      socket.addEventListener("error", onError);
    });
  }

  /**
   * Wrap an already-open socket and send the `join`. Use when the socket is
   * created elsewhere (e.g. a server-accepted WebSocket in tests).
   */
  static attach(
    socket: SocketLike,
    peer: PeerId,
    options: SignalingClientOptions = {},
  ): SignalingClient {
    const client = new SignalingClient(socket, peer);
    client.send({
      type: "join",
      peer,
      ...(options.capabilities ? { capabilities: options.capabilities } : {}),
    });
    return client;
  }

  /** Register a handler for incoming server messages; returns an unsubscribe fn. */
  onMessage(handler: ServerMessageHandler): () => void {
    const listener = (event: { data: unknown }) => {
      const data =
        typeof event.data === "string"
          ? event.data
          : new TextDecoder().decode(event.data as ArrayBuffer);
      const message = decodeServerFrame(data);
      if (message !== null) handler(message);
    };
    this.socket.addEventListener("message", listener);
    return () => this.socket.removeEventListener("message", listener as never);
  }

  /** Send a raw protocol message. */
  send(message: ClientMessage): void {
    this.socket.send(encodeClientMessage(message));
  }

  offer(to: PeerId, sdp: string): void {
    this.send({ type: "offer", to, sdp });
  }
  answer(to: PeerId, sdp: string): void {
    this.send({ type: "answer", to, sdp });
  }
  ice(to: PeerId, candidate: string): void {
    this.send({ type: "ice", to, candidate });
  }
  relay(to: PeerId, payload: unknown): void {
    this.send({ type: "relay", to, payload });
  }

  /** Announce departure and close the socket. */
  leave(): void {
    this.send({ type: "leave" });
    this.socket.close();
  }

  /** Close the socket without announcing. */
  close(): void {
    this.socket.close();
  }
}
