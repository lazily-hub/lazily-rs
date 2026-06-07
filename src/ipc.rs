//! Transport-agnostic snapshot/delta protocol for `lazily-ipc`.
//!
//! This module deliberately does not know whether messages move through a Unix
//! socket, pipe, WebSocket, or shared-memory ring buffer. It defines the stable
//! serializable state image and the permission-filtered construction helpers
//! that transports can carry.

use crate::distributed::{NodeId, PeerId, PeerPermissions, RemoteOp};

/// Serialized value bytes for a node.
///
/// The higher `lazily-serde` layer owns type-aware encoding and decoding. IPC
/// treats the payload as opaque bytes after the producing graph has serialized
/// the node value.
pub type IpcPayload = Vec<u8>;

/// Serializable state for one allowlisted node in a [`Snapshot`] or `NodeAdd`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeState {
    /// Concrete serialized value bytes.
    Payload(IpcPayload),
    /// A known node whose value cannot be serialized.
    Opaque,
}

/// Full state for one node in a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeSnapshot {
    /// Wire-stable node identifier.
    pub node: NodeId,
    /// Producer-defined type tag for decoding `state`.
    pub type_tag: String,
    /// Serialized value bytes, or `Opaque` when the node is visible but
    /// type-erased serialization was not available.
    pub state: NodeState,
}

impl NodeSnapshot {
    /// Create a visible node carrying serialized value bytes.
    pub fn payload(node: NodeId, type_tag: impl Into<String>, payload: IpcPayload) -> Self {
        Self {
            node,
            type_tag: type_tag.into(),
            state: NodeState::Payload(payload),
        }
    }

    /// Create a visible node whose value cannot be serialized.
    pub fn opaque(node: NodeId, type_tag: impl Into<String>) -> Self {
        Self {
            node,
            type_tag: type_tag.into(),
            state: NodeState::Opaque,
        }
    }
}

/// Directed dependency edge in a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EdgeSnapshot {
    /// Node that depends on `dependency`.
    pub dependent: NodeId,
    /// Node read by `dependent`.
    pub dependency: NodeId,
}

impl EdgeSnapshot {
    /// Create a dependency edge.
    pub fn new(dependent: NodeId, dependency: NodeId) -> Self {
        Self {
            dependent,
            dependency,
        }
    }

    fn is_readable_by(self, permissions: &PeerPermissions, peer: PeerId) -> bool {
        can_read(permissions, peer, self.dependent) && can_read(permissions, peer, self.dependency)
    }
}

/// Full graph image sent on connect or resync.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// Context-level wire sequence number.
    pub epoch: u64,
    /// Visible node state.
    pub nodes: Vec<NodeSnapshot>,
    /// Visible dependency edges.
    pub edges: Vec<EdgeSnapshot>,
    /// Visible root/source nodes.
    pub roots: Vec<NodeId>,
}

impl Snapshot {
    /// Construct a snapshot.
    pub fn new(
        epoch: u64,
        nodes: Vec<NodeSnapshot>,
        edges: Vec<EdgeSnapshot>,
        roots: Vec<NodeId>,
    ) -> Self {
        Self {
            epoch,
            nodes,
            edges,
            roots,
        }
    }

    /// Return a peer-specific snapshot that omits non-readable nodes entirely.
    ///
    /// Edges are retained only when both endpoint nodes are readable, and roots
    /// preserve their input order after filtering.
    pub fn filter_readable(&self, permissions: &PeerPermissions, peer: PeerId) -> Self {
        let nodes = self
            .nodes
            .iter()
            .filter(|node| can_read(permissions, peer, node.node))
            .cloned()
            .collect();
        let edges = self
            .edges
            .iter()
            .copied()
            .filter(|edge| edge.is_readable_by(permissions, peer))
            .collect();
        let roots = permissions.filter_readable(peer, self.roots.iter().copied());

        Self {
            epoch: self.epoch,
            nodes,
            edges,
            roots,
        }
    }
}

/// One incremental graph mutation in a [`Delta`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeltaOp {
    /// A source cell was changed to `payload`.
    CellSet { node: NodeId, payload: IpcPayload },
    /// A lazily recomputed slot published a concrete value.
    SlotValue { node: NodeId, payload: IpcPayload },
    /// A node was dirtied without publishing a concrete value.
    Invalidate { node: NodeId },
    /// A new node became visible.
    NodeAdd {
        node: NodeId,
        type_tag: String,
        state: NodeState,
    },
    /// A node was removed.
    NodeRemove { node: NodeId },
    /// A dependency edge was added.
    EdgeAdd {
        dependent: NodeId,
        dependency: NodeId,
    },
    /// A dependency edge was removed.
    EdgeRemove {
        dependent: NodeId,
        dependency: NodeId,
    },
}

impl DeltaOp {
    /// Construct a `CellSet`.
    pub fn cell_set(node: NodeId, payload: IpcPayload) -> Self {
        Self::CellSet { node, payload }
    }

