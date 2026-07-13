//! Phase 6 of the RelayCell backpressure plan — the extra reactive policies.
//!
//! See `lazily-spec/docs/relaycell.md` §7 and
//! `lazily-spec/docs/relaycell-backpressure-analysis.md` §6 / the case matrix
//! rows 8–18. Each policy is an optional reactive stage composed onto a relay
//! egress; they only change *where/when* a relay flushes or *which* ops survive,
//! so a commutative/associative policy's converged state is unchanged
//! (`LazilyFormal.Relay.relay_converges` / `reorder_adjacent`).
//!
//! Time is modeled by a **logical clock** (a monotone tick) so the behaviour is
//! deterministic and portable — a binding drives `tick`/`advance` from its own
//! runtime timer.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::hash::Hash;

use crate::Context;
use crate::merge::MergePolicy;
use crate::relay::{BackpressurePolicy, BoundDim, Overflow, RelayCell};

/// Case 9 — **rate-limited egress (token bucket).** Egress is paced: a drain is
/// permitted only when a token is available; ingress backpressures when the
/// bucket is empty. Refilled `refill_per_tick` tokens per logical tick, capped at
/// `capacity`.
#[derive(Debug, Clone)]
pub struct RatePolicy {
    capacity: u64,
    tokens: u64,
    refill_per_tick: u64,
}

impl RatePolicy {
    pub fn new(capacity: u64, refill_per_tick: u64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_per_tick,
        }
    }

    /// Tokens currently available.
    pub fn tokens(&self) -> u64 {
        self.tokens
    }

    /// Try to consume one token for an egress; returns `true` if paced through.
    pub fn try_egress(&mut self) -> bool {
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Advance the logical clock, refilling the bucket (saturating at capacity).
    pub fn tick(&mut self) {
        self.tokens = (self.tokens + self.refill_per_tick).min(self.capacity);
    }
}

/// Case 8 — **time-windowed coalescence (debounce/throttle).** Accumulates ops
/// into the current window; the window flushes when it reaches `window_ops` ops
/// **or** on an explicit `tick` (the debounce/throttle interval boundary). Because
/// a window is just a flush group, associativity keeps the converged state
/// unchanged (`flushGroupingIrrelevant`).
#[derive(Debug, Clone)]
pub struct WindowPolicy {
    window_ops: u64,
    pending: u64,
}

impl WindowPolicy {
    pub fn new(window_ops: u64) -> Self {
        Self {
            window_ops: window_ops.max(1),
            pending: 0,
        }
    }

    /// Record one ingress; returns `true` when the window is full and should flush.
    pub fn on_ingress(&mut self) -> bool {
        self.pending += 1;
        if self.pending >= self.window_ops {
            self.pending = 0;
            true
        } else {
            false
        }
    }

    /// The debounce/throttle interval elapsed: flush whatever is pending.
    pub fn tick(&mut self) -> bool {
        if self.pending > 0 {
            self.pending = 0;
            true
        } else {
            false
        }
    }
}

/// Case 10 — **TTL / deadline expiry.** Drops elements whose age exceeds `ttl`
/// against a logical clock. Lossy-by-age (explicit); used to shed cold data.
#[derive(Debug, Clone)]
pub struct ExpiryPolicy {
    ttl: u64,
    now: u64,
}

impl ExpiryPolicy {
    pub fn new(ttl: u64) -> Self {
        Self { ttl, now: 0 }
    }

    /// Advance the logical clock.
    pub fn advance(&mut self, by: u64) {
        self.now += by;
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    /// Whether an element stamped at `stamped_at` is still live (not expired).
    pub fn is_live(&self, stamped_at: u64) -> bool {
        self.now.saturating_sub(stamped_at) <= self.ttl
    }

    /// Retain only the live elements of a timestamped batch (drop the aged tail).
    pub fn retain_live<T>(&self, batch: Vec<(u64, T)>) -> Vec<T> {
        batch
            .into_iter()
            .filter(|(ts, _)| self.is_live(*ts))
            .map(|(_, v)| v)
            .collect()
    }
}

/// Case 11 — **priority egress.** A max-priority storage: ingress carries a
/// priority; egress pops the highest priority first (**not** FIFO). Reordering,
/// so sound for a commutative merge downstream (`reorder_adjacent`).
#[derive(Debug, Default)]
pub struct PriorityStorage<T> {
    // (priority, seq) as the heap key; seq breaks ties FIFO within a priority.
    heap: BinaryHeap<(u64, Reverse<u64>, usize)>,
    items: Vec<Option<T>>,
    seq: u64,
}

impl<T> PriorityStorage<T> {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            items: Vec::new(),
            seq: 0,
        }
    }

    pub fn push(&mut self, priority: u64, value: T) {
        let idx = self.items.len();
        self.items.push(Some(value));
        self.heap.push((priority, Reverse(self.seq), idx));
        self.seq += 1;
    }

    /// Pop the highest-priority element (FIFO within equal priority).
    pub fn pop(&mut self) -> Option<T> {
        while let Some((_, _, idx)) = self.heap.pop() {
            if let Some(v) = self.items[idx].take() {
                return Some(v);
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

/// Case 18 — **keyed sharding.** N independent relays keyed by `K`; an op routes
/// to its key's shard. Merging *across* shards requires a **commutative** merge
/// (shards accumulate in parallel, order across keys is not defined). The
/// converged per-key state equals a single relay per key.
pub struct KeyedRelay<K, T, M> {
    shards: HashMap<K, RelayCell<T, M>>,
    high_water: u64,
    overflow: Overflow,
}

impl<K, T, M> KeyedRelay<K, T, M>
where
    K: Eq + Hash + Clone,
    T: Clone + PartialEq + 'static,
    M: MergePolicy<T>,
{
    /// Create a keyed relay. Panics if `overflow` is `Conflate` on a
    /// non-conflating policy (same construction guard as `RelayCell`).
    pub fn new(high_water: u64, overflow: Overflow) -> Self {
        Self {
            shards: HashMap::new(),
            high_water,
            overflow,
        }
    }

    /// Route `op` to `key`'s shard, creating the shard on first use.
    pub fn ingress(&mut self, ctx: &Context, key: K, op: T) {
        let hw = self.high_water;
        let ov = self.overflow;
        let relay = self.shards.entry(key).or_insert_with(|| {
            RelayCell::new(
                ctx,
                BackpressurePolicy::new(ctx, BoundDim::Count, hw, hw / 2, ov),
            )
            .expect("keyed relay shard config")
        });
        relay.ingress(ctx, op);
    }

    /// Drain a key's coalesced window.
    pub fn drain(&self, ctx: &Context, key: &K) -> Option<T> {
        self.shards.get(key).and_then(|r| r.drain(ctx))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.shards.keys()
    }
}
