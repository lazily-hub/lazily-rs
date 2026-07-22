use std::cell::RefCell;
use std::rc::Rc;
#[cfg(any(feature = "thread-safe", feature = "async"))]
use std::sync::Arc;

#[cfg(any(feature = "thread-safe", feature = "async"))]
use parking_lot::Mutex;

use crate::Context;
#[cfg(feature = "async")]
use crate::async_context::{
    AsyncComputeContext, AsyncContext, AsyncEffectHandle, AsyncSignalHandle, AsyncSource,
};
use crate::cell::Computed;
use crate::cell::Source;
use crate::effect::Effect;
#[cfg(feature = "thread-safe")]
use crate::thread_safe::{ThreadSafeContext, ThreadSafeSignalHandle};

type TransitionFn<S, E> = dyn Fn(&S, &E) -> Option<S>;
#[cfg(any(feature = "thread-safe", feature = "async"))]
type ThreadSafeTransitionFn<S, E> = dyn Fn(&S, &E) -> Option<S> + Send + Sync;

/// A finite state machine backed by a reactive [`Context`].
///
/// `StateMachine` wraps a [`Source<S>`] as the current state and a pure
/// transition function `Fn(&S, &E) -> Option<S>`. Sending an event evaluates
/// the transition function; if it returns `Some(new_state)` the cell is
/// updated, triggering any dependent slots/signals/effects. If it returns
/// `None` the transition is rejected (guard).
///
/// Because the state lives in a [`Source`], any `ctx.computed`,
/// `ctx.signal`, or `ctx.effect` that reads [`StateMachine::state_handle`]
/// automatically recomputes or reruns when the machine transitions — no
/// manual notification wiring is needed.
///
/// # Threading
///
/// `StateMachine` is single-threaded (backed by [`Context`] which uses `RefCell`).
/// For cross-thread state machines, mirror the pattern using
/// [`ThreadSafeContext`](crate::ThreadSafeContext).
///
/// # Example
///
/// ```no_run
/// use lazily::{Context, StateMachine};
///
/// #[derive(PartialEq, Clone, Debug)]
/// enum Light { Red, Green, Yellow }
///
/// #[derive(Debug)]
/// enum Tick { Advance }
///
/// let ctx = Context::new();
/// let m = StateMachine::new(&ctx, Light::Red, |s, _: &Tick| match s {
///     Light::Red    => Some(Light::Green),
///     Light::Green  => Some(Light::Yellow),
///     Light::Yellow => Some(Light::Red),
/// });
///
/// m.send(&ctx, Tick::Advance);
/// assert_eq!(m.state(&ctx), Light::Green);
/// ```
pub struct StateMachine<S, E>
where
    S: PartialEq + Clone + 'static,
    E: 'static,
{
    state: Source<S>,
    transition: Rc<TransitionFn<S, E>>,
}