    /// Construct a `SlotValue`.
    pub fn slot_value(node: NodeId, payload: IpcPayload) -> Self {
        Self::SlotValue { node, payload }
    }

    /// Construct an `Invalidate`.
    pub fn invalidate(node: NodeId) -> Self {
        Self::Invalidate { node }
    }

    fn filter_readable(&self, permissions: &PeerPermissions, peer: PeerId) -> Option<Self> {
        match self {
            Self::CellSet { node, .. }
            | Self::SlotValue { node, .. }
            | Self::Invalidate { node }
            | Self::NodeAdd { node, .. }
            | Self::NodeRemove { node } => can_read(permissions, peer, *node).then(|| self.clone()),
            Self::EdgeAdd {
                dependent,
                dependency,
            }
            | Self::EdgeRemove {
                dependent,
                dependency,
            } => (can_read(permissions, peer, *dependent)
                && can_read(permissions, peer, *dependency))
            .then(|| self.clone()),
        }
    }
}

/// Incremental change set emitted after one outermost batch flush.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Delta {
    /// Receiver epoch this delta must apply after.
    pub base_epoch: u64,
    /// New epoch after applying this delta.
    pub epoch: u64,
    /// Coalesced operations for this flush.
    pub ops: Vec<DeltaOp>,
}

impl Delta {
    /// Construct a strictly sequential delta with `epoch == base_epoch + 1`.
    ///
    /// Panics if `base_epoch` is `u64::MAX`; a sender cannot advance beyond the
    /// maximum epoch and must start a fresh snapshot/session instead.
    pub fn next(base_epoch: u64, ops: Vec<DeltaOp>) -> Self {
        let epoch = base_epoch
            .checked_add(1)
            .expect("ipc epoch overflow requires a fresh snapshot/session");
        Self {
            base_epoch,
            epoch,
            ops,
        }
    }

    /// Construct a delta with explicit epochs.
    pub fn new(base_epoch: u64, epoch: u64, ops: Vec<DeltaOp>) -> Self {
        Self {
            base_epoch,
            epoch,
            ops,
        }
    }

    /// Whether this delta is exactly the next delta after `last_epoch`.
    pub fn is_next_after(&self, last_epoch: u64) -> bool {
        self.base_epoch == last_epoch
            && self
                .base_epoch
                .checked_add(1)
                .is_some_and(|next| self.epoch == next)
    }

    /// Return the receiver action for this delta.
    pub fn apply_status(&self, last_epoch: u64) -> DeltaApplyStatus {
        if self.is_next_after(last_epoch) {
            DeltaApplyStatus::Apply
        } else {
            DeltaApplyStatus::ResyncRequired {
                last_epoch,
                base_epoch: self.base_epoch,
                epoch: self.epoch,
            }
        }
    }

    /// Return a peer-specific delta that omits non-readable operations entirely.
    pub fn filter_readable(&self, permissions: &PeerPermissions, peer: PeerId) -> Self {
        let ops = self
            .ops
            .iter()
            .filter_map(|op| op.filter_readable(permissions, peer))
            .collect();

        Self {
            base_epoch: self.base_epoch,
            epoch: self.epoch,
            ops,
        }
    }
}

/// Receiver decision for an incoming [`Delta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaApplyStatus {
    /// Apply the delta and advance the receiver epoch.
    Apply,
    /// Discard this delta and request a fresh [`Snapshot`].
    ResyncRequired {
        /// Receiver's current epoch.
        last_epoch: u64,
        /// Delta's advertised base epoch.
        base_epoch: u64,
        /// Delta's advertised target epoch.
        epoch: u64,
    },
}

/// Tagged IPC protocol message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IpcMessage {
    /// Full graph image.
    Snapshot(Snapshot),
    /// Incremental graph update.
    Delta(Delta),
}

/// Transport sink for IPC messages.
pub trait IpcSink {
    /// Transport-specific error type.
    type Error;

    /// Send one IPC protocol message.
    fn send(&mut self, message: &IpcMessage) -> Result<(), Self::Error>;

    /// Send a snapshot.
    fn send_snapshot(&mut self, snapshot: &Snapshot) -> Result<(), Self::Error> {
        self.send(&IpcMessage::Snapshot(snapshot.clone()))
    }

    /// Send a delta.
    fn send_delta(&mut self, delta: &Delta) -> Result<(), Self::Error> {
        self.send(&IpcMessage::Delta(delta.clone()))
    }
}

/// Transport source for IPC messages.
pub trait IpcSource {
    /// Transport-specific error type.
    type Error;

    /// Receive the next IPC message.
    ///
    /// `Ok(None)` means the source is currently exhausted or closed, depending
    /// on the transport implementation.
    fn recv(&mut self) -> Result<Option<IpcMessage>, Self::Error>;
}

fn can_read(permissions: &PeerPermissions, peer: PeerId, node: NodeId) -> bool {
    permissions.is_allowed(peer, RemoteOp::read(node))
}
