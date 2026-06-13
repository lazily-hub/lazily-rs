//! Networked (non-loopback) str0m DataChannel integration test (#lzwebrtcnet).
//!
//! Unlike `webrtc_transport.rs` (deterministic in-memory loopback) and the
//! `Str0mLoopback` synthetic-clock backend, this drives two `Str0mNet` peers
//! over **real UDP sockets on `127.0.0.1`** — real DTLS/SCTP handshake, real
//! timers, a background driver thread per peer. It is the localhost slice of the
//! "real peer-to-peer round-trip" gate; a cross-host round trip through the live
//! #yxjw signaling Worker remains operator-gated.

#![cfg(feature = "webrtc-str0m")]

use std::time::Duration;

use lazily::{
    IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId, PeerPermissions,
    Snapshot, Str0mNet, WebRtcSink, WebRtcSource,
};

#[test]
fn networked_datachannel_carries_a_permission_filtered_snapshot() {
    let bind = "127.0.0.1:0".parse().unwrap();

    // Offer/answer exchanged in-process here; over `SignalingClient` on the wire.
    let (offerer, offer_sdp) = Str0mNet::offer(bind).expect("offer");
    let (answerer, answer_sdp) = Str0mNet::answer(bind, &offer_sdp).expect("answer");

    offerer.accept_answer(&answer_sdp).expect("accept answer");
    // Trickle each peer's host candidate to the other.
    offerer
        .add_remote_candidate(answerer.local_candidate())
        .expect("offerer adds answerer candidate");
    answerer
        .add_remote_candidate(offerer.local_candidate())
        .expect("answerer adds offerer candidate");

    assert!(
        offerer.wait_open(Duration::from_secs(15)),
        "offerer data channel should open over real UDP"
    );
    assert!(
        answerer.wait_open(Duration::from_secs(15)),
        "answerer data channel should open over real UDP"
    );

    // offerer -> answerer, with the offerer applying per-peer read filtering.
    let peer = PeerId(1);
    let mut perms = PeerPermissions::new();
    perms.allow_many(peer, OpKind::Read, [NodeId(1)]);

    let mut sink = WebRtcSink::new(offerer.channel(), perms, peer);
    let mut source = WebRtcSource::new(answerer.channel());

    let snapshot = Snapshot::new(
        1,
        vec![
            NodeSnapshot::payload(NodeId(1), "t", vec![1, 2, 3]),
            NodeSnapshot::payload(NodeId(2), "t", vec![4, 5, 6]),
        ],
        vec![],
        vec![NodeId(1), NodeId(2)],
    );
    sink.send(&IpcMessage::Snapshot(snapshot)).expect("send");

    let mut got = None;
    for _ in 0..200 {
        if let Some(msg) = source.recv().expect("recv") {
            got = Some(msg);
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    match got.expect("snapshot to arrive over the networked str0m data channel") {
        IpcMessage::Snapshot(s) => {
            let ids: Vec<u64> = s.nodes.iter().map(|n| n.node.0).collect();
            assert_eq!(ids, vec![1], "node 2 must be filtered out for this peer");
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
}