impl<S, E> StateMachine<S, E>
where
    S: PartialEq + Clone + 'static,
    E: 'static,
{
    /// Create a new state machine with an initial state and transition function.
    ///
    /// The transition function is pure: given the current state and an event,
    /// it returns `Some(new_state)` for a valid transition or `None` to reject
    /// the event (guard).
    pub fn new<F>(ctx: &Context, initial: S, transition: F) -> Self
    where
        F: Fn(&S, &E) -> Option<S> + 'static,
    {
        Self {
            state: ctx.source(initial),
            transition: Rc::new(transition),
        }
    }

    /// Send an event to the machine.
    ///
    /// Returns `true` if the transition function accepted the event (returned
    /// `Some`), `false` if it was rejected (returned `None`). Note: a
    /// self-transition that returns `Some(equal_state)` is accepted but will
    /// not invalidate dependents (the `PartialEq` guard on the underlying cell
    /// suppresses no-op updates).
    pub fn send(&self, ctx: &Context, event: E) -> bool {
        let current = ctx.get(&self.state);
        match (self.transition)(&current, &event) {
            Some(next) => {
                ctx.set(&self.state, next);
                true
            }
            None => false,
        }
    }

    /// Read the current state.
    pub fn state(&self, ctx: &Context) -> S {
        ctx.get(&self.state)
    }

    /// Returns the underlying cell handle so other reactive nodes
    /// (slots, signals, effects) can depend on the machine's state.
    ///
    /// Any `ctx.computed`, `ctx.signal`, or `ctx.effect` that
    /// reads this handle will automatically recompute or rerun when the
    /// machine transitions to a different state.
    pub fn state_handle(&self) -> Source<S> {
        self.state
    }

    /// Register an effect that fires with `(old, new)` whenever the machine
    /// transitions to a different state.
    ///
    /// The handler is **not** called on registration (initial run); it only
    /// fires on subsequent state changes. The returned [`Effect`] can be
    /// disposed to stop observing.
    ///
    /// This is the state-machine analog of on-enter/on-exit: the handler
    /// receives both the previous and new state, so it can dispatch per-state
    /// enter/exit logic from a single observer.
    pub fn on_transition<F>(&self, ctx: &Context, handler: F) -> Effect
    where
        F: Fn(&S, &S) + 'static,
    {
        let state = self.state;
        let prev: Rc<RefCell<Option<S>>> = Rc::new(RefCell::new(None));
        let handler = Rc::new(handler);
        ctx.effect(move |ctx| {
            let current = ctx.get(&state);
            let mut prev_ref = prev.borrow_mut();
            if let Some(ref old) = *prev_ref
                && old != &current
            {
                handler(old, &current);
            }
            *prev_ref = Some(current);
        })
    }

    /// Create a signal that is `true` when the machine is in the `target`
    /// state and `false` otherwise.
    ///
    /// Useful for conditional rendering, hierarchical guards, or composing
    /// multiple machines. The signal is eager — it always reflects the current
    /// machine state without requiring a manual read.
    pub fn state_is(&self, ctx: &Context, target: S) -> Computed<bool> {
        let state = self.state;
        ctx.signal(move |ctx| ctx.get(&state) == target)
    }
}

/// A finite state machine backed by a reactive [`ThreadSafeContext`].
///
/// This is the thread-safe counterpart to [`StateMachine`]: it mirrors the
/// same `Source<S>` + pure transition-function design but requires the
/// transition function and state to be `Send + Sync + 'static`, so the machine
/// (and the context that backs it) can be shared across OS threads.
///
/// Because the state lives in a [`Source`], any `ctx.computed`,
/// `ctx.signal`, or `ctx.effect` that reads [`ThreadSafeStateMachine::state_handle`]
/// automatically recomputes or reruns when the machine transitions.
///
/// # Example
///
/// ```no_run
/// use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
///
/// #[derive(PartialEq, Clone, Debug)]
/// enum Light { Red, Green, Yellow }
///
/// #[derive(Debug)]
/// enum Tick { Advance }
///
/// let ctx = ThreadSafeContext::new();
/// let m = ThreadSafeStateMachine::new(&ctx, Light::Red, |s, _: &Tick| match s {
///     Light::Red    => Some(Light::Green),
///     Light::Green  => Some(Light::Yellow),
///     Light::Yellow => Some(Light::Red),
/// });
///
/// m.send(&ctx, Tick::Advance);
/// assert_eq!(m.state(&ctx), Light::Green);
/// ```
#[cfg(feature = "thread-safe")]
pub struct ThreadSafeStateMachine<S, E>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    state: Source<S>,
    transition: Arc<ThreadSafeTransitionFn<S, E>>,
}

#[cfg(feature = "thread-safe")]
impl<S, E> Clone for ThreadSafeStateMachine<S, E>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state,
            transition: Arc::clone(&self.transition),
        }
    }
}

