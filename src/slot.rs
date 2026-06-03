use std::marker::PhantomData;

use crate::Context;
use crate::context::SlotId;

/// A typed handle to a lazily-computed slot within a [`Context`].
///
/// The handle itself is lightweight (just an id) and does not own the data.
/// All data lives inside the Context.
pub struct SlotHandle<T> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<T>,
}

impl<T> SlotHandle<T> {
    pub(crate) fn new(id: SlotId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Get this slot's value through its owning context.
    ///
    /// This is an ergonomic alias for [`Context::get`].
    pub fn get(&self, ctx: &Context) -> T
    where
        T: Clone + 'static,
    {
        ctx.get(self)
    }

    /// Clear this slot's cached value and recursively clear all dependents.
    ///
    /// The slot will recompute on the next [`Context::get`] call.
    pub fn clear(&self, ctx: &Context) {
        ctx.clear_slot(self.id);
        ctx.flush_effects_after_invalidation();
    }
}

// Handles are Copy/Clone since they're just ids.
impl<T> Clone for SlotHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SlotHandle<T> {}
