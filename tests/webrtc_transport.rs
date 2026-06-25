//! Loopback integration tests for the WebRTC DataChannel IPC transport (#webrtc3).
//!
//! Uses the dependency-light `InMemoryDataChannel` loopback (no real network /
//! WebRTC stack) to exercise the `WebRtcSink`/`WebRtcSource` bridge end to end:
//! snapshot sync, delta ordering, per-peer permission-filtered routing, and
//! reconnect-after-drop re-sync via a fresh snapshot.

#![cfg(feature = "webrtc")]

use lazily::{
    Delta, DeltaOp, InMemoryDataChannel, IpcCodec, IpcMessage, IpcSink, IpcSource, NodeId,
    NodeSnapshot, OpKind, PeerId, PeerPermissions, Snapshot, WebRtcSink, WebRtcSource,
    WebRtcTransportError,
};

const PEER: PeerId = PeerId(7);

fn perms_for(nodes: &[u64]) -> PeerPermissions {
    let mut p = PeerPermissions::new();
    p.allow_many(PEER, OpKind::Read, nodes.iter().map(|&n| NodeId(n)));
    p
}

fn snapshot(nodes: &[u64]) -> Snapshot {
    Snapshot::new(
        1,
        nodes
            .iter()
            .map(|&n| NodeSnapshot::payload(NodeId(n), "t", vec![n as u8]))
            .collect(),
        vec![],
        nodes.iter().map(|&n| NodeId(n)).collect(),
    )
}

fn loopback(
    perms: PeerPermissions,
) -> (
    WebRtcSink<InMemoryDataChannel>,
    WebRtcSource<InMemoryDataChannel>,
) {
    let (a, b) = InMemoryDataChannel::pair();
    (WebRtcSink::new(a, perms, PEER), WebRtcSource::new(b))
}

fn loopback_with_codec(
    perms: PeerPermissions,
    codec: IpcCodec,
) -> (
    WebRtcSink<InMemoryDataChannel>,
    WebRtcSource<InMemoryDataChannel>,
) {
    let (a, b) = InMemoryDataChannel::pair();
    (
        WebRtcSink::with_codec(a, perms, PEER, codec),
        WebRtcSource::with_codec(b, codec),
    )
}

fn node_ids(s: &Snapshot) -> Vec<u64> {
    s.nodes.iter().map(|n| n.node.0).collect()
}

#[test]
fn snapshot_round_trips() {
    let (mut sink, mut source) = loopback(perms_for(&[1, 2, 3]));
    sink.send(&IpcMessage::Snapshot(snapshot(&[1, 2, 3])))
        .unwrap();
    match source.recv().unwrap().expect("a message") {
        IpcMessage::Snapshot(s) => assert_eq!(node_ids(&s), vec![1, 2, 3]),
        other => panic!("expected snapshot, got {other:?}"),
    }
    assert!(source.recv().unwrap().is_none());
}

#[cfg(feature = "ipc-msgpack")]
#[test]
fn snapshot_round_trips_with_msgpack_codec() {
    let (mut sink, mut source) = loopback_with_codec(perms_for(&[1, 2, 3]), IpcCodec::MessagePack);
    assert_eq!(sink.codec(), IpcCodec::MessagePack);
    assert_eq!(source.codec(), IpcCodec::MessagePack);

    sink.send(&IpcMessage::Snapshot(snapshot(&[1, 2, 3])))
        .unwrap();
    match source.recv().unwrap().expect("a message") {
        IpcMessage::Snapshot(s) => assert_eq!(node_ids(&s), vec![1, 2, 3]),
        other => panic!("expected snapshot, got {other:?}"),
    }
}

#[test]
fn deltas_arrive_in_order() {
    let (mut sink, mut source) = loopback(perms_for(&[1, 2, 3, 4, 5]));
    for epoch in 1..=5u64 {
        let delta = Delta::new(
            epoch - 1,
            epoch,
            vec![DeltaOp::Invalidate {
                node: NodeId(epoch),
            }],
        );
        sink.send(&IpcMessage::Delta(delta)).unwrap();
    }
    for epoch in 1..=5u64 {
        match source.recv().unwrap().expect("a delta") {
            IpcMessage::Delta(d) => assert_eq!(d.epoch, epoch, "deltas must arrive in send order"),
            other => panic!("expected delta, got {other:?}"),
        }
    }
    assert!(source.recv().unwrap().is_none());
}

#[test]
fn permission_filtered_routing_drops_unreadable() {
    // The peer may read node 1 only.
    let (mut sink, mut source) = loopback(perms_for(&[1]));

    sink.send(&IpcMessage::Snapshot(snapshot(&[1, 2, 3])))
        .unwrap();
    match source.recv().unwrap().expect("a message") {
        IpcMessage::Snapshot(s) => assert_eq!(node_ids(&s), vec![1], "nodes 2,3 must be filtered"),
        other => panic!("expected snapshot, got {other:?}"),
    }

    // A delta touching only a denied node arrives with its ops filtered out.
    let delta = Delta::new(0, 1, vec![DeltaOp::Invalidate { node: NodeId(2) }]);
    sink.send(&IpcMessage::Delta(delta)).unwrap();
    match source.recv().unwrap().expect("a delta") {
        IpcMessage::Delta(d) => assert!(d.ops.is_empty(), "denied-node op must be filtered out"),
        other => panic!("expected delta, got {other:?}"),
    }
}

#[test]
fn reconnect_after_close_resyncs_via_fresh_snapshot() {
    let (mut sink, mut source) = loopback(perms_for(&[1]));

    // Drop the connection.
    sink.channel().clone().close();
    assert!(matches!(
        sink.send(&IpcMessage::Snapshot(snapshot(&[1]))),
        Err(WebRtcTransportError::Closed)
    ));
    assert!(matches!(source.recv(), Err(WebRtcTransportError::Closed)));

    // Reconnect: a fresh pair + a fresh snapshot re-syncs (deltas are not
    // replayed across a reconnect — see the transport plan).
    let (mut sink2, mut source2) = loopback(perms_for(&[1]));
    sink2.send(&IpcMessage::Snapshot(snapshot(&[1]))).unwrap();
    assert!(matches!(
        source2.recv().unwrap(),
        Some(IpcMessage::Snapshot(_))
    ));
}
