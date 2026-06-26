//! Runtime integration of the distributed CRDT cell plane (`#lzcrdtplane5b`,
//! the FINAL phase of `#lzcrdtplane`).
//!
//! Plan: `tasks/software/plan-lazily-distributed-crdt-plane.md`.
//!
//! Phases 1–4 built the *plane primitives* — the [`CrdtPlane`] clock + stamp
//! frontier, the [`OpLog`] anti-entropy substrate, the per-cell
//! [`ReplicatedCell`]/register CRDTs, and frontier-driven Seq/Text GC. Phase 5a
//! built the *wire format* — [`CrdtOp`]/[`CrdtSync`]/[`WireStamp`] and the
//! permission-filtered [`IpcSink`]/[`IpcSource`] seam. This module is the glue
//! that makes them a live runtime:
//!
//! - **`merge:crdt` root-cell registry.** [`CrdtPlaneRuntime`] owns the session's
//!   replicated root cells, addressed by [`NodeId`] with an optional wire-stable
//!   [`NodeKey`] (producer projection, `#lzwirekey`) that survives `NodeId`
//!   churn.
//! - **Local edit → op.** [`local_update`](CrdtPlaneRuntime::local_update)
//!   mutates a typed cell, stamps the edit on the plane clock, records the
//!   converged state in the op log, and returns the [`CrdtOp`] to broadcast.
//! - **Remote op → reactive graph.** [`ingest`](CrdtPlaneRuntime::ingest) folds
//!   each not-yet-seen op into its target replica via
//!   [`ReplicatedCell::merge_remote`] — driving downstream derived slots — and
//!   advances the clock + stamp frontier so the causal-stability watermark and
//!   tombstone GC stay sound.
//! - **Anti-entropy frames.** [`sync_frame`](CrdtPlaneRuntime::sync_frame) /
//!   [`sync_reply`](CrdtPlaneRuntime::sync_reply) advertise the local frontier
//!   and ship the ops a peer is missing; delivery is bounded, idempotent, and
//!   resumable.
//!
//! With `< 2` live writers the plane is inert: nothing calls `ingest`, the
//! stability frontier withholds GC, and the single-producer Snapshot/Delta
//! mirror behaves exactly as before.
//!
//! Cell state crosses the wire as JSON ([`IpcValue::Inline`]); the module is
//! gated on `webrtc` (which pulls `ipc` + `serde_json` + a concrete transport)
//! so the runtime always has a codec and a transport seam to ride.

use std::any::Any;
use std::collections::BTreeMap;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::cell::CellHandle;
use crate::context::Context;
use crate::crdt::{CellCrdt, CrdtPlane, HlcStamp, OpLog, ReplicatedCell, StampFrontier};
use crate::distributed::{NodeId, PeerId};
use crate::ipc::{CrdtOp, CrdtSync, IpcValue, KeyIndex, NodeKey, WireStamp};

