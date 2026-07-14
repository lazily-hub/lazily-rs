//! Phase 4 of the realtime + distributed primitives plan — `#lzpresence`
//! presence + ephemeral plane.
//!
//! See `lazily-spec/docs/presence.md` and the formal model
//! `lazily-formal/LazilyFormal/Presence.lean`. The CRDT plane is durable;
//! collaborative apps also need an **ephemeral** plane that does not persist
//! (live cursors, typing indicators, presence). Each primitive is a pure compute
//! **core** (a keyed map / single value + TTL over the logical clock —
//! `BytesPayload`) split from a reactive **cell** projecting the live view onto a
//! `Cell` (invalidates only on a live-view change).
//!
//! The ephemeral plane is distinct from the durable plane: the [`Ephemeral`]
//! marker tags values that MUST NOT be persisted, and a durable sink is generic
//! over [`Durable`], so handing it an ephemeral value fails to compile:
//!
//! ```compile_fail
//! use lazily::{Durable, EphemeralValue};
//! fn persist<T: Durable>(_v: T) {}
//! // `EphemeralValue` is `Ephemeral`, not `Durable`:
//! persist(EphemeralValue("cursor"));
//! ```

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::Context;
use crate::cell::CellHandle;

/// Marker: a value on the **ephemeral** plane. MUST NOT be persisted.
pub trait Ephemeral {}

/// Marker: a value that may be written to the durable outbox.
pub trait Durable {}

/// A newtype witnessing the [`Ephemeral`] marker (used by the compile-fail
/// doctest and by ephemeral payloads).
pub struct EphemeralValue<T>(pub T);
impl<T> Ephemeral for EphemeralValue<T> {}

// ===========================================================================
// Ephemeral single value
// ===========================================================================

/// Single-value auto-expiry compute core — "the last value seen in window N".
#[derive(Debug, Clone)]
pub struct EphemeralCore<T> {
    value: Option<T>,
    expiry: u64,
}

impl<T: Clone> Default for EphemeralCore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> EphemeralCore<T> {
    pub fn new() -> Self {
        Self {
            value: None,
            expiry: 0,
        }
    }
    /// Set the value, expiring at `now + ttl`.
    pub fn set(&mut self, value: T, now: u64, ttl: u64) {
        self.value = Some(value);
        self.expiry = now + ttl;
    }
    /// Clear the value once `now ≥ expiry`.
    pub fn tick(&mut self, now: u64) {
        if self.value.is_some() && now >= self.expiry {
            self.value = None;
        }
    }
    pub fn value(&self) -> Option<T> {
        self.value.clone()
    }
}

impl<T> Ephemeral for EphemeralCore<T> {}

/// Reactive single-value ephemeral cell.
pub struct EphemeralCell<T> {
    core: RefCell<EphemeralCore<T>>,
    value: CellHandle<Option<T>>,
}

impl<T: Clone + PartialEq + 'static> EphemeralCell<T> {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(EphemeralCore::new()),
            value: ctx.cell(None),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let v = self.core.borrow().value();
        self.value.set(ctx, v);
    }
    pub fn set(&self, ctx: &Context, value: T, now: u64, ttl: u64) {
        self.core.borrow_mut().set(value, now, ttl);
        self.refresh(ctx);
    }
    pub fn tick(&self, ctx: &Context, now: u64) {
        self.core.borrow_mut().tick(now);
        self.refresh(ctx);
    }
    pub fn value(&self, ctx: &Context) -> Option<T> {
        self.value.get(ctx)
    }
    pub fn value_cell(&self) -> CellHandle<Option<T>> {
        self.value
    }
}

impl<T> Ephemeral for EphemeralCell<T> {}

// ===========================================================================
// Keyed per-peer ephemeral map (shared by presence + awareness)
// ===========================================================================

/// Per-key ephemeral map with TTL eviction — the shared core behind presence and
/// awareness. Each entry carries an expiry; `tick` evicts lapsed entries.
#[derive(Debug, Clone)]
pub struct EphemeralMapCore<K, V> {
    entries: BTreeMap<K, (V, u64)>,
}

impl<K: Ord + Clone, V: Clone> Default for EphemeralMapCore<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, V: Clone> EphemeralMapCore<K, V> {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
    /// Set/refresh `key`'s value (last-writer wins), expiring at `now + ttl`.
    pub fn set(&mut self, key: K, value: V, now: u64, ttl: u64) {
        self.entries.insert(key, (value, now + ttl));
    }
    /// Drop `key` immediately (membership `Dead`/`Left`).
    pub fn evict(&mut self, key: &K) {
        self.entries.remove(key);
    }
    /// Evict entries whose TTL has lapsed (`now ≥ expiry`).
    pub fn tick(&mut self, now: u64) {
        self.entries.retain(|_, (_, expiry)| now < *expiry);
    }
    /// The live value for `key` (respecting `now`).
    pub fn get(&self, key: &K, now: u64) -> Option<V> {
        self.entries
            .get(key)
            .filter(|(_, expiry)| now < *expiry)
            .map(|(v, _)| v.clone())
    }
    /// The live key → value map at `now`.
    pub fn present(&self, now: u64) -> BTreeMap<K, V> {
        self.entries
            .iter()
            .filter(|(_, (_, expiry))| now < *expiry)
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect()
    }
}

