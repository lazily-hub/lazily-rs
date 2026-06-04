import { describe, expect, it } from "vitest";
import { SignalingPermissions } from "../src/permissions.js";

describe("SignalingPermissions (open mode)", () => {
  it("allows every op without explicit grants", () => {
    const perms = new SignalingPermissions("open");
    expect(perms.isAllowed(1, { kind: "join" })).toBe(true);
    expect(perms.isAllowed(1, { kind: "relay", to: 2 })).toBe(true);
    expect(perms.check(1, { kind: "offer", to: 9 }).ok).toBe(true);
  });
});

describe("SignalingPermissions (allowlist mode)", () => {
  it("is default-deny", () => {
    const perms = new SignalingPermissions("allowlist");
    expect(perms.isAllowed(1, { kind: "join" })).toBe(false);
    expect(perms.isAllowed(1, { kind: "relay", to: 2 })).toBe(false);
  });

  it("gates join and directed signal independently per target", () => {
    const perms = new SignalingPermissions("allowlist");
    perms.allowJoin(1);
    expect(perms.isAllowed(1, { kind: "join" })).toBe(true);
    // join does not imply the right to signal anyone
    expect(perms.isAllowed(1, { kind: "relay", to: 2 })).toBe(false);

    perms.allowSignal(1, 2);
    expect(perms.isAllowed(1, { kind: "relay", to: 2 })).toBe(true);
    // only the granted target
    expect(perms.isAllowed(1, { kind: "relay", to: 3 })).toBe(false);
  });

  it("isolates peers", () => {
    const perms = new SignalingPermissions("allowlist");
    perms.allowMany(1, [2]);
    expect(perms.isAllowed(2, { kind: "join" })).toBe(false);
    expect(perms.isAllowed(2, { kind: "relay", to: 1 })).toBe(false);
  });

  it("allowMany grants join plus listed targets; revokePeer clears", () => {
    const perms = new SignalingPermissions("allowlist");
    perms.allowMany(1, [2, 3]);
    expect(perms.isAllowed(1, { kind: "join" })).toBe(true);
    expect(perms.isAllowed(1, { kind: "offer", to: 3 })).toBe(true);

    expect(perms.revokePeer(1)).toBe(true);
    expect(perms.isAllowed(1, { kind: "join" })).toBe(false);
    expect(perms.revokePeer(1)).toBe(false);
  });
});
