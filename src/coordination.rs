//! Phase 3 of the realtime + distributed primitives plan — `#lzcoord`
//! distributed coordination.
//!
//! See `lazily-spec/docs/coordination.md` and the formal model
//! `lazily-formal/LazilyFormal/Coordination.lean`. Lease / leader / lock /
//! semaphore / barrier + quorum primitives, each a pure compute **core** (a
//! state machine over integers / peer ids — `BytesPayload`, C++-eligible) split
//! from a reactive **cell** projecting the salient reader onto a `Cell`. Time is
//! the logical clock; `expiry` is a tick value the runtime drives.

use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::Context;
use crate::cell::Source;

// ===========================================================================
// Lease + fencing token
// ===========================================================================

/// Single-writer lease authority with a monotone fencing token.
#[derive(Debug, Clone)]
pub struct LeaseCore<P> {
    holder: Option<P>,
    expiry: u64,
    fence: u64,
}

impl<P: Clone + PartialEq> Default for LeaseCore<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Clone + PartialEq> LeaseCore<P> {
    pub fn new() -> Self {
        Self {
            holder: None,
            expiry: 0,
            fence: 0,
        }
    }

    fn is_expired(&self, now: u64) -> bool {
        self.holder.is_some() && now >= self.expiry
    }

    /// Whether the lease is currently held (and not expired at `now`).
    pub fn is_held(&self, now: u64) -> bool {
        self.holder.is_some() && !self.is_expired(now)
    }

    /// The live holder at `now`.
    pub fn holder(&self, now: u64) -> Option<P> {
        if self.is_held(now) {
            self.holder.clone()
        } else {
            None
        }
    }

    pub fn fence(&self) -> u64 {
        self.fence
    }

    /// Grant if free/expired (new grant increments `fence`); renew by the holder
    /// keeps the same fence; held by another → `None`.
    pub fn acquire(&mut self, peer: P, now: u64, ttl: u64) -> Option<u64> {
        let free = match &self.holder {
            None => true,
            Some(_) => self.is_expired(now),
        };
        if free {
            self.fence += 1;
            self.holder = Some(peer);
            self.expiry = now + ttl;
            return Some(self.fence);
        }
        if self.holder.as_ref() == Some(&peer) {
            self.expiry = now + ttl; // renew keeps fence
            return Some(self.fence);
        }
        None
    }

    /// Extend the expiry if `peer` is the live holder.
    pub fn renew(&mut self, peer: P, now: u64, ttl: u64) -> bool {
        if self.is_held(now) && self.holder.as_ref() == Some(&peer) {
            self.expiry = now + ttl;
            true
        } else {
            false
        }
    }

    /// Drop the grant if `peer` holds it.
    pub fn release(&mut self, peer: &P) {
        if self.holder.as_ref() == Some(peer) {
            self.holder = None;
        }
    }

    /// Expire the grant when `now ≥ expiry`; returns the expiry edge.
    pub fn tick(&mut self, now: u64) -> bool {
        if self.is_expired(now) {
            self.holder = None;
            true
        } else {
            false
        }
    }
}

/// Reactive lease: projects the holder onto a `Cell` (invalidates on holder
/// change).
pub struct LeaseCell<P> {
    core: RefCell<LeaseCore<P>>,
    holder: Source<Option<P>>,
}

impl<P: Clone + PartialEq + 'static> LeaseCell<P> {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(LeaseCore::new()),
            holder: ctx.cell(None),
        }
    }

    fn refresh(&self, ctx: &Context, now: u64) {
        let h = self.core.borrow().holder(now);
        self.holder.set(ctx, h);
    }

    pub fn acquire(&self, ctx: &Context, peer: P, now: u64, ttl: u64) -> Option<u64> {
        let r = self.core.borrow_mut().acquire(peer, now, ttl);
        self.refresh(ctx, now);
        r
    }

    pub fn renew(&self, ctx: &Context, peer: P, now: u64, ttl: u64) -> bool {
        let r = self.core.borrow_mut().renew(peer, now, ttl);
        self.refresh(ctx, now);
        r
    }

    pub fn release(&self, ctx: &Context, peer: &P, now: u64) {
        self.core.borrow_mut().release(peer);
        self.refresh(ctx, now);
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let r = self.core.borrow_mut().tick(now);
        self.refresh(ctx, now);
        r
    }

    pub fn holder(&self, now: u64) -> Option<P> {
        self.core.borrow().holder(now)
    }
    pub fn is_held(&self, now: u64) -> bool {
        self.core.borrow().is_held(now)
    }
    pub fn fence(&self) -> u64 {
        self.core.borrow().fence()
    }
    pub fn holder_cell(&self) -> Source<Option<P>> {
        self.holder
    }
}

