//! Keyed reactive collections: [`CellMap`] and [`CellFamily`] (#lzcellfamily).
//!
//! `Context` addresses nodes by opaque [`SlotId`](crate::context). These types
//! add a *keyed* layer on top: a hash collection whose **membership is itself
//! reactive**, with one independently-tracked value cell per entry.
//!
//! # Fine-grained vs. coarse
//!
//! Modelling a collection as a single `ctx.cell(HashMap<K, V>)` is *coarse*:
//! every single-entry mutation replaces the whole map, so any reader of any
//! entry is invalidated and (over a wire) the entire map is re-sent.
//!
//! [`CellMap`] is *fine-grained*. Each entry is its own [`CellHandle<V>`], so:
//!
//! - A reader that depends on entry `a` is **not** invalidated when entry `b`
//!   changes — only that entry's dependents recompute.
//! - Membership (the set of keys) is tracked by a dedicated version cell, so
//!   [`keys`](CellMap::keys) / [`len`](CellMap::len) readers recompute only
//!   when keys are **added or removed**, not when an existing value changes.
//!
//! [`CellFamily`] layers a value factory on top of [`CellMap`]: a parameterized
//! factory (à la Recoil/Jotai `atomFamily`) that lazily mints and caches one
//! cell per key on first access via [`CellFamily::get`].
//!
//! ```
//! use lazily::{CellMap, Context};
//!
//! let ctx = Context::new();
//! let scores: CellMap<&'static str, i32> = CellMap::new(&ctx);
//! let alice = scores.entry(&ctx, "alice", 10);
//! let bob = scores.entry(&ctx, "bob", 20);
//!
//! // A computed over the whole collection recomputes only on membership change.
//! let n = ctx.computed({
//!     let scores = scores.clone();
//!     move |ctx| scores.len(ctx)
//! });
//! assert_eq!(ctx.get(&n), 2);
//!
//! // Mutating an existing entry does not change membership.
//! alice.set(&ctx, 11);
//! assert_eq!(ctx.get(&n), 2);
//! assert_eq!(bob.get(&ctx), 20);
//! ```

use std::cell::{Cell as StdCell, RefCell};
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::Context;
use crate::cell::CellHandle;

/// A keyed reactive collection: a hash map of `K -> CellHandle<V>` with reactive
/// membership and independently-tracked per-entry value cells.
///
/// Cheap to [`Clone`] (an `Rc` to the shared inner state) so it can be captured
/// by compute/effect closures. All operations are taken against the owning
/// [`Context`]; like the rest of `lazily`, the graph data lives in the context.
pub struct CellMap<K, V> {
    inner: Rc<CellMapInner<K, V>>,
}

struct CellMapInner<K, V> {
    /// Per-key value cells. Each entry is its own reactive node.
    entries: RefCell<HashMap<K, CellHandle<V>>>,
    /// Insertion-ordered authoritative key list (snapshot returned by `keys`).
    order: RefCell<Vec<K>>,
    /// Reactive *set-membership* signal. Holds a monotonic version bumped only
    /// when the **set** of keys changes (add/remove). Reading it (in
    /// `len`/`contains_key`/`is_empty`) subscribes the caller to membership
    /// changes without coupling to entry values *or to pure reordering*.
    membership: CellHandle<u64>,
    /// Plain (untracked) mirror of the membership version so mutators can bump
    /// the reactive cell without registering a spurious dependency.
    version: StdCell<u64>,
    /// Reactive *order* signal. Bumped on add/remove **and on move/reorder**.
    /// `keys` subscribes here so an atomic ordered move (`#lzcellmove`)
    /// invalidates key-order readers without disturbing `len`/`contains_key`
    /// readers that only care about set identity.
    order_signal: CellHandle<u64>,
    /// Untracked mirror of the order version.
    order_version: StdCell<u64>,
}

