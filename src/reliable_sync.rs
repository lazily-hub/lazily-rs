//! Reliable sync protocol (`#lzsync`).
//!
//! Delivery-reliability over the `Snapshot`/`Delta`/`CrdtSync` planes
//! (`lazily-spec` § Reliable Sync): gap recovery, at-least-once outbox, and
//! OR-set / LWW liveness cells. The correctness backstop is `lazily-formal`
//! `ReliableSync.lean`; the cross-language pins are
//! `lazily-spec/conformance/reliable-sync/`.
//!
//! Three pure-protocol pieces (identical logic in every binding, no I/O / clock /
//! storage engine baked in):
//!
//! - [`ResyncCoordinator`] — receiver-side decision function over the inbound
//!   frame stream (`Apply` / `RequestSnapshot` / `Ignore`), multi-epoch-span aware.
//! - [`DurableOutbox`] — sender-side at-least-once contract (append-before-send,
//!   ack-through, replay-from-cursor). Ships [`InMemoryOutbox`] as the default; a
//!   host plugs a durable store (agent-doc: SQLite) behind the trait, and the
//!   crash-replay conformance test exercises a reference file-backed impl.
//! - [`OrSet`] / [`WireLwwRegister`] — the liveness cells that ride the CrdtSync plane.
//!
//! The reverse-channel control frames are [`IpcMessage::ResyncRequest`] and
//! [`IpcMessage::OutboxAck`] — variants on the same framed, codec-negotiated,
//! bidirectional message plane as `Snapshot`/`Delta`/`CrdtSync`, so they share
//! one encode/decode path, one demux point, one FFI kind, and one in-band order.
//! They match the `conformance/reliable-sync/` fixtures and round-trip through
//! json + msgpack like the state frames.

use crate::ipc::{Delta, IpcMessage, IpcSink, IpcSource, OutboxAck, ResyncRequest, WireStamp};
use crate::outbox::DurableOutbox;
use std::collections::{BTreeSet, VecDeque};

/// Receiver decision for an inbound frame (spec § ResyncCoordinator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncAction {
    /// Apply the frame and advance the receiver epoch.
    Apply,
    /// A gap was detected; request a fresh `Snapshot` covering `from_epoch`.
    RequestSnapshot {
        /// The receiver's current `last_epoch`.
        from_epoch: u64,
    },
    /// Drop the frame (already-applied re-delivery, malformed, a duplicate
    /// request suppressed while resyncing, or a reverse-channel control frame
    /// arriving at a data receiver).
    Ignore,
}

/// Receiver-side reliable-sync coordinator.
///
/// Holds `last_epoch` (the highest epoch fully applied) and a `resyncing` flag
/// (a `RequestSnapshot` is outstanding until a covering `Snapshot` lands, so
/// further ahead-of-cursor deltas are ignored instead of re-requesting).
///
/// `ingest` advances `last_epoch` on `Apply` — the caller MUST fold the frame's
/// ops into its projection on `Apply`. This mirrors the `ReliableSync.step` Lean
/// model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResyncCoordinator {
    last_epoch: u64,
    resyncing: bool,
}

impl ResyncCoordinator {
    /// A coordinator at epoch 0 (fresh; a `Snapshot` seeds the first real epoch).
    pub fn new() -> Self {
        Self::default()
    }

    /// A coordinator that has already applied through `last_epoch`.
    pub fn with_epoch(last_epoch: u64) -> Self {
        Self {
            last_epoch,
            resyncing: false,
        }
    }

    /// The highest epoch fully applied.
    pub fn last_epoch(&self) -> u64 {
        self.last_epoch
    }

    /// Whether a resync request is outstanding (awaiting a covering snapshot).
    pub fn is_resyncing(&self) -> bool {
        self.resyncing
    }

