use std::marker::PhantomData;
use std::rc::Rc;

use crate::{CellHandle, Context, SlotHandle};

#[cfg(feature = "thread-safe")]
use crate::ThreadSafeContext;

/// A single-threaded lazily context tagged with a schema/context-family type.
///
/// The tag has no runtime cost. It prevents mixing slot and cell handles from
/// different schemas while reusing the existing [`Context`] graph.
pub struct TypedContext<Schema> {
    inner: Context,
    _schema: PhantomData<fn() -> Schema>,
}

/// Read-only typed context view passed into typed slot computations.
pub struct TypedContextRef<'a, Schema> {
    inner: &'a Context,
    _schema: PhantomData<fn() -> Schema>,
}

/// A slot handle bound to a typed context schema.
pub struct TypedSlotHandle<Schema, T> {
    raw: SlotHandle<T>,
    _schema: PhantomData<fn() -> Schema>,
}

/// A cell handle bound to a typed context schema.
pub struct TypedCellHandle<Schema, T> {
    raw: CellHandle<T>,
    _schema: PhantomData<fn() -> Schema>,
}

/// Context-like typed views that can memoize decorator-style slot/cell
/// factories.
///
/// Implemented for both [`TypedContext`] and [`TypedContextRef`], so a factory
/// can be called from ordinary code and from inside another slot callback while
/// returning the same context-local handle.
pub trait TypedFactoryContext {
    type Schema: 'static;

    fn memoized_slot<K, T, F>(&self, compute: F) -> TypedSlotHandle<Self::Schema, T>
    where
        K: 'static,
        T: 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Self::Schema>) -> T + 'static;

    fn memoized_cell<K, T, F>(&self, init: F) -> TypedCellHandle<Self::Schema, T>
    where
        K: 'static,
        T: PartialEq + 'static,
        F: for<'a> FnOnce(&TypedContextRef<'a, Self::Schema>) -> T;
}

#[cfg(feature = "thread-safe")]
/// A thread-safe lazily context tagged with a schema/context-family type.
pub struct TypedThreadSafeContext<Schema> {
    inner: ThreadSafeContext,
    _schema: PhantomData<fn() -> Schema>,
}

#[cfg(feature = "thread-safe")]
/// Read-only typed thread-safe context view passed into typed slot computations.
pub struct TypedThreadSafeContextRef<'a, Schema> {
    inner: &'a ThreadSafeContext,
    _schema: PhantomData<fn() -> Schema>,
}

#[cfg(feature = "thread-safe")]
/// A thread-safe slot handle bound to a typed context schema.
pub struct TypedThreadSafeSlotHandle<Schema, T> {
    raw: SlotHandle<T>,
    _schema: PhantomData<fn() -> Schema>,
}

#[cfg(feature = "thread-safe")]
/// A thread-safe cell handle bound to a typed context schema.
pub struct TypedThreadSafeCellHandle<Schema, T> {
    raw: CellHandle<T>,
    _schema: PhantomData<fn() -> Schema>,
}

impl<Schema> TypedContext<Schema> {
    pub fn new() -> Self {
        Self {
            inner: Context::new(),
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> &Context {
        &self.inner
    }

    pub fn as_ref(&self) -> TypedContextRef<'_, Schema> {
        TypedContextRef::new(&self.inner)
    }

    pub fn cell<T>(&self, value: T) -> TypedCellHandle<Schema, T>
    where
        T: PartialEq + 'static,
    {
        TypedCellHandle::new(self.inner.cell(value))
    }

    pub fn slot<T, F>(&self, compute: F) -> TypedSlotHandle<Schema, T>
    where
        T: 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Schema>) -> T + 'static,
    {
        let raw = self.inner.slot(move |ctx| {
            let typed = TypedContextRef::new(ctx);
            compute(&typed)
        });
        TypedSlotHandle::new(raw)
    }

