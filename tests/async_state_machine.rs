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
use tokio::sync::Barrier;

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

#[derive(PartialEq, Clone, Debug)]
enum ActiveDoor {
    Primary,
    Secondary,
}

#[derive(Debug)]
enum ActiveDoorEvent {
    Toggle,
}

#[tokio::test]
async fn async_derived_slot_drops_stale_machine_dependency_after_branch_switch() {
    let ctx = AsyncContext::new();
    let active = AsyncStateMachine::new(
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

    let recomputes = Arc::new(Mutex::new(0usize));
    let recomputes_inner = recomputes.clone();
    let selected_label = ctx.memo_async(move |cctx| {
        let recomputes = recomputes_inner.clone();
        async move {
            *recomputes.lock().unwrap() += 1;
            match cctx.get_cell(&active_state) {
                ActiveDoor::Primary => match cctx.get_cell(&primary_state) {
                    Door::Closed => "primary:closed",
                    Door::Opening => "primary:opening",
                    Door::Open => "primary:open",
                    Door::Closing => "primary:closing",
                },
                ActiveDoor::Secondary => match cctx.get_cell(&secondary_state) {
                    Door::Closed => "secondary:closed",
                    Door::Opening => "secondary:opening",
                    Door::Open => "secondary:open",
                    Door::Closing => "secondary:closing",
                },
            }
        }
    });

    assert_eq!(ctx.get_async(&selected_label).await, "primary:closed");
    primary.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(ctx.get_async(&selected_label).await, "primary:opening");
    active.send(&ctx, ActiveDoorEvent::Toggle);
    assert_eq!(ctx.get_async(&selected_label).await, "secondary:closed");
    let after_switch = *recomputes.lock().unwrap();

    primary.send(&ctx, DoorEvent::FullyOpen);
    assert_eq!(ctx.get_async(&selected_label).await, "secondary:closed");
    assert_eq!(
        *recomputes.lock().unwrap(),
        after_switch,
        "branch switch must remove the stale primary state dependency"
    );

    secondary.send(&ctx, DoorEvent::ButtonPressed);
    assert_eq!(ctx.get_async(&selected_label).await, "secondary:opening");
    assert_eq!(*recomputes.lock().unwrap(), after_switch + 1);
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

#[tokio::test]
async fn async_recreating_on_transition_observer_starts_fresh_after_dispose() {
    let ctx = AsyncContext::new();
    let m = garage_door(&ctx);

    let first = Arc::new(Mutex::new(Vec::<(Door, Door)>::new()));
    let first_inner = first.clone();
    let observer = m.on_transition(&ctx, move |old, new| {
        first_inner.lock().unwrap().push((old.clone(), new.clone()));
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    ctx.dispose_async_effect(&observer);

    let second = Arc::new(Mutex::new(Vec::<(Door, Door)>::new()));
    let second_inner = second.clone();
    let _observer = m.on_transition(&ctx, move |old, new| {
        second_inner
            .lock()
            .unwrap()
            .push((old.clone(), new.clone()));
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    m.send(&ctx, DoorEvent::FullyOpen);
    tokio::time::sleep(Duration::from_millis(50)).await;
    m.send(&ctx, DoorEvent::ButtonPressed);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        first.lock().unwrap().clone(),
        vec![(Door::Closed, Door::Opening)],
        "disposed observer must not leak callbacks after recreation"
    );
    assert_eq!(
        second.lock().unwrap().clone(),
        vec![(Door::Opening, Door::Open), (Door::Open, Door::Closing)],
        "new observer should seed from current state and only see future transitions"
    );
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

#[tokio::test]
async fn async_machine_handles_concurrent_send_and_state_reads() {
    let ctx = Arc::new(AsyncContext::new());
    let m = AsyncStateMachine::new(&ctx, 0usize, |_, next: &usize| Some(*next));
    let start = Arc::new(Barrier::new(5));

    let writer_ctx = Arc::clone(&ctx);
    let writer_machine = m.clone();
    let writer_start = Arc::clone(&start);
    let writer = tokio::spawn(async move {
        writer_start.wait().await;
        for value in 1..=250 {
            assert!(writer_machine.send(&writer_ctx, value));
        }
    });

    let mut readers = (0..4)
        .map(|_| {
            let reader_ctx = Arc::clone(&ctx);
            let reader_machine = m.clone();
            let reader_start = Arc::clone(&start);
            tokio::spawn(async move {
                reader_start.wait().await;
                let mut last_seen = 0usize;
                for _ in 0..250 {
                    let current = reader_machine.state(&reader_ctx);
                    assert!(current <= 250);
                    last_seen = last_seen.max(current);
                    tokio::task::yield_now().await;
                }
                last_seen
            })
        })
        .collect::<Vec<_>>();

    writer.await.unwrap();
    let mut max_seen = 0usize;
    for reader in readers.drain(..) {
        max_seen = max_seen.max(reader.await.unwrap());
    }

    assert_eq!(m.state(&ctx), 250);
    assert!(max_seen <= 250);
}
