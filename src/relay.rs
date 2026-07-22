//! Phase 2 of the RelayCell backpressure plan — the in-proc `RelayCell` core.
//!
//! See `lazily-spec/docs/relaycell.md` and
//! `lazily-spec/docs/relaycell-backpressure-analysis.md` §4.2/§4.4. A `RelayCell`
//! is an **algebra-typed conflating relay**: it accumulates a fast ingress into a
//! **hot head** (a [`MergePolicy`] fold), bounds it with a reactive
//! [`BackpressurePolicy`], and lets a slow egress **drain** the coalesced window.
//!
//! It is a *composite*, not a new node (analysis §4.2): the hot head is a cell,
//! its `depth`/`is_full`/`is_empty` reads are demand-driven [`Computed`]s, so an
//! unobserved relay costs `N·⊕` and no more (the merge cost law). The **converged
//! egress state is independent of the drain schedule** whenever `⊕` is associative
//! — the invariant pinned by `LazilyFormal.Relay.relay_converges`.

use std::marker::PhantomData;

use crate::Context;
use crate::cell::Computed;
use crate::cell::Source;
use crate::merge::MergePolicy;

/// What a bound measures (analysis §4.4). The Phase-2 core meters `Count`; the
/// other dimensions are wired as the metering closure evolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundDim {
    Count,
    Bytes,
    Keys,
    Age,
}

/// The action taken when the hot head crosses `high_water` (analysis §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    /// Refuse ingress; the producer backpressures (observes `is_full`). Lossless.
    Block,
    /// Discard the incoming op. Lossy.
    DropNewest,
    /// Reset the window to the incoming op, discarding what accumulated. Lossy.
    DropOldest,
    /// Keep merging — the coalescence *is* the bound. Lossless for converged
    /// state; requires `M::CONFLATES`.
    Conflate,
    /// Page the accumulated window to a durable tail (Phase 3 `SpillStore`).
    Spill,
}

/// Reactive backpressure limits (analysis §4.4). Every field is a cell, so an
/// operator or an adaptive controller retunes it live and dependent relays react.
/// Hysteresis (`high_water` ≠ `low_water`) prevents flapping.
pub struct BackpressurePolicy {
    pub dimension: Source<BoundDim>,
    pub high_water: Source<u64>,
    pub low_water: Source<u64>,
    pub overflow: Source<Overflow>,
}

impl BackpressurePolicy {
    pub fn new(
        ctx: &Context,
        dimension: BoundDim,
        high_water: u64,
        low_water: u64,
        overflow: Overflow,
    ) -> Self {
        Self {
            dimension: ctx.source(dimension),
            high_water: ctx.source(high_water),
            low_water: ctx.source(low_water),
            overflow: ctx.source(overflow),
        }
    }
}

/// Why a construction/merge-swap was rejected (analysis §4.3 flag validation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayConfigError {
    /// `Conflate` chosen for a non-conflating policy (`RawFifo`).
    ConflateNotBounding,
}

/// The outcome of a single `ingress` op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressOutcome {
    /// Merged into an empty window (window depth was 0).
    Accepted,
    /// Merged into a non-empty window (coalesced with prior ops).
    Conflated,
    /// Dropped by `DropNewest`/`DropOldest` overflow.
    Dropped,
    /// Refused by `Block` overflow; the producer must retry after a drain.
    Blocked,
}

/// The algebra-typed conflating relay (Phase 2, in-proc core).
pub struct RelayCell<T, M> {
    /// Hot head: the current window's coalesced value (`None` = empty window).
    head: Source<Option<T>>,
    /// Ops merged into the current window since the last drain (the `Count` bound).
    pending: Source<u64>,
    policy: BackpressurePolicy,
    /// Demand-driven reader: current window depth.
    depth: Computed<u64>,
    /// Demand-driven reader: depth ≥ `high_water`.
    is_full: Computed<bool>,
    /// Demand-driven reader: the window is empty.
    is_empty: Computed<bool>,
    _marker: PhantomData<M>,
}

