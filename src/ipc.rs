//! Transport-agnostic snapshot/delta protocol for `lazily-ipc`.
//!
//! This module deliberately does not know whether messages move through a Unix
//! socket, pipe, WebSocket, or shared-memory ring buffer. It defines the stable
//! serializable state image and the permission-filtered construction helpers
//! that transports can carry.

use crate::distributed::{NodeId, PeerId, PeerPermissions, RemoteOp};
use std::fmt;

/// Bytes reserved before every shared-memory blob payload.
pub const SHM_BLOB_HEADER_LEN: usize = 40;

const SHM_BLOB_MAGIC: u32 = 0x4c5a_5348; // "LZSH"
const SHM_BLOB_VERSION: u16 = 1;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Serialized value bytes for a node.
///
/// The higher `lazily-serde` layer owns type-aware encoding and decoding. IPC
/// treats the payload as opaque bytes after the producing graph has serialized
/// the node value.
pub type IpcPayload = Vec<u8>;

/// Descriptor for a payload stored in a shared-memory blob arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShmBlobRef {
    /// Offset of the blob header from the beginning of the shared arena.
    pub offset: u64,
    /// Payload length in bytes, excluding the arena header.
    pub len: u64,
    /// Per-write generation used to reject stale descriptors after wraparound.
    pub generation: u64,
    /// IPC epoch associated with the message that published this blob.
    pub epoch: u64,
    /// Non-cryptographic payload checksum for torn-write/stale-region checks.
    pub checksum: u64,
}

/// IPC value stored inline or by shared-memory blob reference.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IpcValue {
    /// Inline serialized bytes.
    Inline(IpcPayload),
    /// Descriptor for bytes stored in a shared-memory blob arena.
    SharedBlob(ShmBlobRef),
}

impl From<IpcPayload> for IpcValue {
    fn from(payload: IpcPayload) -> Self {
        Self::Inline(payload)
    }
}

impl From<ShmBlobRef> for IpcValue {
    fn from(blob: ShmBlobRef) -> Self {
        Self::SharedBlob(blob)
    }
}

/// Error returned by [`ShmBlobArena`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShmBlobArenaError {
    /// The backing buffer cannot hold even one blob header plus one payload byte.
    CapacityTooSmall {
        /// Actual backing-buffer capacity.
        capacity: usize,
        /// Minimum usable capacity.
        min_capacity: usize,
    },
    /// Payload is larger than the largest single blob this arena can hold.
    BlobTooLarge {
        /// Requested payload length.
        len: usize,
        /// Maximum payload length.
        max_len: usize,
    },
    /// Descriptor points outside this arena.
    DescriptorOutOfBounds {
        /// Descriptor offset.
        offset: u64,
        /// Descriptor payload length.
        len: u64,
        /// Backing-buffer capacity.
        capacity: usize,
    },
    /// Descriptor/header metadata did not match.
    DescriptorMismatch {
        /// Mismatched field name.
        field: &'static str,
    },
    /// Payload checksum did not match the descriptor/header checksum.
    ChecksumMismatch {
        /// Expected checksum.
        expected: u64,
        /// Actual checksum.
        actual: u64,
    },
    /// The arena generation counter overflowed.
    GenerationOverflow,
}

impl fmt::Display for ShmBlobArenaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapacityTooSmall {
                capacity,
                min_capacity,
            } => write!(
                f,
                "SHM blob arena capacity {capacity} is smaller than minimum {min_capacity}"
            ),
            Self::BlobTooLarge { len, max_len } => {
                write!(f, "SHM blob length {len} exceeds maximum {max_len}")
            }
            Self::DescriptorOutOfBounds {
                offset,
                len,
                capacity,
            } => write!(
                f,
                "SHM blob descriptor offset={offset} len={len} exceeds arena capacity {capacity}"
            ),
            Self::DescriptorMismatch { field } => {
                write!(f, "SHM blob descriptor mismatch for {field}")
            }
            Self::ChecksumMismatch { expected, actual } => write!(
                f,
                "SHM blob checksum mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::GenerationOverflow => write!(f, "SHM blob generation counter overflowed"),
        }
    }
}

impl std::error::Error for ShmBlobArenaError {}

