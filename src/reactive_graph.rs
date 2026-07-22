//! Capability traits over `lazily`'s execution models (`#lzspecedgeindex`).
//!
//! `lazily` ships three contexts — [`Context`](crate::Context),
//! [`ThreadSafeContext`](crate::ThreadSafeContext), and
//! [`AsyncContext`](crate::AsyncContext) — that share a graph model but not a
//! type. Code that only wants to *tear down* or *inspect* a graph should not
//! have to pick one, or be written three times.
//!
//! ## Shape of the hierarchy
//!
//! [`ReactiveGraph`] is deliberately **bound-free**: disposal, teardown scopes,
//! batching, and degree counts care nothing about a value's `PartialEq`,
//! `Clone`, `Send`, or `Sync`. Only construction and reads do, so those live in
//! [`SyncReactiveGraph`] and [`AsyncReactiveGraph`], where each declares the
//! bounds its implementors actually need.
//!
//! That split is the whole point. Putting `cell`/`set_cell` in the base trait
//! would force the union of all three contexts' value bounds onto every caller,
//! including generic teardown code that never touches a value. Keeping the most
//! reusable trait free of unnecessary bounds is worth the extra trait.
//!
//! A union bound does remain *inside* [`SyncReactiveGraph`] — `Context` needs no
//! `Send + Sync` while `ThreadSafeContext` does — but it is confined to callers
//! who construct or read, and inherent methods are untouched either way.
//!
//! ## Why four traits: the axes are orthogonal
//!
//! Read discipline and thread-safety vary independently:
//!
//! |                 | sync reads          | async reads              |
//! |-----------------|---------------------|--------------------------|
//! | not thread-safe | [`Context`](crate::Context) | *(empty, but conceivable)* |
//! | thread-safe     | [`ThreadSafeContext`](crate::ThreadSafeContext) | [`AsyncContext`](crate::AsyncContext) |
//!
//! The empty cell is real rather than hypothetical: single-threaded async — a
//! wasm context, or a `current_thread` runtime — is fully concurrent, uses
//! futures, and requires neither `Send` nor `Sync`. Merging read discipline
//! into the thread-safety marker would make that cell unrepresentable; kept
//! separate, such a context would satisfy [`AsyncReactiveGraph`] and simply
//! fail [`ThreadSafeReactiveGraph`], and the hierarchy already has a place for
//! it.
//!
//! The framing that keeps these axes from tangling: a graph is never itself
//! "concurrent" or "parallel" — those describe how a *program* uses it. A graph
//! has capabilities: how you read it, and whether you can share it across
//! threads. These traits name capabilities, not usage patterns.
//!
//! ## Handle types are associated
//!
//! `Context` and `ThreadSafeContext` share `Computed`/`Source`;
//! `AsyncContext` has its own. So the handle types are associated rather than
//! concrete, following [`TypedFactoryContext`](crate::TypedFactoryContext)'s
//! `type Schema`.

use crate::context::{GraphNode, Read, Write};

/// A scope that disposes what it owns when it ends.
///
/// Implemented by all three teardown scope types. They differ in whether they
/// borrow or own their context — `Context` owns its state so its scope must
/// borrow, while the other two are already `Arc` handles and their scopes own —
/// but they agree on what a scope *does*, which is what this captures.
pub trait Teardown {
    /// How many nodes this scope owns.
    fn len(&self) -> usize;

    /// Whether this scope owns nothing.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Cancel the scope's teardown: ending it afterwards disposes nothing, and
    /// its nodes revert to plain context ownership. The nodes are untouched.
    fn disarm(self)
    where
        Self: Sized;
}

/// The graph operations that need no bounds on node values.
///
/// Everything here — disposal, scopes, batching, degree introspection — is
/// about graph *structure*, so generic code over this trait pays for nothing it
/// does not use. Construction and reads live in [`SyncReactiveGraph`] /
/// [`AsyncReactiveGraph`]. Both capability traits expose the Cell-kernel
/// vocabulary: `source`, `get`, and `set`.
///
/// ```
/// use lazily::{Context, ReactiveGraph, SyncReactiveGraph};
///
/// // Written once, no value bounds, works for every execution model.
/// fn leaked_edges<G: ReactiveGraph>(graph: &G, node: &G::Source<i64>) -> usize {
///     graph.dependent_count(node)
/// }
///
/// let ctx = Context::new();
/// let topic = SyncReactiveGraph::source(&ctx, 1i64);
/// let derived = SyncReactiveGraph::computed(&ctx, move |c| c.get(&topic) + 1);
/// assert_eq!(SyncReactiveGraph::get(&ctx, &derived), 2);
/// assert_eq!(leaked_edges(&ctx, &topic), 1);
///
/// ReactiveGraph::dispose_slot(&ctx, &derived);
/// assert_eq!(leaked_edges(&ctx, &topic), 0);
/// ```
pub trait ReactiveGraph {
    /// This graph's derived-slot handle.
    ///
    /// Bounded by [`GraphNode`] and `Copy` so generic code can take degrees of
    /// any handle and pass handles around freely. Every handle in the crate is
    /// an id, so both hold everywhere.
    type Computed<T>: GraphNode + Copy;
    /// This graph's source-cell handle.
    type Source<T>: GraphNode + Copy;
    /// This graph's effect handle.
    type Effect: GraphNode + Copy;
    /// This graph's teardown scope.
    type Scope<'a>: Teardown
    where
        Self: 'a;

