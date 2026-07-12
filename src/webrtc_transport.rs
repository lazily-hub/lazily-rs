//! WebRTC DataChannel IPC transport — abstraction layer (#webrtc2 / #jpf1).
//!
//! Plan: `tasks/software/plan-lazily-webrtc-transport.md`.
//!
//! This module is the transport **structure**, not a concrete network backend:
//!
//! - [`DataChannel`] — the minimal ordered/reliable byte-frame surface a concrete
//!   WebRTC `RTCDataChannel` backend (webrtc-rs / str0m) must provide. Wiring a
//!   real backend (establishing the `RTCPeerConnection` via the existing
//!   [`crate::SignalingClient`] SDP/ICE handshake) is a deliberate follow-up so a
//!   ~100-crate WebRTC stack is not pulled into the default dependency graph.
//! - [`WebRtcSink`] / [`WebRtcSource`] — the synchronous [`IpcSink`]/[`IpcSource`]
//!   bridge over any `DataChannel`, with **outbound per-peer permission
//!   filtering**. Keeping the IPC traits synchronous lets this transport drop in
//!   beside the in-process and socket transports; a real async backend bridges to
//!   the channel internally (e.g. an mpsc drained by a tokio task — see the plan).
//! - [`InMemoryDataChannel`] — a deterministic in-process loopback channel for the
//!   `#webrtc3` tests, requiring no real network or WebRTC stack.
//!
//! **Inbound permission enforcement** is intentionally *not* done in
//! [`WebRtcSource`]: the transport carries frames, and the graph-apply layer is
//! the authority that checks `RemoteOp::write` before mutating local state. The
//! source therefore delivers the decoded message verbatim.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use crate::distributed::{PeerId, PeerPermissions};
use crate::ipc::{DecodeError, EncodeError, IpcCodec, IpcMessage, IpcSink, IpcSource};

/// Minimal ordered, reliable, bidirectional byte-frame channel.
///
/// Each frame is one whole serialized [`IpcMessage`]; ordering and reliability
/// are the backend's responsibility (a WebRTC DataChannel opened with
/// `ordered: true`). The methods are non-blocking so they satisfy the
/// synchronous [`IpcSink`]/[`IpcSource`] contracts.
pub trait DataChannel {
    /// Backend transport error type.
    type Error;

    /// Enqueue one serialized frame for delivery. Must not block.
    fn send_frame(&self, frame: Vec<u8>) -> Result<(), Self::Error>;

    /// Pop the next received frame, or `Ok(None)` when none is pending.
    fn try_recv_frame(&self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Whether the channel is still usable.
    fn is_open(&self) -> bool;
}

/// Error from a [`WebRtcSink`] or [`WebRtcSource`].
#[derive(Debug)]
pub enum WebRtcTransportError<E> {
    /// Underlying [`DataChannel`] error.
    Channel(E),
    /// Frame serialization failure.
    Encode(EncodeError),
    /// Frame deserialization failure.
    Decode(DecodeError),
    /// The channel was closed.
    Closed,
}

impl<E: std::fmt::Display> std::fmt::Display for WebRtcTransportError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Channel(e) => write!(f, "data channel error: {e}"),
            Self::Encode(e) => write!(f, "frame encode error: {e}"),
            Self::Decode(e) => write!(f, "frame decode error: {e}"),
            Self::Closed => write!(f, "data channel closed"),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for WebRtcTransportError<E> {}

/// Permission-filtering [`IpcSink`] over a [`DataChannel`].
///
/// Every outbound `Snapshot`/`Delta` is filtered to what `peer` is allowed to
/// **read** ([`Snapshot::filter_readable`](crate::Snapshot) /
/// `Delta::filter_readable`) before it is serialized and sent, so a peer never
/// receives graph state it is not entitled to.
pub struct WebRtcSink<C> {
    channel: C,
    permissions: PeerPermissions,
    peer: PeerId,
    codec: IpcCodec,
}

impl<C> WebRtcSink<C> {
    /// Wrap a channel with the local permission view and the remote peer id.
    pub fn new(channel: C, permissions: PeerPermissions, peer: PeerId) -> Self {
        Self::with_codec(channel, permissions, peer, IpcCodec::Json)
    }

    /// Wrap a channel with an explicitly negotiated frame codec.
    pub fn with_codec(
        channel: C,
        permissions: PeerPermissions,
        peer: PeerId,
        codec: IpcCodec,
    ) -> Self {
        Self {
            channel,
            permissions,
            peer,
            codec,
        }
    }

    /// Borrow the underlying channel.
    pub fn channel(&self) -> &C {
        &self.channel
    }

    /// Return the configured frame codec.
    pub fn codec(&self) -> IpcCodec {
        self.codec
    }
}

impl<C: DataChannel> IpcSink for WebRtcSink<C> {
    type Error = WebRtcTransportError<C::Error>;

