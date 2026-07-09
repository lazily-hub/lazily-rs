# lazily v0.22.0

Minor release over v0.21.x. Adds the reactive queue primitive
(`#lzqueue`) — the foundation for the distributed-queue roadmap
(`lazily-spec/docs/distributed-queue-prd.md`).

## Highlights

**New: `QueueCell` + `QueueStorage` adapter.** A reactive FIFO queue composed of
cells — not a new cell kind — specified as a single-producer / single-consumer
(SPSC) primitive with an MPSC (multi-producer) usage rule on the same type.
Multiple producers push inside a `Context::batch` boundary; no separate
`MPSCQueueCell` type. The shell / storage split keeps the reactive shell
storage-agnostic: the shell owns the reader-kind version cells and invalidation
logic, the backend owns the actual FIFO data structure and is pluggable via the
`QueueStorage` trait. The default `VecDequeStorage` is unbounded; a bounded form
exposes reactive backpressure.

Implements `lazily-spec/cell-model.md` § "Reactive queues". Backed by the Lean
formal model `lazily-formal/LazilyFormal/QueueCell.lean` (closure monotonicity,
reader-kind independence, total-FIFO, idempotent/terminal close).

## Reader-kind invalidation

Invalidation is scoped to **reader kind**, not individual positions. A push
invalidates `len` / `is_empty` readers (and `head` when transitioning from
empty, and `is_full` when transitioning onto capacity); a pop invalidates
`head` / `len` / `is_empty` readers (and `is_full` when transitioning off
capacity). This is implemented for free by the existing `PartialEq` guard on
`Context::set_cell`: after each op the shell re-derives each reader-kind cell
from storage and writes it back inside a `batch`, so a cell whose value did not
change is not invalidated, and the whole push/pop is one atomic invalidation
pass (no glitching — an observer never sees `len` bumped before `is_full`
flips).

## Bounded reactive backpressure

When the backend is bounded (`capacity() → Some(n)`), `is_full` is a reactive
read. A consumer's pop that transitions full → not-full invalidates `is_full`
readers (true → false), enabling push-side effects to react to capacity recovery
without polling — a producer observes `is_full` and backs off; the consumer's
pop wakes it.

## Closure lifecycle

Closure is an observable contract: pop on closed+non-empty drains; pop on
closed+empty returns `QueuePopError::Closed` (distinct from
`QueuePopError::Empty`); push on closed returns `QueuePushError::Closed`; close
is idempotent and terminal.

## Conformance

Five new cross-language compute fixtures replayed in
`tests/queue_conformance.rs`:

- `queuecell_spsc_push_pop.json` — SPSC total FIFO + full invalidation matrix
- `queuecell_popped_head_observation.json` — head reader-kind independence
  (push to non-empty does NOT invalidate head; pop does)
- `queuecell_mpsc_multi_writer.json` — MPSC multi-producer inside `batch()`
  (per-producer FIFO, batch atomicity)
- `queuecell_bounded_backpressure.json` — bounded queue, `is_full` reactive
  backpressure, reject-on-full overflow policy
- `queuecell_closure_lifecycle.json` — drain / Closed-distinct-from-Empty /
  idempotent+terminal close

Plus direct tests of the backpressure effect wiring (no glitching on push) and
the `QueueStorage` adapter seam (a custom bounded-ring backend).

## Future primitives (stubs)

`TopicCell` (SPMC broadcast) and `WorkQueueCell` (competing consumers) are
genuinely distinct primitives — they differ in *invalidation model and handoff
semantics*, not producer/consumer cardinality. They are reserved for future
work (distributed-queue PRD Phases 2–3) and documented as stubs in `queue.rs`.
Lean formal stubs already exist (`TopicCell.lean`, `WorkQueueCell.lean`).

## Changed

### New public API

- **`QueueCell<T, S>`** — the reactive FIFO shell (SPSC + MPSC usage rule).
- **`QueueStorage<T>`** trait — pluggable backend adapter
  (`try_push` / `try_pop` / `peek` / `len` / `capacity` / `is_closed` /
  `close`).
- **`VecDequeStorage<T>`** — the reference unbounded/bounded backend (default).
- **`QueuePushError`** (`Full` / `Closed`), **`QueuePopError`** (`Empty` /
  `Closed`).
- **`QueueReaderHandles<T>`** — handles to all five reader-kind cells for
  effects that subscribe to multiple reader kinds.
- `VecDequeStorage` serializes as a JSON array (behind the `serde` feature) per
  the spec's wire/snapshot shape.

### Migration

No breaking changes to existing API. `lazily-macros` bumps in lockstep
(`=0.22.0`).
