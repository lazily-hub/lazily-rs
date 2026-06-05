import { describe, expect, it } from "vitest";
import { SignalingClient, type SocketLike } from "../src/client.js";
import type { ServerMessage } from "../src/protocol.js";

type Listener = (event: { data: unknown }) => void;

/** In-memory socket for testing the client without a network. */
class FakeSocket implements SocketLike {
  readonly sent: string[] = [];
  closed = false;
  readyState = 1; // OPEN
  private readonly listeners = new Map<string, Set<(...a: never[]) => void>>();

  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.closed = true;
  }
  addEventListener(type: string, listener: (...a: never[]) => void): void {
    let set = this.listeners.get(type);
    if (set === undefined) this.listeners.set(type, (set = new Set()));
    set.add(listener);
  }
  removeEventListener(type: string, listener: (...a: never[]) => void): void {
    this.listeners.get(type)?.delete(listener);
  }
  /** Test helper: fire a non-message lifecycle event (open/close/error). */
  emit(type: "open" | "close" | "error"): void {
    this.listeners.get(type)?.forEach((l) => (l as () => void)());
  }
  /** Test helper: deliver a server message frame. */
  push(message: ServerMessage): void {
    const event = { data: JSON.stringify(message) };
    this.listeners.get("message")?.forEach((l) => (l as Listener)(event));
  }
  sentJson(): unknown[] {
    return this.sent.map((s) => JSON.parse(s));
  }
}

describe("SignalingClient", () => {
  it("attach sends a join frame matching the wire protocol", () => {
    const socket = new FakeSocket();
    SignalingClient.attach(socket, 1);
    expect(socket.sentJson()).toEqual([{ type: "join", peer: 1 }]);
  });

  it("attach includes capabilities when provided", () => {
    const socket = new FakeSocket();
    SignalingClient.attach(socket, 1, { capabilities: ["crdt"] });
    expect(socket.sentJson()).toEqual([
      { type: "join", peer: 1, capabilities: ["crdt"] },
    ]);
  });

  it("connect resolves and joins when the socket is already open", async () => {
    const socket = new FakeSocket();
    const client = await SignalingClient.connect("wss://x", "room", 2, {
      createWebSocket: () => socket,
    });
    expect(client.peer).toBe(2);
    expect(socket.sentJson()).toEqual([{ type: "join", peer: 2 }]);
  });

  it("connect builds the /session/:id URL and waits for open", async () => {
    const socket = new FakeSocket();
    socket.readyState = 0; // CONNECTING
    let seenUrl = "";
    const pending = SignalingClient.connect("wss://host/", "abc", 3, {
      createWebSocket: (u) => {
        seenUrl = u;
        return socket;
      },
    });
    socket.readyState = 1;
    socket.emit("open");
    const client = await pending;
    expect(seenUrl).toBe("wss://host/session/abc");
    expect(client.peer).toBe(3);
    expect(socket.sentJson()).toEqual([{ type: "join", peer: 3 }]);
  });

  it("send helpers produce correct frames", () => {
    const socket = new FakeSocket();
    const client = SignalingClient.attach(socket, 1);
    client.offer(2, "SDP");
    client.answer(2, "ANS");
    client.ice(2, "CAND");
    client.relay(2, { delta: 7 });
    client.leave();
    expect(socket.sentJson().slice(1)).toEqual([
      { type: "offer", to: 2, sdp: "SDP" },
      { type: "answer", to: 2, sdp: "ANS" },
      { type: "ice", to: 2, candidate: "CAND" },
      { type: "relay", to: 2, payload: { delta: 7 } },
      { type: "leave" },
    ]);
    expect(socket.closed).toBe(true);
  });

  it("onMessage decodes server frames and unsubscribes", () => {
    const socket = new FakeSocket();
    const client = SignalingClient.attach(socket, 1);
    const got: ServerMessage[] = [];
    const off = client.onMessage((m) => got.push(m));

    socket.push({ type: "welcome", peer: 1, peers: [2] });
    socket.push({ type: "relay", from: 2, payload: { x: 1 } });
    off();
    socket.push({ type: "peer-left", peer: 2 });

    expect(got).toEqual([
      { type: "welcome", peer: 1, peers: [2] },
      { type: "relay", from: 2, payload: { x: 1 } },
    ]);
  });
});
