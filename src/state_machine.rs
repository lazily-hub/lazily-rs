use std::cell::RefCell;
use std::rc::Rc;

use crate::Context;
use crate::cell::CellHandle;
use crate::effect::EffectHandle;
use crate::signal::SignalHandle;

type TransitionFn<S, E> = dyn Fn(&S, &E) -> Option<S>;

/// A finite state machine backed by a reactive [`Context`].
///
/// `Machine` wraps a [`CellHandle<S>`] as the current state and a pure
/// transition function `Fn(&S, &E) -> Option<S>`. Sending an event evaluates
/// the transition function; if it returns `Some(new_state)` the cell is
/// updated, triggering any dependent slots/signals/effects. If it returns
/// `None` the transition is rejected (guard).
///
/// Because the state lives in a [`CellHandle`], any `ctx.computed`,
/// `ctx.signal`, or `ctx.effect` that reads [`Machine::state_handle`]
/// automatically recomputes or reruns when the machine transitions — no
/// manual notification wiring is needed.
///
/// # Threading
///
/// `Machine` is single-threaded (backed by [`Context`] which uses `RefCell`).
/// For cross-thread state machines, mirror the pattern using
/// [`ThreadSafeContext`](crate::ThreadSafeContext).
///
/// # Example
///
/// ```no_run
/// use lazily::{Context, Machine};
///
/// #[derive(PartialEq, Clone, Debug)]
/// enum Light { Red, Green, Yellow }
///
/// #[derive(Debug)]
/// enum Tick { Advance }
///
/// let ctx = Context::new();
/// let m = Machine::new(&ctx, Light::Red, |s, _: &Tick| match s {
///     Light::Red    => Some(Light::Green),
///     Light::Green  => Some(Light::Yellow),
///     Light::Yellow => Some(Light::Red),
/// });
///
/// m.send(&ctx, Tick::Advance);
/// assert_eq!(m.state(&ctx), Light::Green);
/// ```
pub struct Machine<S, E>
where
    S: PartialEq + Clone + 'static,
    E: 'static,
{
    state: CellHandle<S>,
    transition: Rc<TransitionFn<S, E>>,
}

impl<S, E> Machine<S, E>
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
            state: ctx.cell(initial),
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
        let current = ctx.get_cell(&self.state);
        match (self.transition)(&current, &event) {
            Some(next) => {
                ctx.set_cell(&self.state, next);
                true
            }
            None => false,
        }
    }

    /// Read the current state.
    pub fn state(&self, ctx: &Context) -> S {
        ctx.get_cell(&self.state)
    }

    /// Returns the underlying cell handle so other reactive nodes
    /// (slots, signals, effects) can depend on the machine's state.
    ///
    /// Any `ctx.computed`, `ctx.memo`, `ctx.signal`, or `ctx.effect` that
    /// reads this handle will automatically recompute or rerun when the
    /// machine transitions to a different state.
    pub fn state_handle(&self) -> CellHandle<S> {
        self.state
    }

    /// Register an effect that fires with `(old, new)` whenever the machine
    /// transitions to a different state.
    ///
    /// The handler is **not** called on registration (initial run); it only
    /// fires on subsequent state changes. The returned [`EffectHandle`] can be
    /// disposed to stop observing.
    ///
    /// This is the state-machine analog of on-enter/on-exit: the handler
    /// receives both the previous and new state, so it can dispatch per-state
    /// enter/exit logic from a single observer.
    pub fn on_transition<F>(&self, ctx: &Context, handler: F) -> EffectHandle
    where
        F: Fn(&S, &S) + 'static,
    {
        let state = self.state;
        let prev: Rc<RefCell<Option<S>>> = Rc::new(RefCell::new(None));
        let handler = Rc::new(handler);
        ctx.effect(move |ctx| {
            let current = ctx.get_cell(&state);
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
    pub fn state_is(&self, ctx: &Context, target: S) -> SignalHandle<bool> {
        let state = self.state;
        ctx.signal(move |ctx| ctx.get_cell(&state) == target)
    }
}
