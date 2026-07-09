//! Reactive queue: [`QueueCell`] + pluggable [`QueueStorage`] backend (#lzqueue).
//!
//! A `QueueCell<T>` is a FIFO collection composed of reactive cells — **not a
//! new cell kind** — that adds queue semantics (push to tail, pop from head) to
//! the reactive graph. It is specified as a **single-producer, single-consumer
//! (SPSC)** primitive; **MPSC** (multi-producer) is a *usage rule* on the same
//! primitive — multiple producers push inside a [`Context::batch`] boundary, and
//! the batch serializes the pushes into a deterministic order. There is no
//! separate `MPSCQueueCell` type (`lazily-spec/cell-model.md` § "QueueCell —
//! SPSC primitive with MPSC usage rule").
//!
//! ## Shell vs storage
//!
//! The reactive shell owns the reader-kind version cells (`head` / `len` /
//! `is_empty` / `is_full` / `closed`) and the invalidation logic; it is
//! storage-agnostic. The storage backend owns the actual FIFO data structure and
//! is pluggable via [`QueueStorage`]. The default [`VecDequeStorage`] is an
//! unbounded ring buffer; a bounded variant exposes reactive backpressure via
//! `is_full`. A distributed backend (`RaftQueueStorage`, future work per the
//! distributed-queue PRD) or an external-broker adapter (`KafkaStorage`, etc.)
//! plugs into the same reactive shell.
//!
//! ## Reader-kind invalidation
//!
//! Invalidation is scoped to **reader kind**, not to individual positions. A
//! push invalidates `len` / `is_empty` readers (and `head` when transitioning
//! from empty, and `is_full` when transitioning to capacity); a pop invalidates
//! `head` / `len` / `is_empty` readers (and `is_full` when transitioning off
//! capacity). The head reader observes the *current* head value — after a pop,
//! the head reader sees the next element (or `None`), not a stale value.
//!
//! This reader-kind independence is implemented for free by the existing
//! `PartialEq` guard on [`Context::set_cell`]: after each op the shell re-derives
//! each reader-kind cell from the storage and writes it back, and a cell whose
//! value did not change is not invalidated.
//!
//! ## Closure, bounded backpressure, ordering
//!
//! - **Closure** is an observable contract: pop on closed+non-empty drains;
//!   pop on closed+empty returns [`QueuePopError::Closed`] (distinct from
//!   [`QueuePopError::Empty`]); push on closed is an error; close is idempotent
//!   and terminal.
//! - **Bounded backpressure**: when the backend is bounded, `is_full` is a
//!   reactive read. A consumer's pop that transitions full → not-full
//!   invalidates `is_full` readers (true → false), enabling push-side effects to
//!   react to capacity recovery without polling.
//! - **Ordering**: SPSC gives total FIFO (pop order exactly matches push order).
//!   MPSC gives per-producer FIFO; inter-producer interleaving is deterministic
//!   within a `batch()` but the cross-batch order is batch-sequential.
//!
//! ```
//! use lazily::{Context, QueueCell};
//!
//! let ctx = Context::new();
//! let q: QueueCell<&'static str> = QueueCell::new(&ctx);
//!
//! // SPSC: total FIFO.
//! q.try_push(&ctx, "a").unwrap();
//! q.try_push(&ctx, "b").unwrap();
//! assert_eq!(q.head(&ctx), Some("a"));
//! assert_eq!(q.len(&ctx), 2);
//!
//! assert_eq!(q.try_pop(&ctx).unwrap(), "a");
//! assert_eq!(q.try_pop(&ctx).unwrap(), "b");
//! assert!(q.is_empty(&ctx));
//!
//! // MPSC: multiple producers push inside one batch → one invalidation pass.
//! ctx.batch(|ctx| {
//!     q.try_push(ctx, "p1-a").unwrap();
//!     q.try_push(ctx, "p2-a").unwrap();
//!     q.try_push(ctx, "p1-b").unwrap();
//! });
//! assert_eq!(q.len(&ctx), 3);
//! ```
//!
//! See `lazily-spec/cell-model.md` § "Reactive queues" for the full spec, and
//! `lazily-spec/docs/distributed-queue-prd.md` for the future consensus-backed
//! `RaftQueueStorage` backend.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::Context;
use crate::cell::CellHandle;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes for [`QueueStorage::try_push`] / [`QueueCell::try_push`].
///
/// `Full` and `Closed` are the two observable rejection reasons distinguished by
/// the shell's contract (`lazily-spec/cell-model.md` § "Storage backend
/// contract"). Neither changes queue state, so neither invalidates any reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueuePushError {
    /// The backend is bounded and at capacity. The overflow policy (block /
    /// drop-oldest / drop-newest / reject) is a backend property; the reference
    /// [`VecDequeStorage`] rejects. Distinct from `Closed`.
    Full,
    /// The queue is closed; push is rejected regardless of capacity. Terminal —
    /// once closed, a queue cannot be reopened.
    Closed,
}