    pub fn computed<T, F>(&self, compute: F) -> TypedSlotHandle<Schema, T>
    where
        T: 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Schema>) -> T + 'static,
    {
        self.slot(compute)
    }

    pub fn memo<T, F>(&self, compute: F) -> TypedSlotHandle<Schema, T>
    where
        T: PartialEq + 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Schema>) -> T + 'static,
    {
        let raw = self.inner.memo(move |ctx| {
            let typed = TypedContextRef::new(ctx);
            compute(&typed)
        });
        TypedSlotHandle::new(raw)
    }

    pub fn memoized_slot<K, T, F>(&self, compute: F) -> TypedSlotHandle<Schema, T>
    where
        K: 'static,
        T: 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Schema>) -> T + 'static,
        Schema: 'static,
    {
        <Self as TypedFactoryContext>::memoized_slot::<K, T, F>(self, compute)
    }

    pub fn memoized_cell<K, T, F>(&self, init: F) -> TypedCellHandle<Schema, T>
    where
        K: 'static,
        T: PartialEq + 'static,
        F: for<'a> FnOnce(&TypedContextRef<'a, Schema>) -> T,
        Schema: 'static,
    {
        <Self as TypedFactoryContext>::memoized_cell::<K, T, F>(self, init)
    }

    pub fn get<T>(&self, handle: &TypedSlotHandle<Schema, T>) -> T
    where
        T: Clone + 'static,
    {
        self.inner.get(&handle.raw)
    }

    pub fn get_rc<T>(&self, handle: &TypedSlotHandle<Schema, T>) -> Rc<T>
    where
        T: 'static,
    {
        self.inner.get_rc(&handle.raw)
    }

    pub fn get_cell<T>(&self, handle: &TypedCellHandle<Schema, T>) -> T
    where
        T: Clone + 'static,
    {
        self.inner.get_cell(&handle.raw)
    }

    pub fn get_cell_rc<T>(&self, handle: &TypedCellHandle<Schema, T>) -> Rc<T>
    where
        T: 'static,
    {
        self.inner.get_cell_rc(&handle.raw)
    }

    pub fn set_cell<T>(&self, handle: &TypedCellHandle<Schema, T>, value: T)
    where
        T: PartialEq + 'static,
    {
        self.inner.set_cell(&handle.raw, value);
    }

    pub fn is_set<T>(&self, handle: &TypedSlotHandle<Schema, T>) -> bool
    where
        T: 'static,
    {
        self.inner.is_set(&handle.raw)
    }
}

impl<Schema> Default for TypedContext<Schema> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Schema: 'static> TypedFactoryContext for TypedContext<Schema> {
    type Schema = Schema;

    fn memoized_slot<K, T, F>(&self, compute: F) -> TypedSlotHandle<Self::Schema, T>
    where
        K: 'static,
        T: 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Self::Schema>) -> T + 'static,
    {
        let raw = self.inner.memoized_slot::<K, T, _>(move |ctx| {
            let typed = TypedContextRef::new(ctx);
            compute(&typed)
        });
        TypedSlotHandle::new(raw)
    }

    fn memoized_cell<K, T, F>(&self, init: F) -> TypedCellHandle<Self::Schema, T>
    where
        K: 'static,
        T: PartialEq + 'static,
        F: for<'a> FnOnce(&TypedContextRef<'a, Self::Schema>) -> T,
    {
        let raw = self.inner.memoized_cell::<K, T, _>(move |ctx| {
            let typed = TypedContextRef::new(ctx);
            init(&typed)
        });
        TypedCellHandle::new(raw)
    }
}

impl<'a, Schema> TypedContextRef<'a, Schema> {
    fn new(inner: &'a Context) -> Self {
        Self {
            inner,
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> &'a Context {
        self.inner
    }

    pub fn get<T>(&self, handle: &TypedSlotHandle<Schema, T>) -> T
    where
        T: Clone + 'static,
    {
        self.inner.get(&handle.raw)
    }

    pub fn get_rc<T>(&self, handle: &TypedSlotHandle<Schema, T>) -> Rc<T>
    where
        T: 'static,
    {
        self.inner.get_rc(&handle.raw)
    }

    pub fn get_cell<T>(&self, handle: &TypedCellHandle<Schema, T>) -> T
    where
        T: Clone + 'static,
    {
        self.inner.get_cell(&handle.raw)
    }

    pub fn get_cell_rc<T>(&self, handle: &TypedCellHandle<Schema, T>) -> Rc<T>
    where
        T: 'static,
    {
        self.inner.get_cell_rc(&handle.raw)
    }

    pub fn memoized_slot<K, T, F>(&self, compute: F) -> TypedSlotHandle<Schema, T>
    where
        K: 'static,
        T: 'static,
        F: for<'b> Fn(&TypedContextRef<'b, Schema>) -> T + 'static,
        Schema: 'static,
    {
        <Self as TypedFactoryContext>::memoized_slot::<K, T, F>(self, compute)
    }

    pub fn memoized_cell<K, T, F>(&self, init: F) -> TypedCellHandle<Schema, T>
    where
        K: 'static,
        T: PartialEq + 'static,
        F: for<'b> FnOnce(&TypedContextRef<'b, Schema>) -> T,
        Schema: 'static,
    {
        <Self as TypedFactoryContext>::memoized_cell::<K, T, F>(self, init)
    }
}

impl<Schema: 'static> TypedFactoryContext for TypedContextRef<'_, Schema> {
    type Schema = Schema;