/// Fixed-size blob arena suitable for a shared-memory transport.
///
/// The arena writes a small header before each payload. Readers validate the
/// header, generation, epoch, payload length, and checksum before returning a
/// slice. The backing storage is generic so callers can use an owned `Vec<u8>`
/// for tests or an OS-specific memory mapping in a transport crate.
#[derive(Debug, Clone)]
pub struct ShmBlobArena<B = Vec<u8>> {
    bytes: B,
    write_offset: usize,
    next_generation: u64,
}

impl ShmBlobArena<Vec<u8>> {
    /// Create a Vec-backed arena with `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Result<Self, ShmBlobArenaError> {
        Self::from_buffer(vec![0; capacity])
    }
}

impl<B> ShmBlobArena<B>
where
    B: AsRef<[u8]> + AsMut<[u8]>,
{
    /// Wrap an existing byte buffer.
    pub fn from_buffer(bytes: B) -> Result<Self, ShmBlobArenaError> {
        let capacity = bytes.as_ref().len();
        let min_capacity = SHM_BLOB_HEADER_LEN + 1;
        if capacity < min_capacity {
            return Err(ShmBlobArenaError::CapacityTooSmall {
                capacity,
                min_capacity,
            });
        }

        Ok(Self {
            bytes,
            write_offset: 0,
            next_generation: 1,
        })
    }

    /// Total arena capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.bytes.as_ref().len()
    }

    /// Maximum payload length this arena can hold in one blob.
    pub fn max_blob_len(&self) -> usize {
        self.capacity() - SHM_BLOB_HEADER_LEN
    }

    /// Current write cursor offset.
    pub fn write_offset(&self) -> usize {
        self.write_offset
    }

    /// Borrow the backing bytes.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }

    /// Mutably borrow the backing bytes.
    ///
    /// Transport implementations can use this to expose the backing memory to
    /// OS mapping setup. Mutating bytes after writing descriptors may make old
    /// descriptors fail validation.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        self.bytes.as_mut()
    }

    /// Consume the arena and return its backing storage.
    pub fn into_inner(self) -> B {
        self.bytes
    }

    /// Write a payload and return a descriptor suitable for an IPC message.
    pub fn write_blob(
        &mut self,
        epoch: u64,
        payload: &[u8],
    ) -> Result<ShmBlobRef, ShmBlobArenaError> {
        let capacity = self.capacity();
        let max_len = self.max_blob_len();
        if payload.len() > max_len {
            return Err(ShmBlobArenaError::BlobTooLarge {
                len: payload.len(),
                max_len,
            });
        }

        let total_len = SHM_BLOB_HEADER_LEN + payload.len();
        if self.write_offset + total_len > capacity {
            self.write_offset = 0;
        }

        let generation = self.next_generation;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(ShmBlobArenaError::GenerationOverflow)?;

        let offset = self.write_offset;
        let checksum = checksum(payload);
        let descriptor = ShmBlobRef {
            offset: offset as u64,
            len: payload.len() as u64,
            generation,
            epoch,
            checksum,
        };

        let payload_offset = offset + SHM_BLOB_HEADER_LEN;
        let payload_end = payload_offset + payload.len();
        write_header(self.bytes.as_mut(), offset, descriptor);
        self.bytes.as_mut()[payload_offset..payload_end].copy_from_slice(payload);

        self.write_offset += total_len;
        if self.write_offset == capacity {
            self.write_offset = 0;
        }

        Ok(descriptor)
    }

    /// Read and validate a previously written blob.
    pub fn read_blob(&self, descriptor: ShmBlobRef) -> Result<&[u8], ShmBlobArenaError> {
        let capacity = self.capacity();
        let offset = usize::try_from(descriptor.offset).map_err(|_| {
            ShmBlobArenaError::DescriptorOutOfBounds {
                offset: descriptor.offset,
                len: descriptor.len,
                capacity,
            }
        })?;
        let len = usize::try_from(descriptor.len).map_err(|_| {
            ShmBlobArenaError::DescriptorOutOfBounds {
                offset: descriptor.offset,
                len: descriptor.len,
                capacity,
            }
        })?;
        let total_len = SHM_BLOB_HEADER_LEN.checked_add(len).ok_or(
            ShmBlobArenaError::DescriptorOutOfBounds {
                offset: descriptor.offset,
                len: descriptor.len,
                capacity,
            },
        )?;
        if offset
            .checked_add(total_len)
            .is_none_or(|end| end > capacity)
        {
            return Err(ShmBlobArenaError::DescriptorOutOfBounds {
                offset: descriptor.offset,
                len: descriptor.len,
                capacity,
            });
        }

        let header = read_header(self.bytes.as_ref(), offset)?;
        if header != descriptor {
            return Err(mismatch_field(header, descriptor));
        }

        let payload_offset = offset + SHM_BLOB_HEADER_LEN;
        let payload = &self.bytes.as_ref()[payload_offset..payload_offset + len];
        let actual = checksum(payload);
        if actual != descriptor.checksum {
            return Err(ShmBlobArenaError::ChecksumMismatch {
                expected: descriptor.checksum,
                actual,
            });
        }

        Ok(payload)
    }
}

