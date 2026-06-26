//! Transport-agnostic snapshot/delta protocol for `lazily-ipc`.
//!
//! This module deliberately does not know whether messages move through a Unix
//! socket, pipe, WebSocket, or shared-memory ring buffer. It defines the stable
//! serializable state image and the permission-filtered construction helpers
//! that transports can carry.
//!
//! FFI adapters should treat encoded [`IpcMessage`] values as owned byte buffers
//! crossing the ABI. They should not expose live Rust contexts, references,
//! closures, or typed handles to foreign runtimes.

use crate::distributed::{NodeId, PeerId, PeerPermissions, RemoteOp};
use std::collections::HashMap;
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

/// Maximum encoded byte length of a [`NodeKey`] path.
pub const NODE_KEY_MAX_LEN: usize = 1024;
/// Maximum number of `/`-separated segments in a [`NodeKey`].
pub const NODE_KEY_MAX_SEGMENTS: usize = 32;

/// Why a [`NodeKey`] failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKeyError {
    /// The path was empty.
    Empty,
    /// The path exceeded [`NODE_KEY_MAX_LEN`] bytes.
    TooLong {
        /// Byte length of the offending path.
        len: usize,
    },
    /// The path had more than [`NODE_KEY_MAX_SEGMENTS`] segments.
    TooManySegments {
        /// Segment count of the offending path.
        segments: usize,
    },
    /// The path contained an empty segment (leading/trailing/double `/`).
    EmptySegment,
}

impl fmt::Display for NodeKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "node key path is empty"),
            Self::TooLong { len } => {
                write!(
                    f,
                    "node key path is {len} bytes, exceeds {NODE_KEY_MAX_LEN}"
                )
            }
            Self::TooManySegments { segments } => write!(
                f,
                "node key has {segments} segments, exceeds {NODE_KEY_MAX_SEGMENTS}"
            ),
            Self::EmptySegment => write!(f, "node key path has an empty segment"),
        }
    }
}

impl std::error::Error for NodeKeyError {}

/// Wire-stable keyed address for a collection entry.
///
/// A `NodeKey` is a `/`-joined path (e.g. `scores/alice`, `sheet/A1`,
/// `outer/k1/inner/k2`). Unlike [`NodeId`] — the volatile internal handle —
/// a `NodeKey` is producer-defined and stable across NodeId churn: a
/// removed-then-readded entry keeps the same key, so a peer can subscribe to
/// "entry `scores/alice`" without maintaining an out-of-band key→NodeId map.
/// A multi-segment path addresses nested collections (an entry of a `CellMap`
/// inside a `CellMap` entry) with no extra machinery.
///
/// Length and segment count are bounded ([`NODE_KEY_MAX_LEN`],
/// [`NODE_KEY_MAX_SEGMENTS`]) to cap attacker-controlled growth; oversized keys
/// are rejected on construction and on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey(String);

impl NodeKey {
    /// Construct a validated key from a `/`-joined path.
    pub fn new(path: impl Into<String>) -> Result<Self, NodeKeyError> {
        let path = path.into();
        Self::validate(&path)?;
        Ok(Self(path))
    }

    /// Construct a key from already-validated segments.
    ///
    /// Segments are joined with `/`; the result is validated.
    pub fn from_segments<I, S>(segments: I) -> Result<Self, NodeKeyError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let joined = segments
            .into_iter()
            .map(|s| s.as_ref().to_owned())
            .collect::<Vec<_>>()
            .join("/");
        Self::new(joined)
    }

    fn validate(path: &str) -> Result<(), NodeKeyError> {
        if path.is_empty() {
            return Err(NodeKeyError::Empty);
        }
        if path.len() > NODE_KEY_MAX_LEN {
            return Err(NodeKeyError::TooLong { len: path.len() });
        }
        let mut segments = 0usize;
        for segment in path.split('/') {
            if segment.is_empty() {
                return Err(NodeKeyError::EmptySegment);
            }
            segments += 1;
        }
        if segments > NODE_KEY_MAX_SEGMENTS {
            return Err(NodeKeyError::TooManySegments { segments });
        }
        Ok(())
    }

    /// The full `/`-joined path.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Iterate the path segments.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for NodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for NodeKey {
    type Error = NodeKeyError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for NodeKey {
    type Error = NodeKeyError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl serde::Serialize for NodeKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for NodeKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let path = String::deserialize(deserializer)?;
        Self::new(path).map_err(serde::de::Error::custom)
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
///
/// `key` serialization is format-aware: self-describing codecs (JSON,
/// MessagePack) omit it when `None` so pre-`key` encoders/decoders round-trip
/// unchanged; positional Postcard always writes the optional discriminant so
/// the binary schema stays stable. See [`NodeKey`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSnapshot {
    /// Wire-stable node identifier.
    pub node: NodeId,
    /// Producer-defined type tag for decoding `state`.
    pub type_tag: String,
    /// Serialized value bytes, or `Opaque` when the node is visible but
    /// type-erased serialization was not available.
    pub state: NodeState,
    /// Optional wire-stable keyed address for this node (a `CellMap`/`CellFamily`
    /// entry's path). `None` keeps today's opaque-NodeId-only addressing.
    pub key: Option<NodeKey>,
}