// ===========================================================================
// Leader / follower / candidate
// ===========================================================================

/// The local node's role, derived from lease ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderRole {
    Leader,
    Follower,
    Candidate,
}

/// Reactive leadership over a lease from node `me`'s perspective.
pub struct LeaderCell<P> {
    core: RefCell<LeaseCore<P>>,
    me: P,
    current_leader: Source<Option<P>>,
}

impl<P: Clone + PartialEq + 'static> LeaderCell<P> {
    pub fn new(ctx: &Context, me: P) -> Self {
        Self {
            core: RefCell::new(LeaseCore::new()),
            me,
            current_leader: ctx.cell(None),
        }
    }

    fn refresh(&self, ctx: &Context, now: u64) {
        let l = self.core.borrow().holder(now);
        self.current_leader.set(ctx, l);
    }

    /// Try to acquire leadership for `me`.
    pub fn campaign(&self, ctx: &Context, now: u64, ttl: u64) -> LeaderRole {
        self.core.borrow_mut().acquire(self.me.clone(), now, ttl);
        self.refresh(ctx, now);
        self.role(now)
    }

    /// Simulate another peer contending (mostly for tests / co-hosted nodes).
    pub fn contend(&self, ctx: &Context, peer: P, now: u64, ttl: u64) -> LeaderRole {
        self.core.borrow_mut().acquire(peer, now, ttl);
        self.refresh(ctx, now);
        self.role(now)
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> LeaderRole {
        self.core.borrow_mut().tick(now);
        self.refresh(ctx, now);
        self.role(now)
    }

    pub fn current_leader(&self, now: u64) -> Option<P> {
        self.core.borrow().holder(now)
    }

    pub fn role(&self, now: u64) -> LeaderRole {
        match self.core.borrow().holder(now) {
            Some(h) if h == self.me => LeaderRole::Leader,
            Some(_) => LeaderRole::Follower,
            None => LeaderRole::Candidate,
        }
    }

    pub fn current_leader_cell(&self) -> Source<Option<P>> {
        self.current_leader
    }
}

// ===========================================================================
// Distributed lock + fencing
// ===========================================================================

/// Reactive distributed mutex over a lease + fencing token.
pub struct LockCell<P> {
    core: RefCell<LeaseCore<P>>,
    is_locked: Source<bool>,
}

impl<P: Clone + PartialEq + 'static> LockCell<P> {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(LeaseCore::new()),
            is_locked: ctx.cell(false),
        }
    }

    fn refresh(&self, ctx: &Context, now: u64) {
        let held = self.core.borrow().is_held(now);
        self.is_locked.set(ctx, held);
    }

    /// Acquire the lock, returning a fencing token, or `None` if held.
    pub fn acquire(&self, ctx: &Context, peer: P, now: u64, ttl: u64) -> Option<u64> {
        let r = self.core.borrow_mut().acquire(peer, now, ttl);
        self.refresh(ctx, now);
        r
    }

    pub fn release(&self, ctx: &Context, peer: &P, now: u64) {
        self.core.borrow_mut().release(peer);
        self.refresh(ctx, now);
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let r = self.core.borrow_mut().tick(now);
        self.refresh(ctx, now);
        r
    }

    /// Whether `fence` is the current (non-stale) fencing token.
    pub fn validate(&self, fence: u64) -> bool {
        self.core.borrow().fence() == fence
    }

    pub fn is_locked(&self, now: u64) -> bool {
        self.core.borrow().is_held(now)
    }
    pub fn fence(&self) -> u64 {
        self.core.borrow().fence()
    }
    pub fn is_locked_cell(&self) -> Source<bool> {
        self.is_locked
    }
}

// ===========================================================================
// Semaphore
// ===========================================================================

/// Bounded permit pool compute core.
#[derive(Debug, Clone, Copy)]
pub struct SemaphoreCore {
    capacity: u64,
    acquired: u64,
}

impl SemaphoreCore {
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            acquired: 0,
        }
    }
    pub fn available(&self) -> u64 {
        self.capacity - self.acquired
    }
    pub fn acquire(&mut self) -> bool {
        if self.acquired < self.capacity {
            self.acquired += 1;
            true
        } else {
            false
        }
    }
    pub fn release(&mut self) {
        if self.acquired > 0 {
            self.acquired -= 1;
        }
    }
}

