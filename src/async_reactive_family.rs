//! Async keyed reactive family (`#lzmatmode`, async flavor).
//!
//! The [`AsyncContext`] analog of [`ReactiveFamily`](crate::ReactiveFamily): keys `K`
//! map to per-entry async reactive nodes ([`AsyncCellHandle<V>`] input cells /
//! [`AsyncSlotHandle<V>`] derived slots) allocated per the family's
//! [`MaterializationMode`]. Like [`ThreadSafeReactiveFamily`](crate::ThreadSafeReactiveFamily)
//! it keeps its present-set state behind an `Arc<Mutex<..>>` (the [`AsyncContext`] is
//! itself `Send + Sync`), so it can live in a cross-task owner.
//!
//! The eager/lazy contract and present-set monotonicity are identical to the
//! single-threaded family. The transparency law is **eventual**: an async derived
//! slot read is `None` while pending and resolves to the canonical value — so
//! `observe` returns [`Option<V>`]. Input cells are always resolved
//! (`observe` returns `Some`). Drive a slot to resolution with
//! [`AsyncContext::get_async`] on the handle from [`AsyncSlotFamily::get`].
//!
//! To keep the three families API-parallel the per-key factory is the same sync
//! `Fn(&K) -> V` as the sync/thread-safe families; a derived slot wraps it in a
//! ready future. Mirrors the async materialization case in lazily-spec and the
//! `AsyncMaterialization` proofs (eventual transparency) in lazily-formal.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use crate::reactive_family::{EntryKind, MaterializationMode};
use crate::{AsyncCellHandle, AsyncContext, AsyncSlotHandle};

mod sealed {
    pub trait Sealed {}
}

/// The node kinds an async family entry can take — the [`AsyncContext`] analog of
/// [`FamilyHandle`](crate::FamilyHandle). Sealed to [`AsyncCellHandle`] (input cells)
/// and [`AsyncSlotHandle`] (derived slots).
pub trait AsyncFamilyHandle<V>: sealed::Sealed + Copy + Send + Sync + 'static {
    /// This handle's entry kind. `AsyncCellHandle` is [`EntryKind::Cell`] (always
    /// materialized, always resolved); `AsyncSlotHandle` is [`EntryKind::Slot`]
    /// (mode-governed, resolves asynchronously).
    const KIND: EntryKind;

    /// Allocate the node for one entry on `ctx`. `compute` is the per-key value
    /// producer; a cell sets the value directly, a derived slot wraps it in a ready
    /// future as its async recomputation.
    fn materialize(ctx: &AsyncContext, compute: Arc<dyn Fn() -> V + Send + Sync>) -> Self
    where
        V: PartialEq + Clone + Send + Sync + 'static;

    /// Non-blocking read: `Some(value)` for a materialized cell or a resolved slot,
    /// `None` for a slot still pending. Drive a pending slot to resolution with
    /// [`AsyncContext::get_async`].
    fn observe(self, ctx: &AsyncContext) -> Option<V>
    where
        V: Clone + Send + Sync + 'static;
}

impl<V> sealed::Sealed for AsyncCellHandle<V> {}
impl<V: Send + Sync + 'static> AsyncFamilyHandle<V> for AsyncCellHandle<V> {
    const KIND: EntryKind = EntryKind::Cell;

    fn materialize(ctx: &AsyncContext, compute: Arc<dyn Fn() -> V + Send + Sync>) -> Self
    where
        V: PartialEq + Clone + Send + Sync + 'static,
    {
        ctx.cell(compute())
    }

    fn observe(self, ctx: &AsyncContext) -> Option<V>
    where
        V: Clone + Send + Sync + 'static,
    {
        Some(ctx.get_cell(&self))
    }
}

