import { SELF } from "cloudflare:test";
import { afterEach, describe, expect, it } from "vitest";
import type { ClientMessage, ServerMessage } from "../src/protocol.js";

/** Sockets opened during a test, closed in afterEach for clean DO teardown. */
const openClients: Client[] = [];

afterEach(async () => {
  for (const client of openClients.splice(0)) client.close();
  // Let the Durable Object process the server-side close events before the
  // pool pops the per-test isolated-storage frame.
  await scheduler.wait(50);
});

/** Buffers incoming server frames and lets a test await them in order. */
class Client {
  private readonly queue: ServerMessage[] = [];
  private readonly waiters: ((m: ServerMessage) => void)[] = [];

  constructor(private readonly ws: WebSocket) {
    ws.addEventListener("message", (event: MessageEvent) => {
      const data =
        typeof event.data === "string"
          ? event.data
          : new TextDecoder().decode(event.data as ArrayBuffer);
      const message = JSON.parse(data) as ServerMessage;
      const waiter = this.waiters.shift();
      if (waiter !== undefined) waiter(message);
      else this.queue.push(message);
    });
  }

  send(message: ClientMessage): void {
    this.ws.send(JSON.stringify(message));
  }

  next(): Promise<ServerMessage> {
    const buffered = this.queue.shift();
    if (buffered !== undefined) return Promise.resolve(buffered);
    return new Promise((resolve) => this.waiters.push(resolve));
  }

  close(): void {
    this.ws.close();
  }
}

async function connect(session: string): Promise<Client> {
  const response = await SELF.fetch(`https://signaling.test/session/${session}`, {
    headers: { Upgrade: "websocket" },
  });
  const ws = response.webSocket;
  if (ws === null) throw new Error("expected a WebSocket on the response");
  ws.accept();
  const client = new Client(ws);
  openClients.push(client);
  return client;
}

describe("signaling Worker", () => {
  it("serves a health probe", async () => {
    const response = await SELF.fetch("https://signaling.test/health");
    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");
  });

  it("rejects unknown paths and non-WebSocket session requests", async () => {
    expect((await SELF.fetch("https://signaling.test/nope")).status).toBe(404);
    expect(
      (await SELF.fetch("https://signaling.test/session/abc")).status,
    ).toBe(426);
  });

  it("brokers join, roster, and relay between two peers", async () => {
    const a = await connect("room-1");
    a.send({ type: "join", peer: 1 });
    expect(await a.next()).toEqual({ type: "welcome", peer: 1, peers: [] });

    const b = await connect("room-1");
    b.send({ type: "join", peer: 2 });
    expect(await b.next()).toEqual({ type: "welcome", peer: 2, peers: [1] });
    // a learns about b
    expect(await a.next()).toEqual({ type: "peer-joined", peer: 2 });

    // a relays an opaque CRDT payload to b
    a.send({ type: "relay", to: 2, payload: { delta: 7 } });
    expect(await b.next()).toEqual({
      type: "relay",
      from: 1,
      payload: { delta: 7 },
    });

    // disconnect propagates peer-left
    b.close();
    expect(await a.next()).toEqual({ type: "peer-left", peer: 2 });
  });

  it("isolates distinct session ids", async () => {
    const a = await connect("alpha");
    a.send({ type: "join", peer: 10 });
    expect(await a.next()).toEqual({ type: "welcome", peer: 10, peers: [] });

    const b = await connect("beta");
    b.send({ type: "join", peer: 20 });
    // beta is a different room: roster does not include peer 10
    expect(await b.next()).toEqual({ type: "welcome", peer: 20, peers: [] });
  });
});
