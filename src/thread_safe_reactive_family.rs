//! Thread-safe keyed reactive family (`#lzmatmode`, thread-safe flavor).
//!
//! The `Send + Sync` analog of [`ReactiveFamily`](crate::ReactiveFamily): keys `K`
//! map to per-entry reactive nodes ([`CellHandle<V>`] input cells / [`SlotHandle<V>`]
//! derived slots) allocated on a [`ThreadSafeContext`] per the family's
//! [`MaterializationMode`]. Where [`ReactiveFamily`] is `Rc`-based and single-threaded,
//! this family keeps its present-set state behind an `Arc<Mutex<..>>`, so it can live
//! in a `Send` owner shared across threads (for example a relay hub stored behind a
//! global mutex, where an `Rc`-based family cannot go).
//!
//! It obeys the same three laws as the single-threaded family (see the
//! [`reactive_family`](crate::reactive_family) module docs):
//! - **Eager/lazy contract:** eager materializes every declared node at build; lazy
//!   defers derived (slot) nodes to first read. Cell entries are always materialized.
//! - **Observational transparency:** `observe(key)` returns an identical value under
//!   either mode.
//! - **Present-set monotonicity:** the materialized set only grows (deferral, never
//!   de-allocation).
//!
//! Mirrors the `ThreadSafeReactiveFamily` conformance case in lazily-spec and the
//! `Materialization` proofs in lazily-formal.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use crate::reactive_family::{EntryKind, MaterializationMode};
use crate::{CellHandle, SlotHandle, ThreadSafeContext};

mod sealed {
    pub trait Sealed {}
}

/// The node kinds a thread-safe family entry can take — the `Send + Sync` analog of
/// [`FamilyHandle`](crate::FamilyHandle). Sealed to [`CellHandle`] (input cells) and
/// [`SlotHandle`] (derived slots); bindings do not add new kinds.
pub trait ThreadSafeFamilyHandle<V>: sealed::Sealed + Copy + Send + Sync + 'static {
    /// This handle's entry kind. `CellHandle` is [`EntryKind::Cell`] (always
    /// materialized); `SlotHandle` is [`EntryKind::Slot`] (mode-governed).
    const KIND: EntryKind;

    /// Allocate the node for one entry on `ctx`, with `compute` producing its
    /// canonical value. An input cell sets the value directly; a derived slot wraps
    /// `compute` as its recomputation. The closure is `Send + Sync` so a slot's
    /// recompute can run on any thread the context is driven from.
    fn materialize(
        ctx: &ThreadSafeContext,
        compute: impl Fn(&ThreadSafeContext) -> V + Send + Sync + 'static,
    ) -> Self
    where
        V: PartialEq + Clone + Send + Sync + 'static;

    /// Read this entry's value through its owning context (subscribes the caller as
    /// any cell/slot read does).
    fn observe(self, ctx: &ThreadSafeContext) -> V
    where
        V: Clone + Send + Sync + 'static;
}

impl<V> sealed::Sealed for CellHandle<V> {}
impl<V: Send + Sync + 'static> ThreadSafeFamilyHandle<V> for CellHandle<V> {
    const KIND: EntryKind = EntryKind::Cell;

    fn materialize(
        ctx: &ThreadSafeContext,
        compute: impl Fn(&ThreadSafeContext) -> V + Send + Sync + 'static,
    ) -> Self
    where
        V: PartialEq + Clone + Send + Sync + 'static,
    {
        // An input has no derivation: materialize by setting its value directly.
        ctx.cell(compute(ctx))
    }

    fn observe(self, ctx: &ThreadSafeContext) -> V
    where
        V: Clone + Send + Sync + 'static,
    {
        ctx.get_cell(&self)
    }
}

