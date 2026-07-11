//! The unified keyed reactive family ([`ReactiveFamily`]) and its materialization
//! mode (`#lzmatmode`).
//!
//! `lazily-spec/cell-model.md` § "The `ReactiveFamily` vehicle" fixes a **keyed
//! reactive family** that maps keys `K` to per-entry reactive nodes and abstracts
//! over the entry's **handle kind** via a type parameter (`ReactiveFamily<K, V,
//! H>`). The handle kind is the [`FamilyHandle`] trait, implemented by both
//! [`CellHandle`] (input cells) and [`SlotHandle`] (derived slots):
//!
//! - **Cell entries** ([`EntryKind::Cell`], `H = CellHandle`) are **input** nodes.
//!   An input has no derivation to defer, so it is **always materialized**
//!   regardless of mode. The keyed cell collection ([`CellFamily`](crate::CellFamily))
//!   is this input-cell specialization.
//! - **Slot entries** ([`EntryKind::Slot`], `H = SlotHandle`) are **derived**
//!   nodes. These are what materialization mode governs.
//!
//! # Materialization mode
//!
//! Materialization mode is **orthogonal** to cell kind: it fixes *when a derived
//! cell's backing node is allocated*, never what it computes or how it converges,
//! and it MUST NOT be observable through any cell's value.
//!
//! - [`MaterializationMode::Eager`] (**default**) — every derived node is
//!   allocated when the family is built. A read is a direct node access.
//! - [`MaterializationMode::Lazy`] (opt-in) — a derived node is allocated on its
//!   **first read** ("materialize on pull"), addressed by key. A never-read
//!   derived cell is never allocated. Lazy is a keyed overlay on the eager core,
//!   not a second engine: the first read of key `k` builds the *same* node the
//!   eager build would have, then caches it.
//!
//! Entry kind is orthogonal to mode (proved in `lazily-formal`'s `Materialization`
//! module as `cell_entries_materialized_in_every_mode` /
//! `slot_entries_deferred_under_lazy`): choosing lazy defers only slot entries,
//! never cell entries. Observational transparency
//! (`observe (build eager s) id = observe (build lazy s) id = s.val id`) holds:
//! mode changes allocation timing and memory, never observed values.
//!
//! ```
//! use lazily::{Context, MaterializationMode, ReactiveFamily, SlotHandle};
//!
//! let ctx = Context::new();
//! // A derived (slot) family of key*3 over a large keyed address space, built
//! // lazily: nothing is allocated until a key is read.
//! let fam: ReactiveFamily<u32, u32, SlotHandle<u32>> =
//!     ReactiveFamily::lazy(&ctx, 0..1_000_000, |&k| k * 3);
//! assert_eq!(fam.present_count(), 0);
//!
//! // First read of a key materializes just that entry ("materialize on pull").
//! assert_eq!(fam.observe(&ctx, 5), 15);
//! assert_eq!(fam.present_count(), 1);
//! assert!(fam.is_present(&5));
//! assert!(!fam.is_present(&6));
//!
//! // Eager builds the same values up front — observationally identical.
//! let eager: ReactiveFamily<u32, u32, SlotHandle<u32>> =
//!     ReactiveFamily::eager(&ctx, 0..4, |&k| k * 3);
//! assert_eq!(eager.mode(), MaterializationMode::Eager);
//! assert_eq!(eager.present_count(), 4);
//! assert_eq!(eager.observe(&ctx, 2), fam.observe(&ctx, 2));
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::Context;
use crate::cell::CellHandle;
use crate::slot::SlotHandle;

/// Which kind of reactive node a [`ReactiveFamily`] entry is — the handle-kind
/// axis the family abstracts over, kept orthogonal to [`MaterializationMode`].
///
/// Mirrors `EntryKind` in `lazily-formal`'s `Materialization` module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// An **input** cell ([`CellHandle`]) — always materialized, any mode.
    Cell,
    /// A **derived** slot ([`SlotHandle`]) — materialized eagerly, or lazily on
    /// first read.
    Slot,
}

