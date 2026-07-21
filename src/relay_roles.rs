//! Phase 5 of the RelayCell backpressure plan — the `Inbox` / `Outbox` role
//! facades over `RelayCell`.
//!
//! See `lazily-spec/docs/relaycell.md` §6 and
//! `lazily-spec/docs/relaycell-backpressure-analysis.md` §4.7. `RelayCell` is
//! direction-neutral; `Inbox` and `Outbox` are **role facades** (typed
//! constructors with direction-appropriate defaults), not reimplementations —
//! mirroring "MPSC is a *usage* of `QueueCell`, not a subtype". They differ in
//! the **backpressure-propagation contract**:
//!
//! - **`Outbox`** (app → transport): backpressures the **local producer**, which
//!   is directly blockable via `is_full`. Default overflow `Conflate` (state) —
//!   a slow egress collapses to the latest merged value.
//! - **`Inbox`** (transport → app): backpressures the **remote peer**, which is
//!   *not* directly blockable — only via transport flow control (withhold
//!   credits / TCP window). Modeled here by a **credit meter**: the app grants
//!   credits; when exhausted, the inbox is not ready and the transport must stop
//!   delivering (the remote throttles).
//!
//! A network link is `Outbox → Transport → Inbox`, and end-to-end backpressure
//! is a chain of relays sharing one `RelayCell` core.

use crate::Context;
use crate::cell::FormulaCell;
use crate::merge::MergePolicy;
use crate::relay::{
    BackpressurePolicy, BoundDim, IngressOutcome, Overflow, RelayCell, RelayConfigError,
};

/// The app → transport send side (analysis §4.7). Backpressures the local
/// producer directly via `is_full`.
pub struct Outbox<T, M> {
    relay: RelayCell<T, M>,
}

impl<T, M> Outbox<T, M>
where
    T: Clone + PartialEq + 'static,
    M: MergePolicy<T>,
{
    /// Build an outbox bounded by `high_water` with the role default overflow
    /// (`Conflate` — the state-broadcast case). Validates the policy flags.
    pub fn new(ctx: &Context, high_water: u64) -> Result<Self, RelayConfigError> {
        Self::with_overflow(ctx, BoundDim::Count, high_water, Overflow::Conflate)
    }

    /// Build an outbox with an explicit dimension/overflow (e.g. `Spill` for a
    /// lossless event channel).
    pub fn with_overflow(
        ctx: &Context,
        dimension: BoundDim,
        high_water: u64,
        overflow: Overflow,
    ) -> Result<Self, RelayConfigError> {
        let policy = BackpressurePolicy::new(ctx, dimension, high_water, high_water / 2, overflow);
        Ok(Self {
            relay: RelayCell::new(ctx, policy)?,
        })
    }

    /// The local producer sends an op. A `Blocked` outcome is the producer's
    /// backpressure signal — it should await a drain before retrying.
    pub fn send(&self, ctx: &Context, op: T) -> IngressOutcome {
        self.relay.ingress(ctx, op)
    }

    /// The transport drains the coalesced window for egress.
    pub fn drain(&self, ctx: &Context) -> Option<T> {
        self.relay.drain(ctx)
    }

    /// The producer-facing backpressure signal (window at/over the watermark).
    pub fn is_full(&self) -> FormulaCell<bool> {
        self.relay.is_full()
    }

    /// Access the underlying relay (for wiring extra egress stages).
    pub fn relay(&self) -> &RelayCell<T, M> {
        &self.relay
    }
}

/// The transport → app receive side (analysis §4.7). Cannot block the remote
/// directly; backpressure is a **credit meter** the app replenishes.
pub struct Inbox<T, M> {
    relay: RelayCell<T, M>,
    credits: u64,
    max_credits: u64,
}

impl<T, M> Inbox<T, M>
where
    T: Clone + PartialEq + 'static,
    M: MergePolicy<T>,
{
    /// Build an inbox bounded by `high_water` with the role default overflow
    /// (`Conflate` for inbound state) and a credit budget of `max_credits`.
    pub fn new(ctx: &Context, high_water: u64, max_credits: u64) -> Result<Self, RelayConfigError> {
        Self::with_overflow(ctx, high_water, Overflow::Conflate, max_credits)
    }

    pub fn with_overflow(
        ctx: &Context,
        high_water: u64,
        overflow: Overflow,
        max_credits: u64,
    ) -> Result<Self, RelayConfigError> {
        let policy =
            BackpressurePolicy::new(ctx, BoundDim::Count, high_water, high_water / 2, overflow);
        Ok(Self {
            relay: RelayCell::new(ctx, policy)?,
            credits: max_credits,
            max_credits,
        })
    }

    /// Whether the transport may deliver another message (a credit is available).
    /// When `false`, the transport must stop reading → the remote throttles
    /// (TCP window / withheld ack).
    pub fn ready(&self) -> bool {
        self.credits > 0
    }

    /// Credits currently available to the remote.
    pub fn credits(&self) -> u64 {
        self.credits
    }

    /// The transport delivers a received op. Consumes a credit; the caller MUST
    /// have checked [`ready`](Inbox::ready) (a delivery without credit still
    /// applies but drives `credits` to zero, signalling the remote to stop).
    pub fn receive(&mut self, ctx: &Context, op: T) -> IngressOutcome {
        self.credits = self.credits.saturating_sub(1);
        self.relay.ingress(ctx, op)
    }

    /// The app consumes the coalesced window and replenishes `n` credits (up to
    /// the budget), re-opening the remote's flow.
    pub fn consume(&mut self, ctx: &Context, replenish: u64) -> Option<T> {
        let out = self.relay.drain(ctx);
        self.credits = (self.credits + replenish).min(self.max_credits);
        out
    }
}
