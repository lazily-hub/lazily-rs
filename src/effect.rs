use std::marker::PhantomData;

use crate::Context;
use crate::context::SlotId;

/// Return value accepted by [`Context::effect`].
///
/// Returning `()` registers no cleanup. Returning a closure registers that
/// closure as cleanup for the current effect run; it will run before the next
/// rerun and when the effect is disposed.
pub trait EffectCallbackResult {
    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>>;
}

impl EffectCallbackResult for () {
    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>> {
        None
    }
}

impl<F> EffectCallbackResult for F
where
    F: FnOnce() + 'static,
{
    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>> {
        Some(Box::new(self))
    }
}

/// A typed handle to an effect within a [`Context`].
///
/// Effects run immediately when created, automatically track any slots/cells
/// read during the run, and rerun after those dependencies are invalidated.
pub struct EffectHandle {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<()>,
}

impl EffectHandle {
    pub(crate) fn new(id: SlotId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Dispose this effect.
    ///
    /// Disposal unsubscribes the effect from its dependencies, removes any
    /// pending scheduled run, and runs the latest cleanup if one exists.
    pub fn dispose(&self, ctx: &Context) {
        ctx.dispose_effect(self);
    }

    /// Check whether this effect is still registered in the context.
    pub fn is_active(&self, ctx: &Context) -> bool {
        ctx.is_effect_active(self)
    }
}

impl Clone for EffectHandle {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for EffectHandle {}