/// Failure modes for [`QueueStorage::try_pop`] / [`QueueCell::try_pop`].
///
/// `Empty` and `Closed` are distinct observable signals: `Empty` means "try
/// again later," `Closed` means "the producer is done and the queue is drained."
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueuePopError {
    /// The queue is open but contains no elements.
    Empty,
    /// The queue is closed and empty — the producer is done and all buffered
    /// elements have been consumed. Pop on a closed *non-empty* queue still
    /// drains (returns the next element); only closed+empty yields `Closed`.
    Closed,
}

// ---------------------------------------------------------------------------
// QueueStorage trait
// ---------------------------------------------------------------------------

/// Pluggable FIFO storage backend for a [`QueueCell`].
///
/// The shell / storage split (`lazily-spec/cell-model.md` § "Reactive shell vs
/// storage backend") keeps the reactive shell storage-agnostic: the shell owns
/// the reader-kind version cells and invalidation logic, the backend owns the
/// actual FIFO data structure. The default backend is [`VecDequeStorage`]
/// (unbounded `VecDeque`); future backends include `RaftQueueStorage` (embedded
/// consensus, per the distributed-queue PRD) and `KafkaStorage` /
/// `RedisStreamStorage` / `SqsStorage` (external-broker adapters).
///
/// # Conformance
///
/// A conforming backend MUST:
///
/// 1. **FIFO order** — `try_pop` returns elements in `try_push` order.
/// 2. **Cardinality compatibility** — its native producer/consumer shape is a
///    superset of the shell's required shape (SPSC shell = any backend; MPSC
///    usage requires a multi-writer backend).
/// 3. **Bounded contract (optional)** — a bounded backend exposes
///    [`capacity`](QueueStorage::capacity) as `Some(n)` and `try_push` returns
///    [`Full`](QueuePushError::Full) at capacity. The overflow policy is a
///    backend property.
/// 4. **Position identity** — invalidation is phrased over reader kind, not
///    storage indices. A ring-buffer backend whose slot index wraps MUST NOT
///    cause spurious invalidations; the shell layers its own logical version
///    counters (the reader-kind cells) above the storage.
//
// `is_empty` is deliberately NOT on this trait: emptiness is a shell-level
// reader kind, not a storage property (the shell derives `is_empty` from
// `len()`). See `lazily-spec/cell-model.md` § "Storage backend contract".
#[allow(clippy::len_without_is_empty)]
pub trait QueueStorage<T> {
    /// Append `value` to the tail. Returns [`QueuePushError::Full`] if bounded
    /// and at capacity, or [`QueuePushError::Closed`] if the queue is closed.
    /// On error the queue state is unchanged.
    fn try_push(&mut self, value: T) -> Result<(), QueuePushError>;

    /// Remove and return the head element. Returns [`QueuePopError::Empty`] if
    /// open and empty, or [`QueuePopError::Closed`] if closed and empty. Pop on
    /// a closed *non-empty* queue drains (returns the next element).
    fn try_pop(&mut self) -> Result<T, QueuePopError>;

