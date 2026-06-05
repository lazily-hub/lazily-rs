import { SELF } from "cloudflare:test";
import { afterEach, describe, expect, it } from "vitest";
import { SignalingClient, type SocketLike } from "../src/client.js";
import type { ServerMessage } from "../src/protocol.js";

/** Buffers a client's incoming messages so tests can await them in order. */
class Session {
  private readonly queue: ServerMessage[] = [];
  private readonly waiters: ((m: ServerMessage) => void)[] = [];

  constructor(readonly client: SignalingClient) {
    client.onMessage((m) => {
      const waiter = this.waiters.shift();
      if (waiter !== undefined) waiter(m);
      else this.queue.push(m);
    });
  }

  next(): Promise<ServerMessage> {
    const buffered = this.queue.shift();
    if (buffered !== undefined) return Promise.resolve(buffered);
    return new Promise((resolve) => this.waiters.push(resolve));
  }
}

const sessions: Session[] = [];

afterEach(() => {
  for (const s of sessions.splice(0)) s.client.close();
});

async function join(room: string, peer: number): Promise<Session> {
  const response = await SELF.fetch(`https://signaling.test/session/${room}`, {
    headers: { Upgrade: "websocket" },
  });
  const ws = response.webSocket;
  if (ws === null) throw new Error("expected a WebSocket on the response");
  ws.accept();
  const client = SignalingClient.attach(ws as unknown as SocketLike, peer);
  const session = new Session(client);
  sessions.push(session);
  return session;
}

describe("SignalingClient end-to-end against the Worker", () => {
  it("joins, discovers peers, and relays through the real Durable Object", async () => {
    const a = await join("client-room", 1);
    expect(await a.next()).toEqual({ type: "welcome", peer: 1, peers: [] });

    const b = await join("client-room", 2);
    expect(await b.next()).toEqual({ type: "welcome", peer: 2, peers: [1] });
    expect(await a.next()).toEqual({ type: "peer-joined", peer: 2 });

    a.client.relay(2, { delta: 9 });
    expect(await b.next()).toEqual({
      type: "relay",
      from: 1,
      payload: { delta: 9 },
    });

    a.client.offer(2, "SDP-OFFER");
    expect(await b.next()).toEqual({ type: "offer", from: 1, sdp: "SDP-OFFER" });
  });
});
