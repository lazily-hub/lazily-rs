use std::marker::PhantomData;

use crate::Context;
use crate::context::SlotId;

/// A typed handle to a mutable cell within a [`Context`].
///
/// Like [`SlotHandle`], this is a lightweight id. The actual value lives
/// inside the Context.
pub struct CellHandle<T> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<T>,
}

impl<T> CellHandle<T> {
    pub(crate) fn new(id: SlotId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Set this cell's value through its owning context.
    ///
    /// This is an ergonomic alias for [`Context::set_cell`].
    pub fn set(&self, ctx: &Context, value: T)
    where
        T: PartialEq + 'static,
    {
        ctx.set_cell(self, value);
    }

    /// Clear all dependent slots without changing the cell's value.
    ///
    /// Useful when you know derived caches are stale but the input hasn't
    /// changed (e.g., an external resource was mutated).
    pub fn clear_dependents(&self, ctx: &Context) {
        ctx.clear_cell_dependents(self.id);
    }
}

impl<T> Clone for CellHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for CellHandle<T> {}
