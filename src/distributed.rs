//! Per-peer permission boundary for remote reactive-graph access (#39c5).
//!
//! This is the **policy layer** the `lazily-ipc` (snapshot + incremental
//! update) and `lazily-distributed` (CRDT cell plane) roadmap consult before a
//! remote peer may observe or mutate a shared reactive graph. It is pure logic
//! with no runtime dependencies: it answers "may peer *P* perform operation
//! *O*?" and nothing else. Snapshot/delta construction, CRDT merge, and the
//! single-writer effect authority all live in higher layers that gate their
//! work through the allowlist defined here.
//!
//! Two invariants from `SPEC.md` shape the API:
//!
//! - **Default deny.** A peer with no allowlist entry may do nothing. A peer is
//!   granted exactly the `(op-kind, node)` pairs explicitly added via
//!   [`PeerPermissions::allow`]; everything else is denied.
//! - **Omission, not redaction.** When building a snapshot or delta, a node the
//!   peer may not read is dropped *entirely* — not emitted as an `Opaque`
//!   placeholder — so a peer cannot infer the existence of nodes outside its
//!   allowlist. [`PeerPermissions::filter_readable`] applies this filter at
//!   construction time, before serialization, so the full and incremental paths
//!   share one boundary.
//!
//! The wire-facing identifiers and [`RemoteOp`] derive `serde` when the `serde`
//! feature is enabled, since they cross the network; [`PeerPermissions`] is
//! local server-side state and intentionally does not.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

/// Stable, transport-facing identifier for a reactive node (cell or slot).
///
/// Mirrors the `slot_id` carried in `lazily-ipc` snapshots and deltas. It is
/// deliberately decoupled from the crate-internal `SlotId` so the on-the-wire
/// identity stays stable independent of internal allocation details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(pub u64);

/// Identifies a remote peer participating in a distributed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PeerId(pub u64);

/// The category of access a [`RemoteOp`] requests, independent of its target.
///
/// The three kinds map to the three things a remote peer can do against a
/// shared graph: pull state, push a source-cell write, or trigger an effect on
/// the irreversible-effect plane. They are gated separately — read access never
/// implies write access, and neither implies the right to trigger effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OpKind {
    /// Read a node's value into a snapshot or delta.
    Read,
    /// Write a new value to a source cell.
    Write,
    /// Trigger an effect on the irreversible-effect plane.
    TriggerEffect,
}

/// A single operation a remote peer may request against the shared graph.
///
/// This is the unit the #39c5 allowlist gates and the unit `lazily-ipc`
/// serializes across the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RemoteOp {
    /// What kind of access the op requests.
    pub kind: OpKind,
    /// The node the op targets.
    pub node: NodeId,
}

impl RemoteOp {
    /// A request to read `node` into a snapshot or delta.
    pub fn read(node: NodeId) -> Self {
        Self {
            kind: OpKind::Read,
            node,
        }
    }

    /// A request to write a new value to source cell `node`.
    pub fn write(node: NodeId) -> Self {
        Self {
            kind: OpKind::Write,
            node,
        }
    }

    /// A request to trigger effect `node` on the irreversible-effect plane.
    pub fn trigger_effect(node: NodeId) -> Self {
        Self {
            kind: OpKind::TriggerEffect,
            node,
        }
    }
}

/// Error returned when a peer requests an operation outside its allowlist.
///
/// Carries the offending `peer` and `op` so callers can log or audit the
/// denial without recomputing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionDenied {
    /// The peer that requested the operation.
    pub peer: PeerId,
    /// The operation that was denied.
    pub op: RemoteOp,
}

impl fmt::Display for PermissionDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "peer {} denied {:?} on node {}",
            self.peer.0, self.op.kind, self.op.node.0
        )
    }
}

impl std::error::Error for PermissionDenied {}

/// The set of operations a single peer is allowed to perform.
///
/// Stored as three node sets — one per [`OpKind`] — so the op kinds are gated
/// independently. Internal to [`PeerPermissions`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PeerAllowlist {
    readable: HashSet<NodeId>,
    writable: HashSet<NodeId>,
    triggerable: HashSet<NodeId>,
}

impl PeerAllowlist {
    fn set_mut(&mut self, kind: OpKind) -> &mut HashSet<NodeId> {
        match kind {
            OpKind::Read => &mut self.readable,
            OpKind::Write => &mut self.writable,
            OpKind::TriggerEffect => &mut self.triggerable,
        }
    }

    fn set_ref(&self, kind: OpKind) -> &HashSet<NodeId> {
        match kind {
            OpKind::Read => &self.readable,
            OpKind::Write => &self.writable,
            OpKind::TriggerEffect => &self.triggerable,
        }
    }

    fn is_empty(&self) -> bool {
        self.readable.is_empty() && self.writable.is_empty() && self.triggerable.is_empty()
    }
}