/// When a [`ReactiveFamily`]'s derived (slot) entries are allocated. Orthogonal
/// to [`EntryKind`]; never observable on the value axis.
///
/// Mirrors `Mode` in `lazily-formal`'s `Materialization` module. The default is
/// [`Eager`](MaterializationMode::Eager) (`Mode.default = Mode.eager`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaterializationMode {
    /// Allocate every derived node up front at build time. The shared
    /// high-performance core and the required default.
    #[default]
    Eager,
    /// Allocate a derived node on its first read, keyed rather than
    /// handle-addressed. An opt-in overlay on the eager core.
    Lazy,
}

mod sealed {
    pub trait Sealed {}
}

/// The entry-handle axis a [`ReactiveFamily`] abstracts over. Implemented by
/// [`CellHandle`] (input cells) and [`SlotHandle`] (derived slots) only — the
/// two node kinds of the cell model. Sealed: bindings do not add new kinds.
pub trait FamilyHandle<V>: sealed::Sealed + Copy + 'static {
    /// This handle's entry kind. `CellHandle` is [`EntryKind::Cell`] (always
    /// materialized); `SlotHandle` is [`EntryKind::Slot`] (mode-governed).
    const KIND: EntryKind;

    /// Allocate the node for one entry in `ctx`, with `compute` producing its
    /// canonical value. An input cell sets the value directly; a derived slot
    /// wraps `compute` as its recomputation.
    fn materialize(ctx: &Context, compute: impl Fn(&Context) -> V + 'static) -> Self
    where
        V: PartialEq + Clone + 'static;

    /// Read this entry's value through its owning context (subscribes the caller
    /// as any cell/slot read does).
    fn observe(self, ctx: &Context) -> V
    where
        V: Clone + 'static;
}

impl<V> sealed::Sealed for CellHandle<V> {}
impl<V: 'static> FamilyHandle<V> for CellHandle<V> {
    const KIND: EntryKind = EntryKind::Cell;

    fn materialize(ctx: &Context, compute: impl Fn(&Context) -> V + 'static) -> Self
    where
        V: PartialEq + Clone + 'static,
    {
        // An input has no derivation: materialize by setting its value directly.
        ctx.cell(compute(ctx))
    }

    fn observe(self, ctx: &Context) -> V
    where
        V: Clone + 'static,
    {
        ctx.get_cell(&self)
    }
}

impl<V> sealed::Sealed for SlotHandle<V> {}
impl<V: 'static> FamilyHandle<V> for SlotHandle<V> {
    const KIND: EntryKind = EntryKind::Slot;

    fn materialize(ctx: &Context, compute: impl Fn(&Context) -> V + 'static) -> Self
    where
        V: PartialEq + Clone + 'static,
    {
        // A derived node: the same node an eager build would allocate.
        ctx.computed(compute)
    }

    fn observe(self, ctx: &Context) -> V
    where
        V: Clone + 'static,
    {
        ctx.get(&self)
    }
}

struct FamilyInner<K, V, H> {
    mode: MaterializationMode,
    /// Canonical per-key value producer (a derived slot's recompute; an input
    /// cell's initial value).
    factory: Rc<dyn Fn(&K) -> V>,
    /// Currently-allocated entries (the "present" set). Grows on materialize,
    /// never shrinks silently — deferral, not de-allocation.
    materialized: RefCell<HashMap<K, H>>,
    /// Insertion order of the present set (stable snapshot for `present_keys`).
    order: RefCell<Vec<K>>,
}

/// The unified keyed reactive family (`#lzmatmode`): keys `K` map to per-entry
/// reactive nodes of handle kind `H` ([`CellHandle<V>`] for input cells,
/// [`SlotHandle<V>`] for derived slots), allocated per its [`MaterializationMode`].
///
/// Cheap to [`Clone`] (an `Rc` to shared inner state) so it can be captured by
/// compute/effect closures. Operations run against the owning [`Context`], like
/// the rest of `lazily`.
///
/// See the module docs for the eager/lazy contract and the
/// [`CellFamily`](crate::CellFamily) input-cell specialization.
pub struct ReactiveFamily<K, V, H> {
    inner: Rc<FamilyInner<K, V, H>>,
}

