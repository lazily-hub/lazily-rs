//! Concrete **WebSocket** [`DataChannel`] backend (#akp3 / #lzwstransport),
//! behind the `websocket` feature.
//!
//! A WebSocket is already ordered and reliable, so — unlike WebRTC — it
//! satisfies the [`DataChannel`] frame contract *directly*: no SDP/ICE handshake
//! and no sans-IO pump are needed (contrast `str0m_backend`). The only
//! impedance mismatch is async-vs-sync: `tokio-tungstenite` is async, but
//! [`DataChannel`] is synchronous and non-blocking so it can drop in beside the
//! in-process and WebRTC transports. [`WsDataChannel`] bridges the two with a
//! background tokio task and a pair of queues:
//!
//! - **outbound:** [`DataChannel::send_frame`] pushes onto an unbounded mpsc
//!   (a sync, non-blocking enqueue); the driver task drains it and writes each
//!   frame as one binary WebSocket message.
//! - **inbound:** the driver task reads WebSocket messages and pushes their
//!   payloads onto a shared queue that [`DataChannel::try_recv_frame`] pops.
//!
//! Each frame is one whole serialized `IpcMessage`, exactly as the
//! `WebRtcSink`/`WebRtcSource` bridge (permission filtering + codec) expects, so
//! that bridge runs unchanged over a real WebSocket. [`WsDataChannel::from_stream`]
//! accepts any already-upgraded `WebSocketStream`, so the same backend serves a
//! real network socket and the deterministic in-process loopback used in tests.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

use crate::webrtc_transport::DataChannel;

/// Error from a [`WsDataChannel`].
#[derive(Debug)]
pub enum WsError {
    /// The channel was closed (driver task ended or peer hung up).
    Closed,
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "websocket data channel closed"),
        }
    }
}

impl std::error::Error for WsError {}

/// Aborts the driver task when the last [`WsDataChannel`] handle is dropped, so a
/// dropped channel does not leak a task pumping a dead socket.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A [`DataChannel`] backed by a WebSocket connection.
///
/// Construct from any upgraded [`WebSocketStream`] with
/// [`WsDataChannel::from_stream`]; the constructor spawns the driver task, so it
/// must be called from within a tokio runtime. Cloning shares one underlying
/// connection (the queues and driver are reference-counted).
#[derive(Clone)]
pub struct WsDataChannel {
    outbound: mpsc::UnboundedSender<Vec<u8>>,
    inbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    open: Arc<AtomicBool>,
    _driver: Arc<AbortOnDrop>,
}

impl WsDataChannel {
    /// Wrap an already-upgraded WebSocket as a [`DataChannel`].
    ///
    /// Spawns the background driver that moves frames between the queues and the
    /// socket; must be called inside a tokio runtime.
    pub fn from_stream<S>(ws: WebSocketStream<S>) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let inbound = Arc::new(Mutex::new(VecDeque::new()));
        let open = Arc::new(AtomicBool::new(true));
        let driver = tokio::spawn(drive(ws, rx, inbound.clone(), open.clone()));
        Self {
            outbound: tx,
            inbound,
            open,
            _driver: Arc::new(AbortOnDrop(driver)),
        }
    }
}

/// Background task: forward queued outbound frames onto the socket and queue
/// inbound socket messages, until either side closes.
async fn drive<S>(
    ws: WebSocketStream<S>,
    mut outbound_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    inbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    open: Arc<AtomicBool>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut write, mut read) = ws.split();
    loop {
        tokio::select! {
            outgoing = outbound_rx.recv() => match outgoing {
                // One whole serialized IpcMessage per binary frame.
                Some(frame) => {
                    if write.send(Message::Binary(frame)).await.is_err() {
                        break;
                    }
                }
                // All senders (every WsDataChannel clone) dropped: close cleanly.
                None => {
                    let _ = write.close().await;
                    break;
                }
            },
            incoming = read.next() => match incoming {
                Some(Ok(Message::Binary(payload))) => {
                    inbound.lock().push_back(payload);
                }
                // Tolerate text frames carrying the same JSON payload.
                Some(Ok(Message::Text(text))) => {
                    inbound.lock().push_back(text.into_bytes());
                }
                // Control frames (ping/pong) are handled by tungstenite; ignore here.
                Some(Ok(_)) => {}
                // Close frame, stream end, or transport error: stop.
                Some(Err(_)) | None => break,
            },
        }
    }
    open.store(false, Ordering::SeqCst);
}