impl<V> sealed::Sealed for AsyncSlotHandle<V> {}
impl<V: Send + Sync + 'static> AsyncFamilyHandle<V> for AsyncSlotHandle<V> {
    const KIND: EntryKind = EntryKind::Slot;

    fn materialize(ctx: &AsyncContext, compute: Arc<dyn Fn() -> V + Send + Sync>) -> Self
    where
        V: PartialEq + Clone + Send + Sync + 'static,
    {
        // A derived node whose async recompute is a ready future of the sync value.
        ctx.computed_async(move |_actx| {
            let v = compute();
            async move { v }
        })
    }

    fn observe(self, ctx: &AsyncContext) -> Option<V>
    where
        V: Clone + Send + Sync + 'static,
    {
        ctx.get(&self)
    }
}

/// Present-set state, guarded by the family's `Mutex`.
struct FamilyState<K, H> {
    materialized: HashMap<K, H>,
    order: Vec<K>,
}

struct FamilyInner<K, V, H> {
    mode: MaterializationMode,
    factory: Arc<dyn Fn(&K) -> V + Send + Sync>,
    state: Mutex<FamilyState<K, H>>,
}

/// The async unified keyed reactive family (`#lzmatmode`): keys `K` map to per-entry
/// async reactive nodes of handle kind `H` ([`AsyncCellHandle<V>`] input cells,
/// [`AsyncSlotHandle<V>`] derived slots), allocated per its [`MaterializationMode`].
///
/// Cheap to [`Clone`] (an `Arc` to shared inner state) and `Send + Sync`. See the
/// module docs for the eager/lazy contract and the eventual-transparency law.
pub struct AsyncReactiveFamily<K, V, H> {
    inner: Arc<FamilyInner<K, V, H>>,
}

impl<K, V, H> Clone for AsyncReactiveFamily<K, V, H> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, V, H> AsyncReactiveFamily<K, V, H>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: PartialEq + Clone + Send + Sync + 'static,
    H: AsyncFamilyHandle<V>,
{
    fn build(
        ctx: &AsyncContext,
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
            if H::KIND == EntryKind::Cell || mode == MaterializationMode::Eager {
                fam.materialize_key(ctx, key);
            }
        }
        fam
    }

    /// Build an **eager** family (the default mode): every declared key allocated now.
    pub fn eager(
        ctx: &AsyncContext,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        Self::build(ctx, MaterializationMode::Eager, keys, factory)
    }

    /// Build a **lazy** family: derived (slot) entries deferred to first read; input
    /// cells still materialized at build.
    pub fn lazy(
        ctx: &AsyncContext,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        Self::build(ctx, MaterializationMode::Lazy, keys, factory)
    }

    /// Build a family in the **default** mode (eager). Alias for [`eager`](Self::eager).
    pub fn new(
        ctx: &AsyncContext,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + Send + Sync + 'static,
    ) -> Self {
        Self::eager(ctx, keys, factory)
    }

    fn materialize_key(&self, ctx: &AsyncContext, key: K) -> H {
        // Fast path under the lock; release before touching `ctx`.
        {
            let state = self
                .inner
                .state
                .lock()
                .expect("family state mutex poisoned");
            if let Some(handle) = state.materialized.get(&key) {
                return *handle;
            }
        }
        let factory = Arc::clone(&self.inner.factory);
        let k = key.clone();
        let compute: Arc<dyn Fn() -> V + Send + Sync> = Arc::new(move || factory(&k));
        let handle = H::materialize(ctx, compute);
        let mut state = self
            .inner
            .state
            .lock()
            .expect("family state mutex poisoned");
        // First writer wins on a race so the key keeps a stable handle.
        if let Some(existing) = state.materialized.get(&key) {
            return *existing;
        }
        state.materialized.insert(key.clone(), handle);
        state.order.push(key);
        handle
    }

    /// Get the entry handle for `key`, materializing it on first access. For a slot
    /// family this is the [`AsyncSlotHandle`] to drive with [`AsyncContext::get_async`].
    pub fn get(&self, ctx: &AsyncContext, key: K) -> H {
        self.materialize_key(ctx, key)
    }

    /// Non-blocking observe: `Some(value)` for a cell or resolved slot, `None` for a
    /// pending slot. The eventual-transparency law: once resolved, this equals the
    /// canonical value under either mode.
    pub fn observe(&self, ctx: &AsyncContext, key: K) -> Option<V> {
        self.get(ctx, key).observe(ctx)
    }

    /// Whether `key` is currently materialized (present). Non-reactive.
    pub fn is_present(&self, key: &K) -> bool {
        self.inner
            .state
            .lock()
            .expect("family state mutex poisoned")
            .materialized
            .contains_key(key)
    }

    /// The currently-materialized keys, in first-materialization order.
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

    /// This family's entry kind.
    pub fn entry_kind(&self) -> EntryKind {
        H::KIND
    }
}