    /// Peek the current head element without removing it. `None` when empty.
    /// The shell reads this to materialize its `head` reader-kind cell.
    fn peek(&self) -> Option<&T>;

    /// Current number of buffered elements.
    fn len(&self) -> usize;

    /// Bounded capacity, or `None` for an unbounded backend. When `Some(n)`,
    /// the shell exposes `is_full` as a reactive read.
    fn capacity(&self) -> Option<usize>;

    /// Whether the queue has been closed. Close is terminal — once true, it
    /// stays true.
    fn is_closed(&self) -> bool;

    /// Close the queue. Idempotent — closing an already-closed queue is a
    /// no-op. After close, [`try_push`](QueueStorage::try_push) returns
    /// [`Closed`](QueuePushError::Closed); [`try_pop`](QueueStorage::try_pop)
    /// continues to drain buffered elements and returns
    /// [`Closed`](QueuePopError::Closed) only once empty.
    fn close(&mut self);
}

// ---------------------------------------------------------------------------
// VecDequeStorage — the reference unbounded/bounded backend
// ---------------------------------------------------------------------------

/// The reference [`QueueStorage`] backend: a `VecDeque`-backed FIFO, optionally
/// bounded.
///
/// The unbounded form (the default) is what [`QueueCell::new`] uses; the bounded
/// form ([`VecDequeStorage::with_capacity`] / [`QueueCell::with_capacity`])
/// exposes reactive backpressure via the shell's `is_full` reader. The overflow
/// policy is **reject** — `try_push` at capacity returns
/// [`QueuePushError::Full`] (elements are never silently dropped); other
/// backends may choose block / drop-oldest / drop-newest.
///
/// Serializes as a JSON array (element order = FIFO order) for conformance
/// fixture purposes, matching `lazily-spec/cell-model.md` § "Wire and snapshot
/// shape".
#[derive(Debug, Clone, Default)]
pub struct VecDequeStorage<T> {
    elements: VecDeque<T>,
    capacity: Option<usize>,
    closed: bool,
}

impl<T> VecDequeStorage<T> {
    /// Create an unbounded storage (no capacity limit).
    pub fn unbounded() -> Self {
        Self {
            elements: VecDeque::new(),
            capacity: None,
            closed: false,
        }
    }

    /// Create a bounded storage that rejects pushes once it holds `capacity`
    /// elements.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` (a zero-capacity queue can never accept an
    /// element and has no useful semantics).
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "VecDequeStorage capacity must be > 0");
        Self {
            elements: VecDeque::with_capacity(capacity),
            capacity: Some(capacity),
            closed: false,
        }
    }

    /// Snapshot the buffered elements in FIFO order (clone of each element).
    /// Non-reactive — for snapshot/serde and conformance-fixture verification.
    pub fn elements(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.elements.iter().cloned().collect()
    }
}

impl<T> QueueStorage<T> for VecDequeStorage<T> {
    fn try_push(&mut self, value: T) -> Result<(), QueuePushError> {
        if self.closed {
            return Err(QueuePushError::Closed);
        }
        if let Some(cap) = self.capacity
            && self.elements.len() >= cap
        {
            return Err(QueuePushError::Full);
        }
        self.elements.push_back(value);
        Ok(())
    }

    fn try_pop(&mut self) -> Result<T, QueuePopError> {
        match self.elements.pop_front() {
            Some(v) => Ok(v),
            None => {
                if self.closed {
                    Err(QueuePopError::Closed)
                } else {
                    Err(QueuePopError::Empty)
                }
            }
        }
    }

    fn peek(&self) -> Option<&T> {
        self.elements.front()
    }

    fn len(&self) -> usize {
        self.elements.len()
    }

    fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    fn is_closed(&self) -> bool {
        self.closed
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

#[cfg(feature = "serde")]
impl<T> serde::Serialize for VecDequeStorage<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(None)?;
        for e in &self.elements {
            seq.serialize_element(e)?;
        }
        seq.end()
    }
}