impl DataChannel for WsDataChannel {
    type Error = WsError;

    fn send_frame(&self, frame: Vec<u8>) -> Result<(), Self::Error> {
        if !self.is_open() {
            return Err(WsError::Closed);
        }
        // Non-blocking enqueue; the driver task performs the actual async write.
        self.outbound.send(frame).map_err(|_| WsError::Closed)
    }

    fn try_recv_frame(&self) -> Result<Option<Vec<u8>>, Self::Error> {
        // Drain already-received frames even after the socket closed, so a
        // final message is never dropped on the floor.
        Ok(self.inbound.lock().pop_front())
    }

    fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webrtc_transport::{WebRtcSink, WebRtcSource};
    use crate::{
        Delta, DeltaOp, IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId,
        PeerPermissions, Snapshot,
    };
    use std::time::Duration;

    /// Build a connected pair of `WsDataChannel`s over an in-process duplex
    /// stream with a *real* WebSocket handshake — deterministic, no network.
    async fn loopback() -> (WsDataChannel, WsDataChannel) {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            tokio_tungstenite::accept_async(server_io)
                .await
                .expect("server accept")
        });
        let (client_ws, _resp) = tokio_tungstenite::client_async("ws://localhost/", client_io)
            .await
            .expect("client connect");
        let server_ws = server.await.expect("server join");
        (
            WsDataChannel::from_stream(client_ws),
            WsDataChannel::from_stream(server_ws),
        )
    }

    /// Pump the runtime until `source` yields a message or the bound is hit.
    async fn recv_bounded<C: DataChannel>(source: &mut WebRtcSource<C>) -> Option<IpcMessage>
    where
        C::Error: std::fmt::Debug,
    {
        for _ in 0..500 {
            if let Some(msg) = source.recv().expect("recv") {
                return Some(msg);
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        None
    }

    #[tokio::test]
    async fn ws_loopback_carries_permission_filtered_snapshot() {
        let (client, server) = loopback().await;

        let peer = PeerId(1);
        let mut perms = PeerPermissions::new();
        perms.allow_many(peer, OpKind::Read, [NodeId(1), NodeId(2)]);

        let mut sink = WebRtcSink::new(client, perms, peer);
        let mut source = WebRtcSource::new(server);

        // Node 3 is NOT in the peer's read allowlist — it must be omitted.
        let snapshot = Snapshot::new(
            1,
            vec![
                NodeSnapshot::payload(NodeId(1), "t", vec![1, 2, 3]),
                NodeSnapshot::payload(NodeId(2), "t", vec![4, 5, 6]),
                NodeSnapshot::payload(NodeId(3), "t", vec![7, 8, 9]),
            ],
            vec![],
            vec![NodeId(1), NodeId(2), NodeId(3)],
        );
        sink.send(&IpcMessage::Snapshot(snapshot)).unwrap();

        match recv_bounded(&mut source)
            .await
            .expect("snapshot to arrive over the websocket")
        {
            IpcMessage::Snapshot(s) => {
                let ids: Vec<u64> = s.nodes.iter().map(|n| n.node.0).collect();
                assert_eq!(ids, vec![1, 2], "unreadable node 3 must be omitted");
            }
            other => panic!("expected snapshot, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ws_loopback_preserves_delta_order() {
        let (client, server) = loopback().await;

        let peer = PeerId(1);
        let mut perms = PeerPermissions::new();
        perms.allow_many(peer, OpKind::Read, [NodeId(1)]);

        let mut sink = WebRtcSink::new(client, perms, peer);
        let mut source = WebRtcSource::new(server);

        for epoch in 1..=3u64 {
            let delta = Delta::new(
                epoch - 1,
                epoch,
                vec![DeltaOp::cell_set(NodeId(1), vec![epoch as u8])],
            );
            sink.send(&IpcMessage::Delta(delta)).unwrap();
        }

        let mut epochs = Vec::new();
        for _ in 0..3 {
            match recv_bounded(&mut source).await.expect("delta to arrive") {
                IpcMessage::Delta(d) => epochs.push(d.epoch),
                other => panic!("expected delta, got {other:?}"),
            }
        }
        assert_eq!(epochs, vec![1, 2, 3], "deltas must arrive in send order");
    }
}