/// Object-safe erasure over a `merge:crdt` root cell, so the runtime registry can
/// hold heterogeneous register/CRDT cells keyed by [`NodeId`] and still merge a
/// remote wire state into the right typed replica.
trait PlaneCell {
    /// Decode a remote replica's converged state from `bytes` and merge it into
    /// this cell, pushing the converged value into the reactive graph. Returns
    /// `true` iff the local value changed (a redundant or undecodable merge is a
    /// no-op).
    fn merge_state(&mut self, ctx: &Context, bytes: &[u8]) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<C> PlaneCell for ReplicatedCell<C>
where
    C: CellCrdt + Serialize + DeserializeOwned + 'static,
    C::Value: PartialEq + Clone + 'static,
{
    fn merge_state(&mut self, ctx: &Context, bytes: &[u8]) -> bool {
        match serde_json::from_slice::<C>(bytes) {
            Ok(remote) => self.merge_remote(ctx, &remote),
            Err(_) => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The live runtime that bridges the distributed CRDT plane to a reactive graph's
/// `merge:crdt` root cells (`#lzcrdtplane5b`).
///
/// One runtime per shared session per replica. It owns the [`CrdtPlane`]
/// (identity + clock + stamp frontier + membership + GC), the [`OpLog`]
/// anti-entropy substrate, and the registry of replicated root cells.
pub struct CrdtPlaneRuntime {
    plane: CrdtPlane,
    log: OpLog<CrdtOp>,
    cells: BTreeMap<NodeId, Box<dyn PlaneCell>>,
    keys: KeyIndex,
}

impl CrdtPlaneRuntime {
    /// Create a runtime for the local `peer`.
    pub fn new(peer: PeerId) -> Self {
        Self {
            plane: CrdtPlane::new(peer),
            log: OpLog::new(),
            cells: BTreeMap::new(),
            keys: KeyIndex::new(),
        }
    }

    /// The local replica identity.
    pub fn peer(&self) -> PeerId {
        self.plane.peer()
    }

    /// Immutable access to the underlying plane (clock, stamp frontier,
    /// membership, GC drivers).
    pub fn plane(&self) -> &CrdtPlane {
        &self.plane
    }

    /// Mutable access to the underlying plane — drive frontier-based Seq/Text
    /// tombstone GC (`CrdtPlane::gc_seq` / `gc_text`) from the same watermark the
    /// runtime advances on every applied op.
    pub fn plane_mut(&mut self) -> &mut CrdtPlane {
        &mut self.plane
    }

    /// Number of registered `merge:crdt` root cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether no cells are registered.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Register a `merge:crdt` root cell under `node`, optionally projecting a
    /// wire-stable [`NodeKey`] (`#lzwirekey`) so the cell stays addressable across
    /// `NodeId` churn. Replicas that share a session must register the same CRDT
    /// type `C` under the same `node`/`key`.
    pub fn register<C>(&mut self, node: NodeId, key: Option<NodeKey>, cell: ReplicatedCell<C>)
    where
        C: CellCrdt + Serialize + DeserializeOwned + 'static,
        C::Value: PartialEq + Clone + 'static,
    {
        if let Some(key) = key {
            self.keys.insert(key, node);
        }
        self.cells.insert(node, Box::new(cell));
    }

    /// The reactive [`CellHandle`] of a registered cell — depend on it from a
    /// derived slot so the graph recomputes when a remote op converges.
    pub fn handle<C>(&self, node: NodeId) -> Option<CellHandle<C::Value>>
    where
        C: CellCrdt + 'static,
        C::Value: PartialEq + Clone + 'static,
    {
        let cell = self
            .cells
            .get(&node)?
            .as_any()
            .downcast_ref::<ReplicatedCell<C>>()?;
        Some(cell.handle())
    }

    /// The current converged value of a registered cell.
    pub fn value<C>(&self, node: NodeId) -> Option<C::Value>
    where
        C: CellCrdt + 'static,
        C::Value: PartialEq + Clone + 'static,
    {
        let cell = self
            .cells
            .get(&node)?
            .as_any()
            .downcast_ref::<ReplicatedCell<C>>()?;
        Some(cell.value())
    }

    /// Apply a local edit to the cell at `node`.
    ///
    /// The plane clock ticks first (at wall time `now_micros`) and the fresh
    /// [`HlcStamp`] is handed to `mutate` so stamp-ordered registers
    /// ([`LwwRegister`](crate::LwwRegister)) can use it; commutative registers
    /// ([`MvRegister`](crate::MvRegister)/[`PnCounter`](crate::PnCounter)) ignore
    /// it. If the edit changes the value, the converged state is recorded in the
    /// op log and returned as the [`CrdtOp`] to broadcast; an unchanged edit, an
    /// unknown `node`, or a type mismatch returns `None`.
    pub fn local_update<C, F>(
        &mut self,
        ctx: &Context,
        node: NodeId,
        now_micros: u64,
        mutate: F,
    ) -> Option<CrdtOp>
    where
        C: CellCrdt + Serialize + DeserializeOwned + 'static,
        C::Value: PartialEq + Clone + 'static,
        F: FnOnce(&mut C, HlcStamp),
    {
        let stamp = self.plane.tick(now_micros);
        let state = {
            let cell = self
                .cells
                .get_mut(&node)?
                .as_any_mut()
                .downcast_mut::<ReplicatedCell<C>>()?;
            if !cell.update(ctx, |c| mutate(c, stamp)) {
                return None;
            }
            serde_json::to_vec(cell.crdt()).ok()?
        };
        let wire = WireStamp::from(stamp);
        let op = match self.keys.key_for_node(node).cloned() {
            Some(key) => CrdtOp::keyed(node, key, wire, IpcValue::Inline(state)),
            None => CrdtOp::new(node, wire, IpcValue::Inline(state)),
        };
        self.log.record(stamp, op.clone());
        Some(op)
    }

    /// Ingest a remote anti-entropy frame: fold every not-yet-seen [`CrdtOp`] into
    /// its target replica (driving the reactive graph) exactly once, advancing the
    /// plane clock + stamp frontier so the causal-stability watermark stays sound.
    /// Returns the number of ops newly applied.
    ///
    /// Re-delivering a frame the receiver already has is a no-op (the op log
    /// dedups by stamp), so the exchange is idempotent and resumable.
    pub fn ingest(&mut self, ctx: &Context, sync: &CrdtSync, now_micros: u64) -> usize {
        for (_, wire) in &sync.frontier {
            let stamp = HlcStamp::from(*wire);
            if stamp.peer != self.plane.peer() {
                self.plane.observe_remote(stamp, now_micros);
            }
        }
        let incoming = sync
            .ops
            .iter()
            .map(|op| (HlcStamp::from(op.stamp), op.clone()));
        // Disjoint field borrows so the apply closure can touch the plane, the
        // registry, and the key index while the op log dedups.
        let Self {
            plane,
            log,
            cells,
            keys,
        } = self;
        log.apply_remote(incoming, |stamp, op| {
            plane.observe_remote(*stamp, now_micros);
            let node = op
                .key
                .as_ref()
                .and_then(|key| keys.node_for_key(key))
                .unwrap_or(op.node);
            if let (Some(cell), IpcValue::Inline(bytes)) = (cells.get_mut(&node), &op.state) {
                cell.merge_state(ctx, bytes);
            }
        })
    }

    /// This replica's stamp frontier in wire form — the per-peer highest observed
    /// stamp it advertises so a peer can compute what it is missing.
    pub fn wire_frontier(&self) -> Vec<(u64, WireStamp)> {
        self.plane
            .frontier()
            .iter()
            .map(|(peer, stamp)| (peer.0, WireStamp::from(stamp)))
            .collect()
    }

    /// A frame shipping the *entire* op log plus this replica's frontier. Safe to
    /// resend (the receiver dedups); use it for an initial full anti-entropy
    /// round when the peer's frontier is unknown.
    pub fn sync_frame(&self) -> CrdtSync {
        self.sync_frame_since(&StampFrontier::new())
    }

    /// A frame advertising this replica's frontier and shipping only the ops a
    /// peer described by `since` has not yet observed.
    pub fn sync_frame_since(&self, since: &StampFrontier) -> CrdtSync {
        let ops = self
            .log
            .missing_since(since)
            .into_iter()
            .map(|(_, op)| op)
            .collect();
        CrdtSync::new(self.wire_frontier(), ops)
    }

    /// Reply to a peer's anti-entropy `request`: ship exactly the ops the
    /// requester (described by `request.frontier`) is missing. The pairwise pull
    /// half of the protocol.
    pub fn sync_reply(&self, request: &CrdtSync) -> CrdtSync {
        self.sync_frame_since(&wire_frontier_to_stamp(&request.frontier))
    }
}

/// Lift a wire frontier advertisement back into a [`StampFrontier`].
fn wire_frontier_to_stamp(frontier: &[(u64, WireStamp)]) -> StampFrontier {
    let mut stamp_frontier = StampFrontier::new();
    for (peer, wire) in frontier {
        stamp_frontier.observe(PeerId(*peer), HlcStamp::from(*wire));
    }
    stamp_frontier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LwwRegister, MvRegister, PnCounter};

    fn lww_cell(ctx: &Context, value: i64) -> ReplicatedCell<LwwRegister<i64>> {
        // Seed at the zero stamp for the local peer; the first local_update will
        // beat it.
        let seed = HlcStamp::from(WireStamp {
            wall_time: 0,
            logical: 0,
            peer: 0,
        });
        ReplicatedCell::lww(ctx, value, seed)
    }

    #[test]
    fn local_update_emits_keyed_op_and_records_it() {
        let ctx = Context::new();
        let mut rt = CrdtPlaneRuntime::new(PeerId(1));
        let key = NodeKey::new("counter").unwrap();
        rt.register(NodeId(7), Some(key.clone()), lww_cell(&ctx, 0));

        let op = rt
            .local_update::<LwwRegister<i64>, _>(&ctx, NodeId(7), 100, |r, s| {
                r.set(42, s);
            })
            .expect("changed write yields an op");

        assert_eq!(op.node, NodeId(7));
        assert_eq!(
            op.key.as_ref(),
            Some(&key),
            "producer key projected onto op"
        );
        assert_eq!(rt.value::<LwwRegister<i64>>(NodeId(7)), Some(42));
        // The op is in the log: a full frame ships exactly it.
        assert_eq!(rt.sync_frame().ops.len(), 1);
    }

    #[test]
    fn unchanged_local_write_emits_nothing() {
        let ctx = Context::new();
        let mut rt = CrdtPlaneRuntime::new(PeerId(1));
        rt.register(NodeId(1), None, lww_cell(&ctx, 5));
        // Re-writing the current value at a fresh stamp advances the register
        // stamp but does not change the value, so the reactive graph is untouched
        // and no op is emitted.
        let op = rt.local_update::<LwwRegister<i64>, _>(&ctx, NodeId(1), 200, |r, s| {
            r.set(5, s);
        });
        assert!(op.is_none(), "a value-preserving write emits no op");
        assert_eq!(rt.value::<LwwRegister<i64>>(NodeId(1)), Some(5));
    }

    #[test]
    fn ingest_is_idempotent() {
        let ctx_a = Context::new();
        let mut a = CrdtPlaneRuntime::new(PeerId(1));
        a.register(NodeId(1), None, lww_cell(&ctx_a, 0));
        let op = a
            .local_update::<LwwRegister<i64>, _>(&ctx_a, NodeId(1), 100, |r, s| {
                r.set(11, s);
            })
            .unwrap();

        let ctx_b = Context::new();
        let mut b = CrdtPlaneRuntime::new(PeerId(2));
        b.register(NodeId(1), None, lww_cell(&ctx_b, 0));

        let frame = CrdtSync::new(a.wire_frontier(), vec![op]);
        assert_eq!(b.ingest(&ctx_b, &frame, 100), 1, "first apply lands");
        assert_eq!(b.ingest(&ctx_b, &frame, 101), 0, "re-apply is a no-op");
        assert_eq!(b.value::<LwwRegister<i64>>(NodeId(1)), Some(11));
    }

    #[test]
    fn pn_counter_converges_under_concurrent_increments() {
        // Two replicas independently increment the same PN-counter cell; after a
        // mutual exchange both see the summed value (per-peer max merge).
        let ctx_a = Context::new();
        let mut a = CrdtPlaneRuntime::new(PeerId(1));
        a.register(
            NodeId(3),
            None,
            ReplicatedCell::<PnCounter>::counter(&ctx_a),
        );
        let ctx_b = Context::new();
        let mut b = CrdtPlaneRuntime::new(PeerId(2));
        b.register(
            NodeId(3),
            None,
            ReplicatedCell::<PnCounter>::counter(&ctx_b),
        );

        let op_a = a
            .local_update::<PnCounter, _>(&ctx_a, NodeId(3), 100, |c, _| c.increment(PeerId(1), 3))
            .unwrap();
        let op_b = b
            .local_update::<PnCounter, _>(&ctx_b, NodeId(3), 100, |c, _| c.increment(PeerId(2), 5))
            .unwrap();

        b.ingest(&ctx_b, &CrdtSync::new(a.wire_frontier(), vec![op_a]), 101);
        a.ingest(&ctx_a, &CrdtSync::new(b.wire_frontier(), vec![op_b]), 101);

        assert_eq!(a.value::<PnCounter>(NodeId(3)), Some(8));
        assert_eq!(b.value::<PnCounter>(NodeId(3)), Some(8));
    }

    #[test]
    fn mutual_exchange_expands_membership_and_arms_the_watermark() {
        // A single replica's frontier covers only itself — not a sound
        // cross-replica GC watermark. A mutual exchange folds the other peer into
        // membership, so the causal-stability frontier then spans both.
        let ctx_a = Context::new();
        let mut a = CrdtPlaneRuntime::new(PeerId(1));
        a.register(
            NodeId(1),
            None,
            ReplicatedCell::<MvRegister<i64>>::multi_value(&ctx_a),
        );
        let ctx_b = Context::new();
        let mut b = CrdtPlaneRuntime::new(PeerId(2));
        b.register(
            NodeId(1),
            None,
            ReplicatedCell::<MvRegister<i64>>::multi_value(&ctx_b),
        );

        let op_a = a
            .local_update::<MvRegister<i64>, _>(&ctx_a, NodeId(1), 100, |r, s| {
                r.set(1, s.peer);
            })
            .unwrap();
        let op_b = b
            .local_update::<MvRegister<i64>, _>(&ctx_b, NodeId(1), 100, |r, s| {
                r.set(2, s.peer);
            })
            .unwrap();
        assert_eq!(a.plane().membership().count(), 1, "B not seen yet");

        b.ingest(&ctx_b, &CrdtSync::new(a.wire_frontier(), vec![op_a]), 101);
        a.ingest(&ctx_a, &CrdtSync::new(b.wire_frontier(), vec![op_b]), 101);

        assert_eq!(
            a.plane().membership().count(),
            2,
            "B folded into membership"
        );
        assert_eq!(b.plane().membership().count(), 2);
        assert!(
            a.plane().stability_frontier().is_some(),
            "both members observed -> watermark active, GC may run"
        );
        assert!(b.plane().stability_frontier().is_some());
        // Concurrent MV writes are both retained (neither dominates).
        let mut a_vals = a.value::<MvRegister<i64>>(NodeId(1)).unwrap();
        a_vals.sort_unstable();
        assert_eq!(a_vals, vec![1, 2]);
    }
}