impl<T, M> RelayCell<T, M>
where
    T: Clone + PartialEq + 'static,
    M: MergePolicy<T>,
{
    /// Build a relay over `policy`, validating the initial overflow against the
    /// policy's algebra flags (analysis §4.3): `Conflate` requires
    /// `M::CONFLATES`.
    pub fn new(ctx: &Context, policy: BackpressurePolicy) -> Result<Self, RelayConfigError> {
        if policy.overflow.get(ctx) == Overflow::Conflate && !M::CONFLATES {
            return Err(RelayConfigError::ConflateNotBounding);
        }
        let head: Source<Option<T>> = ctx.source(None);
        let pending = ctx.source(0u64);
        let depth = ctx.computed(move |c| pending.get(c));
        let high_water = policy.high_water;
        let is_full = ctx.computed(move |c| depth.get(c) >= high_water.get(c));
        let is_empty = ctx.computed(move |c| head.get(c).is_none());
        Ok(Self {
            head,
            pending,
            policy,
            depth,
            is_full,
            is_empty,
            _marker: PhantomData,
        })
    }

    /// Whether the current overflow choice is legal for `M` — a runtime guard
    /// mirroring `new`'s construction-time check (the overflow cell is reactive).
    pub fn overflow_is_legal(&self, ctx: &Context) -> bool {
        self.policy.overflow.get(ctx) != Overflow::Conflate || M::CONFLATES
    }

    /// Demand-driven reader: current window depth (`Count`).
    pub fn depth(&self) -> Computed<u64> {
        self.depth
    }
    /// Demand-driven reader: window is at/over `high_water`.
    pub fn is_full(&self) -> Computed<bool> {
        self.is_full
    }
    /// Demand-driven reader: window is empty (nothing to drain).
    pub fn is_empty(&self) -> Computed<bool> {
        self.is_empty
    }

    fn read_pending(&self, ctx: &Context) -> u64 {
        self.pending.get(ctx)
    }

    fn read_full(&self, ctx: &Context) -> bool {
        self.read_pending(ctx) >= self.policy.high_water.get(ctx)
    }

    fn merge_into_head(&self, ctx: &Context, op: T) {
        let cur = self.head.get(ctx);
        let next = match cur {
            None => op,
            Some(v) => M::merge(&v, op),
        };
        self.head.set(ctx, Some(next));
    }

    /// Ingest one op. Applies the reactive overflow policy when the window is at
    /// `high_water`; otherwise merges the op into the hot head under `M`.
    pub fn ingress(&self, ctx: &Context, op: T) -> IngressOutcome {
        let was_empty = self.pending.get(ctx) == 0;

        if self.read_full(ctx) {
            match self.policy.overflow.get(ctx) {
                Overflow::Block => return IngressOutcome::Blocked,
                Overflow::DropNewest => return IngressOutcome::Dropped,
                Overflow::DropOldest => {
                    // Discard the accumulated window, restart from this op.
                    self.head.set(ctx, Some(op));
                    self.pending.set(ctx, 1);
                    return IngressOutcome::Dropped;
                }
                // Conflate keeps merging (the coalescence is the bound); Spill is
                // Phase 3 and, until wired, degrades to Conflate for a bounding
                // policy. Both fall through to the merge below.
                Overflow::Conflate | Overflow::Spill => {}
            }
        }

        self.merge_into_head(ctx, op);
        self.pending.set(ctx, self.pending.get(ctx) + 1);
        if was_empty {
            IngressOutcome::Accepted
        } else {
            IngressOutcome::Conflated
        }
    }

    /// Drain the coalesced window: take the hot head's value and reset the window.
    /// Returns `None` for an empty window. The egress folds successive drains into
    /// its own accumulator; `relay_converges` guarantees that fold equals the flat
    /// fold of every ingested op, for any drain schedule.
    pub fn drain(&self, ctx: &Context) -> Option<T> {
        let cur = self.head.get(ctx);
        if cur.is_some() {
            self.head.set(ctx, None);
            self.pending.set(ctx, 0);
        }
        cur
    }

    /// Peek the current coalesced window without draining.
    pub fn peek(&self, ctx: &Context) -> Option<T> {
        self.head.get(ctx)
    }
}