impl serde::Serialize for NodeSnapshot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        // Omit a `None` key only in self-describing formats; positional codecs
        // (Postcard) must always carry the field so the schema stays stable.
        let emit_key = self.key.is_some() || !serializer.is_human_readable();
        let mut st = serializer.serialize_struct("NodeSnapshot", 3 + emit_key as usize)?;
        st.serialize_field("node", &self.node)?;
        st.serialize_field("type_tag", &self.type_tag)?;
        st.serialize_field("state", &self.state)?;
        if emit_key {
            st.serialize_field("key", &self.key)?;
        }
        st.end()
    }
}

impl<'de> serde::Deserialize<'de> for NodeSnapshot {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename = "NodeSnapshot")]
        struct Raw {
            node: NodeId,
            type_tag: String,
            state: NodeState,
            #[serde(default)]
            key: Option<NodeKey>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            node: raw.node,
            type_tag: raw.type_tag,
            state: raw.state,
            key: raw.key,
        })
    }
}

impl NodeSnapshot {
    /// Create a visible node carrying serialized value bytes.
    pub fn payload(node: NodeId, type_tag: impl Into<String>, payload: IpcPayload) -> Self {
        Self {
            node,
            type_tag: type_tag.into(),
            state: NodeState::Payload(payload),
            key: None,
        }
    }

    /// Create a visible node whose value cannot be serialized.
    pub fn opaque(node: NodeId, type_tag: impl Into<String>) -> Self {
        Self {
            node,
            type_tag: type_tag.into(),
            state: NodeState::Opaque,
            key: None,
        }
    }

    /// Create a visible node whose value lives in a shared-memory blob arena.
    pub fn shared_blob(node: NodeId, type_tag: impl Into<String>, blob: ShmBlobRef) -> Self {
        Self {
            node,
            type_tag: type_tag.into(),
            state: NodeState::SharedBlob(blob),
            key: None,
        }
    }