    /// Classify + fold an inbound `Delta`. On `Apply` this advances `last_epoch`
    /// to `delta.epoch` (multi-epoch-span aware) and clears `resyncing`.
    pub fn ingest_delta(&mut self, delta: &Delta) -> ResyncAction {
        if delta.base_epoch == self.last_epoch {
            // Contiguous. Accept any span >= 1; reject an empty/backward epoch.
            if delta.epoch >= delta.base_epoch.saturating_add(1) {
                self.last_epoch = delta.epoch;
                self.resyncing = false;
                ResyncAction::Apply
            } else {
                ResyncAction::Ignore
            }
        } else if delta.base_epoch < self.last_epoch {
            // Already applied — a re-delivery (outbox replay / retry). Idempotent.
            ResyncAction::Ignore
        } else {
            // Gap: base_epoch > last_epoch. Request a covering snapshot once.
            if self.resyncing {
                ResyncAction::Ignore
            } else {
                self.resyncing = true;
                ResyncAction::RequestSnapshot {
                    from_epoch: self.last_epoch,
                }
            }
        }
    }

    /// Adopt a `Snapshot` at `snapshot_epoch` — a full-state frame always applies,
    /// setting `last_epoch` and clearing `resyncing`.
    pub fn ingest_snapshot(&mut self, snapshot_epoch: u64) -> ResyncAction {
        self.last_epoch = snapshot_epoch;
        self.resyncing = false;
        ResyncAction::Apply
    }

    /// Classify an inbound [`IpcMessage`]. `CrdtSync` is handled by the CRDT
    /// plane, and the reverse-channel control frames (`ResyncRequest` /
    /// `OutboxAck`) are for the *sender*'s driver, not this data receiver, so
    /// both are `Ignore`d here.
    pub fn ingest(&mut self, msg: &IpcMessage) -> ResyncAction {
        match msg {
            IpcMessage::Snapshot(s) => self.ingest_snapshot(s.epoch),
            IpcMessage::Delta(d) => self.ingest_delta(d),
            IpcMessage::CrdtSync(_) | IpcMessage::ResyncRequest(_) | IpcMessage::OutboxAck(_) => {
                ResyncAction::Ignore
            }
        }
    }

    /// The [`IpcMessage::OutboxAck`] control frame to advertise this receiver's
    /// resume cursor on reconnect (and for periodic retention advance).
    pub fn ack(&self) -> IpcMessage {
        IpcMessage::OutboxAck(OutboxAck {
            through_epoch: self.last_epoch,
        })
    }
}

/// Sender-side at-least-once outbox contract (spec § DurableOutbox).
///
/// Every frame is durably `append`ed **before** it is sent, retained until the
/// peer proves receipt (`ack_through`), and `replay_from` a reconnect cursor
/// re-sends everything the peer has not yet acked. Combined with the receiver's
/// idempotent `Ignore` of already-applied deltas, this is at-least-once delivery
/// with exactly-once effect.
/// An observed-remove set (OR-set) liveness cell.
///
/// Models one entry's presence via add/remove tags: a `(doc, pid)` is *present*
/// iff some add-tag is not shadowed by a remove that observed it. This gives the
/// add-wins-over-stale-remove bias liveness needs (a re-open concurrent with a
/// lagging close keeps the doc open). The join is the union of both tag sets, so
/// it is a semilattice — out-of-order and duplicate delivery converge
/// (`ReliableSync.joinOR_*`, `orset_add_wins_over_stale_remove`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrSet {
    adds: BTreeSet<String>,
    removes: BTreeSet<String>,
}

impl OrSet {
    /// An empty OR-set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a presence tag (an editor open / attach event mints a fresh tag).
    pub fn add(&mut self, tag: impl Into<String>) {
        self.adds.insert(tag.into());
    }

    /// Remove, observing `tags` — only the add-tags this remove saw are shadowed.
    pub fn remove_observed<I, S>(&mut self, tags: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for t in tags {
            self.removes.insert(t.into());
        }
    }

    /// Whether the entry is currently present (some add-tag not shadowed).
    pub fn present(&self) -> bool {
        self.adds.difference(&self.removes).next().is_some()
    }

    /// Join another replica's OR-set (union of adds and of removes).
    pub fn join(&mut self, other: &OrSet) {
        self.adds.extend(other.adds.iter().cloned());
        self.removes.extend(other.removes.iter().cloned());
    }
}