impl<K, V> Ephemeral for EphemeralMapCore<K, V> {}

/// Reactive per-peer presence: heartbeat-kept, membership- and TTL-evicted.
pub struct PresenceCell<K, V> {
    core: RefCell<EphemeralMapCore<K, V>>,
    present: CellHandle<BTreeMap<K, V>>,
    ttl: u64,
}

impl<K: Ord + Clone + 'static, V: Clone + PartialEq + 'static> PresenceCell<K, V> {
    pub fn new(ctx: &Context, ttl: u64) -> Self {
        Self {
            core: RefCell::new(EphemeralMapCore::new()),
            present: ctx.cell(BTreeMap::new()),
            ttl,
        }
    }
    fn refresh(&self, ctx: &Context, now: u64) {
        let p = self.core.borrow().present(now);
        self.present.set(ctx, p);
    }
    /// Heartbeat a peer's presence (expiring at `now + ttl`).
    pub fn heartbeat(&self, ctx: &Context, peer: K, value: V, now: u64) {
        self.core.borrow_mut().set(peer, value, now, self.ttl);
        self.refresh(ctx, now);
    }
    /// Evict a peer on membership loss.
    pub fn evict(&self, ctx: &Context, peer: &K, now: u64) {
        self.core.borrow_mut().evict(peer);
        self.refresh(ctx, now);
    }
    pub fn tick(&self, ctx: &Context, now: u64) {
        self.core.borrow_mut().tick(now);
        self.refresh(ctx, now);
    }
    pub fn present(&self, ctx: &Context) -> BTreeMap<K, V> {
        self.present.get(ctx)
    }
    pub fn present_cell(&self) -> CellHandle<BTreeMap<K, V>> {
        self.present
    }
}

impl<K, V> Ephemeral for PresenceCell<K, V> {}

/// Reactive typed ephemeral broadcast (cursors / selections): last-writer-per-peer
/// with a TTL.
pub struct AwarenessCell<K, V> {
    core: RefCell<EphemeralMapCore<K, V>>,
    present: CellHandle<BTreeMap<K, V>>,
    ttl: u64,
}

impl<K: Ord + Clone + 'static, V: Clone + PartialEq + 'static> AwarenessCell<K, V> {
    pub fn new(ctx: &Context, ttl: u64) -> Self {
        Self {
            core: RefCell::new(EphemeralMapCore::new()),
            present: ctx.cell(BTreeMap::new()),
            ttl,
        }
    }
    fn refresh(&self, ctx: &Context, now: u64) {
        let p = self.core.borrow().present(now);
        self.present.set(ctx, p);
    }
    /// Set a peer's awareness value (last-writer wins, no merge).
    pub fn set(&self, ctx: &Context, peer: K, value: V, now: u64) {
        self.core.borrow_mut().set(peer, value, now, self.ttl);
        self.refresh(ctx, now);
    }
    pub fn tick(&self, ctx: &Context, now: u64) {
        self.core.borrow_mut().tick(now);
        self.refresh(ctx, now);
    }
    pub fn get(&self, peer: &K, now: u64) -> Option<V> {
        self.core.borrow().get(peer, now)
    }
    pub fn present(&self, ctx: &Context) -> BTreeMap<K, V> {
        self.present.get(ctx)
    }
    pub fn present_cell(&self) -> CellHandle<BTreeMap<K, V>> {
        self.present
    }
}

impl<K, V> Ephemeral for AwarenessCell<K, V> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ephemeral_expires_and_overwrites() {
        let mut e = EphemeralCore::new();
        e.set("a", 0, 5);
        e.tick(3);
        assert_eq!(e.value(), Some("a"));
        e.tick(5);
        assert_eq!(e.value(), None);
        e.set("b", 6, 5);
        e.set("c", 10, 5); // overwrite before expiry
        assert_eq!(e.value(), Some("c"));
    }

    #[test]
    fn presence_evict_and_ttl() {
        let mut m = EphemeralMapCore::<u64, &str>::new();
        m.set(1, "online", 0, 5);
        m.set(2, "online", 1, 5);
        m.evict(&2);
        assert_eq!(m.present(2).len(), 1);
        m.tick(6); // peer 1 expires at 5
        assert!(m.present(6).is_empty());
    }

    #[test]
    fn awareness_last_writer() {
        let mut m = EphemeralMapCore::<u64, &str>::new();
        m.set(1, "cursor-a", 0, 5);
        m.set(1, "cursor-a2", 2, 5);
        assert_eq!(m.get(&1, 2), Some("cursor-a2"));
    }
}
