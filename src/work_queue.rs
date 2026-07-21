//! Competing-consumer reactive work queue (`#lzworkqueue`).
//!
//! This is the portable local-authority lifecycle from lazily-spec. The owning
//! instance serializes `claim`; distributed/HA deployments put that decision
//! behind their leader or consensus log while preserving this API.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::rc::Rc;

use crate::{Context, FormulaCell};

/// A stable queued item. `attempts` counts leases already issued for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkQueueItem<T> {
    pub item_id: u64,
    pub value: T,
    pub attempts: u32,
}

/// One exclusive, worker-owned delivery lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkQueueDelivery<T, I = String> {
    pub delivery_id: u64,
    pub item_id: u64,
    pub value: T,
    pub worker: I,
    pub attempt: u32,
    pub deadline: u64,
}

/// Why an item exhausted its delivery budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkQueueDeadLetterReason {
    Nack,
    Expired,
}

/// A terminal poison-message record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkQueueDeadLetter<T> {
    pub item_id: u64,
    pub value: T,
    pub attempts: u32,
    pub reason: WorkQueueDeadLetterReason,
}

/// Independent reactive reader kinds for queue lifecycle state.
#[derive(Debug, Clone, Copy)]
pub struct WorkQueueReaderHandles {
    pub pending_len: FormulaCell<usize>,
    pub is_empty: FormulaCell<bool>,
    pub in_flight_len: FormulaCell<usize>,
    pub dead_letter_len: FormulaCell<usize>,
}

struct WorkQueueState<T, I> {
    pending: VecDeque<WorkQueueItem<T>>,
    in_flight: HashMap<u64, WorkQueueDelivery<T, I>>,
    dead_letters: Vec<WorkQueueDeadLetter<T>>,
    next_item_id: u64,
    next_delivery_id: u64,
}

#[derive(Clone, Copy)]
struct Counts {
    pending: usize,
    in_flight: usize,
    dead_letters: usize,
}

impl<T, I> WorkQueueState<T, I> {
    fn counts(&self) -> Counts {
        Counts {
            pending: self.pending.len(),
            in_flight: self.in_flight.len(),
            dead_letters: self.dead_letters.len(),
        }
    }
}

struct WorkQueueInner<T, I> {
    state: Rc<RefCell<WorkQueueState<T, I>>>,
    readers: WorkQueueReaderHandles,
    visibility_timeout: u64,
    max_deliveries: u32,
}

/// A pull-based work queue where N consumers compete for exclusive delivery.
pub struct WorkQueueCell<T, I = String> {
    inner: Rc<WorkQueueInner<T, I>>,
}