    /// Attach a wire-stable [`NodeKey`] to this node (builder style).
    pub fn with_key(mut self, key: NodeKey) -> Self {
        self.key = Some(key);
        self
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
///
/// `NodeAdd`'s `key` serialization is format-aware (see [`NodeSnapshot`]):
/// self-describing codecs omit a `None` key; positional Postcard keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        /// Optional wire-stable keyed address for the new node (see
        /// [`NodeKey`]). `None` keeps opaque-NodeId-only addressing.
        key: Option<NodeKey>,
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

fn delta_op_key_ref_is_none(key: &&Option<NodeKey>) -> bool {
    key.is_none()
}

impl serde::Serialize for DeltaOp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Two borrowed shadows differing only in `NodeAdd.key` omission. The
        // human-readable shadow omits a `None` key; the binary shadow always
        // writes it so positional Postcard keeps a stable schema.
        #[derive(serde::Serialize)]
        #[serde(rename = "DeltaOp")]
        enum Hr<'a> {
            CellSet {
                node: &'a NodeId,
                payload: &'a IpcValue,
            },
            SlotValue {
                node: &'a NodeId,
                payload: &'a IpcValue,
            },
            Invalidate {
                node: &'a NodeId,
            },
            NodeAdd {
                node: &'a NodeId,
                type_tag: &'a String,
                state: &'a NodeState,
                #[serde(skip_serializing_if = "delta_op_key_ref_is_none")]
                key: &'a Option<NodeKey>,
            },
            NodeRemove {
                node: &'a NodeId,
            },
            EdgeAdd {
                dependent: &'a NodeId,
                dependency: &'a NodeId,
            },
            EdgeRemove {
                dependent: &'a NodeId,
                dependency: &'a NodeId,
            },
        }
        #[derive(serde::Serialize)]
        #[serde(rename = "DeltaOp")]
        enum Bin<'a> {
            CellSet {
                node: &'a NodeId,
                payload: &'a IpcValue,
            },
            SlotValue {
                node: &'a NodeId,
                payload: &'a IpcValue,
            },
            Invalidate {
                node: &'a NodeId,
            },
            NodeAdd {
                node: &'a NodeId,
                type_tag: &'a String,
                state: &'a NodeState,
                key: &'a Option<NodeKey>,
            },
            NodeRemove {
                node: &'a NodeId,
            },
            EdgeAdd {
                dependent: &'a NodeId,
                dependency: &'a NodeId,
            },
            EdgeRemove {
                dependent: &'a NodeId,
                dependency: &'a NodeId,
            },
        }

        if serializer.is_human_readable() {
            match self {
                DeltaOp::CellSet { node, payload } => Hr::CellSet { node, payload },
                DeltaOp::SlotValue { node, payload } => Hr::SlotValue { node, payload },
                DeltaOp::Invalidate { node } => Hr::Invalidate { node },
                DeltaOp::NodeAdd {
                    node,
                    type_tag,
                    state,
                    key,
                } => Hr::NodeAdd {
                    node,
                    type_tag,
                    state,
                    key,
                },
                DeltaOp::NodeRemove { node } => Hr::NodeRemove { node },
                DeltaOp::EdgeAdd {
                    dependent,
                    dependency,
                } => Hr::EdgeAdd {
                    dependent,
                    dependency,
                },
                DeltaOp::EdgeRemove {
                    dependent,
                    dependency,
                } => Hr::EdgeRemove {
                    dependent,
                    dependency,
                },
            }
            .serialize(serializer)
        } else {
            match self {
                DeltaOp::CellSet { node, payload } => Bin::CellSet { node, payload },
                DeltaOp::SlotValue { node, payload } => Bin::SlotValue { node, payload },
                DeltaOp::Invalidate { node } => Bin::Invalidate { node },
                DeltaOp::NodeAdd {
                    node,
                    type_tag,
                    state,
                    key,
                } => Bin::NodeAdd {
                    node,
                    type_tag,
                    state,
                    key,
                },
                DeltaOp::NodeRemove { node } => Bin::NodeRemove { node },
                DeltaOp::EdgeAdd {
                    dependent,
                    dependency,
                } => Bin::EdgeAdd {
                    dependent,
                    dependency,
                },
                DeltaOp::EdgeRemove {
                    dependent,
                    dependency,
                } => Bin::EdgeRemove {
                    dependent,
                    dependency,
                },
            }
            .serialize(serializer)
        }
    }
}

impl<'de> serde::Deserialize<'de> for DeltaOp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename = "DeltaOp")]
        enum Wire {
            CellSet {
                node: NodeId,
                payload: IpcValue,
            },
            SlotValue {
                node: NodeId,
                payload: IpcValue,
            },
            Invalidate {
                node: NodeId,
            },
            NodeAdd {
                node: NodeId,
                type_tag: String,
                state: NodeState,
                #[serde(default)]
                key: Option<NodeKey>,
            },
            NodeRemove {
                node: NodeId,
            },
            EdgeAdd {
                dependent: NodeId,
                dependency: NodeId,
            },
            EdgeRemove {
                dependent: NodeId,
                dependency: NodeId,
            },
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::CellSet { node, payload } => DeltaOp::CellSet { node, payload },
            Wire::SlotValue { node, payload } => DeltaOp::SlotValue { node, payload },
            Wire::Invalidate { node } => DeltaOp::Invalidate { node },
            Wire::NodeAdd {
                node,
                type_tag,
                state,
                key,
            } => DeltaOp::NodeAdd {
                node,
                type_tag,
                state,
                key,
            },
            Wire::NodeRemove { node } => DeltaOp::NodeRemove { node },
            Wire::EdgeAdd {
                dependent,
                dependency,
            } => DeltaOp::EdgeAdd {
                dependent,
                dependency,
            },
            Wire::EdgeRemove {
                dependent,
                dependency,
            } => DeltaOp::EdgeRemove {
                dependent,
                dependency,
            },
        })
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

/// Consumer-side registry mapping wire-stable [`NodeKey`]s to current
/// [`NodeId`]s.
///
/// A subscriber expresses interest as a key (`scores/alice`); the index keeps
/// that key resolvable across NodeId churn. When an entry is removed and later
/// re-added under a fresh NodeId, ingesting the `NodeRemove` + keyed `NodeAdd`
/// (or a fresh `Snapshot`) repoints the key at the new NodeId, so the
/// key-expressed subscription stays valid. The mapping is bijective: each
/// NodeId resolves back to at most one key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyIndex {
    forward: HashMap<NodeKey, NodeId>,
    reverse: HashMap<NodeId, NodeKey>,
}