#[cfg(feature = "thread-safe")]
impl<S, E> ThreadSafeStateMachine<S, E>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    /// Create a new thread-safe state machine with an initial state and
    /// transition function.
    ///
    /// The transition function is pure: given the current state and an event,
    /// it returns `Some(new_state)` for a valid transition or `None` to reject
    /// the event (guard). It must be `Send + Sync` so it can be invoked from
    /// any thread that shares the owning [`ThreadSafeContext`].
    pub fn new<F>(ctx: &ThreadSafeContext, initial: S, transition: F) -> Self
    where
        F: Fn(&S, &E) -> Option<S> + Send + Sync + 'static,
    {
        Self {
            state: ctx.source(initial),
            transition: Arc::new(transition),
        }
    }

    /// Send an event to the machine.
    ///
    /// Returns `true` if the transition function accepted the event (returned
    /// `Some`), `false` if it was rejected (returned `None`). A self-transition
    /// that returns `Some(equal_state)` is accepted but will not invalidate
    /// dependents (the `PartialEq` guard on the underlying cell suppresses
    /// no-op updates).
    pub fn send(&self, ctx: &ThreadSafeContext, event: E) -> bool {
        let current = ctx.get(&self.state);
        match (self.transition)(&current, &event) {
            Some(next) => {
                ctx.set(&self.state, next);
                true
            }
            None => false,
        }
    }

    /// Read the current state.
    pub fn state(&self, ctx: &ThreadSafeContext) -> S {
        ctx.get(&self.state)
    }

    /// Returns the underlying cell handle so other reactive nodes
    /// (slots, signals, effects) can depend on the machine's state.
    ///
    /// Any `ctx.computed`, `ctx.signal`, or `ctx.effect` that
    /// reads this handle will automatically recompute or rerun when the
    /// machine transitions to a different state.
    pub fn state_handle(&self) -> Source<S> {
        self.state
    }

    /// Register an effect that fires with `(old, new)` whenever the machine
    /// transitions to a different state.
    ///
    /// The handler is **not** called on registration (initial run); it only
    /// fires on subsequent state changes. The returned [`Effect`] can be
    /// disposed via [`ThreadSafeContext::dispose_effect`] to stop observing.
    ///
    /// This is the state-machine analog of on-enter/on-exit: the handler
    /// receives both the previous and new state, so it can dispatch per-state
    /// enter/exit logic from a single observer.
    pub fn on_transition<F>(&self, ctx: &ThreadSafeContext, handler: F) -> Effect
    where
        F: Fn(&S, &S) + Send + Sync + 'static,
    {
        let state = self.state;
        let prev: Arc<Mutex<Option<S>>> = Arc::new(Mutex::new(None));
        let handler = Arc::new(handler);
        ctx.effect(move |ctx: &ThreadSafeContext| {
            let current = ctx.get(&state);
            let mut prev_ref = prev.lock();
            if let Some(ref old) = *prev_ref
                && old != &current
            {
                handler(old, &current);
            }
            *prev_ref = Some(current);
        })
    }

    /// Create an eager signal that is `true` when the machine is in the
    /// `target` state and `false` otherwise.
    ///
    /// Useful for conditional rendering, hierarchical guards, or composing
    /// multiple machines. The signal is eager — it always reflects the current
    /// machine state without requiring a manual read.
    pub fn state_is(&self, ctx: &ThreadSafeContext, target: S) -> ThreadSafeSignalHandle<bool> {
        let state = self.state;
        ctx.signal(move |ctx: &ThreadSafeContext| ctx.get(&state) == target)
    }
}

/// A finite state machine backed by a reactive [`AsyncContext`].
///
/// This is the async (Tokio) counterpart to [`StateMachine`]. The state lives
/// in an [`AsyncSource<S>`]; because cells are the synchronous input layer
/// of [`AsyncContext`], [`AsyncStateMachine::send`] and
/// [`AsyncStateMachine::state`] are synchronous. Reactive observers
/// ([`AsyncStateMachine::on_transition`], [`AsyncStateMachine::state_is`]) use
/// the async effect/signal APIs, so their bodies may `await` other async
/// reactive nodes.
///
/// # Example
///
/// ```no_run
/// use lazily::{AsyncContext, AsyncStateMachine};
///
/// #[derive(PartialEq, Clone, Debug)]
/// enum Light { Red, Green, Yellow }
///
/// #[derive(Debug)]
/// enum Tick { Advance }
///
/// let ctx = AsyncContext::new();
/// let m = AsyncStateMachine::new(&ctx, Light::Red, |s, _: &Tick| match s {
///     Light::Red    => Some(Light::Green),
///     Light::Green  => Some(Light::Yellow),
///     Light::Yellow => Some(Light::Red),
/// });
///
/// m.send(&ctx, Tick::Advance);
/// assert_eq!(m.state(&ctx), Light::Green);
/// ```
#[cfg(feature = "async")]
pub struct AsyncStateMachine<S, E>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    state: AsyncSource<S>,
    transition: Arc<ThreadSafeTransitionFn<S, E>>,
}

#[cfg(feature = "async")]
impl<S, E> Clone for AsyncStateMachine<S, E>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state,
            transition: Arc::clone(&self.transition),
        }
    }
}

