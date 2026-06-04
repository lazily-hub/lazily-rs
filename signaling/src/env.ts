/** Worker bindings for the lazily-distributed signaling server (#yxjw). */
export interface Env {
  /** Durable Object namespace; one instance per session id. */
  SIGNALING_ROOM: DurableObjectNamespace;
  /** Permission mode: `"open"` (default) or `"allowlist"` (#39c5 default-deny). */
  SIGNALING_MODE?: string;
}