/// A last-writer-wins register liveness cell (per-pid `alive`, owner lease).
///
/// Keyed by [`WireStamp`] (`(wall_time, logical, peer)` total order): the highest
/// stamp wins, so an OS process-exit write (`alive = false` at a fresh stamp)
/// dominates a stale re-assert. Join is the stamp-max, a semilattice
/// (`ReliableSync.joinReg_*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireLwwRegister<V> {
    stamp: WireStamp,
    value: V,
}

impl<V: Clone> WireLwwRegister<V> {
    /// A register holding `value` written at `stamp`.
    pub fn new(stamp: WireStamp, value: V) -> Self {
        Self { stamp, value }
    }

    /// The current value.
    pub fn value(&self) -> &V {
        &self.value
    }

    /// The current decisive stamp.
    pub fn stamp(&self) -> WireStamp {
        self.stamp
    }

    /// Write `value` at `stamp` iff it dominates the current stamp.
    pub fn set(&mut self, stamp: WireStamp, value: V) {
        if stamp > self.stamp {
            self.stamp = stamp;
            self.value = value;
        }
    }

    /// Join another replica's register (keep the higher stamp).
    pub fn join(&mut self, other: &WireLwwRegister<V>) {
        if other.stamp > self.stamp {
            self.stamp = other.stamp;
            self.value = other.value.clone();
        }
    }
}

/// Monotonic clock seam (spec § SyncDriver — policy injected, no runtime in core).
///
/// The driver never *schedules* itself; the host calls [`SyncDriver::tick`] on
/// its own cadence and supplies wall-free monotonic millis so the driver can
/// timestamp progress and expose a stall signal without owning a clock source.
pub trait Clock {
    /// Milliseconds from an arbitrary fixed origin; monotonic, non-decreasing.
    fn now_millis(&self) -> u64;
}

/// Sender-side answer to a peer's [`ResyncRequest`] (spec § SyncDriver).
///
/// When a receiver detects a gap it can no longer close from retained deltas, it
/// asks for a covering `Snapshot`; the host plugs its projection in here to
/// produce one at `epoch >= from_epoch`. This is the app-supplied half of the
/// `resync_convergence` guarantee (drop the delta suffix, adopt the snapshot).
pub trait SnapshotProvider {
    /// A full-state [`IpcMessage::Snapshot`] covering `from_epoch` (its `epoch`
    /// MUST be `>= from_epoch`).
    fn snapshot(&self, from_epoch: u64) -> IpcMessage;
}

/// What one [`SyncDriver::tick`] accomplished (spec § SyncDriver).
///
/// `applied` are the inbound `Snapshot`/`Delta`/`CrdtSync` frames the host MUST
/// fold into its projection this tick — the driver has already advanced the
/// receiver cursor for them, so folding is the caller's remaining obligation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    /// Data frames pushed to the sink this tick (fresh enqueues + reconnect replays).
    pub sent: usize,
    /// Inbound frames the host must fold into its projection (`Apply`ed).
    pub applied: Vec<IpcMessage>,
    /// A gap was detected inbound and a [`ResyncRequest`] was emitted to the peer.
    pub resync_requested: bool,
    /// Inbound [`ResyncRequest`]s answered with a provider snapshot this tick.
    pub snapshots_served: usize,
    /// The peer's ack cursor after this tick (our outbox retention / resume point).
    pub peer_acked_through: u64,
    /// Outbox frames still unacked (retained for reconnect replay).
    pub retained: usize,
}

/// A transport error surfaced by [`SyncDriver::tick`].
///
/// A *sink* failure is not fatal — the frame is retained in the outbox and
/// replayed on the next [`SyncDriver::on_reconnect`], per the spec's
/// retain-on-fail / resync-on-reconnect loop shape — so it is reported as a
/// stall, not an error. Only a *source* read failure is returned as an error,
/// signalling the host to re-establish the transport and call `on_reconnect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverError<RE> {
    /// The inbound source failed to read; the host should reconnect.
    Source(RE),
}

