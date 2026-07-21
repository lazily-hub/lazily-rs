//! Integration tests for the `StateMachine` state machine primitive.
//!
//! A `StateMachine` wraps a `SourceCell<S>` + transition function, exposing
//! `send(event)`, `state()`, `on_transition()`, and `state_is()`.

use std::cell::RefCell;
use std::rc::Rc;

use lazily::{Context, StateMachine};

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
    let m = StateMachine::new(&ctx, Light::Red, |s, _: &Tick| match s {
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

fn garage_door(ctx: &Context) -> StateMachine<Door, DoorEvent> {
    StateMachine::new(ctx, Door::Closed, |s, e| match (s, e) {
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

    let m = StateMachine::new(&ctx, 0i32, move |s, _: &()| {
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

#[derive(PartialEq, Clone, Debug)]
enum ActiveDoor {
    Primary,
    Secondary,
}

#[derive(Debug)]
enum ActiveDoorEvent {
    Toggle,
}

#[test]
fn derived_slot_drops_stale_machine_dependency_after_branch_switch() {
    let ctx = Context::new();
    let active = StateMachine::new(
        &ctx,
        ActiveDoor::Primary,
        |s, _: &ActiveDoorEvent| match s {
            ActiveDoor::Primary => Some(ActiveDoor::Secondary),
            ActiveDoor::Secondary => Some(ActiveDoor::Primary),
        },
    );
    let primary = garage_door(&ctx);
    let secondary = garage_door(&ctx);
    let active_state = active.state_handle();
    let primary_state = primary.state_handle();
    let secondary_state = secondary.state_handle();

    let recomputes = Rc::new(RefCell::new(0usize));
    let recomputes_inner = recomputes.clone();
    let selected_label = ctx.memo(move |ctx| {
        *recomputes_inner.borrow_mut() += 1;
        match active_state.get(ctx) {
            ActiveDoor::Primary => match primary_state.get(ctx) {
                Door::Closed => "primary:closed",
                Door::Opening => "primary:opening",
                Door::Open => "primary:open",
                Door::Closing => "primary:closing",
            },
            ActiveDoor::Secondary => match secondary_state.get(ctx) {
                Door::Closed => "secondary:closed",
                Door::Opening => "secondary:opening",
                Door::Open => "secondary:open",
                Door::Closing => "secondary:closing",
            },
        }
    });

    assert_eq!(selected_label.get(&ctx), "primary:closed");
    primary.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(selected_label.get(&ctx), "primary:opening");
    active.send(&ctx, ActiveDoorEvent::Toggle);
    assert_eq!(selected_label.get(&ctx), "secondary:closed");
    let after_switch = *recomputes.borrow();

    primary.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(selected_label.get(&ctx), "secondary:closed");
    assert_eq!(
        *recomputes.borrow(),
        after_switch,
        "branch switch must remove the stale primary state dependency"
    );

    secondary.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(selected_label.get(&ctx), "secondary:opening");
    assert_eq!(*recomputes.borrow(), after_switch + 1);
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

#[test]
fn recreating_on_transition_observer_starts_fresh_after_dispose() {
    let ctx = Context::new();
    let m = garage_door(&ctx);

    let first = Rc::new(RefCell::new(Vec::<(Door, Door)>::new()));
    let first_inner = first.clone();
    let observer = m.on_transition(&ctx, move |old, new| {
        first_inner.borrow_mut().push((old.clone(), new.clone()));
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    observer.dispose(&ctx);

    let second = Rc::new(RefCell::new(Vec::<(Door, Door)>::new()));
    let second_inner = second.clone();
    let _observer = m.on_transition(&ctx, move |old, new| {
        second_inner.borrow_mut().push((old.clone(), new.clone()));
    });

    m.send(&ctx, DoorEvent::FullyOpen);
    m.send(&ctx, DoorEvent::ButtonPressed);

    assert_eq!(
        first.borrow().clone(),
        vec![(Door::Closed, Door::Opening)],
        "disposed observer must not leak callbacks after recreation"
    );
    assert_eq!(
        second.borrow().clone(),
        vec![(Door::Opening, Door::Open), (Door::Open, Door::Closing)],
        "new observer should seed from current state and only see future transitions"
    );
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
