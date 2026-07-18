//! `#lzperfaudit` Phase 3 wire-format optimization benchmarks.
//!
//! Measures the three spec-ratified wire wins:
//! - `#lzspecfrontiersuppress` — omitted frontier on CrdtSync (wire size + round-trip)
//! - `#lzspecbase64` — base64 byte arrays vs JSON-u8 arrays (wire size + round-trip)
//! - `#lzspecintern` — batch string-intern table for repeated type_tag (wire size + round-trip)

#![cfg(feature = "json-base64")]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use lazily::{
    CrdtOp, CrdtSync, EdgeSnapshot, IpcMessage, NodeId, NodeSnapshot, Snapshot, WireStamp,
};

const FRONTIER_PEERS: usize = 8;
const INTERN_NODE_COUNT: usize = 256;

fn make_crdt_sync_with_frontier() -> IpcMessage {
    let frontier: Vec<(u64, WireStamp)> = (1..=FRONTIER_PEERS as u64)
        .map(|p| {
            (
                p,
                WireStamp {
                    wall_time: 1000 + p,
                    logical: p,
                    peer: p,
                },
            )
        })
        .collect();
    let ops: Vec<CrdtOp> = (1u64..=4)
        .map(|i| {
            CrdtOp::new(
                NodeId(i),
                WireStamp {
                    wall_time: 1000 + i,
                    logical: i,
                    peer: 1,
                },
                vec![i as u8; 16],
            )
        })
        .collect();
    IpcMessage::CrdtSync(CrdtSync::new(frontier, ops))
}

fn make_crdt_sync_ops_only() -> IpcMessage {
    let ops: Vec<CrdtOp> = (1u64..=4)
        .map(|i| {
            CrdtOp::new(
                NodeId(i),
                WireStamp {
                    wall_time: 1000 + i,
                    logical: i,
                    peer: 1,
                },
                vec![i as u8; 16],
            )
        })
        .collect();
    IpcMessage::CrdtSync(CrdtSync::ops_only(ops))
}

fn make_snapshot_with_payload(size: usize) -> IpcMessage {
    let payload = vec![0xACu8; size];
    IpcMessage::Snapshot(Snapshot::new(
        1,
        vec![NodeSnapshot::payload(NodeId(1), "bytes", payload)],
        vec![EdgeSnapshot::new(NodeId(1), NodeId(1))],
        vec![NodeId(1)],
    ))
}

fn make_snapshot_many_tags(count: usize) -> IpcMessage {
    let nodes: Vec<NodeSnapshot> = (0..count)
        .map(|i| {
            NodeSnapshot::payload(
                NodeId(i as u64 + 1),
                if i % 4 == 0 {
                    "counter"
                } else if i % 4 == 1 {
                    "lww"
                } else if i % 4 == 2 {
                    "mv"
                } else {
                    "pn-counter"
                },
                vec![i as u8],
            )
        })
        .collect();
    let roots: Vec<NodeId> = nodes.iter().map(|n| n.node).collect();
    IpcMessage::Snapshot(Snapshot::new(1, nodes, vec![], roots))
}

fn bench_frontier_suppress(c: &mut Criterion) {
    let mut group = c.benchmark_group("lzspec_frontier_suppress");
    let with_frontier = make_crdt_sync_with_frontier();
    let ops_only = make_crdt_sync_ops_only();

    let with_frontier_bytes = with_frontier.encode_json().unwrap().len();
    let ops_only_bytes = ops_only.encode_json().unwrap().len();

    group.throughput(Throughput::Bytes(ops_only_bytes as u64));
    group.bench_function("encode_with_frontier", |b| {
        b.iter(|| black_box(with_frontier.encode_json().unwrap()));
    });
    group.bench_function("encode_ops_only", |b| {
        b.iter(|| black_box(ops_only.encode_json().unwrap()));
    });
    group.bench_function("decode_with_frontier", |b| {
        let encoded = with_frontier.encode_json().unwrap();
        b.iter(|| black_box(IpcMessage::decode_json(&encoded).unwrap()));
    });
    group.bench_function("decode_ops_only", |b| {
        let encoded = ops_only.encode_json().unwrap();
        b.iter(|| black_box(IpcMessage::decode_json(&encoded).unwrap()));
    });
    group.finish();

    println!(
        "\n#lzspecfrontiersuppress wire: with_frontier={B0}B ops_only={B1}B (savings {pct:.0}%)\n",
        B0 = with_frontier_bytes,
        B1 = ops_only_bytes,
        pct = (1.0 - ops_only_bytes as f64 / with_frontier_bytes as f64) * 100.0
    );
}

fn bench_base64(c: &mut Criterion) {
    let mut group = c.benchmark_group("lzspec_base64");
    for &size in &[64usize, 1_024, 16_384] {
        let msg = make_snapshot_with_payload(size);
        let canonical = msg.encode_json().unwrap();
        let b64 = msg.encode_json_base64().unwrap();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("encode_json_u8", size), &msg, |b, msg| {
            b.iter(|| black_box(msg.encode_json().unwrap()));
        });
        group.bench_with_input(BenchmarkId::new("encode_base64", size), &msg, |b, msg| {
            b.iter(|| black_box(msg.encode_json_base64().unwrap()));
        });
        group.bench_with_input(
            BenchmarkId::new("decode_json_u8", size),
            &canonical,
            |b, encoded| {
                b.iter(|| black_box(IpcMessage::decode_json(encoded).unwrap()));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("decode_base64", size),
            &b64,
            |b, encoded| {
                b.iter(|| black_box(IpcMessage::decode_json_base64(encoded).unwrap()));
            },
        );

        println!(
            "\n#lzspecbase64 @ {size}B payload: json_u8={B0}B base64={B1}B (savings {pct:.0}%)\n",
            B0 = canonical.len(),
            B1 = b64.len(),
            pct = (1.0 - b64.len() as f64 / canonical.len() as f64) * 100.0
        );
    }
    group.finish();
}

fn bench_intern(c: &mut Criterion) {
    let mut group = c.benchmark_group("lzspec_intern");
    let msg = make_snapshot_many_tags(INTERN_NODE_COUNT);
    let canonical = serde_json::to_vec(&msg).unwrap();
    let interned = msg.encode_json_intern().unwrap();

    group.throughput(Throughput::Bytes(interned.len() as u64));
    group.bench_function("encode_inline", |b| {
        b.iter(|| black_box(serde_json::to_vec(&msg).unwrap()));
    });
    group.bench_function("encode_intern", |b| {
        b.iter(|| black_box(msg.encode_json_intern().unwrap()));
    });
    group.bench_function("decode_inline", |b| {
        b.iter(|| black_box(serde_json::from_slice::<IpcMessage>(&canonical).unwrap()));
    });
    group.bench_function("decode_intern", |b| {
        b.iter(|| black_box(IpcMessage::decode_json_intern(&interned).unwrap()));
    });
    group.finish();

    println!(
        "\n#lzspecintern @ {INTERN_NODE_COUNT} nodes: inline={B0}B interned={B1}B (savings {pct:.0}%)\n",
        B0 = canonical.len(),
        B1 = interned.len(),
        pct = (1.0 - interned.len() as f64 / canonical.len() as f64) * 100.0
    );
}

criterion_group!(benches, bench_frontier_suppress, bench_base64, bench_intern);
criterion_main!(benches);