/// Serializable state for one allowlisted node in a [`Snapshot`] or `NodeAdd`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeState {
    /// Concrete serialized value bytes.
    Payload(IpcPayload),
    /// Descriptor for a concrete value stored in a shared-memory blob arena.
    SharedBlob(ShmBlobRef),
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

    /// Create a visible node whose value lives in a shared-memory blob arena.
    pub fn shared_blob(node: NodeId, type_tag: impl Into<String>, blob: ShmBlobRef) -> Self {
        Self {
            node,
            type_tag: type_tag.into(),
            state: NodeState::SharedBlob(blob),
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
    CellSet { node: NodeId, payload: IpcValue },
    /// A lazily recomputed slot published a concrete value.
    SlotValue { node: NodeId, payload: IpcValue },
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
    pub fn cell_set(node: NodeId, payload: impl Into<IpcValue>) -> Self {
        Self::CellSet {
            node,
            payload: payload.into(),
        }
    }

    /// Construct a `SlotValue`.
    pub fn slot_value(node: NodeId, payload: impl Into<IpcValue>) -> Self {
        Self::SlotValue {
            node,
            payload: payload.into(),
        }
    }

    /// Construct a `CellSet` whose value is stored in a shared-memory blob.
    pub fn cell_set_blob(node: NodeId, blob: ShmBlobRef) -> Self {
        Self::cell_set(node, blob)
    }

    /// Construct a `SlotValue` whose value is stored in a shared-memory blob.
    pub fn slot_value_blob(node: NodeId, blob: ShmBlobRef) -> Self {
        Self::slot_value(node, blob)
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

fn write_header(bytes: &mut [u8], offset: usize, descriptor: ShmBlobRef) {
    let header = &mut bytes[offset..offset + SHM_BLOB_HEADER_LEN];
    write_u32(header, 0, SHM_BLOB_MAGIC);
    write_u16(header, 4, SHM_BLOB_VERSION);
    write_u16(header, 6, SHM_BLOB_HEADER_LEN as u16);
    write_u64(header, 8, descriptor.generation);
    write_u64(header, 16, descriptor.epoch);
    write_u64(header, 24, descriptor.len);
    write_u64(header, 32, descriptor.checksum);
}

fn read_header(bytes: &[u8], offset: usize) -> Result<ShmBlobRef, ShmBlobArenaError> {
    let header = &bytes[offset..offset + SHM_BLOB_HEADER_LEN];
    let magic = read_u32(header, 0);
    if magic != SHM_BLOB_MAGIC {
        return Err(ShmBlobArenaError::DescriptorMismatch { field: "magic" });
    }
    let version = read_u16(header, 4);
    if version != SHM_BLOB_VERSION {
        return Err(ShmBlobArenaError::DescriptorMismatch { field: "version" });
    }
    let header_len = read_u16(header, 6);
    if usize::from(header_len) != SHM_BLOB_HEADER_LEN {
        return Err(ShmBlobArenaError::DescriptorMismatch {
            field: "header_len",
        });
    }

    Ok(ShmBlobRef {
        offset: offset as u64,
        generation: read_u64(header, 8),
        epoch: read_u64(header, 16),
        len: read_u64(header, 24),
        checksum: read_u64(header, 32),
    })
}

fn mismatch_field(actual: ShmBlobRef, expected: ShmBlobRef) -> ShmBlobArenaError {
    let field = if actual.generation != expected.generation {
        "generation"
    } else if actual.epoch != expected.epoch {
        "epoch"
    } else if actual.len != expected.len {
        "len"
    } else if actual.checksum != expected.checksum {
        "checksum"
    } else {
        "offset"
    };

    ShmBlobArenaError::DescriptorMismatch { field }
}

fn checksum(payload: &[u8]) -> u64 {
    payload.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("slice size checked"),
    )
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("slice size checked"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("slice size checked"),
    )
}