// ---------------------------------------------------------------------------
// QueueCell — the reactive shell
// ---------------------------------------------------------------------------

struct QueueCellInner<T, S> {
    storage: std::cell::RefCell<S>,
    // Reader-kind version cells. The shell re-derives these from storage after
    // each op; the `PartialEq` guard on `set_cell` means a cell whose value did
    // not change is not invalidated — this is what implements reader-kind
    // independence for free.
    head: CellHandle<Option<T>>,
    len: CellHandle<usize>,
    is_empty: CellHandle<bool>,
    is_full: CellHandle<bool>,
    closed: CellHandle<bool>,
}

/// A reactive FIFO queue — SPSC primitive with an MPSC usage rule
/// (`#lzqueue`).
///
/// Cheap to [`Clone`] (an `Rc` to shared shell state), so the same queue can be
/// handed to producer and consumer closures. The reactive shell wraps a
/// pluggable [`QueueStorage`] backend (default [`VecDequeStorage`]); the shell
/// owns the reader-kind version cells (`head` / `len` / `is_empty` / `is_full`
/// / `closed`) and invalidates by reader kind — a push to a non-empty queue does
/// NOT invalidate the `head` reader, a pop does. See the module docs for the
/// full reader-kind independence contract.
pub struct QueueCell<T, S = VecDequeStorage<T>> {
    inner: Rc<QueueCellInner<T, S>>,
}

impl<T, S> Clone for QueueCell<T, S> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> QueueCell<T, VecDequeStorage<T>>
where
    T: PartialEq + Clone + 'static,
{
    /// Create an unbounded queue (the default reference backend).
    pub fn new(ctx: &Context) -> Self {
        Self::with_storage(ctx, VecDequeStorage::unbounded())
    }

    /// Create a bounded queue with `capacity`. Exposes reactive backpressure via
    /// [`is_full`](Self::is_full): a pop that transitions full → not-full
    /// invalidates `is_full` readers.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0`.
    pub fn with_capacity(ctx: &Context, capacity: usize) -> Self {
        Self::with_storage(ctx, VecDequeStorage::with_capacity(capacity))
    }
}

