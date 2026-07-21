//! The Cell kernel (`#lzcellkernel`) — `SourceCell` / `FormulaCell` over a single
//! genus `Cell<T, K>`.
//!
//! See `tasks/software/lazily-cell-kernel-design.md`. One node type with a
//! **kind** type parameter `K` replaces the former `SlotHandle` / `CellHandle` /
//! `SignalHandle` / `MergeCellHandle` handle zoo and the vestigial `Reactive<T>`
//! / `Source<T>` read/write traits:
//!
//! ```text
//! Cell<T, K>                    genus — a node with a readable value
//! ├─ SourceCell<T, M>           written from outside; folds under policy M
//! └─ FormulaCell<T>             computed from upstream
//! ```
//!
//! Both aliases answer the same question — *where does a node's value come
//! from* — so the pair is exhaustive: `SourceCell` from outside, `FormulaCell`
//! from upstream. `Effect` stays outside the hierarchy (a sink, no value).
//!
//! ## Write protection without a trait (§3)
//!
//! Reads live on `impl<T, K> Cell<T, K>` (every kind reads via [`get`]). Writes
//! live on `impl<T, M: MergePolicy<T>> Cell<T, Source<M>>` — so `set`/`merge`
//! exist **only** on the source instantiation, and `formula.set(…)` is a
//! *"no method found"* compile error with no trait in sight. The merge policy
//! `M` lives inside `Source<M>`, exactly where writes exist.
//!
//! A `SourceCell` reads and writes; a `FormulaCell` only reads:
//!
//! ```
//! use lazily::Context;
//! let ctx = Context::new();
//! let n = ctx.source(1i32);                 // SourceCell<i32>
//! n.set(&ctx, 2);                           // ok — `set` lives on the source kind
//! let doubled = ctx.formula(move |c| n.get(c) * 2).drive(&ctx);
//! assert_eq!(doubled.get(&ctx), 4);
//! ```
//!
//! Writing a formula is a compile error — no trait involved, just a missing
//! method on `Cell<T, Formula>`:
//!
//! ```compile_fail
//! use lazily::Context;
//! let ctx = Context::new();
//! let f = ctx.formula(|_| 1i32);            // FormulaCell<i32>
//! f.set(&ctx, 2);                           // ERROR[E0599]: no method named `set`
//! ```
//!
//! [`get`]: Cell::get

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::Context;
use crate::KeepLatest;
use crate::context::SlotId;
use crate::effect::EffectHandle;
use crate::merge::MergePolicy;

// ---------------------------------------------------------------------------
// Kind markers
// ---------------------------------------------------------------------------

/// Kind marker for a **source** cell — a node written from outside, folding
/// accumulated writes under merge policy `M`. Carries the policy so it exists
/// exactly where writes exist (`set`/`merge` on `Cell<T, Source<M>>`).
///
/// This is the marker that reuses the name of the former `Source<T>` *trait*
/// (now deleted): a `Source` is graph-theoretically a node with no incoming
/// edges, and API-wise the writable kind.
pub struct Source<M>(PhantomData<M>);

/// Kind marker for a **formula** cell — a node computed from upstream. A driven
/// formula (`formula().drive()`) is still this kind; drivenness is graph state,
/// not a distinct type.
pub struct Formula;

// ---------------------------------------------------------------------------
// The genus
// ---------------------------------------------------------------------------

/// A typed handle to a reactive node within a [`Context`] — the genus of the
/// kernel. Lightweight: just a recycled [`SlotId`] into the arena; the value
/// lives inside the `Context`.
///
/// Two handles are equal when they address the same underlying node — the
/// observable cell-identity contract behind atomic moves (`#lzcellmove`) and
/// keyed reconciliation.
pub struct Cell<T, K> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<(T, K)>,
}

/// A cell written from outside, folding writes under policy `M`
/// (default [`KeepLatest`], i.e. last-writer-wins replace). `SourceCell<T>` is a
/// plain input cell; `SourceCell<T, Sum>` folds additively; etc.
pub type SourceCell<T, M = KeepLatest> = Cell<T, Source<M>>;

/// A cell computed from upstream. Lazy by default; `formula().drive()` makes it
/// eager (a driven formula).
pub type FormulaCell<T> = Cell<T, Formula>;