/// Full-duplex reliable-sync loop driver (spec § SyncDriver).
///
/// One driver drives one peer connection over a caller-supplied
/// [`IpcSink`]/[`IpcSource`] pair (agent-doc wraps its Unix-domain socket). It
/// composes the three pure-protocol pieces into the loop shape the spec pins:
///
/// 1. **drain** — pop host-enqueued outbound data frames, `append` each to the
///    [`DurableOutbox`] *before* sending (at-least-once durability), send via the sink;
/// 2. **retain-on-fail** — a send error leaves the frame in the outbox (unacked)
///    and stops the drain; it is re-sent on the next reconnect;
/// 3. **receive** — read inbound frames, route control frames (`OutboxAck` →
///    advance retention; `ResyncRequest` → answer with a provider snapshot) and
///    feed data frames through the [`ResyncCoordinator`] (`Apply` → hand to the
///    host + owe an ack; `RequestSnapshot` → emit a `ResyncRequest`; `Ignore` → drop);
/// 4. **resync-on-reconnect** — [`on_reconnect`](Self::on_reconnect) replays the
///    unacked outbox suffix from the peer's ack cursor and re-advertises our own
///    receiver cursor, so a dropped-frame gap converges.
///
/// The driver owns no threads, no clock source, and no storage engine — the host
/// injects all three (`Clock`, the transport pair, the outbox) and decides the
/// tick cadence. Threading and backoff are host policy.
pub struct SyncDriver<S, R, O, C, P>
where
    S: IpcSink,
    R: IpcSource,
    O: DurableOutbox,
    C: Clock,
    P: SnapshotProvider,
{
    sink: S,
    source: R,
    outbox: O,
    clock: C,
    provider: P,
    coordinator: ResyncCoordinator,
    /// Host-enqueued outbound data frames staged before append-then-send.
    pending: VecDeque<(u64, IpcMessage)>,
    /// Highest epoch the peer has acked — our outbox retention + reconnect resume cursor.
    peer_acked_through: u64,
    /// We applied an inbound frame and owe the peer an `OutboxAck` (retried until sent).
    ack_owed: bool,
    /// A reconnect happened; the next tick replays the unacked outbox suffix.
    replay_pending: bool,
    /// `Some(millis)` since the last sink send failure; `None` when the sink is healthy.
    stalled_since: Option<u64>,
}