    /// Tear down a derived slot: detach both edge directions, invalidate the
    /// surviving readers, and recycle the id.
    fn dispose_slot<T: 'static>(&self, handle: &Self::Computed<T>);

    /// Tear down a source cell: detach its dependents, invalidate them, and
    /// recycle the id.
    fn dispose_cell<T: 'static>(&self, handle: &Self::Source<T>);

    /// Tear down an effect, running its cleanup.
    fn dispose_effect(&self, handle: &Self::Effect);

    /// Open a teardown scope: nodes created through it are disposed when it
    /// ends, in reverse creation order.
    fn scope(&self) -> Self::Scope<'_>;

    /// Run `run` with invalidation batched until it returns.
    fn batch<R>(&self, run: impl FnOnce(&Self) -> R) -> R;

    /// How many nodes currently depend on `node` — the size of its reverse edge
    /// set. Returns 0 for a disposed or unknown node.
    fn dependent_count(&self, node: &impl GraphNode) -> usize;

    /// How many nodes `node` currently depends on — the size of its forward
    /// edge set. Returns 0 for a disposed or unknown node.
    fn dependency_count(&self, node: &impl GraphNode) -> usize;
}

/// A graph that is safe to move and share across threads.
///
/// This is `Send + Sync` — a *safety property*, not a claim about concurrency or
/// parallelism. Concurrency is interleaved progress and does not require threads
/// at all (single-threaded async is fully concurrent); parallelism is
/// simultaneous execution. `Send + Sync` is the precondition that permits the
/// latter, which is what "thread-safe" names and why this trait is not called
/// `Concurrent...`.
///
/// Blanket-implemented, so it is a capability marker rather than something a
/// context opts into: [`ThreadSafeContext`](crate::ThreadSafeContext) and
/// [`AsyncContext`](crate::AsyncContext) satisfy it — both are `Arc`-based
/// handles — while [`Context`](crate::Context) does not. Bound on it when
/// generic code must move a graph between threads.
pub trait ThreadSafeReactiveGraph: ReactiveGraph + Send + Sync + 'static {}

impl<T: ReactiveGraph + Send + Sync + 'static> ThreadSafeReactiveGraph for T {}

/// Construction and reads for the synchronous graphs.
///
/// The `Send + Sync` bounds are the union of what
/// [`Context`](crate::Context) and
/// [`ThreadSafeContext`](crate::ThreadSafeContext) require — `Context` does not
/// need them, but a trait method's bounds are fixed and an impl cannot add its
/// own. The cost is confined to generic callers that construct or read; both
/// contexts' inherent methods keep their exact original bounds.
pub trait SyncReactiveGraph: ReactiveGraph {
    /// The per-recompute **compute view** handed to `computed`/`effect`
    /// closures (`#lzcellkernel`). This is the associated type that lets those
    /// closures receive a *value-threaded* view instead of relying on an ambient
    /// thread-local: [`Context`](crate::Context) projects its fortified
    /// [`Compute`](crate::Compute), whose recomputing-node id travels as a value,
    /// while [`ThreadSafeContext`](crate::ThreadSafeContext) — which keeps its own
    /// ambient engine — projects itself. The GAT lifetime `'a` binds the view to
    /// the recompute call; `Context::Compute<'a>` borrows the context for `'a`,
    /// so the `where Self: 'a` bound is load-bearing.
    type Compute<'a>
    where
        Self: 'a;

    /// Create a source cell.
    fn source<T>(&self, value: T) -> Self::Source<T>
    where
        T: PartialEq + Send + Sync + 'static;

    /// Read any handle supported by this graph.
    fn get<H>(&self, handle: &H) -> <H as Read<Self>>::Output
    where
        Self: Sized,
        H: Read<Self> + ?Sized,
    {
        handle.read(self)
    }