/// Per-peer allowlist gating which [`RemoteOp`]s each peer may perform (#39c5).
///
/// **Default deny:** a peer with no entry is allowed nothing. Permissions are
/// granted explicitly with [`allow`](Self::allow) /
/// [`allow_many`](Self::allow_many) and removed with
/// [`revoke`](Self::revoke) / [`revoke_peer`](Self::revoke_peer). Higher layers
/// gate every remote request through [`check`](Self::check) (fail-closed) and
/// build snapshots/deltas through [`filter_readable`](Self::filter_readable).
///
/// This is local policy state and is intentionally **not** serializable; only
/// the wire-facing [`RemoteOp`] / [`NodeId`] / [`PeerId`] types are.
#[derive(Debug, Clone, Default)]
pub struct PeerPermissions {
    peers: HashMap<PeerId, PeerAllowlist>,
}

impl PeerPermissions {
    /// Create an empty permission table (every peer denied everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant `peer` permission to perform `op`.
    ///
    /// Returns `true` if the permission was newly added, `false` if `peer`
    /// already held it.
    pub fn allow(&mut self, peer: PeerId, op: RemoteOp) -> bool {
        self.peers
            .entry(peer)
            .or_default()
            .set_mut(op.kind)
            .insert(op.node)
    }

    /// Grant `peer` an `kind` of access over many nodes at once.
    ///
    /// Convenience for seeding a peer's allowlist (e.g. the shared subgraph it
    /// is permitted to observe) without one [`allow`](Self::allow) call per
    /// node.
    pub fn allow_many<I>(&mut self, peer: PeerId, kind: OpKind, nodes: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        self.peers
            .entry(peer)
            .or_default()
            .set_mut(kind)
            .extend(nodes);
    }

    /// Revoke `op` from `peer`.
    ///
    /// Returns `true` if the permission was present. Drops the peer's entry
    /// entirely once its last permission is removed so the table does not
    /// accumulate empty allowlists.
    pub fn revoke(&mut self, peer: PeerId, op: RemoteOp) -> bool {
        let Some(allow) = self.peers.get_mut(&peer) else {
            return false;
        };
        let removed = allow.set_mut(op.kind).remove(&op.node);
        if allow.is_empty() {
            self.peers.remove(&peer);
        }
        removed
    }

    /// Remove every permission held by `peer` (e.g. on disconnect).
    ///
    /// Returns `true` if `peer` had any permissions.
    pub fn revoke_peer(&mut self, peer: PeerId) -> bool {
        self.peers.remove(&peer).is_some()
    }

    /// Whether `peer` is allowed to perform `op`. Default-deny.
    pub fn is_allowed(&self, peer: PeerId, op: RemoteOp) -> bool {
        self.peers
            .get(&peer)
            .is_some_and(|allow| allow.set_ref(op.kind).contains(&op.node))
    }

    /// Fail-closed permission check.
    ///
    /// `Ok(())` when `peer` may perform `op`, otherwise
    /// `Err(PermissionDenied)` carrying the offending peer and op.
    pub fn check(&self, peer: PeerId, op: RemoteOp) -> Result<(), PermissionDenied> {
        if self.is_allowed(peer, op) {
            Ok(())
        } else {
            Err(PermissionDenied { peer, op })
        }
    }