impl<V> sealed::Sealed for SlotHandle<V> {}
impl<V: Send + Sync + 'static> ThreadSafeFamilyHandle<V> for SlotHandle<V> {
    const KIND: EntryKind = EntryKind::Slot;

    fn materialize(
        ctx: &ThreadSafeContext,
        compute: impl Fn(&ThreadSafeContext) -> V + Send + Sync + 'static,
    ) -> Self
    where
        V: PartialEq + Clone + Send + Sync + 'static,
    {
        // A derived node: the same node an eager build would allocate.
        ctx.computed(compute)
    }

    fn observe(self, ctx: &ThreadSafeContext) -> V
    where
        V: Clone + Send + Sync + 'static,
    {
        ctx.get(&self)
    }
}

/// Present-set state, guarded by the family's `Mutex`.
struct FamilyState<K, H> {
    /// Currently-allocated entries (the "present" set). Grows on materialize,
    /// never shrinks silently — deferral, not de-allocation.
    materialized: HashMap<K, H>,
    /// Insertion order of the present set (stable snapshot for `present_keys`).
    order: Vec<K>,
}

struct FamilyInner<K, V, H> {
    mode: MaterializationMode,
    /// Canonical per-key value producer (a derived slot's recompute; an input cell's
    /// initial value). `Send + Sync` so it can be shared across threads.
    factory: Arc<dyn Fn(&K) -> V + Send + Sync>,
    state: Mutex<FamilyState<K, H>>,
}

/// The thread-safe unified keyed reactive family (`#lzmatmode`): keys `K` map to
/// per-entry reactive nodes of handle kind `H` ([`CellHandle<V>`] for input cells,
/// [`SlotHandle<V>`] for derived slots), allocated per its [`MaterializationMode`].
///
/// Cheap to [`Clone`] (an `Arc` to shared inner state) and `Send + Sync`, so it can be
/// captured by compute/effect closures and stored in a cross-thread owner. Operations
/// run against the owning [`ThreadSafeContext`].
///
/// See the module docs for the eager/lazy contract and the
/// [`ThreadSafeCellFamily`]/[`ThreadSafeSlotFamily`] kind specializations.
pub struct ThreadSafeReactiveFamily<K, V, H> {
    inner: Arc<FamilyInner<K, V, H>>,
}

