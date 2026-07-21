//! Actor recipe — a mailbox actor from `QueueCell` + RPC message passing.
//!
//! An *actor* is a unit of private state that talks to the outside world only
//! through messages. This recipe wires one from two lazily primitives and
//! nothing else — no thread, no polling loop, no async runtime:
//!
//! * **Mailbox** — a `QueueCell<Request>` the actor drains *reactively*.
//!   `QueueCell` invalidates its `is_empty` reader on the empty → non-empty
//!   transition, so a `try_push` reruns the actor's drain effect the instant a
//!   message lands. The single-threaded scheduler flushes effects synchronously
//!   at the end of the push, so by the time `send` returns the message has been
//!   handled.
//! * **RPC / request–response** — each request carries a correlation `id`. The
//!   actor answers `Get` by pushing a `Reply { id, .. }` onto a shared
//!   **outbox** `QueueCell<Reply>`; the caller pops the reply matching its id.
//!   Correlation-by-id (rather than a reply `QueueCell` embedded per message)
//!   keeps every payload `PartialEq + Clone`, which is what `QueueCell<T>`
//!   requires of its element.
//! * **Fire-and-forget** — `Add` produces no reply. It is pure message passing:
//!   the actor mutates its private state and returns nothing.
//!
//! The actor's own state (`total`) lives in a plain `Cell` behind an `Rc`, not a
//! reactive cell: it is the actor's encapsulated secret, deliberately kept out
//! of the reactive graph so the drain effect subscribes to the mailbox alone
//! and terminates cleanly once drained.
//!
//! Run with: `cargo run --example actor_rpc`

use std::cell::Cell as StdCell;
use std::rc::Rc;

use lazily::{Context, Effect, QueueCell};

/// A message addressed to the counter actor. `id` correlates an RPC request
/// with its reply; fire-and-forget requests carry an id too (unused on the
/// reply path) so the wire shape stays uniform.
#[derive(Clone, PartialEq)]
enum Request {
    /// Fire-and-forget: add `n` to the running total. No reply.
    Add { id: u64, n: i64 },
    /// RPC: report the current total on the outbox, tagged with `id`.
    Get { id: u64 },
}

/// The actor's reply to a `Get`, correlated back to the request by `id`.
#[derive(Clone, PartialEq, Debug)]
struct Reply {
    id: u64,
    total: i64,
}

/// A handle to a spawned counter actor. Cloneable (both queues are `Rc`
/// handles); dropping the last `CounterActor` drops the drain effect and tears
/// the actor down.
#[derive(Clone)]
struct CounterActor {
    mailbox: QueueCell<Request>,
    outbox: QueueCell<Reply>,
    // Keeping the effect handle alive keeps the actor draining. `Rc` so the
    // handle survives every `CounterActor` clone.
    _drain: Rc<Effect>,
}

impl CounterActor {
    /// Spawn a counter actor on `ctx`, starting from `start`.
    ///
    /// Installs a reactive drain effect: subscribed to the mailbox's `is_empty`
    /// reader, it wakes on every push, drains the mailbox to empty, applies each
    /// request to the private total, and answers each `Get` on the outbox.
    fn spawn(ctx: &Context, start: i64) -> Self {
        let mailbox: QueueCell<Request> = QueueCell::new(ctx);
        let outbox: QueueCell<Reply> = QueueCell::new(ctx);
        // Private, non-reactive actor state — the encapsulated secret.
        let total = Rc::new(StdCell::new(start));

        let drain = {
            let mailbox = mailbox.clone();
            let outbox = outbox.clone();
            let total = Rc::clone(&total);
            ctx.effect(move |ctx| {
                // Reading `is_empty` subscribes the effect to the mailbox: the
                // next empty → non-empty push reruns us. Drain fully to empty so
                // one wake handles a whole batch of messages.
                while !mailbox.is_empty(ctx) {
                    let Ok(request) = mailbox.try_pop(ctx) else {
                        break;
                    };
                    match request {
                        Request::Add { n, .. } => {
                            total.set(total.get() + n);
                        }
                        Request::Get { id } => {
                            let reply = Reply {
                                id,
                                total: total.get(),
                            };
                            // Outbox is unbounded; a push only fails if closed.
                            let _ = outbox.try_push(ctx, reply);
                        }
                    }
                }
            })
        };

        Self {
            mailbox,
            outbox,
            _drain: Rc::new(drain),
        }
    }

    /// Send a message. Because the scheduler flushes effects synchronously, the
    /// actor has fully handled `request` (and posted any reply) by the time this
    /// returns.
    fn send(&self, ctx: &Context, request: Request) {
        // Mailbox is unbounded; push only fails if closed.
        let _ = self.mailbox.try_push(ctx, request);
    }

    /// Blocking-free RPC: send a `Get` and take the correlated reply. Single
    /// threaded, so the reply is already on the outbox when `send` returns —
    /// this just pops the entry whose id matches.
    fn get(&self, ctx: &Context, id: u64) -> i64 {
        self.send(ctx, Request::Get { id });
        // Drain the outbox, returning the reply for our id. In a single-caller
        // demo there is exactly one waiting reply; the id match is what makes
        // this safe with many concurrent callers sharing the outbox.
        loop {
            match self.outbox.try_pop(ctx) {
                Ok(reply) if reply.id == id => return reply.total,
                // A reply for someone else — requeue and keep looking. (Won't
                // happen in this single-caller demo, but shows the contract.)
                Ok(other) => {
                    let _ = self.outbox.try_push(ctx, other);
                }
                Err(_) => panic!("no reply for request {id}"),
            }
        }
    }
}

fn main() {
    let ctx = Context::new();
    let actor = CounterActor::spawn(&ctx, 0);

    // Fire-and-forget message passing: no reply, handled the moment it lands.
    actor.send(&ctx, Request::Add { id: 1, n: 5 });
    actor.send(&ctx, Request::Add { id: 2, n: 3 });

    // RPC request–response, correlated by id.
    let total = actor.get(&ctx, 100);
    println!("after Add(5), Add(3): total = {total}"); // 8
    assert_eq!(total, 8);

    // The actor keeps running; more messages, another RPC.
    actor.send(&ctx, Request::Add { id: 3, n: -2 });
    let total = actor.get(&ctx, 101);
    println!("after Add(-2):        total = {total}"); // 6
    assert_eq!(total, 6);

    println!("actor recipe ok");
}
