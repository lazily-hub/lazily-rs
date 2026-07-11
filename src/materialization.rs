//! Materialization mode — eager default / lazy opt-in (`#lzmatmode`).
//!
//! Cell *kind* (`Cell` / `Slot` / `Signal` / `merge:`) fixes **how a cell
//! converges**. **Materialization mode** is an *orthogonal* axis: it fixes
//! **when a derived cell's backing node is allocated** — not what it computes,
//! how it converges, or how it merges. It trades **memory + first-touch latency**
//! against **cold full-scan cost**, and MUST NOT be observable through the value
//! of any cell (`lazily-spec/cell-model.md` § "Materialization mode").
//!
//! Two modes, mirroring `lazily-formal`'s `Materialization` module:
//!
//! - **Eager (default).** Every derived cell's node is allocated when the graph
//!   is constructed. A read is a direct node access; a full recompute pays only
//!   compute (allocation already happened at build). Implementations MUST default
//!   to eager (`default_mode_eager`).
//! - **Lazy (opt-in).** A derived cell's node is allocated on its **first read**
//!   ("materialize on pull"), addressed by a **key** rather than a held handle. A
//!   never-read derived cell is never allocated. Lazy is a **keyed overlay on the
//!   eager core**, not a second graph engine: the first read of key `k`
//!   constructs the *same* node the eager build would have, then caches it.
//!
//! [`MaterializedFamily`] is the keyed-context constructor that exposes the
//! choice. Lazy is an **explicit per-construction opt-in** ([`MaterializedFamily::lazy`]),
//! never the default and never a per-read toggle on an eager handle. The derived
//! value is produced by a factory over the owning [`Context`] and a key, so each
//! entry is a real reactive slot (built on `ctx.memo`); *lazy evaluation*
//! (leaving off-viewport slots dirty until read) is provided in **both** modes
//! and is independent of materialization (§ "Reactivity is orthogonal").
//!
//! # Observational transparency (normative)
//!
//! For every key and every read, the observed value is identical under either
//! mode — mode changes allocation timing and memory, never results:
//!
//! ```text
//! observe(eager, key) = observe(lazy, key) = factory(key)   ∀ key
//! ```
//!
//! This mirrors `observe_canonical` / `eager_lazy_observationally_equivalent` in
//! `lazily-formal`. The consequences preserved here:
//!
//! 1. **Same values** — a lazy read returns the value an eager read would.
//! 2. **No churn from allocation** — materializing one node never changes another
//!    node's observed value.
//! 3. **Deferral, not de-allocation** — lazy only *grows* the materialized set,
//!    which is a subset of the eager set.
//! 4. **Reactivity is orthogonal** — both modes evaluate lazily; lazy
//!    materialization *additionally* defers allocation.
//!
//! ```
//! use lazily::{Context, MaterializationMode, MaterializedFamily};
//!
//! let ctx = Context::new();
//! // A derived value per key: the key doubled.
//! let factory = |_ctx: &Context, k: &u32| k * 2;
//!
//! // Eager (the default): every key materialized up front.
//! let eager = MaterializedFamily::eager(&ctx, [1u32, 2, 3], factory);
//! assert_eq!(eager.mode(), MaterializationMode::Eager);
//! assert!(eager.is_materialized(&2));
//! assert_eq!(eager.materialized_count(), 3);
//!
//! // Lazy (opt-in): nothing materialized until first read.
//! let lazy = MaterializedFamily::lazy(&ctx, factory);
//! assert!(!lazy.is_materialized(&2));
//! assert_eq!(lazy.observe(&ctx, &2), 4); // materialize on pull
//! assert!(lazy.is_materialized(&2));
//! assert!(!lazy.is_materialized(&1)); // never read -> never allocated
//!
//! // Observational transparency: identical read values under either mode.
//! assert_eq!(eager.observe(&ctx, &2), lazy.observe(&ctx, &2));
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::Context;
use crate::slot::SlotHandle;