/// Reactive semaphore: projects `permits_available` onto a `Cell`.
pub struct SemaphoreCell {
    core: RefCell<SemaphoreCore>,
    available: Source<u64>,
}

impl SemaphoreCell {
    pub fn new(ctx: &Context, capacity: u64) -> Self {
        Self {
            core: RefCell::new(SemaphoreCore::new(capacity)),
            available: ctx.cell(capacity),
        }
    }

    fn refresh(&self, ctx: &Context) {
        let a = self.core.borrow().available();
        self.available.set(ctx, a);
    }

    pub fn acquire(&self, ctx: &Context) -> bool {
        let r = self.core.borrow_mut().acquire();
        self.refresh(ctx);
        r
    }

    pub fn release(&self, ctx: &Context) {
        self.core.borrow_mut().release();
        self.refresh(ctx);
    }

    pub fn permits_available(&self, ctx: &Context) -> u64 {
        self.available.get(ctx)
    }
    pub fn permits_available_cell(&self) -> Source<u64> {
        self.available
    }
}

// ===========================================================================
// Barrier / quorum
// ===========================================================================

/// Wait-for-N gate compute core over distinct arriving peers.
#[derive(Debug, Clone)]
pub struct BarrierCore<P> {
    required: u64,
    arrived: BTreeSet<P>,
}

impl<P: Ord + Clone> BarrierCore<P> {
    pub fn new(required: u64) -> Self {
        Self {
            required,
            arrived: BTreeSet::new(),
        }
    }
    /// Register a distinct arrival; returns whether the gate is open afterward.
    pub fn arrive(&mut self, peer: P) -> bool {
        self.arrived.insert(peer);
        self.is_open()
    }
    pub fn count(&self) -> u64 {
        self.arrived.len() as u64
    }
    pub fn is_open(&self) -> bool {
        self.count() >= self.required
    }
}

/// Reactive wait-for-N gate. `QuorumCell` is a barrier with `required =
/// total / 2 + 1`.
pub struct BarrierCell<P> {
    core: RefCell<BarrierCore<P>>,
    is_open: Source<bool>,
}

impl<P: Ord + Clone + 'static> BarrierCell<P> {
    pub fn new(ctx: &Context, required: u64) -> Self {
        let core = BarrierCore::new(required);
        let open = core.is_open();
        Self {
            core: RefCell::new(core),
            is_open: ctx.cell(open),
        }
    }

    /// A quorum gate: opens at strict majority of `total`.
    pub fn quorum(ctx: &Context, total: u64) -> Self {
        Self::new(ctx, total / 2 + 1)
    }

    fn refresh(&self, ctx: &Context) {
        let o = self.core.borrow().is_open();
        self.is_open.set(ctx, o);
    }

    /// Register an arrival / vote; returns whether the gate is open afterward.
    pub fn arrive(&self, ctx: &Context, peer: P) -> bool {
        let r = self.core.borrow_mut().arrive(peer);
        self.refresh(ctx);
        r
    }

    pub fn count(&self) -> u64 {
        self.core.borrow().count()
    }
    pub fn is_open(&self, ctx: &Context) -> bool {
        self.is_open.get(ctx)
    }
    pub fn is_open_cell(&self) -> Source<bool> {
        self.is_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_fence_monotone_renew_keeps() {
        let mut l = LeaseCore::<u64>::new();
        assert_eq!(l.acquire(1, 0, 10), Some(1));
        assert_eq!(l.acquire(2, 1, 10), None); // held
        assert!(l.renew(1, 5, 10));
        assert_eq!(l.fence(), 1); // renew keeps fence
        assert!(l.tick(15)); // expired
        assert_eq!(l.acquire(2, 16, 10), Some(2)); // new grant increments
    }

    #[test]
    fn semaphore_bounded_ops() {
        let mut s = SemaphoreCore::new(2);
        assert!(s.acquire());
        assert!(s.acquire());
        assert!(!s.acquire()); // full
        assert_eq!(s.available(), 0);
        s.release();
        assert_eq!(s.available(), 1);
    }

    #[test]
    fn quorum_opens_at_majority() {
        let mut b = BarrierCore::<u64>::new(5 / 2 + 1); // 3
        assert!(!b.arrive(1));
        assert!(!b.arrive(2));
        assert!(b.arrive(3)); // majority
        assert!(b.arrive(1)); // idempotent, still open
        assert_eq!(b.count(), 3);
    }
}