impl<T, S> QueueCell<T, S>
where
    T: PartialEq + Clone + 'static,
    S: QueueStorage<T>,
{
    /// Build a queue over an arbitrary [`QueueStorage`] backend. The shell
    /// initializes its reader-kind cells from the backend's current state.
    pub fn with_storage(ctx: &Context, storage: S) -> Self {
        let (head_val, len_val, is_full_val, closed_val) = {
            let is_full_val = match storage.capacity() {
                Some(cap) => storage.len() >= cap,
                None => false,
            };
            (
                storage.peek().cloned(),
                storage.len(),
                is_full_val,
                storage.is_closed(),
            )
        };
        Self {
            inner: Rc::new(QueueCellInner {
                storage: std::cell::RefCell::new(storage),
                head: ctx.cell(head_val),
                len: ctx.cell(len_val),
                is_empty: ctx.cell(len_val == 0),
                is_full: ctx.cell(is_full_val),
                closed: ctx.cell(closed_val),
            }),
        }
    }

    /// Re-derive the reader-kind cells from storage and write them back, in one
    /// atomic invalidation pass (a [`Context::batch`] groups the writes so an
    /// observer never sees a partial state — e.g. `len` bumped but `is_full` not
    /// yet flipped). The `PartialEq` guard on `set_cell` suppresses invalidation
    /// for any cell whose value did not change — this is the reader-kind
    /// independence law. `closed` is intentionally NOT touched here: it only
    /// changes via [`close`](Self::close).
    fn sync_content(&self, ctx: &Context) {
        let (head_val, len_val, is_empty_val, is_full_val) = {
            let s = self.inner.storage.borrow();
            let len_val = s.len();
            let is_full_val = match s.capacity() {
                Some(cap) => len_val >= cap,
                None => false,
            };
            (s.peek().cloned(), len_val, len_val == 0, is_full_val)
        };
        // Batch the writes: a push/pop is a single atomic op, so its reader-kind
        // cells must transition together. Without the batch, each set_cell
        // flushes effects immediately and an observer could glitch (len bumped
        // before is_full flips).
        ctx.batch(|ctx| {
            ctx.set_cell(&self.inner.head, head_val);
            ctx.set_cell(&self.inner.len, len_val);
            ctx.set_cell(&self.inner.is_empty, is_empty_val);
            ctx.set_cell(&self.inner.is_full, is_full_val);
        });
    }

    /// Append `value` to the tail of the queue.
    ///
    /// Returns [`QueuePushError::Full`] if bounded and at capacity (reject
    /// policy — the default `VecDequeStorage` never silently drops), or
    /// [`QueuePushError::Closed`] if the queue is closed. On error the queue
    /// state is unchanged and no reader is invalidated.
    ///
    /// Invalidates `head` (only when transitioning from empty), `len`, and
    /// `is_empty` readers as appropriate; `is_full` when transitioning onto
    /// capacity. Does not touch `closed`.
    pub fn try_push(&self, ctx: &Context, value: T) -> Result<(), QueuePushError> {
        let result = self.inner.storage.borrow_mut().try_push(value);
        if result.is_ok() {
            self.sync_content(ctx);
        }
        result
    }

    /// Remove and return the head element.
    ///
    /// Returns [`QueuePopError::Empty`] if open and empty, or
    /// [`QueuePopError::Closed`] if closed and empty. Pop on a closed
    /// *non-empty* queue drains (returns the next element).
    ///
    /// Invalidates `head` (always — the head value changes), `len`, and
    /// `is_empty` (when transitioning to empty) readers as appropriate;
    /// `is_full` when transitioning off capacity.
    pub fn try_pop(&self, ctx: &Context) -> Result<T, QueuePopError> {
        let result = self.inner.storage.borrow_mut().try_pop();
        if result.is_ok() {
            self.sync_content(ctx);
        }
        result
    }

    /// Close the queue. Idempotent — closing an already-closed queue is a
    /// no-op (no invalidation). Terminal — once closed, a queue cannot be
    /// reopened. After close, [`try_push`](Self::try_push) returns `Closed`;
    /// [`try_pop`](Self::try_pop) continues to drain and returns `Closed` only
    /// once empty.
    ///
    /// Invalidates the `closed` reader only on the false → true transition.
    pub fn close(&self, ctx: &Context) {
        let was_closed = self.inner.storage.borrow().is_closed();
        if was_closed {
            return;
        }
        self.inner.storage.borrow_mut().close();
        ctx.set_cell(&self.inner.closed, true);
    }

    // -- Reactive reader-kind reads ----------------------------------------

    /// Reactive read of the current head value. `None` when the queue is empty.
    /// A reader is invalidated when the head value *changes* — every pop, and a
    /// push only when transitioning from empty.
    pub fn head(&self, ctx: &Context) -> Option<T> {
        ctx.get_cell(&self.inner.head)
    }

    /// Handle to the `head` reader-kind cell, for wiring derived computeds
    /// directly. Subscribe-to-head semantics: invalidated on head-value change.
    pub fn head_handle(&self) -> CellHandle<Option<T>> {
        self.inner.head
    }

    /// Reactive read of the number of buffered elements. Invalidated whenever
    /// the count changes (every successful push/pop).
    pub fn len(&self, ctx: &Context) -> usize {
        ctx.get_cell(&self.inner.len)
    }

    /// Reactive emptiness check. Invalidated only on the empty ↔ non-empty
    /// transition.
    pub fn is_empty(&self, ctx: &Context) -> bool {
        ctx.get_cell(&self.inner.is_empty)
    }

    /// Reactive fullness check (only meaningful when the backend is bounded).
    /// Invalidated on the full ↔ not-full transition — this is the backpressure
    /// signal: a producer observes `is_full` and backs off; a consumer's pop
    /// that transitions full → not-full invalidates the producer's `is_full`
    /// subscription and the producer resumes. For an unbounded backend this is
    /// always `false` and never invalidates.
    pub fn is_full(&self, ctx: &Context) -> bool {
        ctx.get_cell(&self.inner.is_full)
    }

    /// Reactive read of the closed flag. Invalidated only on the open → closed
    /// transition.
    pub fn is_closed(&self, ctx: &Context) -> bool {
        ctx.get_cell(&self.inner.closed)
    }

    /// Handles to the reader-kind cells, for advanced wiring (e.g. effects that
    /// subscribe to multiple reader kinds).
    pub fn reader_handles(&self) -> QueueReaderHandles<T> {
        QueueReaderHandles {
            head: self.inner.head,
            len: self.inner.len,
            is_empty: self.inner.is_empty,
            is_full: self.inner.is_full,
            closed: self.inner.closed,
        }
    }

    // -- Non-reactive storage access ---------------------------------------

    /// The backend's capacity, or `None` if unbounded.
    pub fn capacity(&self) -> Option<usize> {
        self.inner.storage.borrow().capacity()
    }
}