/// Materialization strategy for a keyed family of derived cells.
///
/// `Eager` is the shared high-performance core and the **required default**
/// (`Default` yields `Eager`); `Lazy` is the opt-in keyed overlay. See the
/// [module docs](self) and `lazily-formal`'s `Materialization.Mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterializationMode {
    /// Allocate every derived node at build time (the default).
    Eager,
    /// Allocate a derived node on its first read ("materialize on pull").
    Lazy,
}

impl MaterializationMode {
    /// The default materialization mode. Implementations MUST default to eager
    /// (`default_mode_eager`).
    pub const DEFAULT: MaterializationMode = MaterializationMode::Eager;

    /// True when this is the lazy (opt-in, deferred) mode.
    #[inline]
    pub fn is_lazy(self) -> bool {
        matches!(self, MaterializationMode::Lazy)
    }
}

impl Default for MaterializationMode {
    /// Eager is the default mode (`default_mode_eager`).
    fn default() -> Self {
        MaterializationMode::DEFAULT
    }
}

/// Erased factory: derive a key's value from the owning context and the key.
type FactoryFn<K, V> = dyn Fn(&Context, &K) -> V;

/// A keyed family of derived cells with an explicit **materialization mode**
/// (`#lzmatmode`).
///
/// Each key maps to a derived reactive slot produced by a factory
/// `Fn(&Context, &K) -> V`. Under [`MaterializationMode::Eager`] every key given
/// at construction is materialized up front; under [`MaterializationMode::Lazy`]
/// a key's slot is materialized on its first [`observe`](Self::observe) /
/// [`handle`](Self::handle). The two modes are **observationally transparent**:
/// a read returns the same value regardless of mode.
///
/// This is a **keyed overlay on the eager `Context` core**, not a second engine:
/// materializing a key mints exactly the `ctx.memo` slot the eager build would
/// have. Cheap to [`Clone`] (an `Rc` to shared inner state) so it can be captured
/// by compute/effect closures.
pub struct MaterializedFamily<K, V> {
    inner: Rc<Inner<K, V>>,
}

struct Inner<K, V> {
    /// Materialized derived slots, keyed. A key absent here is *not yet
    /// allocated* — the lazy "never touched" case (`present = false`).
    slots: RefCell<HashMap<K, SlotHandle<V>>>,
    /// Derives a key's value. Pure w.r.t. materialization: invoked lazily by the
    /// slot's `memo` compute, so *when* the slot was allocated never changes what
    /// it computes.
    factory: Rc<FactoryFn<K, V>>,
    mode: MaterializationMode,
}