/// An async **input-cell** family: every entry is an always-resolved
/// [`AsyncCellHandle<V>`].
pub type AsyncCellFamily<K, V> = AsyncReactiveFamily<K, V, AsyncCellHandle<V>>;

/// An async **derived-slot** family: entries are [`AsyncSlotHandle<V>`] governed by
/// the family's [`MaterializationMode`], resolved via [`AsyncContext::get_async`].
pub type AsyncSlotFamily<K, V> = AsyncReactiveFamily<K, V, AsyncSlotHandle<V>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn family_is_send_sync() {
        assert_send_sync::<AsyncCellFamily<u64, bool>>();
        assert_send_sync::<AsyncSlotFamily<u64, usize>>();
    }

    #[tokio::test]
    async fn eager_cell_family_resolves_immediately() {
        let ctx = AsyncContext::new();
        let fam: AsyncCellFamily<u64, bool> = AsyncReactiveFamily::eager(&ctx, [1, 2, 3], |_| true);
        assert_eq!(fam.entry_kind(), EntryKind::Cell);
        assert_eq!(fam.present_count(), 3);
        assert_eq!(fam.observe(&ctx, 2), Some(true));
        assert_eq!(fam.present_keys(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn lazy_slot_family_defers_until_read() {
        let ctx = AsyncContext::new();
        let fam: AsyncSlotFamily<u64, usize> =
            AsyncReactiveFamily::lazy(&ctx, [], |k| (*k as usize) * 10);
        assert_eq!(fam.mode(), MaterializationMode::Lazy);
        assert_eq!(fam.present_count(), 0);
        // Materialize + drive to resolution.
        let handle = fam.get(&ctx, 4);
        assert!(fam.is_present(&4));
        assert_eq!(fam.present_count(), 1);
        assert_eq!(ctx.get_async(&handle).await, 40);
    }

    #[tokio::test]
    async fn eventual_transparency_eager_equals_lazy() {
        let ctx_e = AsyncContext::new();
        let eager: AsyncSlotFamily<u64, usize> =
            AsyncReactiveFamily::eager(&ctx_e, [1, 2, 3], |k| (*k as usize) * 2);
        let ctx_l = AsyncContext::new();
        let lazy: AsyncSlotFamily<u64, usize> =
            AsyncReactiveFamily::lazy(&ctx_l, [1, 2, 3], |k| (*k as usize) * 2);
        for k in [1u64, 2, 3] {
            let ve = ctx_e.get_async(&eager.get(&ctx_e, k)).await;
            let vl = ctx_l.get_async(&lazy.get(&ctx_l, k)).await;
            assert_eq!(ve, vl);
        }
    }

    #[tokio::test]
    async fn present_set_grows_monotonically() {
        let ctx = AsyncContext::new();
        let fam: AsyncSlotFamily<u64, usize> = AsyncReactiveFamily::lazy(&ctx, [], |k| *k as usize);
        let _ = fam.get(&ctx, 5);
        let _ = fam.get(&ctx, 5);
        let _ = fam.get(&ctx, 9);
        assert_eq!(fam.present_count(), 2);
        assert_eq!(fam.present_keys(), vec![5, 9]);
    }

    #[tokio::test]
    async fn cell_family_reacts_to_set() {
        let ctx = AsyncContext::new();
        let fam: AsyncCellFamily<u64, bool> = AsyncReactiveFamily::eager(&ctx, [10, 20], |_| true);
        assert_eq!(fam.observe(&ctx, 20), Some(true));
        let h = fam.get(&ctx, 20);
        ctx.set_cell(&h, false);
        assert_eq!(fam.observe(&ctx, 20), Some(false));
    }
}