    /// Write any handle supported by this graph. In the Cell kernel only
    /// [`Source`](crate::Source) implements [`Write`].
    fn set<H>(&self, handle: &H, value: <H as Write<Self>>::Value)
    where
        Self: Sized,
        H: Write<Self> + ?Sized,
    {
        handle.write(self, value);
    }

    /// Compatibility constructor for the pre-Cell-kernel API.
    #[deprecated(note = "use `SyncReactiveGraph::source`")]
    fn cell<T>(&self, value: T) -> Self::Source<T>
    where
        Self: Sized,
        T: PartialEq + Send + Sync + 'static,
    {
        self.source(value)
    }

    /// Compatibility source read for the pre-Cell-kernel API.
    #[deprecated(note = "use `SyncReactiveGraph::get`")]
    fn get_cell<T>(&self, handle: &Self::Source<T>) -> T
    where
        Self: Sized,
        T: Clone + Send + Sync + 'static,
        Self::Source<T>: Read<Self, Output = T>,
    {
        self.get(handle)
    }

    /// Compatibility source write for the pre-Cell-kernel API.
    #[deprecated(note = "use `SyncReactiveGraph::set`")]
    fn set_cell<T>(&self, handle: &Self::Source<T>, value: T)
    where
        Self: Sized,
        T: PartialEq + Send + Sync + 'static,
        Self::Source<T>: Write<Self, Value = T>,
    {
        self.set(handle, value);
    }

    /// Create a lazily-computed derived slot. Guarded (`#lzcellkernel`): an
    /// equal recompute suppresses downstream invalidation, so `T: PartialEq`.
    fn computed<T, F>(&self, compute: F) -> Self::Computed<T>
    where
        T: PartialEq + Send + Sync + 'static,
        F: Fn(&Self::Compute<'_>) -> T + Send + Sync + 'static;

    /// Register an effect. The callback returns its cleanup, which runs before
    /// each rerun and on disposal.
    ///
    /// `C` is a bare closure rather than either context's callback-result
    /// trait: both of those are blanket-implemented for
    /// `FnOnce() + ... + 'static`, so one bound satisfies both.
    fn effect<F, C>(&self, run: F) -> Self::Effect
    where
        F: Fn(&Self::Compute<'_>) -> C + Send + Sync + 'static,
        C: FnOnce() + Send + Sync + 'static;
}

/// Construction and reads for the async graph.
///
/// Separate from [`SyncReactiveGraph`] because reads return futures and because
/// `AsyncContext` additionally requires `Clone` on cell values. Only one
/// context implements this, so its bounds are exactly that context's — no union
/// is needed here.
pub trait AsyncReactiveGraph: ReactiveGraph {
    /// Create a source cell.
    fn source<T>(&self, value: T) -> Self::Source<T>
    where
        T: PartialEq + Clone + Send + Sync + 'static;

    /// Read the currently materialized value of any handle supported by this
    /// graph. Sources return `T`; async computeds return `Option<T>`.
    fn get<H>(&self, handle: &H) -> <H as Read<Self>>::Output
    where
        Self: Sized,
        H: Read<Self> + ?Sized,
    {
        handle.read(self)
    }

    /// Write any handle supported by this graph. Only async sources implement
    /// [`Write`].
    fn set<H>(&self, handle: &H, value: <H as Write<Self>>::Value)
    where
        Self: Sized,
        H: Write<Self> + ?Sized,
    {
        handle.write(self, value);
    }

    /// Read a derived slot, driving its computation if needed.
    fn get_async<T>(&self, handle: &Self::Computed<T>) -> impl Future<Output = T> + Send
    where
        T: Clone + Send + Sync + 'static;

    /// Compatibility constructor for the pre-Cell-kernel API.
    #[deprecated(note = "use `AsyncReactiveGraph::source`")]
    fn cell<T>(&self, value: T) -> Self::Source<T>
    where
        Self: Sized,
        T: PartialEq + Clone + Send + Sync + 'static,
    {
        self.source(value)
    }

    /// Compatibility source read for the pre-Cell-kernel API.
    #[deprecated(note = "use `AsyncReactiveGraph::get`")]
    fn get_cell<T>(&self, handle: &Self::Source<T>) -> T
    where
        Self: Sized,
        T: Clone + Send + Sync + 'static,
        Self::Source<T>: Read<Self, Output = T>,
    {
        self.get(handle)
    }

    /// Compatibility source write for the pre-Cell-kernel API.
    #[deprecated(note = "use `AsyncReactiveGraph::set`")]
    fn set_cell<T>(&self, handle: &Self::Source<T>, value: T)
    where
        Self: Sized,
        T: PartialEq + Clone + Send + Sync + 'static,
        Self::Source<T>: Write<Self, Value = T>,
    {
        self.set(handle, value);
    }
}