#[cfg(feature = "async")]
impl<S, E> AsyncStateMachine<S, E>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    /// Create a new async state machine with an initial state and transition
    /// function.
    ///
    /// The transition function is pure: given the current state and an event,
    /// it returns `Some(new_state)` for a valid transition or `None` to reject
    /// the event (guard). It must be `Send + Sync` so it can be invoked from
    /// any async task that shares the owning [`AsyncContext`].
    pub fn new<F>(ctx: &AsyncContext, initial: S, transition: F) -> Self
    where
        F: Fn(&S, &E) -> Option<S> + Send + Sync + 'static,
    {
        Self {
            state: ctx.source(initial),
            transition: Arc::new(transition),
        }
    }

    /// Send an event to the machine.
    ///
    /// Returns `true` if the transition function accepted the event (returned
    /// `Some`), `false` if it was rejected (returned `None`). A self-transition
    /// that returns `Some(equal_state)` is accepted but will not invalidate
    /// dependents (the `PartialEq` guard on the underlying cell suppresses
    /// no-op updates).
    ///
    /// This is synchronous: cells are the sync input layer of [`AsyncContext`],
    /// so sending an event does not require an await. Derived async
    /// slots/effects that depend on the machine's state are invalidated and
    /// rescheduled synchronously, then re-resolved on the runtime.
    pub fn send(&self, ctx: &AsyncContext, event: E) -> bool {
        let current = ctx.get(&self.state);
        match (self.transition)(&current, &event) {
            Some(next) => {
                ctx.set(&self.state, next);
                true
            }
            None => false,
        }
    }

    /// Read the current state.
    ///
    /// This is synchronous: cells are always materialized in an
    /// [`AsyncContext`], so reading the machine's state does not require an
    /// await.
    pub fn state(&self, ctx: &AsyncContext) -> S {
        ctx.get(&self.state)
    }

    /// Returns the underlying cell handle so other reactive nodes
    /// (async slots, async signals, async effects) can depend on the machine's
    /// state.
    ///
    /// Any `ctx.computed_async`, `ctx.signal_async`, or
    /// `ctx.effect_async` that reads this handle will automatically recompute
    /// or rerun when the machine transitions to a different state.
    pub fn state_handle(&self) -> AsyncSource<S> {
        self.state
    }

    /// Register an async effect that fires with `(old, new)` whenever the
    /// machine transitions to a different state.
    ///
    /// The handler is **not** called on registration (initial run); it only
    /// fires on subsequent state changes. The handler closure is synchronous
    /// (`Fn(&S, &S)`) — it observes the transition, not the async resolution —
    /// but the returned [`AsyncEffectHandle`] can be disposed via
    /// [`AsyncContext::dispose_async_effect`] to stop observing.
    ///
    /// This is the state-machine analog of on-enter/on-exit: the handler
    /// receives both the previous and new state, so it can dispatch per-state
    /// enter/exit logic from a single observer.
    pub fn on_transition<F>(&self, ctx: &AsyncContext, handler: F) -> AsyncEffectHandle
    where
        F: Fn(&S, &S) + Send + Sync + 'static,
    {
        let state = self.state;
        let prev: Arc<Mutex<Option<S>>> = Arc::new(Mutex::new(None));
        let handler = Arc::new(handler);
        ctx.effect_async(move |compute_ctx: AsyncComputeContext| {
            let current = compute_ctx.get(&state);
            let prev = prev.clone();
            let handler = handler.clone();
            async move {
                let mut prev_ref = prev.lock();
                if let Some(ref old) = *prev_ref
                    && old != &current
                {
                    handler(old, &current);
                }
                *prev_ref = Some(current);
                None::<fn()>
            }
        })
    }

    /// Create an eager async signal that is `true` when the machine is in the
    /// `target` state and `false` otherwise.
    ///
    /// Useful for conditional rendering, hierarchical guards, or composing
    /// multiple machines. The signal is eager — a puller effect drives the
    /// recomputation to completion on every invalidation, so it always reflects
    /// the current machine state. Because resolution is asynchronous, the
    /// materialized value lands on the runtime rather than synchronously within
    /// the `send` call; use [`AsyncSignalHandle::get`] for a non-blocking
    /// snapshot or [`AsyncSignalHandle::get_async`] to await the up-to-date
    /// value.
    pub fn state_is(&self, ctx: &AsyncContext, target: S) -> AsyncSignalHandle<bool> {
        let state = self.state;
        ctx.signal_async(move |compute_ctx: AsyncComputeContext| {
            let t = target.clone();
            async move { compute_ctx.get(&state) == t }
        })
    }
}