    fn memoized_slot<K, T, F>(&self, compute: F) -> TypedSlotHandle<Self::Schema, T>
    where
        K: 'static,
        T: 'static,
        F: for<'a> Fn(&TypedContextRef<'a, Self::Schema>) -> T + 'static,
    {
        let raw = self.inner.memoized_slot::<K, T, _>(move |ctx| {
            let typed = TypedContextRef::new(ctx);
            compute(&typed)
        });
        TypedSlotHandle::new(raw)
    }

    fn memoized_cell<K, T, F>(&self, init: F) -> TypedCellHandle<Self::Schema, T>
    where
        K: 'static,
        T: PartialEq + 'static,
        F: for<'a> FnOnce(&TypedContextRef<'a, Self::Schema>) -> T,
    {
        let raw = self.inner.memoized_cell::<K, T, _>(move |ctx| {
            let typed = TypedContextRef::new(ctx);
            init(&typed)
        });
        TypedCellHandle::new(raw)
    }
}

impl<Schema, T> TypedSlotHandle<Schema, T> {
    fn new(raw: SlotHandle<T>) -> Self {
        Self {
            raw,
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> SlotHandle<T> {
        self.raw
    }

    pub fn get(&self, ctx: &TypedContext<Schema>) -> T
    where
        T: Clone + 'static,
    {
        ctx.get(self)
    }

    pub fn get_ref(&self, ctx: &TypedContextRef<'_, Schema>) -> T
    where
        T: Clone + 'static,
    {
        ctx.get(self)
    }

    pub fn clear(&self, ctx: &TypedContext<Schema>) {
        ctx.inner.clear_slot(self.raw.id);
        ctx.inner.flush_effects_after_invalidation();
    }
}

impl<Schema, T> Clone for TypedSlotHandle<Schema, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Schema, T> Copy for TypedSlotHandle<Schema, T> {}

impl<Schema, T> TypedCellHandle<Schema, T> {
    fn new(raw: CellHandle<T>) -> Self {
        Self {
            raw,
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> CellHandle<T> {
        self.raw
    }

    pub fn get(&self, ctx: &TypedContext<Schema>) -> T
    where
        T: Clone + 'static,
    {
        ctx.get_cell(self)
    }

    pub fn get_ref(&self, ctx: &TypedContextRef<'_, Schema>) -> T
    where
        T: Clone + 'static,
    {
        ctx.get_cell(self)
    }

    pub fn set(&self, ctx: &TypedContext<Schema>, value: T)
    where
        T: PartialEq + 'static,
    {
        ctx.set_cell(self, value);
    }

    pub fn clear_dependents(&self, ctx: &TypedContext<Schema>) {
        ctx.inner.clear_cell_dependents(self.raw.id);
    }
}

impl<Schema, T> Clone for TypedCellHandle<Schema, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Schema, T> Copy for TypedCellHandle<Schema, T> {}

#[cfg(feature = "thread-safe")]
impl<Schema> TypedThreadSafeContext<Schema> {
    pub fn new() -> Self {
        Self {
            inner: ThreadSafeContext::new(),
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> &ThreadSafeContext {
        &self.inner
    }

    pub fn as_ref(&self) -> TypedThreadSafeContextRef<'_, Schema> {
        TypedThreadSafeContextRef::new(&self.inner)
    }

    pub fn cell<T>(&self, value: T) -> TypedThreadSafeCellHandle<Schema, T>
    where
        T: PartialEq + Send + Sync + 'static,
    {
        TypedThreadSafeCellHandle::new(self.inner.cell(value))
    }

    pub fn slot<T, F>(&self, compute: F) -> TypedThreadSafeSlotHandle<Schema, T>
    where
        T: Send + Sync + 'static,
        F: for<'a> Fn(&TypedThreadSafeContextRef<'a, Schema>) -> T + Send + Sync + 'static,
    {
        let raw = self.inner.slot(move |ctx| {
            let typed = TypedThreadSafeContextRef::new(ctx);
            compute(&typed)
        });
        TypedThreadSafeSlotHandle::new(raw)
    }

    pub fn computed<T, F>(&self, compute: F) -> TypedThreadSafeSlotHandle<Schema, T>
    where
        T: Send + Sync + 'static,
        F: for<'a> Fn(&TypedThreadSafeContextRef<'a, Schema>) -> T + Send + Sync + 'static,
    {
        self.slot(compute)
    }

    pub fn memo<T, F>(&self, compute: F) -> TypedThreadSafeSlotHandle<Schema, T>
    where
        T: PartialEq + Send + Sync + 'static,
        F: for<'a> Fn(&TypedThreadSafeContextRef<'a, Schema>) -> T + Send + Sync + 'static,
    {
        let raw = self.inner.memo(move |ctx| {
            let typed = TypedThreadSafeContextRef::new(ctx);
            compute(&typed)
        });
        TypedThreadSafeSlotHandle::new(raw)
    }

    pub fn get<T>(&self, handle: &TypedThreadSafeSlotHandle<Schema, T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.inner.get(&handle.raw)
    }

    pub fn get_cell<T>(&self, handle: &TypedThreadSafeCellHandle<Schema, T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.inner.get_cell(&handle.raw)
    }

    pub fn set_cell<T>(&self, handle: &TypedThreadSafeCellHandle<Schema, T>, value: T)
    where
        T: PartialEq + Send + Sync + 'static,
    {
        self.inner.set_cell(&handle.raw, value);
    }

    pub fn is_set<T>(&self, handle: &TypedThreadSafeSlotHandle<Schema, T>) -> bool
    where
        T: Send + Sync + 'static,
    {
        self.inner.is_set(&handle.raw)
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema> Clone for TypedThreadSafeContext<Schema> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _schema: PhantomData,
        }
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema> Default for TypedThreadSafeContext<Schema> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "thread-safe")]
impl<'a, Schema> TypedThreadSafeContextRef<'a, Schema> {
    fn new(inner: &'a ThreadSafeContext) -> Self {
        Self {
            inner,
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> &'a ThreadSafeContext {
        self.inner
    }

    pub fn get<T>(&self, handle: &TypedThreadSafeSlotHandle<Schema, T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.inner.get(&handle.raw)
    }

    pub fn get_cell<T>(&self, handle: &TypedThreadSafeCellHandle<Schema, T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.inner.get_cell(&handle.raw)
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema, T> TypedThreadSafeSlotHandle<Schema, T> {
    fn new(raw: SlotHandle<T>) -> Self {
        Self {
            raw,
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> SlotHandle<T> {
        self.raw
    }

    pub fn get(&self, ctx: &TypedThreadSafeContext<Schema>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        ctx.get(self)
    }

    pub fn get_ref(&self, ctx: &TypedThreadSafeContextRef<'_, Schema>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        ctx.get(self)
    }

    pub fn clear(&self, ctx: &TypedThreadSafeContext<Schema>) {
        ctx.inner.clear(&self.raw);
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema, T> Clone for TypedThreadSafeSlotHandle<Schema, T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema, T> Copy for TypedThreadSafeSlotHandle<Schema, T> {}

#[cfg(feature = "thread-safe")]
impl<Schema, T> TypedThreadSafeCellHandle<Schema, T> {
    fn new(raw: CellHandle<T>) -> Self {
        Self {
            raw,
            _schema: PhantomData,
        }
    }

    pub fn raw(&self) -> CellHandle<T> {
        self.raw
    }

    pub fn get(&self, ctx: &TypedThreadSafeContext<Schema>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        ctx.get_cell(self)
    }

    pub fn get_ref(&self, ctx: &TypedThreadSafeContextRef<'_, Schema>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        ctx.get_cell(self)
    }

    pub fn set(&self, ctx: &TypedThreadSafeContext<Schema>, value: T)
    where
        T: PartialEq + Send + Sync + 'static,
    {
        ctx.set_cell(self, value);
    }

    pub fn clear_dependents(&self, ctx: &TypedThreadSafeContext<Schema>) {
        ctx.inner.clear_cell_dependents(&self.raw);
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema, T> Clone for TypedThreadSafeCellHandle<Schema, T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[cfg(feature = "thread-safe")]
impl<Schema, T> Copy for TypedThreadSafeCellHandle<Schema, T> {}

#[cfg(test)]
mod tests {
    use super::*;

    enum CycleSchema {}

    #[test]
    fn typed_context_binds_slot_and_cell_handles_to_schema() {
        let ctx = TypedContext::<CycleSchema>::new();
        let input = ctx.cell(2);
        let doubled = ctx.slot(move |ctx| ctx.get_cell(&input) * 2);

        assert_eq!(ctx.get(&doubled), 4);
        ctx.set_cell(&input, 3);
        assert_eq!(doubled.get(&ctx), 6);
    }

    #[cfg(feature = "thread-safe")]
    #[test]
    fn typed_thread_safe_context_binds_slot_and_cell_handles_to_schema() {
        let ctx = TypedThreadSafeContext::<CycleSchema>::new();
        let input = ctx.cell(2);
        let doubled = ctx.slot(move |ctx| ctx.get_cell(&input) * 2);

        assert_eq!(ctx.get(&doubled), 4);
        ctx.set_cell(&input, 3);
        assert_eq!(doubled.get(&ctx), 6);
    }
}
