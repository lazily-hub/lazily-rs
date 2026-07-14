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

// `RatePolicy` / `WindowPolicy` / `ExpiryPolicy` were lifted into `rateshape`
// (`#lzrateshape`) so any source can use them, not just a relay egress; the
// crate re-exports them from `rateshape` at the top level, so the relay plane's
// public API and conformance are unchanged.

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
