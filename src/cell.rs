//! The Cell kernel (`#lzcellkernel`) — the two concrete handle structs
//! [`Source`] and [`Computed`].
//!
//! See `tasks/software/lazily-cell-kernel-design.md`. **`Cell` is a conceptual
//! word, not a type**: a *cell* is a value-bearing reactive node, and the two
//! kinds of cell are named by the two handle structs a caller holds:
//!
//! ```text
//! Source<T, M>      handle to a source cell — written from outside; folds under policy M
//! Computed<T>       handle to a computed cell — computed from upstream
//! ```
//!
//! Both answer the same question — *where does a node's value come from* — so
//! the pair is exhaustive: `Source` from outside, `Computed` from upstream.
//! `Effect` stays outside the hierarchy (a sink, no value). There is **no
//! `Cell<T, K>` genus struct**: the former genus dissolves into these two
//! concrete structs, and the former `Source<M>` / `Formula` *kind markers* are
//! gone — `M` is now [`Source`]'s own policy parameter.
//!
//! ## Write protection without a trait (§3)
//!
//! Reads (`get`, `subscribe`, `dispose`) exist on both handles. Writes
//! (`set`/`merge`) exist **only** on [`Source`]. So `computed.set(…)` is a
//! *"no method found"* compile error with no trait in sight, and the merge
//! policy `M` lives on `Source<T, M>`, exactly where writes exist.
//!
//! A `Source` reads and writes; a `Computed` only reads:
//!
//! ```
//! use lazily::Context;
//! let ctx = Context::new();
//! let n = ctx.source(1i32);                 // Source<i32>
//! n.set(&ctx, 2);                           // ok — `set` lives on the source handle
//! let doubled = ctx.computed(move |c| n.get(c) * 2).drive(&ctx);
//! assert_eq!(doubled.get(&ctx), 4);
//! ```
//!
//! Writing a computed cell is a compile error — no trait involved, just a
//! missing method on [`Computed`]:
//!
//! ```compile_fail
//! use lazily::Context;
//! let ctx = Context::new();
//! let f = ctx.computed(|_| 1i32);           // Computed<i32>
//! f.set(&ctx, 2);                           // ERROR[E0599]: no method named `set`
//! ```

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::Context;
use crate::KeepLatest;
use crate::context::SlotId;
use crate::effect::EffectHandle;
use crate::merge::MergePolicy;

// ---------------------------------------------------------------------------
// Source — the source-cell handle
// ---------------------------------------------------------------------------

/// A typed handle to a **source cell** within a [`Context`] — a node written
/// from outside, folding accumulated writes under merge policy `M` (default
/// [`KeepLatest`], i.e. last-writer-wins replace). `Source<T>` is a plain input
/// cell; `Source<T, Sum>` folds additively; etc.
///
/// Lightweight: just a recycled [`SlotId`] into the arena; the value lives
/// inside the `Context`. Two handles are equal when they address the same
/// underlying node — the observable cell-identity contract behind atomic moves
/// (`#lzcellmove`) and keyed reconciliation.
pub struct Source<T, M = KeepLatest> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<(T, M)>,
}

impl<T, M> Source<T, M> {
    pub(crate) fn from_id(id: SlotId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Read this cell's current value through its owning context.
    ///
    /// Registers a dependency when called inside a reactive computation (a
    /// computed-cell compute or an effect run).
    pub fn get(&self, ctx: &Context) -> T
    where
        T: Clone + 'static,
    {
        ctx.read_value::<T>(self.id)
    }

    /// Tear this node down: detach both edge directions, invalidate surviving
    /// readers, and recycle the id.
    pub fn dispose(&self, ctx: &Context)
    where
        T: 'static,
    {
        ctx.dispose_node(self.id);
    }

    /// Run `on_change` now and again on every change to this value. Returns the
    /// backing [`EffectHandle`]; dispose it to unsubscribe.
    pub fn subscribe(
        &self,
        ctx: &Context,
        on_change: impl FnMut(&Context, &T) + 'static,
    ) -> EffectHandle
    where
        T: Clone + 'static,
        M: 'static,
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

impl<T, M: MergePolicy<T>> Source<T, M> {
    /// Replace the value outright (the keep-latest write). Only a [`Source`]
    /// has this method; `computed.set(…)` does not compile.
    pub fn set(&self, ctx: &Context, value: T)
    where
        T: PartialEq + 'static,
    {
        ctx.set_source::<T>(self.id, value);
    }

    /// Fold `op` into the current value under policy `M`. For `KeepLatest` this
    /// is a replace (`Source ≡ Source<T, KeepLatest>`).
    pub fn merge(&self, ctx: &Context, op: T)
    where
        T: PartialEq + Clone + 'static,
    {
        ctx.merge_source::<T, M>(self.id, op);
    }

    /// The plain (keep-latest) view of this source cell, for wiring into derived
    /// readers that want a policy-erased handle. Same underlying node.
    pub fn cell(&self) -> Source<T> {
        Source::from_id(self.id)
    }

    /// Clear all dependent computed cells without changing this cell's value.
    pub fn clear_dependents(&self, ctx: &Context) {
        ctx.clear_cell_dependents(self.id);
    }
}

impl<T, M> Clone for Source<T, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, M> Copy for Source<T, M> {}

impl<T, M> PartialEq for Source<T, M> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T, M> Eq for Source<T, M> {}

impl<T, M> std::fmt::Debug for Source<T, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source").field("id", &self.id).finish()
    }
}