impl<S, R, O, C, P> SyncDriver<S, R, O, C, P>
where
    S: IpcSink,
    R: IpcSource,
    O: DurableOutbox,
    C: Clock,
    P: SnapshotProvider,
{
    /// A fresh driver at receiver epoch 0 (a `Snapshot` seeds the first epoch).
    pub fn new(sink: S, source: R, outbox: O, clock: C, provider: P) -> Self {
        Self::with_epoch(sink, source, outbox, clock, provider, 0)
    }

    /// A driver whose receiver has already applied through `last_epoch` (resume).
    pub fn with_epoch(
        sink: S,
        source: R,
        outbox: O,
        clock: C,
        provider: P,
        last_epoch: u64,
    ) -> Self {
        Self {
            sink,
            source,
            outbox,
            clock,
            provider,
            coordinator: ResyncCoordinator::with_epoch(last_epoch),
            pending: VecDeque::new(),
            peer_acked_through: 0,
            ack_owed: false,
            replay_pending: false,
            stalled_since: None,
        }
    }

    /// Stage an outbound data frame at `epoch` for the next tick's drain. `epoch`
    /// is the frame's accepted-event count (`Delta::epoch` / `Snapshot::epoch`);
    /// it becomes the outbox retention key.
    pub fn enqueue(&mut self, epoch: u64, msg: IpcMessage) {
        self.pending.push_back((epoch, msg));
    }

    /// Signal that the transport was re-established; the next [`tick`](Self::tick)
    /// replays the unacked outbox suffix and re-advertises our receiver cursor.
    pub fn on_reconnect(&mut self) {
        self.replay_pending = true;
        self.ack_owed = true;
        self.stalled_since = None;
    }

    /// The receiver's current applied epoch.
    pub fn last_epoch(&self) -> u64 {
        self.coordinator.last_epoch()
    }

    /// Whether the sink is currently stalled (last send failed, awaiting reconnect).
    pub fn is_stalled(&self) -> bool {
        self.stalled_since.is_some()
    }

    /// Millis the sink has been stalled as of `now`, or `0` when healthy — a
    /// backoff signal for the host scheduler (which owns cadence/backoff policy).
    pub fn stalled_for(&self, now: u64) -> u64 {
        self.stalled_since
            .map_or(0, |since| now.saturating_sub(since))
    }

    /// Borrow the underlying outbox (diagnostics / durable-store flush).
    pub fn outbox(&self) -> &O {
        &self.outbox
    }

    /// Run one loop pass. See the type docs for the drain → retain → receive →
    /// resync shape. Sink failures retain-and-stall (not an error); only an
    /// inbound source read failure returns [`DriverError::Source`].
    pub fn tick(&mut self) -> Result<Progress, DriverError<R::Error>> {
        let now = self.clock.now_millis();
        let mut progress = Progress::default();

        // 1. resync-on-reconnect: replay the unacked outbox suffix, oldest first.
        if self.replay_pending {
            self.replay_pending = false;
            for (_, msg) in self.outbox.replay_from(self.peer_acked_through) {
                if self.sink.send(&msg).is_ok() {
                    progress.sent += 1;
                } else {
                    self.stalled_since = Some(now);
                    self.replay_pending = true; // finish the replay after the next reconnect
                    break;
                }
            }
        }

        // 2. drain fresh enqueues: append-before-send, retain-and-stop on failure.
        //    A pre-existing stall (a prior failed send, no reconnect yet) skips the
        //    drain entirely — do not push into a sink already known to be down.
        while self.stalled_since.is_none() {
            let Some((epoch, msg)) = self.pending.front().cloned() else {
                break;
            };
            self.outbox.append(epoch, msg.clone());
            self.pending.pop_front();
            match self.sink.send(&msg) {
                Ok(()) => {
                    progress.sent += 1;
                    self.stalled_since = None;
                }
                Err(_) => {
                    // Retained in the outbox (unacked) → replayed on reconnect.
                    self.stalled_since = Some(now);
                    break;
                }
            }
        }

        // 3. receive: route control frames + feed data frames through the coordinator.
        loop {
            let msg = match self.source.recv() {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(e) => return Err(DriverError::Source(e)),
            };
            match msg {
                IpcMessage::OutboxAck(ack) => {
                    if ack.through_epoch > self.peer_acked_through {
                        self.peer_acked_through = ack.through_epoch;
                    }
                    self.outbox.ack_through(ack.through_epoch);
                }
                IpcMessage::ResyncRequest(req) => {
                    let snap = self.provider.snapshot(req.from_epoch);
                    if self.sink.send(&snap).is_ok() {
                        progress.snapshots_served += 1;
                    } else {
                        self.stalled_since = Some(now);
                    }
                }
                IpcMessage::CrdtSync(_) => {
                    // Idempotent anti-entropy plane — the host folds it directly.
                    progress.applied.push(msg);
                }
                IpcMessage::Snapshot(_) | IpcMessage::Delta(_) => {
                    match self.coordinator.ingest(&msg) {
                        ResyncAction::Apply => {
                            self.ack_owed = true;
                            progress.applied.push(msg);
                        }
                        ResyncAction::RequestSnapshot { from_epoch } => {
                            let req = IpcMessage::ResyncRequest(ResyncRequest { from_epoch });
                            if self.sink.send(&req).is_ok() {
                                progress.resync_requested = true;
                            } else {
                                self.stalled_since = Some(now);
                            }
                        }
                        ResyncAction::Ignore => {}
                    }
                }
            }
        }

        // 4. advertise our receiver cursor if we applied anything (retry until sent).
        if self.ack_owed && self.sink.send(&self.coordinator.ack()).is_ok() {
            self.ack_owed = false;
        }

        progress.peer_acked_through = self.peer_acked_through;
        progress.retained = self.outbox.retained_epochs().len();
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryOutbox;
    use crate::ipc::Delta;

    fn st(w: u64) -> WireStamp {
        WireStamp {
            wall_time: w,
            logical: 0,
            peer: 1,
        }
    }

    #[test]
    fn coordinator_applies_contiguous_and_advances() {
        let mut c = ResyncCoordinator::with_epoch(40);
        assert_eq!(
            c.ingest_delta(&Delta::new(40, 41, vec![])),
            ResyncAction::Apply
        );
        assert_eq!(c.last_epoch(), 41);
        // multi-epoch span advances straight to the target
        assert_eq!(
            c.ingest_delta(&Delta::new(41, 44, vec![])),
            ResyncAction::Apply
        );
        assert_eq!(c.last_epoch(), 44);
    }

    #[test]
    fn coordinator_ignores_empty_backward_delta() {
        let mut c = ResyncCoordinator::with_epoch(40);
        // epoch < base+1 is malformed → Ignore, no advance
        assert_eq!(
            c.ingest_delta(&Delta::new(40, 40, vec![])),
            ResyncAction::Ignore
        );
        assert_eq!(c.last_epoch(), 40);
    }

    #[test]
    fn coordinator_gap_requests_once_then_ignores() {
        let mut c = ResyncCoordinator::with_epoch(2);
        assert_eq!(
            c.ingest_delta(&Delta::new(3, 4, vec![])),
            ResyncAction::RequestSnapshot { from_epoch: 2 }
        );
        assert!(c.is_resyncing());
        // further ahead-of-cursor deltas are suppressed
        assert_eq!(
            c.ingest_delta(&Delta::new(4, 5, vec![])),
            ResyncAction::Ignore
        );
        // a covering snapshot clears the resyncing state
        assert_eq!(c.ingest_snapshot(5), ResyncAction::Apply);
        assert!(!c.is_resyncing());
        assert_eq!(c.last_epoch(), 5);
    }

    #[test]
    fn ack_carries_last_epoch() {
        let c = ResyncCoordinator::with_epoch(7);
        assert_eq!(
            c.ack(),
            IpcMessage::OutboxAck(OutboxAck { through_epoch: 7 })
        );
    }

    #[test]
    fn outbox_retains_unacked_and_replays_from_cursor() {
        let mut o = InMemoryOutbox::new();
        for e in 41..=43 {
            o.append(e, IpcMessage::Delta(Delta::new(e - 1, e, vec![])));
        }
        o.ack_through(41);
        assert_eq!(o.retained_epochs(), vec![42, 43]);
        let replay: Vec<u64> = o.replay_from(41).iter().map(|(e, _)| *e).collect();
        assert_eq!(replay, vec![42, 43]);
    }

    #[test]
    fn orset_join_is_commutative_and_add_wins() {
        let mut a = OrSet::new();
        a.add("t1");
        let mut b = OrSet::new();
        b.remove_observed(["t1"]);
        b.add("t3"); // re-open with a tag the close never observed
        let mut ab = a.clone();
        ab.join(&b);
        let mut ba = b.clone();
        ba.join(&a);
        assert_eq!(ab, ba, "join is commutative");
        assert!(ab.present(), "add tag t3 not shadowed → present");
    }

    #[test]
    fn lww_join_keeps_higher_stamp() {
        let mut a = WireLwwRegister::new(st(10), true);
        let b = WireLwwRegister::new(st(20), false);
        a.join(&b);
        assert!(!(*a.value()));
        // re-joining a stale lower stamp is a no-op (idempotent under retry)
        a.join(&WireLwwRegister::new(st(5), true));
        assert!(!(*a.value()));
    }

    // ---- SyncDriver: the loop-shape mechanism over a scripted transport ----
    //
    // A SimWorld-style deterministic pair: the sink records what the driver sends
    // (and can be toggled "down" to model a disconnect); the source replays a
    // scripted inbound frame stream (and can inject one read error). No threads,
    // no real socket — every tick is a pure step over injected state.

    use crate::ipc::Snapshot;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct Wire {
        sent: Rc<RefCell<Vec<IpcMessage>>>,
        inbound: Rc<RefCell<VecDeque<IpcMessage>>>,
    }
    struct TestSink {
        sent: Rc<RefCell<Vec<IpcMessage>>>,
        up: Rc<Cell<bool>>,
    }
    struct TestSource {
        inbound: Rc<RefCell<VecDeque<IpcMessage>>>,
        err: Rc<Cell<bool>>,
    }
    impl IpcSink for TestSink {
        type Error = ();
        fn send(&mut self, message: &IpcMessage) -> Result<(), ()> {
            if !self.up.get() {
                return Err(());
            }
            self.sent.borrow_mut().push(message.clone());
            Ok(())
        }
    }
    impl IpcSource for TestSource {
        type Error = ();
        fn recv(&mut self) -> Result<Option<IpcMessage>, ()> {
            if self.err.get() {
                self.err.set(false);
                return Err(());
            }
            Ok(self.inbound.borrow_mut().pop_front())
        }
    }
    struct Zero;
    impl Clock for Zero {
        fn now_millis(&self) -> u64 {
            0
        }
    }
    /// Provider that answers a `ResyncRequest{from}` with a snapshot at `from + 5`.
    struct SnapAhead;
    impl SnapshotProvider for SnapAhead {
        fn snapshot(&self, from_epoch: u64) -> IpcMessage {
            IpcMessage::Snapshot(Snapshot::new(from_epoch + 5, vec![], vec![], vec![]))
        }
    }

    type TestDriver = SyncDriver<TestSink, TestSource, InMemoryOutbox, Zero, SnapAhead>;

    fn driver_at(last_epoch: u64) -> (Wire, Rc<Cell<bool>>, Rc<Cell<bool>>, TestDriver) {
        let wire = Wire::default();
        let up = Rc::new(Cell::new(true));
        let err = Rc::new(Cell::new(false));
        let sink = TestSink {
            sent: wire.sent.clone(),
            up: up.clone(),
        };
        let source = TestSource {
            inbound: wire.inbound.clone(),
            err: err.clone(),
        };
        let driver = SyncDriver::with_epoch(
            sink,
            source,
            InMemoryOutbox::new(),
            Zero,
            SnapAhead,
            last_epoch,
        );
        (wire, up, err, driver)
    }

    fn delta(base: u64, epoch: u64) -> IpcMessage {
        IpcMessage::Delta(Delta::new(base, epoch, vec![]))
    }

    #[test]
    fn driver_drains_append_before_send_and_retains_until_acked() {
        let (wire, _up, _err, mut d) = driver_at(0);
        d.enqueue(1, delta(0, 1));
        d.enqueue(2, delta(1, 2));
        let p = d.tick().unwrap();
        assert_eq!(p.sent, 2, "both fresh frames pushed to the sink");
        assert_eq!(wire.sent.borrow().len(), 2);
        assert_eq!(p.retained, 2, "appended-before-send, retained until acked");
        assert!(!d.is_stalled());

        // Peer proves receipt → the outbox prunes and the resume cursor advances.
        wire.inbound
            .borrow_mut()
            .push_back(IpcMessage::OutboxAck(OutboxAck { through_epoch: 2 }));
        let p = d.tick().unwrap();
        assert_eq!(p.peer_acked_through, 2);
        assert_eq!(p.retained, 0, "acked frames pruned");
    }

    #[test]
    fn driver_retains_on_send_failure_and_replays_on_reconnect() {
        let (wire, up, _err, mut d) = driver_at(0);
        up.set(false); // sink down before the first send
        d.enqueue(1, delta(0, 1));
        let p = d.tick().unwrap();
        assert_eq!(p.sent, 0);
        assert!(d.is_stalled(), "a failed send stalls the driver");
        assert_eq!(
            p.retained, 1,
            "frame retained in the outbox despite the failure"
        );
        assert!(wire.sent.borrow().is_empty());
        assert_eq!(
            d.stalled_for(250),
            250,
            "stall duration is a host backoff signal"
        );

        // Transport recovers → the unacked suffix replays from the ack cursor.
        up.set(true);
        d.on_reconnect();
        let p = d.tick().unwrap();
        assert!(!d.is_stalled());
        assert_eq!(p.sent, 1, "the retained frame is replayed");
        assert!(
            wire.sent
                .borrow()
                .iter()
                .any(|m| matches!(m, IpcMessage::Delta(dd) if dd.epoch == 1)),
            "the replayed delta reached the sink"
        );
    }

    #[test]
    fn driver_applies_delta_and_advertises_receiver_cursor() {
        let (wire, _up, _err, mut d) = driver_at(0);
        wire.inbound.borrow_mut().push_back(delta(0, 1));
        let p = d.tick().unwrap();
        assert_eq!(
            p.applied.len(),
            1,
            "the applied frame is handed to the host"
        );
        assert_eq!(d.last_epoch(), 1);
        assert!(
            wire.sent
                .borrow()
                .iter()
                .any(|m| matches!(m, IpcMessage::OutboxAck(a) if a.through_epoch == 1)),
            "an OutboxAck advertising the new cursor was sent"
        );
    }

    #[test]
    fn driver_redelivery_is_idempotent_no_op() {
        let (wire, _up, _err, mut d) = driver_at(0);
        wire.inbound.borrow_mut().push_back(delta(0, 1));
        assert_eq!(d.tick().unwrap().applied.len(), 1);
        // Re-deliver the exact same frame (an outbox replay from the peer).
        wire.inbound.borrow_mut().push_back(delta(0, 1));
        let p = d.tick().unwrap();
        assert_eq!(p.applied.len(), 0, "already-applied re-delivery is ignored");
        assert_eq!(d.last_epoch(), 1, "cursor does not double-advance");
    }

    #[test]
    fn driver_requests_snapshot_on_inbound_gap() {
        let (wire, _up, _err, mut d) = driver_at(2);
        wire.inbound.borrow_mut().push_back(delta(3, 4)); // base 3 > last 2 → gap
        let p = d.tick().unwrap();
        assert!(p.resync_requested);
        assert!(p.applied.is_empty(), "the gapped delta is not applied");
        assert!(
            wire.sent
                .borrow()
                .iter()
                .any(|m| matches!(m, IpcMessage::ResyncRequest(r) if r.from_epoch == 2)),
            "a ResyncRequest at the current cursor was emitted"
        );
    }

    #[test]
    fn driver_answers_resync_request_with_provider_snapshot() {
        let (wire, _up, _err, mut d) = driver_at(0);
        wire.inbound
            .borrow_mut()
            .push_back(IpcMessage::ResyncRequest(ResyncRequest { from_epoch: 2 }));
        let p = d.tick().unwrap();
        assert_eq!(p.snapshots_served, 1);
        assert!(
            wire.sent
                .borrow()
                .iter()
                .any(|m| matches!(m, IpcMessage::Snapshot(s) if s.epoch == 7)),
            "a covering snapshot (from_epoch + 5) was sent"
        );
    }

    #[test]
    fn driver_surfaces_source_read_error() {
        let (_wire, _up, err, mut d) = driver_at(0);
        err.set(true);
        assert_eq!(d.tick(), Err(DriverError::Source(())));
    }

    #[test]
    fn driver_gap_then_snapshot_converges() {
        // Receiver at 2; a gapped delta triggers a request, then a covering
        // snapshot lands and convergence is restored (resync_convergence shape).
        let (wire, _up, _err, mut d) = driver_at(2);
        wire.inbound.borrow_mut().push_back(delta(4, 5)); // gap
        d.tick().unwrap();
        assert_eq!(d.last_epoch(), 2, "still stuck at the pre-gap cursor");
        wire.inbound
            .borrow_mut()
            .push_back(IpcMessage::Snapshot(Snapshot::new(
                5,
                vec![],
                vec![],
                vec![],
            )));
        let p = d.tick().unwrap();
        assert_eq!(p.applied.len(), 1);
        assert_eq!(d.last_epoch(), 5, "snapshot restored convergence");
    }
}