impl<K, V, H> Clone for ReactiveFamily<K, V, H> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<K, V, H> ReactiveFamily<K, V, H>
where
    K: Eq + Hash + Clone + 'static,
    V: PartialEq + Clone + 'static,
    H: FamilyHandle<V>,
{
    fn build(
        ctx: &Context,
        mode: MaterializationMode,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + 'static,
    ) -> Self {
        let fam = Self {
            inner: Rc::new(FamilyInner {
                mode,
                factory: Rc::new(factory),
                materialized: RefCell::new(HashMap::new()),
                order: RefCell::new(Vec::new()),
            }),
        };
        for key in keys {
            // buildEager materializes every node; buildLazy materializes only
            // input cells (`present := isInput`). A cell entry is always
            // materialized regardless of mode; a slot entry only under eager.
            if H::KIND == EntryKind::Cell || mode == MaterializationMode::Eager {
                fam.materialize_key(ctx, key);
            }
        }
        fam
    }

    /// Build an **eager** family: every declared key's node is allocated now.
    /// This is the default mode ([`MaterializationMode::Eager`]).
    pub fn eager(
        ctx: &Context,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + 'static,
    ) -> Self {
        Self::build(ctx, MaterializationMode::Eager, keys, factory)
    }

    /// Build a **lazy** family: derived (slot) entries are deferred to first
    /// read; input (cell) entries in `keys` are still materialized at build
    /// (cells are always materialized). Pass an empty `keys` for a purely
    /// on-demand slot family.
    pub fn lazy(
        ctx: &Context,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + 'static,
    ) -> Self {
        Self::build(ctx, MaterializationMode::Lazy, keys, factory)
    }

    /// Build a family in the **default** mode (eager). Alias for [`eager`](Self::eager).
    pub fn new(
        ctx: &Context,
        keys: impl IntoIterator<Item = K>,
        factory: impl Fn(&K) -> V + 'static,
    ) -> Self {
        Self::eager(ctx, keys, factory)
    }

    fn materialize_key(&self, ctx: &Context, key: K) -> H {
        if let Some(handle) = self.inner.materialized.borrow().get(&key) {
            return *handle; // warm: already allocated.
        }
        let factory = Rc::clone(&self.inner.factory);
        let k = key.clone();
        let handle = H::materialize(ctx, move |_ctx| factory(&k));
        self.inner
            .materialized
            .borrow_mut()
            .insert(key.clone(), handle);
        self.inner.order.borrow_mut().push(key);
        handle
    }

    /// Get the entry handle for `key`, materializing it on first access (the
    /// lazy pull) and caching it. Under eager mode an entry is already present,
    /// so this returns the cached handle.
    pub fn get(&self, ctx: &Context, key: K) -> H {
        self.materialize_key(ctx, key)
    }

    /// Observe `key`'s value — the headline transparency law: the returned value
    /// is identical under either mode. Materializes the entry if absent.
    pub fn observe(&self, ctx: &Context, key: K) -> V {
        self.get(ctx, key).observe(ctx)
    }

    /// Whether `key` is currently materialized (present in the allocated set).
    /// Non-reactive.
    pub fn is_present(&self, key: &K) -> bool {
        self.inner.materialized.borrow().contains_key(key)
    }

    /// The currently-materialized keys, in first-materialization order. The
    /// present set only grows (deferral, not de-allocation).
    pub fn present_keys(&self) -> Vec<K> {
        self.inner.order.borrow().clone()
    }

    /// Number of currently-materialized entries.
    pub fn present_count(&self) -> usize {
        self.inner.order.borrow().len()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `default_mode_eager`: the default materialization mode is eager.
    #[test]
    fn default_mode_is_eager() {
        assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);
    }

    /// `eager_materializes_all`: eager allocates every declared node up front.
    #[test]
    fn eager_materializes_all_up_front() {
        let ctx = Context::new();
        let fam: ReactiveFamily<u32, u32, SlotHandle<u32>> =
            ReactiveFamily::eager(&ctx, [0, 1, 2, 5, 9], |&k| k * 3);
        assert_eq!(fam.present_count(), 5);
        for k in [0u32, 1, 2, 5, 9] {
            assert!(fam.is_present(&k));
        }
    }

    /// `lazy_defers_slots`: lazy leaves an unread derived slot unallocated.
    #[test]
    fn lazy_defers_slots_until_read() {
        let ctx = Context::new();
        let fam: ReactiveFamily<u32, u32, SlotHandle<u32>> =
            ReactiveFamily::lazy(&ctx, [0, 1, 2, 5, 9], |&k| k * 3);
        assert_eq!(fam.present_count(), 0);
        assert!(!fam.is_present(&5));

        // First read materializes just that key ("materialize on pull").
        assert_eq!(fam.observe(&ctx, 5), 15);
        assert!(fam.is_present(&5));
        assert_eq!(fam.present_keys(), vec![5]);
    }

    /// `eager_lazy_observationally_equivalent` / `observe_canonical`: identical
    /// values under either mode.
    #[test]
    fn eager_and_lazy_observe_identically() {
        let ctx = Context::new();
        let eager: ReactiveFamily<u32, u32, SlotHandle<u32>> =
            ReactiveFamily::eager(&ctx, [0, 1, 2, 5, 9], |&k| k * 3);
        let lazy: ReactiveFamily<u32, u32, SlotHandle<u32>> =
            ReactiveFamily::lazy(&ctx, [0, 1, 2, 5, 9], |&k| k * 3);
        for k in [0u32, 1, 2, 5, 9] {
            assert_eq!(eager.observe(&ctx, k), lazy.observe(&ctx, k));
        }
    }

    /// `materialize_present_monotone`: re-reading a key does not change the
    /// present set; the set only grows.
    #[test]
    fn present_set_is_monotone_across_reads() {
        let ctx = Context::new();
        let fam: ReactiveFamily<u32, u32, SlotHandle<u32>> =
            ReactiveFamily::lazy(&ctx, [1, 2, 3, 4, 5], |&k| k * 2);
        let mut sizes = Vec::new();
        for k in [2u32, 4, 2, 5] {
            fam.observe(&ctx, k);
            sizes.push(fam.present_count());
        }
        // Re-reading 2 does not re-materialize; sizes are non-decreasing.
        assert_eq!(sizes, vec![1, 2, 2, 3]);
        assert_eq!(fam.present_keys(), vec![2, 4, 5]);
    }

    /// `cell_entries_materialized_in_every_mode`: an input-cell family is fully
    /// materialized at build under **either** mode.
    #[test]
    fn cell_family_materialized_in_every_mode() {
        let ctx = Context::new();
        for mode_lazy in [false, true] {
            let keys = ["a", "b", "c"];
            let fam: ReactiveFamily<&str, u32, CellHandle<u32>> = if mode_lazy {
                ReactiveFamily::lazy(&ctx, keys, |_| 0)
            } else {
                ReactiveFamily::eager(&ctx, keys, |_| 0)
            };
            assert_eq!(fam.entry_kind(), EntryKind::Cell);
            // Cells are always present at build, even under lazy.
            assert_eq!(fam.present_count(), 3);
        }
    }

    /// Cell entries are writable inputs (materialized-by-set), distinct from
    /// derived slots.
    #[test]
    fn cell_family_entries_are_writable_inputs() {
        let ctx = Context::new();
        let fam: ReactiveFamily<u32, u32, CellHandle<u32>> =
            ReactiveFamily::eager(&ctx, [7], |&k| k);
        let h = fam.get(&ctx, 7);
        assert_eq!(h.get(&ctx), 7);
        h.set(&ctx, 100);
        assert_eq!(fam.observe(&ctx, 7), 100);
    }
}
