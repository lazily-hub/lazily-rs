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

use crate::ipc::{Delta, IpcMessage, OutboxAck, WireStamp};
use std::collections::BTreeSet;

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
pub trait DurableOutbox {
    /// Persist `msg` at `epoch` before it is handed to the transport.
    fn append(&mut self, epoch: u64, msg: IpcMessage);
    /// The peer proved receipt through `epoch`; retained frames `<= epoch` MAY be pruned.
    fn ack_through(&mut self, epoch: u64);
    /// Retained frames with `epoch > cursor`, in ascending epoch order.
    fn replay_from(&self, cursor: u64) -> Vec<(u64, IpcMessage)>;
    /// Epochs still retained (not yet acked), ascending — for diagnostics/tests.
    fn retained_epochs(&self) -> Vec<u64>;
}

/// In-memory [`DurableOutbox`] — correct within a process lifetime; the default.
#[derive(Debug, Clone, Default)]
pub struct InMemoryOutbox {
    entries: Vec<(u64, IpcMessage)>,
    acked_through: u64,
}

impl InMemoryOutbox {
    /// An empty outbox.
    pub fn new() -> Self {
        Self::default()
    }

    /// The highest acked epoch (retention cursor).
    pub fn acked_through(&self) -> u64 {
        self.acked_through
    }
}

impl DurableOutbox for InMemoryOutbox {
    fn append(&mut self, epoch: u64, msg: IpcMessage) {
        self.entries.push((epoch, msg));
    }

    fn ack_through(&mut self, epoch: u64) {
        if epoch > self.acked_through {
            self.acked_through = epoch;
        }
        self.entries.retain(|(e, _)| *e > self.acked_through);
    }

    fn replay_from(&self, cursor: u64) -> Vec<(u64, IpcMessage)> {
        let mut out: Vec<(u64, IpcMessage)> = self
            .entries
            .iter()
            .filter(|(e, _)| *e > cursor)
            .cloned()
            .collect();
        out.sort_by_key(|(e, _)| *e);
        out
    }

    fn retained_epochs(&self) -> Vec<u64> {
        let mut es: Vec<u64> = self.entries.iter().map(|(e, _)| *e).collect();
        es.sort_unstable();
        es
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