impl<T, I> Clone for WorkQueueCell<T, I> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T, I> WorkQueueCell<T, I>
where
    T: PartialEq + Clone + 'static,
    I: Eq + Hash + Clone + 'static,
{
    /// Create an empty local-authority work queue.
    ///
    /// # Panics
    ///
    /// Panics unless `visibility_timeout > 0` and `max_deliveries >= 1`.
    pub fn new(ctx: &Context, visibility_timeout: u64, max_deliveries: u32) -> Self {
        assert!(
            visibility_timeout > 0,
            "visibility_timeout must be positive"
        );
        assert!(max_deliveries >= 1, "max_deliveries must be at least one");

        let state = Rc::new(RefCell::new(WorkQueueState {
            pending: VecDeque::new(),
            in_flight: HashMap::new(),
            dead_letters: Vec::new(),
            next_item_id: 0,
            next_delivery_id: 0,
        }));

        let pending_len = {
            let state = Rc::clone(&state);
            ctx.memo(move |_| state.borrow().pending.len())
        };
        let is_empty = {
            let state = Rc::clone(&state);
            ctx.memo(move |_| state.borrow().pending.is_empty())
        };
        let in_flight_len = {
            let state = Rc::clone(&state);
            ctx.memo(move |_| state.borrow().in_flight.len())
        };
        let dead_letter_len = {
            let state = Rc::clone(&state);
            ctx.memo(move |_| state.borrow().dead_letters.len())
        };

        Self {
            inner: Rc::new(WorkQueueInner {
                state,
                readers: WorkQueueReaderHandles {
                    pending_len,
                    is_empty,
                    in_flight_len,
                    dead_letter_len,
                },
                visibility_timeout,
                max_deliveries,
            }),
        }
    }

    fn invalidate(&self, ctx: &Context, before: Counts, after: Counts) {
        let mut roots = Vec::with_capacity(4);
        if before.pending != after.pending {
            roots.push(self.inner.readers.pending_len.id);
        }
        if (before.pending == 0) != (after.pending == 0) {
            roots.push(self.inner.readers.is_empty.id);
        }
        if before.in_flight != after.in_flight {
            roots.push(self.inner.readers.in_flight_len.id);
        }
        if before.dead_letters != after.dead_letters {
            roots.push(self.inner.readers.dead_letter_len.id);
        }
        ctx.clear_slots(&roots);
    }

    fn fail_delivery(
        state: &mut WorkQueueState<T, I>,
        delivery: WorkQueueDelivery<T, I>,
        max_deliveries: u32,
        reason: WorkQueueDeadLetterReason,
    ) {
        if delivery.attempt < max_deliveries {
            state.pending.push_back(WorkQueueItem {
                item_id: delivery.item_id,
                value: delivery.value,
                attempts: delivery.attempt,
            });
        } else {
            state.dead_letters.push(WorkQueueDeadLetter {
                item_id: delivery.item_id,
                value: delivery.value,
                attempts: delivery.attempt,
                reason,
            });
        }
    }

    /// Append one item to the pending FIFO and return its stable identity.
    pub fn push(&self, ctx: &Context, value: T) -> u64 {
        let (item_id, before, after) = {
            let mut state = self.inner.state.borrow_mut();
            let before = state.counts();
            let item_id = state.next_item_id;
            state.next_item_id = state
                .next_item_id
                .checked_add(1)
                .expect("item id exhausted");
            state.pending.push_back(WorkQueueItem {
                item_id,
                value,
                attempts: 0,
            });
            (item_id, before, state.counts())
        };
        self.invalidate(ctx, before, after);
        item_id
    }

    /// Claim the oldest pending item for `worker`, or `None` when empty.
    pub fn claim(&self, ctx: &Context, worker: I, now: u64) -> Option<WorkQueueDelivery<T, I>> {
        let (delivery, before, after) = {
            let mut state = self.inner.state.borrow_mut();
            let before = state.counts();
            state.pending.front()?;
            let next_delivery_id = state
                .next_delivery_id
                .checked_add(1)
                .expect("delivery id exhausted");
            let item = state.pending.pop_front()?;
            let delivery_id = state.next_delivery_id;
            state.next_delivery_id = next_delivery_id;
            let delivery = WorkQueueDelivery {
                delivery_id,
                item_id: item.item_id,
                value: item.value,
                worker,
                attempt: item.attempts.saturating_add(1),
                deadline: now.saturating_add(self.inner.visibility_timeout),
            };
            state.in_flight.insert(delivery_id, delivery.clone());
            (delivery, before, state.counts())
        };
        self.invalidate(ctx, before, after);
        Some(delivery)
    }

    /// Settle a matching live delivery. Wrong-worker, stale, and duplicate acks are no-ops.
    pub fn ack(&self, ctx: &Context, worker: &I, delivery_id: u64) -> bool {
        let (before, after) = {
            let mut state = self.inner.state.borrow_mut();
            if !state
                .in_flight
                .get(&delivery_id)
                .is_some_and(|delivery| &delivery.worker == worker)
            {
                return false;
            }
            let before = state.counts();
            state.in_flight.remove(&delivery_id);
            (before, state.counts())
        };
        self.invalidate(ctx, before, after);
        true
    }

    /// Reject a matching delivery, requeueing at the tail or dead-lettering at the limit.
    pub fn nack(&self, ctx: &Context, worker: &I, delivery_id: u64) -> bool {
        let (before, after) = {
            let mut state = self.inner.state.borrow_mut();
            if !state
                .in_flight
                .get(&delivery_id)
                .is_some_and(|delivery| &delivery.worker == worker)
            {
                return false;
            }
            let before = state.counts();
            let delivery = state
                .in_flight
                .remove(&delivery_id)
                .expect("delivery exists");
            Self::fail_delivery(
                &mut state,
                delivery,
                self.inner.max_deliveries,
                WorkQueueDeadLetterReason::Nack,
            );
            (before, state.counts())
        };
        self.invalidate(ctx, before, after);
        true
    }

    /// Requeue/dead-letter every lease with `deadline < now` in delivery-id order.
    pub fn reap_expired(&self, ctx: &Context, now: u64) -> usize {
        let (expired_count, before, after) = {
            let mut state = self.inner.state.borrow_mut();
            let mut expired: Vec<u64> = state
                .in_flight
                .iter()
                .filter_map(|(id, delivery)| (delivery.deadline < now).then_some(*id))
                .collect();
            if expired.is_empty() {
                return 0;
            }
            expired.sort_unstable();
            let before = state.counts();
            for delivery_id in &expired {
                let delivery = state
                    .in_flight
                    .remove(delivery_id)
                    .expect("expired delivery exists");
                Self::fail_delivery(
                    &mut state,
                    delivery,
                    self.inner.max_deliveries,
                    WorkQueueDeadLetterReason::Expired,
                );
            }
            (expired.len(), before, state.counts())
        };
        self.invalidate(ctx, before, after);
        expired_count
    }

    pub fn pending_len(&self, ctx: &Context) -> usize {
        ctx.get(&self.inner.readers.pending_len)
    }

    pub fn is_empty(&self, ctx: &Context) -> bool {
        ctx.get(&self.inner.readers.is_empty)
    }

    pub fn in_flight_len(&self, ctx: &Context) -> usize {
        ctx.get(&self.inner.readers.in_flight_len)
    }

    pub fn dead_letter_len(&self, ctx: &Context) -> usize {
        ctx.get(&self.inner.readers.dead_letter_len)
    }

    pub fn reader_handles(&self) -> WorkQueueReaderHandles {
        self.inner.readers
    }

    /// Non-reactive pending snapshot, oldest first.
    pub fn pending(&self) -> Vec<WorkQueueItem<T>> {
        self.inner.state.borrow().pending.iter().cloned().collect()
    }

    /// Non-reactive in-flight snapshot sorted by delivery id.
    pub fn in_flight(&self) -> Vec<WorkQueueDelivery<T, I>> {
        let mut deliveries: Vec<_> = self
            .inner
            .state
            .borrow()
            .in_flight
            .values()
            .cloned()
            .collect();
        deliveries.sort_by_key(|delivery| delivery.delivery_id);
        deliveries
    }

    /// Non-reactive terminal dead-letter snapshot in failure order.
    pub fn dead_letters(&self) -> Vec<WorkQueueDeadLetter<T>> {
        self.inner.state.borrow().dead_letters.clone()
    }
}
