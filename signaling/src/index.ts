/**
 * Worker entry for the lazily-distributed signaling server (#yxjw).
 *
 * Routes:
 * - `GET /health`            → liveness probe.
 * - `GET /session/:id` (WS)  → upgrades to the `SignalingRoom` Durable Object
 *                              for session `:id`. Peers in the same session id
 *                              share one room and can discover/relay to each
 *                              other; different ids are fully isolated.
 *
 * Internet-scale peer discovery comes from sharding sessions across Durable
 * Object instances: `idFromName(sessionId)` deterministically maps a session to
 * its single global DO, so the fleet scales with the number of sessions.
 */

import type { Env } from "./env.js";

export { SignalingRoom } from "./room.js";

const SESSION_PATH = /^\/session\/([A-Za-z0-9._-]{1,128})$/;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "GET" && url.pathname === "/health") {
      return new Response("ok", {
        headers: { "content-type": "text/plain" },
      });
    }

    const match = SESSION_PATH.exec(url.pathname);
    if (match === null) {
      return new Response("not found", { status: 404 });
    }
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("expected a WebSocket upgrade", { status: 426 });
    }

    const sessionId = match[1];
    const id = env.SIGNALING_ROOM.idFromName(sessionId);
    const stub = env.SIGNALING_ROOM.get(id);
    return stub.fetch(request);
  },
};