impl<K, V, H> Clone for ThreadSafeReactiveFamily<K, V, H> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, V, H> ThreadSafeReactiveFamily<K, V, H>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: PartialEq + Clone + Send + Sync + 'static,
    H: ThreadSafeFamilyHandle<V>,
{
    fn build(
        ctx: &ThreadSafeContext,
        mode: MaterializationMode,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        let fam = Self {
            inner: Arc::new(FamilyInner {
                mode,
                factory: Arc::new(factory),
                state: Mutex::new(FamilyState {
                    materialized: HashMap::new(),
                    order: Vec::new(),
                }),
            }),
        };
        for key in keys {
            // buildEager materializes every node; buildLazy materializes only input
            // cells (`present := isInput`). A cell entry is always materialized
            // regardless of mode; a slot entry only under eager.
            if H::KIND == EntryKind::Cell || mode == MaterializationMode::Eager {
                fam.materialize_key(ctx, key);
            }
        }
        fam
    }

    /// Build an **eager** family: every declared key's node is allocated now. This is
    /// the default mode ([`MaterializationMode::Eager`]).
    pub fn eager(
        ctx: &ThreadSafeContext,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        Self::build(ctx, MaterializationMode::Eager, keys, factory)
    }

    /// Build a **lazy** family: derived (slot) entries are deferred to first read;
    /// input (cell) entries in `keys` are still materialized at build. Pass an empty
    /// `keys` for a purely on-demand slot family.
    pub fn lazy(
        ctx: &ThreadSafeContext,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        Self::build(ctx, MaterializationMode::Lazy, keys, factory)
    }

    /// Build a family in the **default** mode (eager). Alias for [`eager`](Self::eager).
    pub fn new(
        ctx: &ThreadSafeContext,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        Self::eager(ctx, keys, factory)
    }

    fn materialize_key(&self, ctx: &ThreadSafeContext, key: K) -> H {
        // Fast path: already allocated. Release the lock before touching `ctx` so a
        // slot recompute triggered by materialization can never re-enter this lock.
        {
            let state = self
                .inner
                .state
                .lock()
                .expect("family state mutex poisoned");
            if let Some(handle) = state.materialized.get(&key) {
                return *handle; // warm: already allocated.
            }
        }
        let factory = Arc::clone(&self.inner.factory);
        let k = key.clone();
        let handle = H::materialize(ctx, move |_ctx| factory(&k));
        let mut state = self
            .inner
            .state
            .lock()
            .expect("family state mutex poisoned");
        // Lost a materialization race for this key: first writer wins so the key keeps
        // a stable handle (cell-identity). Our freshly-allocated node is orphaned in
        // `ctx` (unreferenced, never observed) — a rare, harmless cost.
        if let Some(existing) = state.materialized.get(&key) {
            return *existing;
        }
        state.materialized.insert(key.clone(), handle);
        state.order.push(key);
        handle
    }

    /// Get the entry handle for `key`, materializing it on first access (the lazy
    /// pull) and caching it. Under eager mode an entry is already present, so this
    /// returns the cached handle.
    pub fn get(&self, ctx: &ThreadSafeContext, key: K) -> H {
        self.materialize_key(ctx, key)
    }

    /// Observe `key`'s value — the transparency law: the returned value is identical
    /// under either mode. Materializes the entry if absent.
    pub fn observe(&self, ctx: &ThreadSafeContext, key: K) -> V {
        self.get(ctx, key).observe(ctx)
    }

    /// Whether `key` is currently materialized (present in the allocated set).
    /// Non-reactive.
    pub fn is_present(&self, key: &K) -> bool {
        self.inner
            .state
            .lock()
            .expect("family state mutex poisoned")
            .materialized
            .contains_key(key)
    }

    /// The currently-materialized keys, in first-materialization order. The present
    /// set only grows (deferral, not de-allocation).
    pub fn present_keys(&self) -> Vec<K> {
        self.inner
            .state
            .lock()
            .expect("family state mutex poisoned")
            .order
            .clone()
    }

    /// Number of currently-materialized entries.
    pub fn present_count(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("family state mutex poisoned")
            .order
            .len()
    }

    /// This family's materialization mode.
    pub fn mode(&self) -> MaterializationMode {
        self.inner.mode
    }

    /// This family's entry kind ([`EntryKind::Cell`] for a cell family,
    /// [`EntryKind::Slot`] for a slot family).
    pub fn entry_kind(&self) -> EntryKind {
        H::KIND
    }
}

/// A thread-safe **input-cell** family: every entry is an always-materialized
/// [`CellHandle<V>`]. The `Send + Sync` analog of [`CellFamily`](crate::CellFamily).
pub type ThreadSafeCellFamily<K, V> = ThreadSafeReactiveFamily<K, V, CellHandle<V>>;

