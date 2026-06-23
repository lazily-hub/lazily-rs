//! Integration tests for the `AsyncStateMachine` primitive (Tokio).
//!
//! `send`/`state` are synchronous (cells are the sync input layer of
//! `AsyncContext`); `on_transition` and `state_is` use the async effect/signal
//! APIs and settle on the runtime.

#![cfg(feature = "async")]

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use lazily::{AsyncContext, AsyncStateMachine};

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

#[tokio::test]
async fn async_machine_transitions_through_all_states() {
    let ctx = AsyncContext::new();
    let m = AsyncStateMachine::new(&ctx, Light::Red, |s, _: &Tick| match s {
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

fn garage_door(ctx: &AsyncContext) -> AsyncStateMachine<Door, DoorEvent> {
    AsyncStateMachine::new(ctx, Door::Closed, |s, e| match (s, e) {
        (Door::Closed, DoorEvent::ButtonPressed) => Some(Door::Opening),
        (Door::Opening, DoorEvent::FullyOpen) => Some(Door::Open),
        (Door::Open, DoorEvent::ButtonPressed) => Some(Door::Closing),
        (Door::Closing, DoorEvent::FullyClosed) => Some(Door::Closed),
        (Door::Closing, DoorEvent::ButtonPressed) => Some(Door::Opening),
        _ => None,
    })
}

#[tokio::test]
async fn async_machine_rejects_invalid_transition() {
    let ctx = AsyncContext::new();
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

#[tokio::test]
async fn async_machine_self_transition_is_accepted_but_no_invalidation() {
    let ctx = AsyncContext::new();
    let call_count = Arc::new(Mutex::new(0usize));
    let call_count_inner = call_count.clone();

    let m = AsyncStateMachine::new(&ctx, 0i32, move |s, _: &()| {
        *call_count_inner.lock().unwrap() += 1;
        Some(*s)
    });

    let state = m.state_handle();
    let recomputes = Arc::new(Mutex::new(0usize));
    let recomputes_inner = recomputes.clone();
    let _watch = ctx.memo_async(move |cctx| {
        let r = recomputes_inner.clone();
        async move {
            *r.lock().unwrap() += 1;
            cctx.get_cell(&state)
        }
    });

    // Prime the memo so the baseline is established.
    let _ = ctx.get_async(&_watch).await;
    let baseline = *recomputes.lock().unwrap();

    assert!(m.send(&ctx, ()));
    assert_eq!(m.state(&ctx), 0);
    // No set_cell change (Some(equal_state)) — memo must not recompute.
    assert_eq!(*recomputes.lock().unwrap(), baseline);
}

// -- Reactive derived state -------------------------------------------------

#[tokio::test]
async fn async_derived_slot_updates_on_transition() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let label = ctx.memo_async(move |cctx| {
        let s = state;
        async move {
            match cctx.get_cell(&s) {
                Door::Closed => "closed",
                Door::Opening => "opening",
                Door::Open => "open",
                Door::Closing => "closing",
            }
        }
    });

    assert_eq!(ctx.get_async(&label).await, "closed");
    m.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(ctx.get_async(&label).await, "opening");
}

#[tokio::test]
async fn async_eager_signal_tracks_machine_state() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);
    let state = m.state_handle();

    let observed = Arc::new(Mutex::new(Vec::<Door>::new()));
    let observed_inner = observed.clone();
    let _sig = ctx.signal_async(move |cctx| {
        let o = observed_inner.clone();
        async move {
            let s = cctx.get_cell(&state);
            o.lock().unwrap().push(s.clone());
            s
        }
    });

    // Let the eager signal's initial resolve settle.
    tokio::time::sleep(Duration::from_millis(50)).await;

    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    m.send(&ctx, DoorEvent::FullyOpen);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        observed.lock().unwrap().clone(),
        vec![Door::Closed, Door::Opening, Door::Open]
    );
}

// -- on_transition observer -------------------------------------------------

#[tokio::test]
async fn async_on_transition_fires_with_old_and_new() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);

    let transitions = Arc::new(Mutex::new(Vec::<(Door, Door)>::new()));
    let transitions_inner = transitions.clone();
    let _observer = m.on_transition(&ctx, move |old, new| {
        transitions_inner
            .lock()
            .unwrap()
            .push((old.clone(), new.clone()));
    });

    // Let the initial effect run settle (it records prev but must not fire).
    tokio::time::sleep(Duration::from_millis(50)).await;

    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    m.send(&ctx, DoorEvent::FullyOpen);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        transitions.lock().unwrap().clone(),
        vec![(Door::Closed, Door::Opening), (Door::Opening, Door::Open)]
    );
}

#[tokio::test]
async fn async_on_transition_does_not_fire_on_rejected_event() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);

    let count = Arc::new(Mutex::new(0usize));
    let count_inner = count.clone();
    let _observer = m.on_transition(&ctx, move |_, _| {
        *count_inner.lock().unwrap() += 1;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    m.send(&ctx, DoorEvent::FullyOpen);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(*count.lock().unwrap(), 0);

    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(*count.lock().unwrap(), 1);
}

// -- state_is signal --------------------------------------------------------

#[tokio::test]
async fn async_state_is_signal_reflects_current_state() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);

    let is_open = m.state_is(&ctx, Door::Open);
    let is_closed = m.state_is(&ctx, Door::Closed);

    // Let the eager signals resolve initially.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!is_open.get(&ctx).unwrap_or(false));
    assert!(is_closed.get(&ctx).unwrap_or(false));

    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!is_open.get(&ctx).unwrap_or(false));
    assert!(!is_closed.get(&ctx).unwrap_or(false));

    m.send(&ctx, DoorEvent::FullyOpen);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(is_open.get(&ctx).unwrap_or(false));
    assert!(!is_closed.get(&ctx).unwrap_or(false));
}

// -- Dispose observer -------------------------------------------------------

#[tokio::test]
async fn async_disposing_on_transition_stops_observing() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);

    let count = Arc::new(Mutex::new(0usize));
    let count_inner = count.clone();
    let observer = m.on_transition(&ctx, move |_, _| {
        *count_inner.lock().unwrap() += 1;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(*count.lock().unwrap(), 1);

    ctx.dispose_async_effect(&observer);

    m.send(&ctx, DoorEvent::FullyOpen);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(*count.lock().unwrap(), 1);
}

// -- Batch transitions ------------------------------------------------------

#[tokio::test]
async fn async_batched_transactions_settle_to_final_state() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);

    // A batch coalesces invalidation; the final read reflects the last send.
    ctx.batch(|ctx| {
        m.send(ctx, DoorEvent::ButtonPressed);
        m.send(ctx, DoorEvent::FullyOpen);
    });

    assert_eq!(m.state(&ctx), Door::Open);
}
