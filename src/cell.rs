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
