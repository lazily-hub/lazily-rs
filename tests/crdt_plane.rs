//! End-to-end runtime integration of the distributed CRDT cell plane
//! (`#lzcrdtplane5b`): two `CrdtPlaneRuntime` replicas exchange anti-entropy
//! frames *through the real `webrtc` `IpcSink`/`IpcSource` transport seam* and
//! converge their `merge:crdt` root cells — driving each replica's reactive
//! graph — with the causal-stability watermark arming once both peers are seen.
//!
//! Requires both feature trees: `distributed` (the plane primitives) and
//! `webrtc` (the wire types + the in-memory DataChannel transport).
#![cfg(all(feature = "distributed", feature = "webrtc"))]

use lazily::{
    Context, CrdtPlaneRuntime, HlcStamp, InMemoryDataChannel, IpcMessage, IpcSink, IpcSource,
    LwwRegister, NodeId, NodeKey, OpKind, PeerId, PeerPermissions, WebRtcSink, WebRtcSource,
    WireStamp,
};

/// Read+write grant on `node` for both peers, so nothing is filtered out.
fn perms(node: u64) -> PeerPermissions {
    let mut p = PeerPermissions::new();
    for peer in [PeerId(1), PeerId(2)] {
        p.allow_many(peer, OpKind::Read, [NodeId(node)]);
        p.allow_many(peer, OpKind::Write, [NodeId(node)]);
    }
    p
}

/// A zero seed stamp for `peer`, beaten by the first real `local_update`.
fn seed(peer: u64) -> HlcStamp {
    HlcStamp::from(WireStamp {
        wall_time: 0,
        logical: 0,
        peer,
    })
}

/// Pump every pending frame from `source` into `rt` at wall time `now`.
fn drain(
    rt: &mut CrdtPlaneRuntime,
    ctx: &Context,
    source: &mut WebRtcSource<InMemoryDataChannel>,
    now: u64,
) {
    while let Some(message) = source.recv().unwrap() {
        if let IpcMessage::CrdtSync(sync) = message {
            rt.ingest(ctx, &sync, now);
        }
    }
}

#[test]
fn two_replicas_converge_over_the_transport_and_drive_the_reactive_graph() {
    let node = NodeId(1);
    let key = NodeKey::new("score").unwrap();

    // --- Replica A (peer 1) ---
    let ctx_a = Context::new();
    let mut a = CrdtPlaneRuntime::new(PeerId(1));
    a.register(node, Some(key.clone()), lww_cell(&ctx_a, seed(1)));

    // --- Replica B (peer 2) ---
    let ctx_b = Context::new();
    let mut b = CrdtPlaneRuntime::new(PeerId(2));
    b.register(node, Some(key.clone()), lww_cell(&ctx_b, seed(2)));

    // A derived slot on B doubles the replicated cell — proves a remote op drives
    // the reactive graph, not just the CRDT value.
    let b_handle = b.handle::<LwwRegister<i64>>(node).unwrap();
    let b_doubled = ctx_b.computed(move |cx| b_handle.get(cx) * 2);

    // --- Transport: one full-duplex in-memory DataChannel pair (A <-> B) ---
    let (a_end, b_end) = InMemoryDataChannel::pair();
    let mut a_sink = WebRtcSink::new(a_end.clone(), perms(node.0), PeerId(2)); // A -> B
    let mut a_source = WebRtcSource::new(a_end);
    let mut b_sink = WebRtcSink::new(b_end.clone(), perms(node.0), PeerId(1)); // B -> A
    let mut b_source = WebRtcSource::new(b_end);

    // --- Concurrent local edits (neither has seen the other) ---
    // A writes later in wall time, so its stamp dominates and wins the LWW merge.
    a.local_update::<LwwRegister<i64>, _>(&ctx_a, node, 300, |r, s| {
        r.set(111, s);
    })
    .unwrap();
    b.local_update::<LwwRegister<i64>, _>(&ctx_b, node, 100, |r, s| {
        r.set(222, s);
    })
    .unwrap();

    // Pre-exchange: each replica sees only its own write; B's derived slot too.
    assert_eq!(a.value::<LwwRegister<i64>>(node), Some(111));
    assert_eq!(b.value::<LwwRegister<i64>>(node), Some(222));
    assert_eq!(ctx_b.get(&b_doubled), 444);
    assert_eq!(a.plane().membership().count(), 1);

    // --- Anti-entropy exchange over the transport ---
    a_sink.send(&IpcMessage::CrdtSync(a.sync_frame())).unwrap();
    b_sink.send(&IpcMessage::CrdtSync(b.sync_frame())).unwrap();
    drain(&mut b, &ctx_b, &mut b_source, 400); // B ingests A's frame
    drain(&mut a, &ctx_a, &mut a_source, 400); // A ingests B's frame

    // --- Both converge to A's value (higher stamp) ---
    assert_eq!(a.value::<LwwRegister<i64>>(node), Some(111));
    assert_eq!(b.value::<LwwRegister<i64>>(node), Some(111));
    // The remote op drove B's reactive graph: the derived slot recomputed.
    assert_eq!(ctx_b.get(&b_doubled), 222);

    // --- The causal-stability watermark is armed once both peers are observed ---
    assert_eq!(a.plane().membership().count(), 2);
    assert_eq!(b.plane().membership().count(), 2);
    assert!(a.plane().stability_frontier().is_some());
    assert!(b.plane().stability_frontier().is_some());

    // --- Convergence is idempotent: re-sending changes nothing ---
    a_sink.send(&IpcMessage::CrdtSync(a.sync_frame())).unwrap();
    drain(&mut b, &ctx_b, &mut b_source, 500);
    assert_eq!(b.value::<LwwRegister<i64>>(node), Some(111));
    assert_eq!(ctx_b.get(&b_doubled), 222);
}

#[test]
fn anti_entropy_reply_ships_only_missing_ops() {
    let node = NodeId(5);
    let ctx_a = Context::new();
    let mut a = CrdtPlaneRuntime::new(PeerId(1));
    a.register(node, None, lww_cell(&ctx_a, seed(1)));

    // Two sequential A writes -> two ops in A's log.
    a.local_update::<LwwRegister<i64>, _>(&ctx_a, node, 100, |r, s| {
        r.set(1, s);
    })
    .unwrap();
    a.local_update::<LwwRegister<i64>, _>(&ctx_a, node, 200, |r, s| {
        r.set(2, s);
    })
    .unwrap();
    assert_eq!(a.sync_frame().ops.len(), 2, "full frame ships both ops");

    // A peer that already advertises A's current frontier is missing nothing.
    let request = lazily::CrdtSync::new(a.wire_frontier(), vec![]);
    assert_eq!(
        a.sync_reply(&request).ops.len(),
        0,
        "a caught-up peer pulls no ops"
    );
}

/// An `i64` LWW root cell seeded at `seed`, beaten by the first real write.
fn lww_cell(ctx: &Context, seed: HlcStamp) -> lazily::ReplicatedCell<LwwRegister<i64>> {
    lazily::ReplicatedCell::lww(ctx, 0, seed)
}