impl<K, V> Clone for CellMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<K, V> CellMap<K, V>
where
    K: Eq + Hash + Clone + 'static,
    V: PartialEq + Clone + 'static,
{
    /// Create an empty collection bound to `ctx`.
    pub fn new(ctx: &Context) -> Self {
        Self {
            inner: Rc::new(CellMapInner {
                entries: RefCell::new(HashMap::new()),
                order: RefCell::new(Vec::new()),
                membership: ctx.cell(0u64),
                version: StdCell::new(0),
                order_signal: ctx.cell(0u64),
                order_version: StdCell::new(0),
            }),
        }
    }

    /// Bump the *order* signal (invalidates `keys` readers). Add/remove also
    /// bump this; a pure move bumps **only** this.
    fn bump_order(&self, ctx: &Context) {
        let next = self.inner.order_version.get().wrapping_add(1);
        self.inner.order_version.set(next);
        ctx.set_cell(&self.inner.order_signal, next);
    }

    /// Bump set-membership (invalidates `len`/`contains_key` readers). Always
    /// paired with an order bump because add/remove change order too.
    fn bump_membership(&self, ctx: &Context) {
        let next = self.inner.version.get().wrapping_add(1);
        self.inner.version.set(next);
        // A write, not a tracked read: membership readers are invalidated, but
        // no dependency is registered on whatever frame called the mutator.
        ctx.set_cell(&self.inner.membership, next);
        // The key set changed, so the ordered key list changed too.
        self.bump_order(ctx);
    }

    /// Return the value cell for `key`, minting it with `default` (computed via
    /// the closure) on first access. Subsequent calls return the cached handle.
    ///
    /// Adding a new key bumps reactive membership; re-fetching an existing key
    /// does not.
    pub fn entry_with(&self, ctx: &Context, key: K, default: impl FnOnce() -> V) -> CellHandle<V> {
        if let Some(handle) = self.inner.entries.borrow().get(&key) {
            return *handle;
        }
        let handle = ctx.cell(default());
        self.inner.entries.borrow_mut().insert(key.clone(), handle);
        self.inner.order.borrow_mut().push(key);
        self.bump_membership(ctx);
        handle
    }

    /// Return the value cell for `key`, minting it with `default` on first
    /// access. Convenience wrapper over [`entry_with`](CellMap::entry_with).
    pub fn entry(&self, ctx: &Context, key: K, default: V) -> CellHandle<V> {
        self.entry_with(ctx, key, || default)
    }

    /// Return the existing value cell for `key`, or `None`. Non-reactive: this
    /// does not subscribe the caller to membership.
    pub fn handle(&self, key: &K) -> Option<CellHandle<V>> {
        self.inner.entries.borrow().get(key).copied()
    }

    /// Read the value at `key` if present. Reactive on that entry only (a reader
    /// is invalidated when this entry changes, not when siblings change).
    pub fn get(&self, ctx: &Context, key: &K) -> Option<V> {
        let handle = self.inner.entries.borrow().get(key).copied();
        handle.map(|h| ctx.get_cell(&h))
    }

    /// Set the value at `key`, inserting a new entry (and bumping membership) if
    /// it does not exist yet. Updating an existing entry leaves membership
    /// untouched and invalidates only that entry's dependents.
    pub fn set(&self, ctx: &Context, key: K, value: V) {
        if let Some(handle) = self.inner.entries.borrow().get(&key).copied() {
            handle.set(ctx, value);
            return;
        }
        self.entry_with(ctx, key, || value);
    }

    /// Get the value at `key`, inserting `factory(&key)` first if the key is
    /// absent — the auto-mint recipe (`CellFamily` without a standing factory;
    /// see `lazily-spec/cell-model.md` § Materialization). Bumps reactive
    /// membership only on insert; an existing key returns its current value.
    pub fn get_or_insert_with(&self, ctx: &Context, key: K, factory: impl FnOnce(&K) -> V) -> V {
        if let Some(handle) = self.inner.entries.borrow().get(&key).copied() {
            return ctx.get_cell(&handle);
        }
        let value = factory(&key);
        let ret = value.clone();
        self.entry_with(ctx, key, || value);
        ret
    }

    /// Remove `key`'s entry. Bumps reactive membership and clears the removed
    /// entry's dependents. Returns whether the key was present.
    ///
    /// Note: the underlying node id is not recycled (the runtime exposes no
    /// node-free API yet); the orphaned cell stops driving any dependents.
    pub fn remove(&self, ctx: &Context, key: &K) -> bool {
        let removed = self.inner.entries.borrow_mut().remove(key);
        let Some(handle) = removed else {
            return false;
        };
        self.inner.order.borrow_mut().retain(|k| k != key);
        handle.clear_dependents(ctx);
        self.bump_membership(ctx);
        true
    }

    /// Reactive snapshot of the keys in their current order. Subscribes the
    /// caller to **order** changes (add/remove **and move/reorder**), not to
    /// per-entry value changes.
    pub fn keys(&self, ctx: &Context) -> Vec<K> {
        let _ = ctx.get_cell(&self.inner.order_signal);
        self.inner.order.borrow().clone()
    }

    /// Current 0-based position of `key` in the order, or `None` if absent.
    /// Non-reactive.
    pub fn position(&self, key: &K) -> Option<usize> {
        self.inner.order.borrow().iter().position(|k| k == key)
    }

    /// Atomically move `key` to `index` in the order (`#lzcellmove`).
    ///
    /// This is the *atomic, optimized* reorder: the entry keeps the **same**
    /// value cell, the same dependents, and its CRDT lineage — unlike the naive
    /// `remove` + `entry` which re-mints the cell and bumps membership twice.
    /// Only the order signal is bumped (once), so `keys` readers recompute but
    /// `len`/`contains_key` readers — which track set identity, not order —
    /// stay cached.
    ///
    /// `index` is clamped to `[0, len)`. Returns whether `key` was present.
    pub fn move_to(&self, ctx: &Context, key: &K, index: usize) -> bool {
        let mut order = self.inner.order.borrow_mut();
        let Some(from) = order.iter().position(|k| k == key) else {
            return false;
        };
        let to = index.min(order.len().saturating_sub(1));
        if from == to {
            return true; // no-op: do not invalidate readers needlessly.
        }
        let k = order.remove(from);
        order.insert(to, k);
        drop(order);
        self.bump_order(ctx);
        true
    }

    /// Atomically move `key` to just before `anchor` in the order
    /// (`#lzcellmove`). No-op if either key is absent or already adjacent in the
    /// requested position. Returns whether the move could be expressed.
    pub fn move_before(&self, ctx: &Context, key: &K, anchor: &K) -> bool {
        let Some(anchor_idx) = self.position(anchor) else {
            return false;
        };
        let from = match self.position(key) {
            Some(i) => i,
            None => return false,
        };
        // Removing `key` first shifts `anchor` left by one when key precedes it.
        let target = if from < anchor_idx {
            anchor_idx - 1
        } else {
            anchor_idx
        };
        self.move_to(ctx, key, target)
    }

    /// Atomically move `key` to just after `anchor` in the order (`#lzcellmove`).
    pub fn move_after(&self, ctx: &Context, key: &K, anchor: &K) -> bool {
        let Some(anchor_idx) = self.position(anchor) else {
            return false;
        };
        let from = match self.position(key) {
            Some(i) => i,
            None => return false,
        };
        let target = if from <= anchor_idx {
            anchor_idx
        } else {
            anchor_idx + 1
        };
        self.move_to(ctx, key, target)
    }

    /// Reactive entry count. Subscribes the caller to membership changes only.
    pub fn len(&self, ctx: &Context) -> usize {
        let _ = ctx.get_cell(&self.inner.membership);
        self.inner.order.borrow().len()
    }

    /// Reactive emptiness check. Subscribes the caller to membership changes.
    pub fn is_empty(&self, ctx: &Context) -> bool {
        self.len(ctx) == 0
    }

    /// Reactive membership test for `key`. Subscribes the caller to membership
    /// changes (add/remove of any key), not to value changes.
    pub fn contains_key(&self, ctx: &Context, key: &K) -> bool {
        let _ = ctx.get_cell(&self.inner.membership);
        self.inner.entries.borrow().contains_key(key)
    }

    /// Non-reactive count. Does not subscribe the caller to anything.
    pub fn len_untracked(&self) -> usize {
        self.inner.order.borrow().len()
    }
}