impl<T, K> Cell<T, K> {
    pub(crate) fn from_id(id: SlotId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Read this cell's current value through its owning context.
    ///
    /// Every kind reads the same way. Registers a dependency when called inside
    /// a reactive computation (a formula compute or an effect run).
    pub fn get(&self, ctx: &Context) -> T
    where
        T: Clone + 'static,
    {
        ctx.read_value::<T>(self.id)
    }

    /// Tear this node down: detach both edge directions, invalidate surviving
    /// readers, and recycle the id. Kind-agnostic — dispatches on the node's own
    /// kind. Disposing a driven formula also tears down its puller.
    pub fn dispose(&self, ctx: &Context)
    where
        T: 'static,
    {
        ctx.dispose_node(self.id);
    }

    /// Run `on_change` now and again on every change to this value. Returns the
    /// backing [`EffectHandle`]; dispose it to unsubscribe.
    ///
    /// Replaces the former `Reactive::subscribe` default method.
    pub fn subscribe(
        &self,
        ctx: &Context,
        on_change: impl FnMut(&Context, &T) + 'static,
    ) -> EffectHandle
    where
        T: Clone + 'static,
        K: 'static,
    {
        let this = *self;
        let cb = RefCell::new(on_change);
        ctx.effect(move |c| {
            let v = this.get(c);
            (cb.borrow_mut())(c, &v);
        })
    }
}

// -- Source-only writes (§3) ------------------------------------------------

impl<T, M: MergePolicy<T>> Cell<T, Source<M>> {
    /// Replace the value outright (the keep-latest write). Only a `SourceCell`
    /// has this method; `formula.set(…)` does not compile.
    pub fn set(&self, ctx: &Context, value: T)
    where
        T: PartialEq + 'static,
    {
        ctx.set_source::<T>(self.id, value);
    }

    /// Fold `op` into the current value under policy `M`. For `KeepLatest` this
    /// is a replace (`Cell ≡ MergeCell<KeepLatest>`).
    pub fn merge(&self, ctx: &Context, op: T)
    where
        T: PartialEq + Clone + 'static,
    {
        ctx.merge_source::<T, M>(self.id, op);
    }

    /// The plain (keep-latest) view of this source cell, for wiring into derived
    /// readers that want a policy-erased handle. Same underlying node.
    ///
    /// Compatibility shim for the former `MergeCellHandle::cell()`.
    pub fn cell(&self) -> SourceCell<T> {
        Cell::from_id(self.id)
    }

    /// Clear all dependent formulas without changing this cell's value.
    pub fn clear_dependents(&self, ctx: &Context) {
        ctx.clear_cell_dependents(self.id);
    }
}

// -- Formula-only lifecycle -------------------------------------------------

impl<T> Cell<T, Formula> {
    /// Read this formula's current value as `Rc<T>`, avoiding a deep clone.
    pub fn get_rc(&self, ctx: &Context) -> Rc<T>
    where
        T: 'static,
    {
        ctx.get_rc(self)
    }

    /// Clear this formula's cached value and recursively clear all dependents.
    /// It will recompute on the next read.
    pub fn clear(&self, ctx: &Context) {
        ctx.clear_slot(self.id);
        ctx.flush_effects_after_invalidation();
    }

    /// **Drive** this formula: make it eager. Attaches a puller [`Effect`] that
    /// re-materializes the formula after every invalidation, so the value goes
    /// directly `v1 -> v2` with no intermediate unset state.
    ///
    /// Idempotent — a second `drive` is a no-op — and returns the **same**
    /// handle (mutated graph state), so the caller keeps reading the formula it
    /// already holds. This is the eager construction that retires the former
    /// `Signal`; the coalescing comes from the scheduler (effects are scheduled,
    /// not inline), so a per-write puller cannot be built.
    ///
    /// [`Effect`]: crate::EffectHandle
    pub fn drive(&self, ctx: &Context) -> Self
    where
        T: 'static,
    {
        ctx.drive_formula::<T>(self.id);
        *self
    }

    /// Reverse of [`drive`](Cell::drive): stop eager recomputation and dispose
    /// the puller. The value remains readable and reverts to lazy (recomputed on
    /// next read). No-op if the formula is not driven.
    pub fn undrive(&self, ctx: &Context) {
        ctx.undrive_formula(self.id);
    }

    /// Whether this formula is currently driven (has an active puller).
    pub fn is_driven(&self, ctx: &Context) -> bool {
        ctx.is_driven(self.id)
    }
}

// -- Shared value-independent trait impls (defined once on the genus) --------

impl<T, K> Clone for Cell<T, K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, K> Copy for Cell<T, K> {}

impl<T, K> PartialEq for Cell<T, K> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T, K> Eq for Cell<T, K> {}

impl<T, K> std::fmt::Debug for Cell<T, K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cell").field("id", &self.id).finish()
    }
}