impl<T> QueueCell<T, VecDequeStorage<T>>
where
    T: PartialEq + Clone + 'static,
{
    /// Snapshot the buffered elements in FIFO order. Non-reactive — for
    /// debugging, snapshot/serde, and conformance-fixture verification. There
    /// is no reactive random-access `queue[N]` reader; per-position reactivity
    /// is the domain of `CellMap`, not `QueueCell`.
    pub fn elements(&self) -> Vec<T> {
        self.inner.storage.borrow().elements()
    }
}

/// Handles to all five reader-kind cells of a [`QueueCell`], for effects that
/// need to subscribe to several reader kinds at once.
#[derive(Debug, Clone, Copy)]
pub struct QueueReaderHandles<T> {
    /// The head value (`None` when empty).
    pub head: CellHandle<Option<T>>,
    /// The element count.
    pub len: CellHandle<usize>,
    /// Whether the queue is empty.
    pub is_empty: CellHandle<bool>,
    /// Whether the queue is at capacity (bounded backpressure signal).
    pub is_full: CellHandle<bool>,
    /// Whether the queue has been closed.
    pub closed: CellHandle<bool>,
}

// ---------------------------------------------------------------------------
// TopicCell / WorkQueueCell — future-work stubs
// ---------------------------------------------------------------------------