    fn send(&mut self, message: &IpcMessage) -> Result<(), Self::Error> {
        if !self.channel.is_open() {
            return Err(WebRtcTransportError::Closed);
        }
        let filtered = match message {
            IpcMessage::Snapshot(s) => {
                IpcMessage::Snapshot(s.filter_readable(&self.permissions, self.peer))
            }
            IpcMessage::Delta(d) => {
                IpcMessage::Delta(d.filter_readable(&self.permissions, self.peer))
            }
            IpcMessage::CrdtSync(s) => {
                IpcMessage::CrdtSync(s.filter_readable(&self.permissions, self.peer))
            }
            // Control frames carry no node content; permission filtering is identity.
            control @ (IpcMessage::ResyncRequest(_) | IpcMessage::OutboxAck(_)) => control.clone(),
        };
        let frame = self
            .codec
            .encode(&filtered)
            .map_err(WebRtcTransportError::Encode)?;
        self.channel
            .send_frame(frame)
            .map_err(WebRtcTransportError::Channel)
    }
}

/// [`IpcSource`] over a [`DataChannel`].
///
/// Delivers each decoded [`IpcMessage`] verbatim. Inbound write-permission
/// enforcement is the graph-apply layer's responsibility (see the module note).
pub struct WebRtcSource<C> {
    channel: C,
    codec: IpcCodec,
}

impl<C> WebRtcSource<C> {
    /// Wrap a channel as an IPC source.
    pub fn new(channel: C) -> Self {
        Self::with_codec(channel, IpcCodec::Json)
    }

    /// Wrap a channel as an IPC source with an explicitly negotiated codec.
    pub fn with_codec(channel: C, codec: IpcCodec) -> Self {
        Self { channel, codec }
    }

    /// Borrow the underlying channel.
    pub fn channel(&self) -> &C {
        &self.channel
    }

    /// Return the configured frame codec.
    pub fn codec(&self) -> IpcCodec {
        self.codec
    }
}

impl<C: DataChannel> IpcSource for WebRtcSource<C> {
    type Error = WebRtcTransportError<C::Error>;

    fn recv(&mut self) -> Result<Option<IpcMessage>, Self::Error> {
        match self
            .channel
            .try_recv_frame()
            .map_err(WebRtcTransportError::Channel)?
        {
            Some(frame) => Ok(Some(
                self.codec
                    .decode(&frame)
                    .map_err(WebRtcTransportError::Decode)?,
            )),
            None => {
                if self.channel.is_open() {
                    Ok(None)
                } else {
                    Err(WebRtcTransportError::Closed)
                }
            }
        }
    }
}

/// In-process loopback [`DataChannel`] for deterministic tests (`#webrtc3`).
///
/// No real network or WebRTC stack. [`InMemoryDataChannel::pair`] returns two
/// cross-wired endpoints: a frame sent on one is received on the other, in order.
#[derive(Clone)]
pub struct InMemoryDataChannel {
    /// Our outbound queue == the peer's inbound queue.
    tx: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Our inbound queue.
    rx: Arc<Mutex<VecDeque<Vec<u8>>>>,
    open: Arc<AtomicBool>,
}

impl InMemoryDataChannel {
    /// Build a connected loopback pair.
    pub fn pair() -> (Self, Self) {
        let a_to_b = Arc::new(Mutex::new(VecDeque::new()));
        let b_to_a = Arc::new(Mutex::new(VecDeque::new()));
        let open = Arc::new(AtomicBool::new(true));
        let a = Self {
            tx: a_to_b.clone(),
            rx: b_to_a.clone(),
            open: open.clone(),
        };
        let b = Self {
            tx: b_to_a,
            rx: a_to_b,
            open,
        };
        (a, b)
    }

    /// Close both ends of the pair (simulates a dropped connection).
    pub fn close(&self) {
        self.open.store(false, Ordering::SeqCst);
    }
}

impl DataChannel for InMemoryDataChannel {
    type Error = std::convert::Infallible;

    fn send_frame(&self, frame: Vec<u8>) -> Result<(), Self::Error> {
        self.tx.lock().push_back(frame);
        Ok(())
    }

    fn try_recv_frame(&self) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(self.rx.lock().pop_front())
    }

    fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::{NodeId, PeerId, PeerPermissions, RemoteOp};
    use crate::ipc::{IpcMessage, NodeSnapshot, Snapshot};

    fn snapshot_two_nodes() -> Snapshot {
        Snapshot::new(
            1,
            vec![
                NodeSnapshot::payload(NodeId(1), "t", vec![1, 2, 3]),
                NodeSnapshot::payload(NodeId(2), "t", vec![4, 5, 6]),
            ],
            vec![],
            vec![NodeId(1), NodeId(2)],
        )
    }

    #[test]
    fn loopback_round_trips_and_filters_unreadable_nodes() {
        let (here, there) = InMemoryDataChannel::pair();
        let peer = PeerId(7);

        // The peer may read node 1 but not node 2.
        let mut perms = PeerPermissions::new();
        perms.allow(peer, RemoteOp::read(NodeId(1)));

        let mut sink = WebRtcSink::new(here, perms, peer);
        let mut source = WebRtcSource::new(there);

        sink.send(&IpcMessage::Snapshot(snapshot_two_nodes()))
            .unwrap();

        let received = source.recv().unwrap().expect("a message");
        match received {
            IpcMessage::Snapshot(s) => {
                let ids: Vec<u64> = s.nodes.iter().map(|n| n.node.0).collect();
                assert_eq!(ids, vec![1], "node 2 must be filtered out for this peer");
            }
            other => panic!("expected snapshot, got {other:?}"),
        }

        // Nothing else pending.
        assert!(source.recv().unwrap().is_none());
    }

    #[test]
    fn closed_channel_reports_closed() {
        let (here, there) = InMemoryDataChannel::pair();
        here.close();
        let mut sink = WebRtcSink::new(here, PeerPermissions::new(), PeerId(1));
        assert!(matches!(
            sink.send(&IpcMessage::Snapshot(snapshot_two_nodes())),
            Err(WebRtcTransportError::Closed)
        ));
        let mut source = WebRtcSource::new(there);
        assert!(matches!(source.recv(), Err(WebRtcTransportError::Closed)));
    }
}
