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
    /// Reactive membership signal. Holds a monotonic version that is bumped on
    /// every add/remove; reading it (in `keys`/`len`/`contains_key`) subscribes
    /// the caller to membership changes without coupling to entry values.
    membership: CellHandle<u64>,
    /// Plain (untracked) mirror of the membership version so mutators can bump
    /// the reactive cell without registering a spurious dependency.
    version: StdCell<u64>,
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
            }),
        }
    }

    fn bump_membership(&self, ctx: &Context) {
        let next = self.inner.version.get().wrapping_add(1);
        self.inner.version.set(next);
        // A write, not a tracked read: membership readers are invalidated, but
        // no dependency is registered on whatever frame called the mutator.
        ctx.set_cell(&self.inner.membership, next);
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

    /// Reactive snapshot of the keys in insertion order. Subscribes the caller
    /// to membership changes (add/remove), not to per-entry value changes.
    pub fn keys(&self, ctx: &Context) -> Vec<K> {
        let _ = ctx.get_cell(&self.inner.membership);
        self.inner.order.borrow().clone()
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
