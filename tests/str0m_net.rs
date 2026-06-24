//! Networked (non-loopback) str0m DataChannel integration test (#lzwebrtcnet).
//!
//! Unlike `webrtc_transport.rs` (deterministic in-memory loopback) and the
//! `Str0mLoopback` synthetic-clock backend, this drives two `Str0mNet` peers
//! over **real UDP sockets on `127.0.0.1`** — real DTLS/SCTP handshake, real
//! timers, a background driver thread per peer. It is the localhost slice of the
//! "real peer-to-peer round-trip" gate; a cross-host round trip through the live
//! #yxjw signaling Worker remains operator-gated.

#![cfg(feature = "webrtc-str0m")]

use std::time::{Duration, Instant};

use lazily::{
    DataChannel, IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId,
    PeerPermissions, Snapshot, Str0mNet, Str0mNetError, WebRtcSink, WebRtcSource,
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
    // A successful handshake must not record an apply-time failure
    // (#lzstr0mnetacceptanswer).
    assert!(
        offerer.last_error().is_none(),
        "happy-path open must leave last_error clear: {:?}",
        offerer.last_error()
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

/// Regression for #lzstr0mframe: a burst of frames large enough to overrun the
/// SCTP send buffer must arrive in order at the remote peer, with no silent
/// drops. Pre-fix the driver's flush loop ignored `Channel::write`'s `Ok(false)`
/// backpressure signal and popped the frame regardless, so a sustained send
/// rate lost frames silently and the remote graph diverged.
#[test]
fn burst_of_frames_arrives_in_order_under_backpressure() {
    let bind = "127.0.0.1:0".parse().unwrap();

    let (offerer, offer_sdp) = Str0mNet::offer(bind).expect("offer");
    let (answerer, answer_sdp) = Str0mNet::answer(bind, &offer_sdp).expect("answer");

    offerer.accept_answer(&answer_sdp).expect("accept answer");
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

    // Use the raw `DataChannel` contract directly (no `IpcMessage` codec) so
    // the assertion is over the bytes the driver actually handed to str0m.
    let send_chan = offerer.channel();
    let recv_chan = answerer.channel();

    // 8 KiB frames × 100 = 800 KiB total, enough to overrun the SCTP send
    // window before the driver's `poll_output`/`recv_from` cycle can drain it,
    // forcing at least one `Ok(false)` backpressure return from `Channel::write`.
    const N: usize = 100;
    const FRAME_LEN: usize = 8 * 1024;
    let mut next: u32 = 0;
    while next < N as u32 {
        let mut payload = vec![0u8; FRAME_LEN];
        payload[..4].copy_from_slice(&next.to_le_bytes());
        match send_chan.send_frame(payload) {
            Ok(()) => next += 1,
            // Backpressure is the documented flow-control signal: yield and
            // retry. The point of the test is that retried frames are NOT
            // silently dropped — they all arrive in order.
            Err(Str0mNetError::Backpressure) => std::thread::sleep(Duration::from_millis(2)),
            Err(e) => panic!("unexpected send error at frame {next}: {e:?}"),
        }
    }

    // Receive every frame; assert count + in-order delivery. The deadline is
    // generous because the SCTP drain + handshake timers interact under
    // contention on a loaded CI runner.
    let mut got: Vec<u32> = Vec::with_capacity(N);
    let deadline = Instant::now() + Duration::from_secs(30);
    while got.len() < N && Instant::now() < deadline {
        match recv_chan.try_recv_frame().expect("recv") {
            Some(frame) => {
                assert_eq!(
                    frame.len(),
                    FRAME_LEN,
                    "frame {} has wrong length",
                    got.len()
                );
                let seq = u32::from_le_bytes(frame[..4].try_into().unwrap());
                got.push(seq);
            }
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }

    assert_eq!(
        got.len(),
        N,
        "all {N} burst frames must arrive — silent drop regression (#lzstr0mframe) returned"
    );
    for (i, &seq) in got.iter().enumerate() {
        assert_eq!(
            seq, i as u32,
            "frame {i} arrived out of order (seq={seq}); ordered/reliable DataChannel contract violated"
        );
    }
}