// ---------------------------------------------------------------------------
// Computed — the computed-cell handle
// ---------------------------------------------------------------------------

/// A typed handle to a **computed cell** within a [`Context`] — a node computed
/// from upstream. Lazy by default; `computed().drive()` makes it eager (a driven
/// computed cell).
///
/// Lightweight: just a recycled [`SlotId`] into the arena. Two handles are equal
/// when they address the same underlying node.
pub struct Computed<T> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<T>,
}

impl<T> Computed<T> {
    pub(crate) fn from_id(id: SlotId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Read this computed cell's current value through its owning context.
    ///
    /// Registers a dependency when called inside a reactive computation.
    pub fn get(&self, ctx: &Context) -> T
    where
        T: Clone + 'static,
    {
        ctx.read_value::<T>(self.id)
    }

    /// Read this computed cell's current value as `Rc<T>`, avoiding a deep clone.
    pub fn get_rc(&self, ctx: &Context) -> Rc<T>
    where
        T: 'static,
    {
        ctx.get_rc(self)
    }

    /// Tear this node down: detach both edge directions, invalidate surviving
    /// readers, and recycle the id. Disposing a driven computed cell also tears
    /// down its puller.
    pub fn dispose(&self, ctx: &Context)
    where
        T: 'static,
    {
        ctx.dispose_node(self.id);
    }

    /// Run `on_change` now and again on every change to this value. Returns the
    /// backing [`EffectHandle`]; dispose it to unsubscribe.
    pub fn subscribe(
        &self,
        ctx: &Context,
        on_change: impl FnMut(&Context, &T) + 'static,
    ) -> EffectHandle
    where
        T: Clone + 'static,
    {
        let this = *self;
        let cb = RefCell::new(on_change);
        ctx.effect(move |c| {
            let v = this.get(c);
            (cb.borrow_mut())(c, &v);
        })
    }

    /// Clear this computed cell's cached value and recursively clear all
    /// dependents. It will recompute on the next read.
    pub fn clear(&self, ctx: &Context) {
        ctx.clear_slot(self.id);
        ctx.flush_effects_after_invalidation();
    }

    /// **Drive** this computed cell: make it eager. Attaches a puller [`Effect`]
    /// that re-materializes it after every invalidation, so the value goes
    /// directly `v1 -> v2` with no intermediate unset state.
    ///
    /// Idempotent — a second `drive` is a no-op — and returns the **same**
    /// handle (mutated graph state), so the caller keeps reading the computed
    /// cell it already holds. This is the eager construction that retires the
    /// former `Signal`; the coalescing comes from the scheduler (effects are
    /// scheduled, not inline), so a per-write puller cannot be built.
    ///
    /// [`Effect`]: crate::EffectHandle
    pub fn drive(&self, ctx: &Context) -> Self
    where
        T: 'static,
    {
        ctx.drive_formula::<T>(self.id);
        *self
    }

    /// Reverse of [`drive`](Computed::drive): stop eager recomputation and
    /// dispose the puller. The value remains readable and reverts to lazy
    /// (recomputed on next read). No-op if the computed cell is not driven.
    pub fn undrive(&self, ctx: &Context) {
        ctx.undrive_formula(self.id);
    }

    /// Whether this computed cell is currently driven (has an active puller).
    pub fn is_driven(&self, ctx: &Context) -> bool {
        ctx.is_driven(self.id)
    }
}

impl<T> Clone for Computed<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Computed<T> {}

impl<T> PartialEq for Computed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Computed<T> {}

impl<T> std::fmt::Debug for Computed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Computed").field("id", &self.id).finish()
    }
}