/// A parameterized factory of reactive cells, keyed by `K` (à la Recoil/Jotai
/// `atomFamily`). The factory lazily mints and caches one [`CellHandle<V>`] per
/// distinct key; repeated [`get`](CellFamily::get)s of the same key return the
/// same cell.
///
/// Built on top of [`CellMap`], so membership is reactive and entries are
/// fine-grained. Cheap to [`Clone`].
///
/// ```
/// use lazily::{CellFamily, Context};
///
/// let ctx = Context::new();
/// // One counter cell per id, defaulting to the id itself.
/// let counters: CellFamily<u32, u32> = CellFamily::new(&ctx, |&id| id);
/// let a = counters.get(&ctx, 7);
/// assert_eq!(a.get(&ctx), 7);
/// // Same key -> same cell.
/// assert_eq!(counters.get(&ctx, 7).get(&ctx), 7);
/// ```
pub struct CellFamily<K, V> {
    map: CellMap<K, V>,
    factory: Rc<dyn Fn(&K) -> V>,
}

impl<K, V> Clone for CellFamily<K, V> {
    fn clone(&self) -> Self {
        Self {
            map: self.map.clone(),
            factory: Rc::clone(&self.factory),
        }
    }
}

impl<K, V> CellFamily<K, V>
where
    K: Eq + Hash + Clone + 'static,
    V: PartialEq + Clone + 'static,
{
    /// Create a family whose entries are produced by `factory` on first access.
    pub fn new(ctx: &Context, factory: impl Fn(&K) -> V + 'static) -> Self {
        Self {
            map: CellMap::new(ctx),
            factory: Rc::new(factory),
        }
    }

    /// Get (minting on first access via the factory) the cell for `key`.
    pub fn get(&self, ctx: &Context, key: K) -> CellHandle<V> {
        let factory = Rc::clone(&self.factory);
        let k = key.clone();
        self.map.entry_with(ctx, key, move || factory(&k))
    }

    /// Borrow the underlying [`CellMap`] for membership/iteration APIs.
    pub fn map(&self) -> &CellMap<K, V> {
        &self.map
    }

    /// Remove `key` from the family (see [`CellMap::remove`]).
    pub fn remove(&self, ctx: &Context, key: &K) -> bool {
        self.map.remove(ctx, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_caches_one_cell_per_key() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        let a1 = map.entry(&ctx, "a", 1);
        let a2 = map.entry(&ctx, "a", 999);
        // Same key -> same cell; the second default is ignored.
        assert_eq!(a1.id, a2.id);
        assert_eq!(a1.get(&ctx), 1);
        assert_eq!(map.len_untracked(), 1);
    }

    #[test]
    fn get_or_insert_with_mints_once_then_returns_existing() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        let mut calls = 0;
        // First access mints via the factory.
        assert_eq!(
            map.get_or_insert_with(&ctx, "a", |_| {
                calls += 1;
                7
            }),
            7
        );
        assert_eq!(map.len_untracked(), 1);
        // Second access returns the existing value; factory is NOT called again.
        assert_eq!(
            map.get_or_insert_with(&ctx, "a", |_| {
                calls += 1;
                999
            }),
            7
        );
        assert_eq!(calls, 1);
        // An explicit set is observed by a subsequent get_or_insert_with.
        map.set(&ctx, "a", 42);
        assert_eq!(map.get_or_insert_with(&ctx, "a", |_| 0), 42);
    }

    #[test]
    fn membership_is_reactive_but_value_changes_are_not() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        let a = map.entry(&ctx, "a", 1);
        map.entry(&ctx, "b", 2);

        let count = ctx.computed({
            let map = map.clone();
            move |ctx| map.len(ctx)
        });
        assert_eq!(ctx.get(&count), 2);

        // Mutating an existing entry must NOT invalidate the membership reader.
        a.set(&ctx, 100);
        assert!(ctx.is_set(&count), "membership reader stayed cached");
        assert_eq!(ctx.get(&count), 2);

        // Adding a key DOES invalidate it.
        map.entry(&ctx, "c", 3);
        assert_eq!(ctx.get(&count), 3);

        // Removing a key invalidates it too.
        assert!(map.remove(&ctx, &"b"));
        assert_eq!(ctx.get(&count), 2);
        assert_eq!(map.keys(&ctx), vec!["a", "c"]);
    }

    #[test]
    fn per_entry_reads_are_independent() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        let a = map.entry(&ctx, "a", 1);
        let b = map.entry(&ctx, "b", 2);

        let view_a = ctx.computed({
            let map = map.clone();
            move |ctx| map.get(ctx, &"a").unwrap_or(0) * 10
        });
        assert_eq!(ctx.get(&view_a), 10);

        // Changing b must not invalidate a's reader.
        b.set(&ctx, 222);
        assert!(ctx.is_set(&view_a), "sibling change must not invalidate");
        assert_eq!(ctx.get(&view_a), 10);

        // Changing a does.
        a.set(&ctx, 5);
        assert_eq!(ctx.get(&view_a), 50);
    }

    #[test]
    fn family_mints_via_factory_and_caches() {
        let ctx = Context::new();
        let fam: CellFamily<u32, u32> = CellFamily::new(&ctx, |&k| k * 2);
        let c7 = fam.get(&ctx, 7);
        assert_eq!(c7.get(&ctx), 14);
        // Same key -> same cell (factory not re-run / value preserved).
        c7.set(&ctx, 100);
        assert_eq!(fam.get(&ctx, 7).get(&ctx), 100);
        assert_eq!(fam.map().len_untracked(), 1);
    }

    #[test]
    fn move_to_reorders_keys_and_keeps_cell_identity() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        let a = map.entry(&ctx, "a", 1);
        map.entry(&ctx, "b", 2);
        map.entry(&ctx, "c", 3);
        assert_eq!(map.keys(&ctx), vec!["a", "b", "c"]);

        // Move "c" to the front.
        assert!(map.move_to(&ctx, &"c", 0));
        assert_eq!(map.keys(&ctx), vec!["c", "a", "b"]);

        // The moved entry keeps the SAME value cell (identity + value intact).
        assert_eq!(map.handle(&"a").unwrap().id, a.id);
        assert_eq!(map.get(&ctx, &"a"), Some(1));
        assert_eq!(map.get(&ctx, &"c"), Some(3));

        // Absent key -> false, no reorder.
        assert!(!map.move_to(&ctx, &"z", 0));
        assert_eq!(map.keys(&ctx), vec!["c", "a", "b"]);
    }

    #[test]
    fn pure_move_invalidates_order_but_not_membership_readers() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        map.entry(&ctx, "a", 1);
        map.entry(&ctx, "b", 2);
        map.entry(&ctx, "c", 3);

        let order_reader = ctx.computed({
            let map = map.clone();
            move |ctx| map.keys(ctx).join(",")
        });
        let count = ctx.computed({
            let map = map.clone();
            move |ctx| map.len(ctx)
        });
        let has_b = ctx.computed({
            let map = map.clone();
            move |ctx| map.contains_key(ctx, &"b")
        });
        assert_eq!(ctx.get(&order_reader), "a,b,c");
        assert_eq!(ctx.get(&count), 3);
        assert!(ctx.get(&has_b));

        // A pure reorder must invalidate the order reader...
        assert!(map.move_to(&ctx, &"a", 2));
        assert_eq!(ctx.get(&order_reader), "b,c,a");
        // ...but NOT the set-identity readers (len / contains_key stay cached).
        assert!(
            ctx.is_set(&count),
            "len reader must stay cached on pure move"
        );
        assert!(
            ctx.is_set(&has_b),
            "contains_key reader must stay cached on pure move"
        );
        assert_eq!(ctx.get(&count), 3);
    }

    #[test]
    fn move_to_is_noop_when_position_unchanged() {
        let ctx = Context::new();
        let map: CellMap<&str, i32> = CellMap::new(&ctx);
        map.entry(&ctx, "a", 1);
        map.entry(&ctx, "b", 2);

        let order_reader = ctx.computed({
            let map = map.clone();
            move |ctx| map.keys(ctx).join(",")
        });
        assert_eq!(ctx.get(&order_reader), "a,b");

        // Moving to its current index is a no-op and must not invalidate.
        assert!(map.move_to(&ctx, &"a", 0));
        assert!(
            ctx.is_set(&order_reader),
            "no-op move must not invalidate keys readers"
        );
        // Index past the end clamps to last position.
        assert!(map.move_to(&ctx, &"a", 99));
        assert_eq!(ctx.get(&order_reader), "b,a");
    }

    #[test]
    fn move_before_and_after_place_relative_to_anchor() {
        let ctx = Context::new();
        let map: CellMap<i32, i32> = CellMap::new(&ctx);
        for k in 0..4 {
            map.entry(&ctx, k, k * 10);
        }
        assert_eq!(map.keys(&ctx), vec![0, 1, 2, 3]);

        // Move 3 before 1.
        assert!(map.move_before(&ctx, &3, &1));
        assert_eq!(map.keys(&ctx), vec![0, 3, 1, 2]);

        // Move 0 after 2.
        assert!(map.move_after(&ctx, &0, &2));
        assert_eq!(map.keys(&ctx), vec![3, 1, 2, 0]);

        // Unknown anchor / key -> false.
        assert!(!map.move_before(&ctx, &3, &99));
        assert!(!map.move_after(&ctx, &99, &2));
    }

    #[test]
    fn contains_key_tracks_membership() {
        let ctx = Context::new();
        let map: CellMap<i32, i32> = CellMap::new(&ctx);
        let has_5 = ctx.computed({
            let map = map.clone();
            move |ctx| map.contains_key(ctx, &5)
        });
        assert!(!ctx.get(&has_5));
        map.entry(&ctx, 5, 50);
        assert!(ctx.get(&has_5));
    }
}