impl<K, V> Clone for MaterializedFamily<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<K, V> MaterializedFamily<K, V>
where
    K: Eq + Hash + Clone + 'static,
    V: PartialEq + Clone + 'static,
{
    /// Construct a family under an explicit mode.
    ///
    /// `keys` seeds the derivable key space; it is materialized up front under
    /// [`MaterializationMode::Eager`] and **ignored** under
    /// [`MaterializationMode::Lazy`] (lazy allocates on first read). Prefer the
    /// intention-revealing [`eager`](Self::eager) / [`lazy`](Self::lazy)
    /// constructors; this generic form mirrors `build : Mode → Spec → Mat`.
    pub fn new<I, F>(ctx: &Context, mode: MaterializationMode, keys: I, factory: F) -> Self
    where
        I: IntoIterator<Item = K>,
        F: Fn(&Context, &K) -> V + 'static,
    {
        let family = Self {
            inner: Rc::new(Inner {
                slots: RefCell::new(HashMap::new()),
                factory: Rc::new(factory),
                mode,
            }),
        };
        if !mode.is_lazy() {
            // Eager build: allocate every node up front (`eager_materializes_all`).
            for key in keys {
                family.materialize(ctx, &key);
            }
        }
        family
    }

    /// Construct an **eager** family: materialize a derived slot for every key in
    /// `keys` at build time (`eager_materializes_all`). This is the default mode.
    pub fn eager<I, F>(ctx: &Context, keys: I, factory: F) -> Self
    where
        I: IntoIterator<Item = K>,
        F: Fn(&Context, &K) -> V + 'static,
    {
        Self::new(ctx, MaterializationMode::Eager, keys, factory)
    }

    /// Construct a **lazy** family: no derived slot is allocated until its first
    /// read (`lazy_defers_slots`). This is the explicit, keyed opt-in — never the
    /// default and never a per-read toggle on an eager handle.
    pub fn lazy<F>(_ctx: &Context, factory: F) -> Self
    where
        F: Fn(&Context, &K) -> V + 'static,
    {
        Self {
            inner: Rc::new(Inner {
                slots: RefCell::new(HashMap::new()),
                factory: Rc::new(factory),
                mode: MaterializationMode::Lazy,
            }),
        }
    }

    /// The materialization mode this family was constructed with.
    #[inline]
    pub fn mode(&self) -> MaterializationMode {
        self.inner.mode
    }

    /// Materialize `key` if absent — minting exactly the `ctx.memo` slot the
    /// eager build would have — and return its handle. A warm call returns the
    /// cached handle unchanged (`materialize` idempotence).
    fn materialize(&self, ctx: &Context, key: &K) -> SlotHandle<V> {
        if let Some(handle) = self.inner.slots.borrow().get(key) {
            return *handle;
        }
        let factory = Rc::clone(&self.inner.factory);
        let k = key.clone();
        // A real reactive derived slot: `memo` applies the PartialEq guard and
        // computes lazily on first read (lazy *evaluation*, orthogonal to
        // materialization mode).
        let handle = ctx.memo(move |ctx| factory(ctx, &k));
        self.inner.slots.borrow_mut().insert(key.clone(), handle);
        handle
    }

    /// Get the derived slot handle for `key`, materializing it on first access.
    ///
    /// Under lazy mode this is the "materialize on pull" step. The returned
    /// handle is a lightweight id into the owning [`Context`].
    pub fn handle(&self, ctx: &Context, key: &K) -> SlotHandle<V> {
        self.materialize(ctx, key)
    }

    /// **Observe** `key`: materialize its slot if absent (the lazy pull), then
    /// return its value. The headline transparency law — the observed value is
    /// identical under either mode (`observe_canonical`).
    pub fn observe(&self, ctx: &Context, key: &K) -> V {
        let handle = self.materialize(ctx, key);
        ctx.get(&handle)
    }

    /// Whether `key`'s node is currently allocated (the formal `present`).
    ///
    /// This does **not** materialize and does **not** register a reactive
    /// dependency — it is a plain introspection read for the deferral / memory
    /// laws (`materialize_present_monotone`, `lazy_present_subset_eager`).
    pub fn is_materialized(&self, key: &K) -> bool {
        self.inner.slots.borrow().contains_key(key)
    }

    /// How many keys are currently materialized (the size of the present set).
    /// Eager families report every seeded key; lazy families report only what
    /// has been read.
    pub fn materialized_count(&self) -> usize {
        self.inner.slots.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default mode is eager (`default_mode_eager`).
    #[test]
    fn default_mode_is_eager() {
        assert_eq!(MaterializationMode::default(), MaterializationMode::Eager);
        assert_eq!(MaterializationMode::DEFAULT, MaterializationMode::Eager);
        assert!(!MaterializationMode::Eager.is_lazy());
        assert!(MaterializationMode::Lazy.is_lazy());
    }

    /// Eager allocates every seeded node up front (`eager_materializes_all`).
    #[test]
    fn eager_materializes_all() {
        let ctx = Context::new();
        let fam = MaterializedFamily::eager(&ctx, [1u32, 2, 3, 4], |_ctx, k| k * 10);
        assert_eq!(fam.mode(), MaterializationMode::Eager);
        for k in [1u32, 2, 3, 4] {
            assert!(fam.is_materialized(&k), "eager key {k} allocated at build");
        }
        assert_eq!(fam.materialized_count(), 4);
    }

    /// Lazy leaves an unread derived slot unallocated (`lazy_defers_slots`).
    #[test]
    fn lazy_defers_slots() {
        let ctx = Context::new();
        let fam = MaterializedFamily::lazy(&ctx, |_ctx, k: &u32| k * 10);
        assert_eq!(fam.mode(), MaterializationMode::Lazy);
        assert_eq!(fam.materialized_count(), 0);
        assert!(!fam.is_materialized(&2));

        // First read materializes on pull.
        assert_eq!(fam.observe(&ctx, &2), 20);
        assert!(fam.is_materialized(&2));
        assert_eq!(fam.materialized_count(), 1);

        // A never-read key stays unallocated.
        assert!(!fam.is_materialized(&3));
    }

    /// A read yields the factory value under *either* mode (`observe_canonical`,
    /// `eager_lazy_observationally_equivalent`).
    #[test]
    fn eager_lazy_observationally_equivalent() {
        let ctx = Context::new();
        let factory = |_ctx: &Context, k: &u32| k * 3 + 1;
        let keys = [0u32, 1, 2, 5, 9];
        let eager = MaterializedFamily::eager(&ctx, keys, factory);
        let lazy = MaterializedFamily::lazy(&ctx, factory);
        for k in keys {
            let expected = k * 3 + 1;
            assert_eq!(eager.observe(&ctx, &k), expected);
            assert_eq!(lazy.observe(&ctx, &k), expected);
            assert_eq!(eager.observe(&ctx, &k), lazy.observe(&ctx, &k));
        }
    }

    /// Materializing one node never changes another node's observed value
    /// (`materialize_preserves_observe` — no churn from allocation).
    #[test]
    fn materialize_preserves_observe() {
        let ctx = Context::new();
        let fam = MaterializedFamily::lazy(&ctx, |_ctx, k: &u32| k * 7);
        let before = fam.observe(&ctx, &4); // materializes 4
        // Materialize an unrelated node.
        let _ = fam.observe(&ctx, &9);
        assert_eq!(fam.observe(&ctx, &4), before);
    }

    /// Lazy only *grows* the present set and it is a subset of the eager set
    /// (`materialize_present_monotone`, `lazy_present_subset_eager`).
    #[test]
    fn lazy_present_monotone_subset_of_eager() {
        let ctx = Context::new();
        let keys = [1u32, 2, 3, 4, 5];
        let factory = |_ctx: &Context, k: &u32| k * 2;
        let eager = MaterializedFamily::eager(&ctx, keys, factory);
        let lazy = MaterializedFamily::lazy(&ctx, factory);

        // Read a subset, checking monotone growth (never shrinks).
        let mut last = 0;
        for k in [2u32, 4] {
            let _ = lazy.observe(&ctx, &k);
            assert!(lazy.materialized_count() >= last, "present set is monotone");
            last = lazy.materialized_count();
            // Re-observing does not grow the set (idempotent materialize).
            let _ = lazy.observe(&ctx, &k);
            assert_eq!(lazy.materialized_count(), last, "warm read is idempotent");
        }

        // Every lazily-present key is also present in the eager build (subset).
        for k in keys {
            if lazy.is_materialized(&k) {
                assert!(eager.is_materialized(&k), "lazy present ⊆ eager present");
            }
        }
        assert_eq!(eager.materialized_count(), keys.len());
    }

    /// Reactivity is orthogonal: a lazily-materialized slot still tracks its
    /// input cell and recomputes on change (§ "Reactivity is orthogonal").
    #[test]
    fn reactivity_is_orthogonal_to_materialization() {
        let ctx = Context::new();
        let base = ctx.cell(10i32);
        let fam = MaterializedFamily::lazy(&ctx, move |ctx, k: &i32| ctx.get_cell(&base) + k);

        assert_eq!(fam.observe(&ctx, &1), 11);
        ctx.set_cell(&base, 100);
        // The already-materialized slot recomputes against the new input.
        assert_eq!(fam.observe(&ctx, &1), 101);
    }

    /// Handles are stable: the same key returns the same slot id (a keyed
    /// overlay, not a fresh node per read).
    #[test]
    fn same_key_same_slot() {
        let ctx = Context::new();
        let fam = MaterializedFamily::lazy(&ctx, |_ctx, k: &u32| k + 1);
        let h1 = fam.handle(&ctx, &7);
        let h2 = fam.handle(&ctx, &7);
        assert_eq!(h1.id, h2.id);
        assert_eq!(fam.materialized_count(), 1);
    }
}
