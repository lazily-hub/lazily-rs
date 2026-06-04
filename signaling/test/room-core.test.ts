import { describe, expect, it } from "vitest";
import { RoomCore, type PeerConnection } from "../src/room-core.js";
import { SignalingPermissions } from "../src/permissions.js";
import type { ServerMessage } from "../src/protocol.js";

class FakePeer implements PeerConnection {
  readonly sent: ServerMessage[] = [];
  closed = false;
  send(message: ServerMessage): void {
    this.sent.push(message);
  }
  close(): void {
    this.closed = true;
  }
  last(): ServerMessage | undefined {
    return this.sent[this.sent.length - 1];
  }
}

describe("RoomCore membership", () => {
  it("welcomes a joining peer with the existing roster and announces it", () => {
    const room = new RoomCore();
    const a = new FakePeer();
    const b = new FakePeer();

    room.handleMessage(a, { type: "join", peer: 1 });
    expect(a.last()).toEqual({ type: "welcome", peer: 1, peers: [] });

    room.handleMessage(b, { type: "join", peer: 2 });
    expect(b.last()).toEqual({ type: "welcome", peer: 2, peers: [1] });
    // a is told about b
    expect(a.last()).toEqual({ type: "peer-joined", peer: 2 });
    expect(room.roster()).toEqual([1, 2]);
  });

  it("rejects a duplicate peer id and a double join on one connection", () => {
    const room = new RoomCore();
    const a = new FakePeer();
    const b = new FakePeer();

    room.handleMessage(a, { type: "join", peer: 1 });
    room.handleMessage(a, { type: "join", peer: 9 });
    expect(a.last()).toMatchObject({ type: "error", code: "already_joined" });

    room.handleMessage(b, { type: "join", peer: 1 });
    expect(b.last()).toMatchObject({ type: "error", code: "duplicate_peer" });
    expect(room.size()).toBe(1);
  });

  it("removes a peer on leave and on disconnect, announcing peer-left", () => {
    const room = new RoomCore();
    const a = new FakePeer();
    const b = new FakePeer();
    room.handleMessage(a, { type: "join", peer: 1 });
    room.handleMessage(b, { type: "join", peer: 2 });

    room.handleMessage(b, { type: "leave" });
    expect(a.last()).toEqual({ type: "peer-left", peer: 2 });
    expect(room.roster()).toEqual([1]);

    room.handleClose(a);
    expect(room.size()).toBe(0);
  });
});

describe("RoomCore routing", () => {
  it("forwards offer/answer/ice/relay to the target stamped with the real sender", () => {
    const room = new RoomCore();
    const a = new FakePeer();
    const b = new FakePeer();
    room.handleMessage(a, { type: "join", peer: 1 });
    room.handleMessage(b, { type: "join", peer: 2 });

    room.handleMessage(a, { type: "offer", to: 2, sdp: "SDP" });
    expect(b.last()).toEqual({ type: "offer", from: 1, sdp: "SDP" });

    room.handleMessage(b, { type: "answer", to: 1, sdp: "ANS" });
    expect(a.last()).toEqual({ type: "answer", from: 2, sdp: "ANS" });

    room.handleMessage(a, { type: "ice", to: 2, candidate: "CAND" });
    expect(b.last()).toEqual({ type: "ice", from: 1, candidate: "CAND" });

    room.handleMessage(a, { type: "relay", to: 2, payload: { delta: [1, 2] } });
    expect(b.last()).toEqual({
      type: "relay",
      from: 1,
      payload: { delta: [1, 2] },
    });
  });

  it("errors on signaling before join and on unknown targets", () => {
    const room = new RoomCore();
    const a = new FakePeer();
    room.handleMessage(a, { type: "relay", to: 2, payload: 1 });
    expect(a.last()).toMatchObject({ type: "error", code: "not_joined" });

    room.handleMessage(a, { type: "join", peer: 1 });
    room.handleMessage(a, { type: "relay", to: 99, payload: 1 });
    expect(a.last()).toMatchObject({ type: "error", code: "unknown_target" });
  });
});

describe("RoomCore permission gating", () => {
  it("denies join and directed ops under an allowlist policy", () => {
    const perms = new SignalingPermissions("allowlist");
    perms.allowMany(1, [2]); // peer 1 may join + signal peer 2
    perms.allowJoin(2); // peer 2 may join, but not signal anyone
    const room = new RoomCore(perms);

    const a = new FakePeer();
    const b = new FakePeer();
    const c = new FakePeer();

    room.handleMessage(a, { type: "join", peer: 1 });
    room.handleMessage(b, { type: "join", peer: 2 });
    expect(room.roster()).toEqual([1, 2]);

    // peer 3 is not allowed to join
    room.handleMessage(c, { type: "join", peer: 3 });
    expect(c.last()).toMatchObject({ type: "error", code: "permission_denied" });

    // peer 1 -> 2 allowed
    room.handleMessage(a, { type: "relay", to: 2, payload: "ok" });
    expect(b.last()).toEqual({ type: "relay", from: 1, payload: "ok" });

    // peer 2 -> 1 not allowed (no allowSignal grant)
    room.handleMessage(b, { type: "relay", to: 1, payload: "no" });
    expect(b.last()).toMatchObject({ type: "error", code: "permission_denied" });
  });
});
