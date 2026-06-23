//! Integration tests for the `Machine` state machine primitive.
//!
//! A `Machine` wraps a `CellHandle<S>` + transition function, exposing
//! `send(event)`, `state()`, `on_transition()`, and `state_is()`.

use std::cell::RefCell;
use std::rc::Rc;

use lazily::{Context, Machine};

// -- Basic FSM --------------------------------------------------------------

#[derive(PartialEq, Clone, Debug)]
enum Light {
    Red,
    Green,
    Yellow,
}

#[derive(Debug)]
enum Tick {
    Advance,
}

#[test]
fn machine_transitions_through_all_states() {
    let ctx = Context::new();
    let m = Machine::new(&ctx, Light::Red, |s, _: &Tick| match s {
        Light::Red => Some(Light::Green),
        Light::Green => Some(Light::Yellow),
        Light::Yellow => Some(Light::Red),
    });

    assert_eq!(m.state(&ctx), Light::Red);
    assert!(m.send(&ctx, Tick::Advance));
    assert_eq!(m.state(&ctx), Light::Green);
    assert!(m.send(&ctx, Tick::Advance));
    assert_eq!(m.state(&ctx), Light::Yellow);
    assert!(m.send(&ctx, Tick::Advance));
    assert_eq!(m.state(&ctx), Light::Red);
}

// -- Guarded transitions ----------------------------------------------------

#[derive(PartialEq, Clone, Debug)]
enum Door {
    Closed,
    Opening,
    Open,
    Closing,
}

#[derive(Debug)]
enum DoorEvent {
    ButtonPressed,
    FullyOpen,
    FullyClosed,
}

fn garage_door(ctx: &Context) -> Machine<Door, DoorEvent> {
    Machine::new(ctx, Door::Closed, |s, e| match (s, e) {
        (Door::Closed, DoorEvent::ButtonPressed) => Some(Door::Opening),
        (Door::Opening, DoorEvent::FullyOpen) => Some(Door::Open),
        (Door::Open, DoorEvent::ButtonPressed) => Some(Door::Closing),
        (Door::Closing, DoorEvent::FullyClosed) => Some(Door::Closed),
        (Door::Closing, DoorEvent::ButtonPressed) => Some(Door::Opening),
        _ => None,
    })
}

#[test]
fn machine_rejects_invalid_transition() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    assert_eq!(m.state(&ctx), Door::Closed);

    assert!(!m.send(&ctx, DoorEvent::FullyOpen));
    assert_eq!(m.state(&ctx), Door::Closed);

    assert!(m.send(&ctx, DoorEvent::ButtonPressed));
    assert_eq!(m.state(&ctx), Door::Opening);

    assert!(!m.send(&ctx, DoorEvent::FullyClosed));
    assert_eq!(m.state(&ctx), Door::Opening);
}

// -- Self-transition --------------------------------------------------------

#[test]
fn machine_self_transition_is_accepted_but_no_invalidation() {
    let ctx = Context::new();
    let call_count = Rc::new(RefCell::new(0usize));
    let call_count_inner = call_count.clone();

    let m = Machine::new(&ctx, 0i32, move |s, _: &()| {
        *call_count_inner.borrow_mut() += 1;
        Some(*s)
    });

    let state = m.state_handle();
    let recomputes = Rc::new(RefCell::new(0usize));
    let recomputes_inner = recomputes.clone();
    let _watch = ctx.memo(move |ctx| {
        *recomputes_inner.borrow_mut() += 1;
        state.get(ctx)
    });

    let baseline = *recomputes.borrow();

    assert!(m.send(&ctx, ()));
    assert_eq!(m.state(&ctx), 0);
    assert_eq!(*recomputes.borrow(), baseline);
}

// -- Reactive derived state -------------------------------------------------

#[test]
fn derived_slot_updates_on_transition() {
    let ctx = Context::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let label = ctx.memo(move |ctx| match state.get(ctx) {
        Door::Closed => "closed",
        Door::Opening => "opening",
        Door::Open => "open",
        Door::Closing => "closing",
    });

    assert_eq!(label.get(&ctx), "closed");
    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(label.get(&ctx), "opening");
}

