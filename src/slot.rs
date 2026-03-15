use std::marker::PhantomData;

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
}

// Handles are Copy/Clone since they're just ids.
impl<T> Clone for SlotHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SlotHandle<T> {}
