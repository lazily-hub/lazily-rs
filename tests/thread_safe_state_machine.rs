//! Integration tests for the `ThreadSafeStateMachine` primitive.
//!
//! Mirrors `tests/state_machine.rs` over the lock-backed `ThreadSafeContext`.
//! A `ThreadSafeStateMachine` wraps a `CellHandle<S>` + `Send + Sync` transition
//! function, exposing `send(event)`, `state()`, `on_transition()`, and
//! `state_is()`.

use std::sync::Arc;
use std::sync::Mutex;

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};

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
fn thread_safe_machine_transitions_through_all_states() {
    let ctx = ThreadSafeContext::new();
    let m = ThreadSafeStateMachine::new(&ctx, Light::Red, |s, _: &Tick| match s {
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

fn garage_door(ctx: &ThreadSafeContext) -> ThreadSafeStateMachine<Door, DoorEvent> {
    ThreadSafeStateMachine::new(ctx, Door::Closed, |s, e| match (s, e) {
        (Door::Closed, DoorEvent::ButtonPressed) => Some(Door::Opening),
        (Door::Opening, DoorEvent::FullyOpen) => Some(Door::Open),
        (Door::Open, DoorEvent::ButtonPressed) => Some(Door::Closing),
        (Door::Closing, DoorEvent::FullyClosed) => Some(Door::Closed),
        (Door::Closing, DoorEvent::ButtonPressed) => Some(Door::Opening),
        _ => None,
    })
}

#[test]
fn thread_safe_machine_rejects_invalid_transition() {
    let ctx = ThreadSafeContext::new();
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
fn thread_safe_machine_self_transition_is_accepted_but_no_invalidation() {
    let ctx = ThreadSafeContext::new();
    let call_count = Arc::new(Mutex::new(0usize));
    let call_count_inner = call_count.clone();

    let m = ThreadSafeStateMachine::new(&ctx, 0i32, move |s, _: &()| {
        *call_count_inner.lock().unwrap() += 1;
        Some(*s)
    });

    let state = m.state_handle();
    let recomputes = Arc::new(Mutex::new(0usize));
    let recomputes_inner = recomputes.clone();
    let _watch = ctx.memo(move |ctx| {
        *recomputes_inner.lock().unwrap() += 1;
        ctx.get_cell(&state)
    });

    let baseline = *recomputes.lock().unwrap();

    assert!(m.send(&ctx, ()));
    assert_eq!(m.state(&ctx), 0);
    assert_eq!(*recomputes.lock().unwrap(), baseline);
}

// -- Reactive derived state -------------------------------------------------

#[test]
fn thread_safe_derived_slot_updates_on_transition() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let label = ctx.memo(move |ctx| match ctx.get_cell(&state) {
        Door::Closed => "closed",
        Door::Opening => "opening",
        Door::Open => "open",
        Door::Closing => "closing",
    });

    assert_eq!(ctx.get(&label), "closed");
    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(ctx.get(&label), "opening");
}

#[test]
fn thread_safe_eager_signal_tracks_machine_state() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let observed = Arc::new(Mutex::new(Vec::<Door>::new()));
    let observed_inner = observed.clone();
    let _sig = ctx.signal(move |ctx| {
        let s = ctx.get_cell(&state);
        observed_inner.lock().unwrap().push(s.clone());
        s
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    m.send(&ctx, DoorEvent::FullyOpen);

    assert_eq!(
        observed.lock().unwrap().clone(),
        vec![Door::Closed, Door::Opening, Door::Open]
    );
}

// -- on_transition observer -------------------------------------------------

#[test]
fn thread_safe_on_transition_fires_with_old_and_new() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);

    let transitions = Arc::new(Mutex::new(Vec::<(Door, Door)>::new()));
    let transitions_inner = transitions.clone();
    let _observer = m.on_transition(&ctx, move |old, new| {
        transitions_inner
            .lock()
            .unwrap()
            .push((old.clone(), new.clone()));
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    m.send(&ctx, DoorEvent::FullyOpen);

    assert_eq!(
        transitions.lock().unwrap().clone(),
        vec![(Door::Closed, Door::Opening), (Door::Opening, Door::Open)]
    );
}

#[test]
fn thread_safe_on_transition_does_not_fire_on_rejected_event() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);

    let count = Arc::new(Mutex::new(0usize));
    let count_inner = count.clone();
    let _observer = m.on_transition(&ctx, move |_, _| {
        *count_inner.lock().unwrap() += 1;
    });

    m.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(*count.lock().unwrap(), 0);

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(*count.lock().unwrap(), 1);
}

// -- state_is signal --------------------------------------------------------

#[test]
fn thread_safe_state_is_signal_reflects_current_state() {
    let ctx = ThreadSafeContext::new();
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
fn thread_safe_effect_cleanup_runs_on_state_exit() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let exits = Arc::new(Mutex::new(Vec::<Door>::new()));
    let exits_for_cleanup = exits.clone();

    let _lifecycle = ctx.effect(move |ctx| {
        let entered = ctx.get_cell(&state);
        let exits_inner = exits_for_cleanup.clone();
        move || exits_inner.lock().unwrap().push(entered)
    });

    assert!(exits.lock().unwrap().is_empty());

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(exits.lock().unwrap().clone(), vec![Door::Closed]);

    m.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(
        exits.lock().unwrap().clone(),
        vec![Door::Closed, Door::Opening]
    );
}

// -- Dispose observer -------------------------------------------------------

#[test]
fn thread_safe_disposing_on_transition_stops_observing() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);

    let count = Arc::new(Mutex::new(0usize));
    let count_inner = count.clone();
    let observer = m.on_transition(&ctx, move |_, _| {
        *count_inner.lock().unwrap() += 1;
    });

    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(*count.lock().unwrap(), 1);

    ctx.dispose_effect(&observer);

    m.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(*count.lock().unwrap(), 1);
}

// -- Batch transitions ------------------------------------------------------

#[test]
fn thread_safe_batched_transactions_settle_before_effects_fire() {
    let ctx = ThreadSafeContext::new();
    let m = garage_door(&ctx);

    let transitions = Arc::new(Mutex::new(0usize));
    let transitions_inner = transitions.clone();
    let _observer = m.on_transition(&ctx, move |_, _| {
        *transitions_inner.lock().unwrap() += 1;
    });

    ctx.batch(|ctx| {
        m.send(ctx, DoorEvent::ButtonPressed);
        m.send(ctx, DoorEvent::FullyOpen);
    });

    assert_eq!(m.state(&ctx), Door::Open);
    assert_eq!(*transitions.lock().unwrap(), 1);
}

// -- Cross-thread sharing ---------------------------------------------------

#[test]
fn thread_safe_machine_shares_state_across_threads() {
    let ctx = Arc::new(ThreadSafeContext::new());
    let m = ThreadSafeStateMachine::new(&ctx, 0i32, |s, e: &i32| Some(*s + *e));
    let ctx_a = Arc::clone(&ctx);
    let m_a = m.clone();
    let handle = std::thread::spawn(move || {
        m_a.send(&ctx_a, 10);
        m_a.state(&ctx_a)
    });
    let from_thread = handle.join().unwrap();
    assert_eq!(from_thread, 10);
    assert_eq!(m.state(&ctx), 10);
}
