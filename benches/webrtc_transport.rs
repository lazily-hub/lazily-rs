//! Criterion bench for the WebRTC DataChannel IPC transport (#webrtc3).
//!
//! Quantifies the `WebRtcSink`/`WebRtcSource` codec + permission-filter overhead
//! (serialize → loopback frame → deserialize) against an in-process baseline (a
//! direct `IpcMessage` clone, no serialization), so the transport cost is known
//! before it is recommended for any path.

#![cfg(feature = "webrtc")]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use lazily::{
    InMemoryDataChannel, IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId,
    PeerPermissions, Snapshot, WebRtcSink, WebRtcSource,
};

const PEER: PeerId = PeerId(1);

fn snapshot(n: u64) -> Snapshot {
    Snapshot::new(
        1,
        (0..n)
            .map(|i| NodeSnapshot::payload(NodeId(i), "t", vec![0u8; 64]))
            .collect(),
        vec![],
        (0..n).map(NodeId).collect(),
    )
}

fn perms(n: u64) -> PeerPermissions {
    let mut p = PeerPermissions::new();
    p.allow_many(PEER, OpKind::Read, (0..n).map(NodeId));
    p
}

fn bench_transport(c: &mut Criterion) {
    let mut group = c.benchmark_group("webrtc_transport");
    for &n in &[16u64, 256, 4096] {
        let snap = snapshot(n);
        let perms = perms(n);

        // WebRTC transport: serialize + per-peer filter + loopback + deserialize.
        group.bench_with_input(BenchmarkId::new("webrtc_send_recv", n), &n, |b, _| {
            b.iter(|| {
                let (a, bb) = InMemoryDataChannel::pair();
                let mut sink = WebRtcSink::new(a, perms.clone(), PEER);
                let mut source = WebRtcSource::new(bb);
                sink.send(black_box(&IpcMessage::Snapshot(snap.clone())))
                    .unwrap();
                black_box(source.recv().unwrap());
            });
        });

        // Baseline: in-process direct clone (no serialization / transport).
        group.bench_with_input(BenchmarkId::new("in_process_clone", n), &n, |b, _| {
            b.iter(|| {
                black_box(IpcMessage::Snapshot(snap.clone()));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_transport);
criterion_main!(benches);
