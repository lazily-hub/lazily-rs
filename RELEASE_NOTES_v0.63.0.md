# lazily-rs v0.63.0 — durable effect CAS, egress ACK refusal

- `EffectCasCore::durable_state` / `EffectCasCore::hydrate` and the
  full-fidelity `EffectCasDurableState` image (retained intents behind an
  in-flight attempt, plan cursors, logical retry times, receipts, terminal
  evidence, actor generation) for cross-process persistence. Hydration
  validates the image and refuses a corrupt one with `EffectCasHydrateError`
  instead of repairing it. With the `serde` feature the image serializes and
  deserialization re-validates plans and identities.
- `EgressCore::ack` refuses an acknowledgement at or beyond `next_sequence`
  with the new `EgressAck::BeyondSent { next_sequence }`, changing no state.
  Previously it lifted the watermark over the next sequence and stranded the
  next enqueued record. `EgressCell`, `ThreadSafeEgressCell` and
  `AsyncEgressCell` inherit the refusal. Exhaustive matches on `EgressAck`
  need a new arm.
- `EgressCore::reconnect` now invalidates `pending` whenever any record is
  retained, so a replay-free reconnect no longer leaves a cached `pending`
  showing old-generation envelopes.
- Conformant to lazily-spec v0.46.0 (effect-CAS fixture, egress
  `beyond_sent` refusal and reconnect steps); CI pins that tag.