impl KeyIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the index with the keyed nodes of a full snapshot.
    pub fn ingest_snapshot(&mut self, snapshot: &Snapshot) {
        self.forward.clear();
        self.reverse.clear();
        for node in &snapshot.nodes {
            if let Some(key) = &node.key {
                self.insert(key.clone(), node.node);
            }
        }
    }

    /// Apply one delta's keyed `NodeAdd` / `NodeRemove` ops to the index.
    pub fn apply_delta(&mut self, delta: &Delta) {
        for op in &delta.ops {
            match op {
                DeltaOp::NodeAdd {
                    node,
                    key: Some(key),
                    ..
                } => self.insert(key.clone(), *node),
                DeltaOp::NodeRemove { node } => self.remove_node(*node),
                _ => {}
            }
        }
    }

    /// Bind `key` to `node`, dropping any stale forward/reverse entries so the
    /// mapping stays bijective across re-keying and NodeId churn.
    pub fn insert(&mut self, key: NodeKey, node: NodeId) {
        if let Some(old_key) = self.reverse.insert(node, key.clone())
            && old_key != key
        {
            self.forward.remove(&old_key);
        }
        if let Some(old_node) = self.forward.insert(key, node)
            && old_node != node
        {
            self.reverse.remove(&old_node);
        }
    }

    /// Drop whatever key currently resolves to `node`.
    pub fn remove_node(&mut self, node: NodeId) {
        if let Some(key) = self.reverse.remove(&node) {
            self.forward.remove(&key);
        }
    }

    /// Resolve a wire-stable key to its current NodeId.
    pub fn node_for_key(&self, key: &NodeKey) -> Option<NodeId> {
        self.forward.get(key).copied()
    }

    /// Resolve a NodeId back to its wire-stable key.
    pub fn key_for_node(&self, node: NodeId) -> Option<&NodeKey> {
        self.reverse.get(&node)
    }

    /// Number of keyed entries.
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    /// Whether the index has no keyed entries.
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

/// Tagged IPC protocol message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IpcMessage {
    /// Full graph image.
    Snapshot(Snapshot),
    /// Incremental graph update.
    Delta(Delta),
}

/// Negotiated codec for serialized [`IpcMessage`] frames.
#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcCodec {
    /// Canonical, inspectable JSON frame encoding.
    #[cfg(any(feature = "ffi", feature = "webrtc"))]
    Json,
    /// Named MessagePack encoding for cross-language binary frames.
    #[cfg(feature = "ipc-msgpack")]
    MessagePack,
    /// Compact postcard encoding for Rust/same-schema peers.
    #[cfg(feature = "ipc-binary")]
    Postcard,
}

#[cfg(any(feature = "ffi", feature = "webrtc"))]
#[allow(clippy::derivable_impls)]
impl Default for IpcCodec {
    fn default() -> Self {
        Self::Json
    }
}

#[cfg(all(not(any(feature = "ffi", feature = "webrtc")), feature = "ipc-msgpack"))]
impl Default for IpcCodec {
    fn default() -> Self {
        Self::MessagePack
    }
}

#[cfg(all(
    not(any(feature = "ffi", feature = "webrtc", feature = "ipc-msgpack")),
    feature = "ipc-binary"
))]
impl Default for IpcCodec {
    fn default() -> Self {
        Self::Postcard
    }
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
impl IpcCodec {
    /// Stable negotiation token for capability handshakes.
    pub const fn name(self) -> &'static str {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json => "json",
            #[cfg(feature = "ipc-msgpack")]
            Self::MessagePack => "msgpack",
            #[cfg(feature = "ipc-binary")]
            Self::Postcard => "postcard",
        }
    }

    /// Encode an IPC message with this codec.
    pub fn encode(self, message: &IpcMessage) -> Result<Vec<u8>, EncodeError> {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json => message.encode_json(),
            #[cfg(feature = "ipc-msgpack")]
            Self::MessagePack => message.encode_msgpack(),
            #[cfg(feature = "ipc-binary")]
            Self::Postcard => message.encode_binary(),
        }
    }

    /// Decode an IPC message with this codec.
    pub fn decode(self, bytes: &[u8]) -> Result<IpcMessage, DecodeError> {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json => IpcMessage::decode_json(bytes),
            #[cfg(feature = "ipc-msgpack")]
            Self::MessagePack => IpcMessage::decode_msgpack(bytes),
            #[cfg(feature = "ipc-binary")]
            Self::Postcard => IpcMessage::decode_binary(bytes),
        }
    }
}