// `TopicCell` (SPMC broadcast / MPMC pub-sub) and `WorkQueueCell` (true MPMC
// with exclusive handoff) are genuinely distinct primitives — they differ in
// *invalidation model and handoff semantics*, not in producer/consumer
// cardinality (see `lazily-spec/cell-model.md` § "Future queue primitives").
//
// They are reserved for future work and are NOT in v1 conformance:
//
// - **TopicCell** — every subscriber receives every pushed element. Each
//   subscriber maintains its own cursor; the topic retains elements until all
//   cursors have advanced past them (GC frontier = slowest subscriber). Lands
//   with the distributed-queue PRD Phase 3. Formal stub:
//   `lazily-formal/LazilyFormal/TopicCell.lean`.
//
// - **WorkQueueCell** — N consumers compete for elements from a shared FIFO;
//   each element is delivered to exactly one consumer (exclusive handoff). This
//   requires an authority (designated leader peer) to serialize pop-assignment —
//   pure CRDT cannot provide it. Lands with the distributed-queue PRD Phase 2
//   (consensus core). Formal stub:
//   `lazily-formal/LazilyFormal/WorkQueueCell.lean`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spsc_fifo_basic() {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);
        assert!(q.is_empty(&ctx));
        assert_eq!(q.head(&ctx), None);

        q.try_push(&ctx, 1).unwrap();
        q.try_push(&ctx, 2).unwrap();
        q.try_push(&ctx, 3).unwrap();

        assert_eq!(q.len(&ctx), 3);
        assert_eq!(q.head(&ctx), Some(1));
        assert_eq!(q.elements(), vec![1, 2, 3]);

        assert_eq!(q.try_pop(&ctx).unwrap(), 1);
        assert_eq!(q.try_pop(&ctx).unwrap(), 2);
        assert_eq!(q.try_pop(&ctx).unwrap(), 3);
        assert_eq!(q.try_pop(&ctx), Err(QueuePopError::Empty));
    }

    #[test]
    fn bounded_rejects_at_capacity() {
        let ctx = Context::new();
        let q = QueueCell::<i32>::with_capacity(&ctx, 2);
        assert_eq!(q.capacity(), Some(2));
        assert!(!q.is_full(&ctx));

        q.try_push(&ctx, 1).unwrap();
        q.try_push(&ctx, 2).unwrap();
        assert!(q.is_full(&ctx));
        assert_eq!(q.try_push(&ctx, 3), Err(QueuePushError::Full));

        // pop frees a slot → is_full flips → reactive backpressure signal
        assert_eq!(q.try_pop(&ctx).unwrap(), 1);
        assert!(!q.is_full(&ctx));
        q.try_push(&ctx, 3).unwrap();
        assert!(q.is_full(&ctx));
    }

    #[test]
    fn closure_lifecycle() {
        let ctx = Context::new();
        let q: QueueCell<&str> = QueueCell::new(&ctx);
        q.try_push(&ctx, "a").unwrap();
        q.try_push(&ctx, "b").unwrap();

        q.close(&ctx);
        assert!(q.is_closed(&ctx));

        // push on closed is an error
        assert_eq!(q.try_push(&ctx, "c"), Err(QueuePushError::Closed));

        // pop on closed+non-empty drains
        assert_eq!(q.try_pop(&ctx).unwrap(), "a");
        assert_eq!(q.try_pop(&ctx).unwrap(), "b");

        // pop on closed+empty returns Closed (distinct from Empty)
        assert_eq!(q.try_pop(&ctx), Err(QueuePopError::Closed));

        // idempotent close — no-op, no invalidation
        q.close(&ctx);
        assert!(q.is_closed(&ctx));
    }

    #[test]
    fn reader_kind_independence_head_not_invalidated_on_push_to_nonempty() {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);

        let head_reader = ctx.computed({
            let q = q.clone();
            move |ctx| q.head(ctx)
        });
        assert_eq!(ctx.get(&head_reader), None);

        q.try_push(&ctx, 1).unwrap();
        // push to empty changes head → invalidated
        assert_eq!(ctx.get(&head_reader), Some(1));

        q.try_push(&ctx, 2).unwrap();
        q.try_push(&ctx, 3).unwrap();
        // head reader still cached (head unchanged) — reader-kind independence
        assert!(
            ctx.is_set(&head_reader),
            "push to non-empty must not invalidate head reader"
        );

        q.try_pop(&ctx).unwrap();
        // pop changes head → invalidated
        assert_eq!(ctx.get(&head_reader), Some(2));
    }

    #[test]
    fn mpsc_via_batch_is_one_invalidation_pass() {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);

        let len_reader = ctx.computed({
            let q = q.clone();
            move |ctx| q.len(ctx)
        });
        assert_eq!(ctx.get(&len_reader), 0);

        ctx.batch(|ctx| {
            q.try_push(ctx, 10).unwrap();
            q.try_push(ctx, 20).unwrap();
            q.try_push(ctx, 30).unwrap();
        });
        // After the batch the len reader is invalidated exactly once and sees 3.
        assert!(
            !ctx.is_set(&len_reader),
            "batch should have invalidated the len reader once"
        );
        assert_eq!(ctx.get(&len_reader), 3);
        assert_eq!(q.elements(), vec![10, 20, 30]);
    }

    #[test]
    fn clone_shares_state() {
        let ctx = Context::new();
        let q: QueueCell<i32> = QueueCell::new(&ctx);
        let producer = q.clone();
        producer.try_push(&ctx, 42).unwrap();
        assert_eq!(q.try_pop(&ctx).unwrap(), 42);
    }
}