#[test]
fn eager_signal_tracks_machine_state() {
    let ctx = Context::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let observed = Rc::new(RefCell::new(Vec::<Door>::new()));
    let observed_inner = observed.clone();
    let _sig = ctx.signal(move |ctx| {
        let s = state.get(ctx);
        observed_inner.borrow_mut().push(s.clone());
        s
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    m.send(&ctx, DoorEvent::FullyOpen);

    assert_eq!(
        observed.borrow().clone(),
        vec![Door::Closed, Door::Opening, Door::Open]
    );
}

// -- on_transition observer -------------------------------------------------

#[test]
fn on_transition_fires_with_old_and_new() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    let transitions = Rc::new(RefCell::new(Vec::<(Door, Door)>::new()));
    let transitions_inner = transitions.clone();
    let _observer = m.on_transition(&ctx, move |old, new| {
        transitions_inner
            .borrow_mut()
            .push((old.clone(), new.clone()));
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    m.send(&ctx, DoorEvent::FullyOpen);

    assert_eq!(
        transitions.borrow().clone(),
        vec![(Door::Closed, Door::Opening), (Door::Opening, Door::Open)]
    );
}

#[test]
fn on_transition_does_not_fire_on_rejected_event() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    let count = Rc::new(RefCell::new(0usize));
    let count_inner = count.clone();
    let _observer = m.on_transition(&ctx, move |_, _| {
        *count_inner.borrow_mut() += 1;
    });

    m.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(*count.borrow(), 0);

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(*count.borrow(), 1);
}

// -- state_is signal --------------------------------------------------------

#[test]
fn state_is_signal_reflects_current_state() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    let is_open = m.state_is(&ctx, Door::Open);
    let is_closed = m.state_is(&ctx, Door::Closed);

    assert!(!is_open.get(&ctx));
    assert!(is_closed.get(&ctx));

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert!(!is_open.get(&ctx));
    assert!(!is_closed.get(&ctx));

    m.send(&ctx, DoorEvent::FullyOpen);
    assert!(is_open.get(&ctx));
    assert!(!is_closed.get(&ctx));
}

// -- Effect cleanup as on-exit ---------------------------------------------

#[test]
fn effect_cleanup_runs_on_state_exit() {
    let ctx = Context::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let exits = Rc::new(RefCell::new(Vec::<Door>::new()));
    let exits_for_cleanup = exits.clone();

    let _lifecycle = ctx.effect(move |ctx| {
        let entered = state.get(ctx);
        let exits_inner = exits_for_cleanup.clone();
        move || exits_inner.borrow_mut().push(entered)
    });

    assert!(exits.borrow().is_empty());

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(exits.borrow().clone(), vec![Door::Closed]);

    m.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(exits.borrow().clone(), vec![Door::Closed, Door::Opening]);
}

// -- Dispose observer -------------------------------------------------------

#[test]
fn disposing_on_transition_stops_observing() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    let count = Rc::new(RefCell::new(0usize));
    let count_inner = count.clone();
    let observer = m.on_transition(&ctx, move |_, _| {
        *count_inner.borrow_mut() += 1;
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(*count.borrow(), 1);

    observer.dispose(&ctx);

    m.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(*count.borrow(), 1);
}

// -- Batch transitions ------------------------------------------------------

#[test]
fn batched_transactions_settle_before_effects_fire() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    let transitions = Rc::new(RefCell::new(0usize));
    let transitions_inner = transitions.clone();
    let _observer = m.on_transition(&ctx, move |_, _| {
        *transitions_inner.borrow_mut() += 1;
    });

    ctx.batch(|ctx| {
        m.send(ctx, DoorEvent::ButtonPressed);
        m.send(ctx, DoorEvent::FullyOpen);
    });

    assert_eq!(m.state(&ctx), Door::Open);
    assert_eq!(*transitions.borrow(), 1);
}
