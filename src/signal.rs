use std::rc::Rc;

use crate::Context;
use crate::effect::EffectHandle;
use crate::slot::SlotHandle;

/// A typed handle to an **eager** derived value within a [`Context`].
///
/// A `Signal` sits one step beyond [`SlotHandle`] on the `Slot -> Cell ->
/// Signal` progression:
///
/// - A [`SlotHandle`] (`ctx.computed`) is **lazy**: invalidation only marks it
///   dirty, and the value is not recomputed until the next read.
/// - A [`CellHandle`](crate::CellHandle) is an always-set mutable input.
/// - A `Signal` is an always-set *derived* value that **eagerly** recomputes
///   the instant any of its dependencies are invalidated.
///
/// Because it recomputes eagerly and is backed by a memoized slot, a signal
/// never exposes an intermediate "unset" value: a dependency change drives the
/// value directly from `v1` to `v2`. Reading a signal always returns a
/// materialized, up-to-date value.
///
/// Internally a signal is a memoized slot plus a small puller effect that
/// re-materializes the slot after every invalidation. The memo guard means a
/// recomputation that yields an equal value does not churn downstream
/// dependents.
pub struct SignalHandle<T> {
    /// Memoized backing slot that holds the derived value.
    pub(crate) slot: SlotHandle<T>,
    /// Puller effect that keeps `slot` eagerly materialized.
    pub(crate) effect: EffectHandle,
}

impl<T> SignalHandle<T> {
    pub(crate) fn new(slot: SlotHandle<T>, effect: EffectHandle) -> Self {
        Self { slot, effect }
    }

    /// Read this signal's current value through its owning context.
    ///
    /// This is an ergonomic alias for [`Context::get_signal`]. The value is
    /// always materialized; there is no unset state to observe.
    pub fn get(&self, ctx: &Context) -> T
    where
        T: Clone + 'static,
    {
        ctx.get_signal(self)
    }

    /// Read this signal's current value as `Rc<T>`, avoiding a deep clone.
    pub fn get_rc(&self, ctx: &Context) -> Rc<T>
    where
        T: 'static,
    {
        ctx.get_signal_rc(self)
    }

    /// Dispose this signal's eager puller.
    ///
    /// After disposal the signal stops eagerly recomputing on invalidation;
    /// the backing value remains readable and behaves like a lazy
    /// [`SlotHandle`] (recomputed on the next read).
    pub fn dispose(&self, ctx: &Context) {
        ctx.dispose_signal(self);
    }

    /// Check whether this signal's eager puller is still active.
    pub fn is_active(&self, ctx: &Context) -> bool {
        ctx.is_signal_active(self)
    }
}

// Handles are Copy/Clone since they're just ids.
impl<T> Clone for SignalHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SignalHandle<T> {}