    /// Retain only the nodes `peer` may read, preserving input order.
    ///
    /// This enforces the **omission** invariant: nodes the peer may not read
    /// are dropped from the result entirely rather than redacted in place, so a
    /// peer cannot infer their existence. Apply it at snapshot/delta
    /// construction, before serialization, so the full and incremental paths
    /// share one boundary.
    pub fn filter_readable<I>(&self, peer: PeerId, nodes: I) -> Vec<NodeId>
    where
        I: IntoIterator<Item = NodeId>,
    {
        match self.peers.get(&peer) {
            Some(allow) => nodes
                .into_iter()
                .filter(|node| allow.readable.contains(node))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Number of peers with at least one permission.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER_A: PeerId = PeerId(1);
    const PEER_B: PeerId = PeerId(2);

    #[test]
    fn default_denies_everything() {
        let perms = PeerPermissions::new();
        assert!(!perms.is_allowed(PEER_A, RemoteOp::read(NodeId(7))));
        assert!(!perms.is_allowed(PEER_A, RemoteOp::write(NodeId(7))));
        assert!(!perms.is_allowed(PEER_A, RemoteOp::trigger_effect(NodeId(7))));
        assert_eq!(perms.peer_count(), 0);
    }

    #[test]
    fn allow_grants_only_that_op() {
        let mut perms = PeerPermissions::new();
        assert!(perms.allow(PEER_A, RemoteOp::read(NodeId(7))));
        // Re-granting the same op reports "already present".
        assert!(!perms.allow(PEER_A, RemoteOp::read(NodeId(7))));

        assert!(perms.is_allowed(PEER_A, RemoteOp::read(NodeId(7))));
        // A read grant does not leak into write or effect-trigger access.
        assert!(!perms.is_allowed(PEER_A, RemoteOp::write(NodeId(7))));
        assert!(!perms.is_allowed(PEER_A, RemoteOp::trigger_effect(NodeId(7))));
        // Nor to other nodes.
        assert!(!perms.is_allowed(PEER_A, RemoteOp::read(NodeId(8))));
    }

    #[test]
    fn op_kinds_are_independent() {
        let mut perms = PeerPermissions::new();
        perms.allow(PEER_A, RemoteOp::write(NodeId(3)));
        perms.allow(PEER_A, RemoteOp::trigger_effect(NodeId(3)));

        assert!(!perms.is_allowed(PEER_A, RemoteOp::read(NodeId(3))));
        assert!(perms.is_allowed(PEER_A, RemoteOp::write(NodeId(3))));
        assert!(perms.is_allowed(PEER_A, RemoteOp::trigger_effect(NodeId(3))));
    }

    #[test]
    fn peers_are_isolated() {
        let mut perms = PeerPermissions::new();
        perms.allow(PEER_A, RemoteOp::read(NodeId(1)));
        // A's grant must not leak to B.
        assert!(!perms.is_allowed(PEER_B, RemoteOp::read(NodeId(1))));
    }

    #[test]
    fn revoke_removes_one_op_and_prunes_empty_peer() {
        let mut perms = PeerPermissions::new();
        perms.allow(PEER_A, RemoteOp::read(NodeId(1)));
        perms.allow(PEER_A, RemoteOp::read(NodeId(2)));

        assert!(perms.revoke(PEER_A, RemoteOp::read(NodeId(1))));
        assert!(!perms.is_allowed(PEER_A, RemoteOp::read(NodeId(1))));
        assert!(perms.is_allowed(PEER_A, RemoteOp::read(NodeId(2))));
        // Peer still present (one permission left).
        assert_eq!(perms.peer_count(), 1);

        // Revoking a missing op is a no-op returning false.
        assert!(!perms.revoke(PEER_A, RemoteOp::read(NodeId(99))));

        // Removing the last permission prunes the peer entry.
        assert!(perms.revoke(PEER_A, RemoteOp::read(NodeId(2))));
        assert_eq!(perms.peer_count(), 0);
    }

    #[test]
    fn revoke_peer_clears_all_ops() {
        let mut perms = PeerPermissions::new();
        perms.allow(PEER_A, RemoteOp::read(NodeId(1)));
        perms.allow(PEER_A, RemoteOp::write(NodeId(1)));

        assert!(perms.revoke_peer(PEER_A));
        assert_eq!(perms.peer_count(), 0);
        assert!(!perms.is_allowed(PEER_A, RemoteOp::read(NodeId(1))));
        // Revoking an unknown peer reports false.
        assert!(!perms.revoke_peer(PEER_B));
    }

    #[test]
    fn allow_many_seeds_a_readable_subgraph() {
        let mut perms = PeerPermissions::new();
        perms.allow_many(PEER_A, OpKind::Read, [NodeId(1), NodeId(2), NodeId(3)]);
        assert!(perms.is_allowed(PEER_A, RemoteOp::read(NodeId(1))));
        assert!(perms.is_allowed(PEER_A, RemoteOp::read(NodeId(3))));
        assert!(!perms.is_allowed(PEER_A, RemoteOp::write(NodeId(1))));
    }

    #[test]
    fn check_is_fail_closed_with_context() {
        let mut perms = PeerPermissions::new();
        perms.allow(PEER_A, RemoteOp::read(NodeId(5)));

        assert!(perms.check(PEER_A, RemoteOp::read(NodeId(5))).is_ok());

        let denied = perms.check(PEER_A, RemoteOp::write(NodeId(5))).unwrap_err();
        assert_eq!(denied.peer, PEER_A);
        assert_eq!(denied.op, RemoteOp::write(NodeId(5)));
    }

    #[test]
    fn filter_readable_omits_non_allowlisted_nodes_in_order() {
        let mut perms = PeerPermissions::new();
        perms.allow_many(PEER_A, OpKind::Read, [NodeId(1), NodeId(3)]);

        // Input order preserved; NodeId(2) and NodeId(4) dropped entirely.
        let visible = perms.filter_readable(PEER_A, [NodeId(1), NodeId(2), NodeId(3), NodeId(4)]);
        assert_eq!(visible, vec![NodeId(1), NodeId(3)]);

        // A peer with no allowlist sees nothing.
        let none = perms.filter_readable(PEER_B, [NodeId(1), NodeId(3)]);
        assert!(none.is_empty());
    }

    #[test]
    fn filter_readable_ignores_write_and_effect_grants() {
        let mut perms = PeerPermissions::new();
        // Write/trigger grants must not make a node visible to reads.
        perms.allow(PEER_A, RemoteOp::write(NodeId(1)));
        perms.allow(PEER_A, RemoteOp::trigger_effect(NodeId(2)));
        let visible = perms.filter_readable(PEER_A, [NodeId(1), NodeId(2)]);
        assert!(visible.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn remote_op_round_trips_through_serde() {
        for op in [
            RemoteOp::read(NodeId(42)),
            RemoteOp::write(NodeId(7)),
            RemoteOp::trigger_effect(NodeId(99)),
        ] {
            let json = serde_json::to_string(&op).unwrap();
            let back: RemoteOp = serde_json::from_str(&json).unwrap();
            assert_eq!(op, back);
        }
    }
}