impl IpcMessage {
    #[cfg(any(feature = "ffi", feature = "webrtc"))]
    pub fn encode_json(&self) -> Result<Vec<u8>, EncodeError> {
        serde_json::to_vec(self).map_err(EncodeError::Json)
    }

    #[cfg(any(feature = "ffi", feature = "webrtc"))]
    pub fn decode_json(bytes: &[u8]) -> Result<Self, DecodeError> {
        serde_json::from_slice(bytes).map_err(DecodeError::Json)
    }

    #[cfg(feature = "ipc-msgpack")]
    pub fn encode_msgpack(&self) -> Result<Vec<u8>, EncodeError> {
        rmp_serde::to_vec_named(self).map_err(EncodeError::Msgpack)
    }

    #[cfg(feature = "ipc-msgpack")]
    pub fn decode_msgpack(bytes: &[u8]) -> Result<Self, DecodeError> {
        rmp_serde::from_slice(bytes).map_err(DecodeError::Msgpack)
    }

    #[cfg(feature = "ipc-binary")]
    pub fn encode_binary(&self) -> Result<Vec<u8>, EncodeError> {
        postcard::to_allocvec(self).map_err(EncodeError::Binary)
    }

    #[cfg(feature = "ipc-binary")]
    pub fn decode_binary(bytes: &[u8]) -> Result<Self, DecodeError> {
        postcard::from_bytes(bytes).map_err(DecodeError::Binary)
    }
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
#[derive(Debug)]
pub enum EncodeError {
    #[cfg(any(feature = "ffi", feature = "webrtc"))]
    Json(serde_json::Error),
    #[cfg(feature = "ipc-msgpack")]
    Msgpack(rmp_serde::encode::Error),
    #[cfg(feature = "ipc-binary")]
    Binary(postcard::Error),
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json(e) => write!(f, "JSON encode: {e}"),
            #[cfg(feature = "ipc-msgpack")]
            Self::Msgpack(e) => write!(f, "MessagePack encode: {e}"),
            #[cfg(feature = "ipc-binary")]
            Self::Binary(e) => write!(f, "binary encode: {e}"),
        }
    }
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
impl std::error::Error for EncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json(e) => Some(e),
            #[cfg(feature = "ipc-msgpack")]
            Self::Msgpack(e) => Some(e),
            #[cfg(feature = "ipc-binary")]
            Self::Binary(e) => Some(e),
        }
    }
}

#[cfg(any(feature = "ffi", feature = "webrtc"))]
impl From<serde_json::Error> for EncodeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

#[cfg(feature = "ipc-msgpack")]
impl From<rmp_serde::encode::Error> for EncodeError {
    fn from(e: rmp_serde::encode::Error) -> Self {
        Self::Msgpack(e)
    }
}

#[cfg(feature = "ipc-binary")]
impl From<postcard::Error> for EncodeError {
    fn from(e: postcard::Error) -> Self {
        Self::Binary(e)
    }
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
#[derive(Debug)]
pub enum DecodeError {
    #[cfg(any(feature = "ffi", feature = "webrtc"))]
    Json(serde_json::Error),
    #[cfg(feature = "ipc-msgpack")]
    Msgpack(rmp_serde::decode::Error),
    #[cfg(feature = "ipc-binary")]
    Binary(postcard::Error),
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json(e) => write!(f, "JSON decode: {e}"),
            #[cfg(feature = "ipc-msgpack")]
            Self::Msgpack(e) => write!(f, "MessagePack decode: {e}"),
            #[cfg(feature = "ipc-binary")]
            Self::Binary(e) => write!(f, "binary decode: {e}"),
        }
    }
}

#[cfg(any(
    feature = "ffi",
    feature = "webrtc",
    feature = "ipc-binary",
    feature = "ipc-msgpack"
))]
impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(any(feature = "ffi", feature = "webrtc"))]
            Self::Json(e) => Some(e),
            #[cfg(feature = "ipc-msgpack")]
            Self::Msgpack(e) => Some(e),
            #[cfg(feature = "ipc-binary")]
            Self::Binary(e) => Some(e),
        }
    }
}

#[cfg(any(feature = "ffi", feature = "webrtc"))]
impl From<serde_json::Error> for DecodeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

#[cfg(feature = "ipc-msgpack")]
impl From<rmp_serde::decode::Error> for DecodeError {
    fn from(e: rmp_serde::decode::Error) -> Self {
        Self::Msgpack(e)
    }
}

#[cfg(feature = "ipc-binary")]
impl From<postcard::Error> for DecodeError {
    fn from(e: postcard::Error) -> Self {
        Self::Binary(e)
    }
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