/// A thread-safe **derived-slot** family: entries are [`SlotHandle<V>`] governed by
/// the family's [`MaterializationMode`].
pub type ThreadSafeSlotFamily<K, V> = ThreadSafeReactiveFamily<K, V, SlotHandle<V>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn family_is_send_sync() {
        // The whole point: a thread-safe family can live in a `Send + Sync` owner.
        assert_send_sync::<ThreadSafeCellFamily<u64, bool>>();
        assert_send_sync::<ThreadSafeSlotFamily<u64, usize>>();
    }

    #[test]
    fn default_mode_is_eager() {
        assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);
    }

    #[test]
    fn eager_cell_family_materializes_all_at_build() {
        let ctx = ThreadSafeContext::new();
        let fam: ThreadSafeCellFamily<u64, bool> =
            ThreadSafeReactiveFamily::eager(&ctx, [1, 2, 3], |_| true);
        assert_eq!(fam.entry_kind(), EntryKind::Cell);
        assert_eq!(fam.mode(), MaterializationMode::Eager);
        assert_eq!(fam.present_count(), 3);
        assert!(fam.is_present(&1) && fam.is_present(&2) && fam.is_present(&3));
        assert_eq!(fam.present_keys(), vec![1, 2, 3]);
    }

    #[test]
    fn lazy_slot_family_defers_until_read() {
        let ctx = ThreadSafeContext::new();
        // Empty declared keys + lazy → nothing materialized until observed.
        let fam: ThreadSafeSlotFamily<u64, usize> =
            ThreadSafeReactiveFamily::lazy(&ctx, [], |k| (*k as usize) * 10);
        assert_eq!(fam.mode(), MaterializationMode::Lazy);
        assert_eq!(fam.present_count(), 0);
        assert!(!fam.is_present(&2));
        assert_eq!(fam.observe(&ctx, 2), 20);
        assert!(fam.is_present(&2));
        assert_eq!(fam.present_count(), 1);
    }

    #[test]
    fn lazy_cell_entries_still_materialize_at_build() {
        let ctx = ThreadSafeContext::new();
        // Cells are always materialized regardless of mode.
        let fam: ThreadSafeCellFamily<u64, bool> =
            ThreadSafeReactiveFamily::lazy(&ctx, [7, 8], |_| false);
        assert_eq!(fam.present_count(), 2);
    }

    #[test]
    fn observational_transparency_eager_equals_lazy() {
        let ctx_e = ThreadSafeContext::new();
        let eager: ThreadSafeSlotFamily<u64, usize> =
            ThreadSafeReactiveFamily::eager(&ctx_e, [1, 2, 3], |k| (*k as usize) * 2);
        let ctx_l = ThreadSafeContext::new();
        let lazy: ThreadSafeSlotFamily<u64, usize> =
            ThreadSafeReactiveFamily::lazy(&ctx_l, [1, 2, 3], |k| (*k as usize) * 2);
        for k in [1u64, 2, 3] {
            assert_eq!(eager.observe(&ctx_e, k), lazy.observe(&ctx_l, k));
        }
    }

    #[test]
    fn present_set_grows_monotonically() {
        let ctx = ThreadSafeContext::new();
        let fam: ThreadSafeSlotFamily<u64, usize> =
            ThreadSafeReactiveFamily::lazy(&ctx, [], |k| *k as usize);
        let _ = fam.observe(&ctx, 5);
        let _ = fam.observe(&ctx, 5); // repeat: no growth
        let _ = fam.observe(&ctx, 9);
        assert_eq!(fam.present_count(), 2);
        assert_eq!(fam.present_keys(), vec![5, 9]);
    }

    #[test]
    fn derived_count_reacts_to_cell_writes() {
        // The agent-doc liveness shape: cell inputs + a derived count that recomputes
        // reactively when a cell flips — no pull-time scan.
        let ctx = ThreadSafeContext::new();
        let liveness: ThreadSafeCellFamily<u64, bool> =
            ThreadSafeReactiveFamily::eager(&ctx, [10, 20, 30], |_| true);
        let live_count = {
            let liveness = liveness.clone();
            ctx.computed(move |c| {
                liveness
                    .present_keys()
                    .into_iter()
                    .filter(|k| liveness.get(c, *k).observe(c))
                    .count()
            })
        };
        assert_eq!(ctx.get(&live_count), 3);
        // Flip one editor offline → derived count recomputes reactively.
        let h20 = liveness.get(&ctx, 20);
        ctx.set_cell(&h20, false);
        assert_eq!(ctx.get(&live_count), 2);
        ctx.set_cell(&h20, true);
        assert_eq!(ctx.get(&live_count), 3);
    }

    #[test]
    fn shared_across_threads() {
        use std::thread;
        let ctx = Arc::new(ThreadSafeContext::new());
        let fam: ThreadSafeCellFamily<u64, bool> =
            ThreadSafeReactiveFamily::eager(&ctx, [1, 2, 3, 4], |_| true);
        let handles: Vec<_> = (1u64..=4)
            .map(|k| {
                let fam = fam.clone();
                let ctx = Arc::clone(&ctx);
                thread::spawn(move || fam.observe(&ctx, k))
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap());
        }
        assert_eq!(fam.present_count(), 4);
    }
}
