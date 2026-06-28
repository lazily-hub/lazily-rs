#![cfg(feature = "ipc")]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use lazily::{EdgeSnapshot, IpcMessage, NodeSnapshot, Snapshot};

const IPC_TPUT_MESSAGE_SIZE_BYTES: [usize; 3] = [64, 1_024, 16_384];
const IPC_NODE_COUNT: usize = 128;

fn make_snapshot(payload_size: usize) -> Snapshot {
    let payload = vec![0xACu8; payload_size];
    Snapshot::new(
        1,
        vec![NodeSnapshot::payload(
            lazily::NodeId(1),
            "bytes",
            payload.clone(),
        )],
        vec![EdgeSnapshot::new(lazily::NodeId(1), lazily::NodeId(1))],
        vec![lazily::NodeId(1)],
    )
}

fn make_delta_chain(count: usize) -> IpcMessage {
    let mut ops = Vec::with_capacity(count);
    let mut value: u8 = 0;
    for idx in 0..count {
        ops.push(lazily::DeltaOp::cell_set(
            lazily::NodeId(idx as u64 + 1),
            vec![value; 16],
        ));
        value = value.wrapping_add(1);
    }
    IpcMessage::Delta(lazily::Delta::next(41, ops))
}

fn benchmark_message_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_message_clone");
    for &size in &IPC_TPUT_MESSAGE_SIZE_BYTES {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("snapshot", size),
            &size,
            |b, payload_size| {
                let message = IpcMessage::Snapshot(make_snapshot(*payload_size));
                b.iter(|| black_box(message.clone()));
            },
        );
    }
    group.bench_function("delta_chain", |b| {
        let message = make_delta_chain(IPC_NODE_COUNT);
        b.iter(|| black_box(message.clone()));
    });
    group.finish();
}

fn benchmark_json_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_message_json_roundtrip");
    #[cfg(any(feature = "ffi", feature = "webrtc"))]
    {
        for &size in &IPC_TPUT_MESSAGE_SIZE_BYTES {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new("snapshot", size),
                &size,
                |b, payload_size| {
                    let message = IpcMessage::Snapshot(make_snapshot(*payload_size));
                    b.iter(|| {
                        let encoded = message.encode_json().unwrap();
                        black_box(IpcMessage::decode_json(&encoded).unwrap());
                    });
                },
            );
        }
    }
    group.finish();
}

fn benchmark_binary_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_message_binary_roundtrip");
    #[cfg(feature = "ipc-binary")]
    {
        for &size in &IPC_TPUT_MESSAGE_SIZE_BYTES {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new("snapshot", size),
                &size,
                |b, payload_size| {
                    let message = IpcMessage::Snapshot(make_snapshot(*payload_size));
                    b.iter(|| {
                        let encoded = message.encode_binary().unwrap();
                        black_box(IpcMessage::decode_binary(&encoded).unwrap());
                    });
                },
            );
        }
    }
    group.finish();
}

fn benchmark_msgpack_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_message_msgpack_roundtrip");
    #[cfg(feature = "ipc-msgpack")]
    {
        for &size in &IPC_TPUT_MESSAGE_SIZE_BYTES {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new("snapshot", size),
                &size,
                |b, payload_size| {
                    let message = IpcMessage::Snapshot(make_snapshot(*payload_size));
                    b.iter(|| {
                        let encoded = message.encode_msgpack().unwrap();
                        black_box(IpcMessage::decode_msgpack(&encoded).unwrap());
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    benchmark_message_clone,
    benchmark_json_round_trip,
    benchmark_binary_round_trip,
    benchmark_msgpack_round_trip
);
criterion_main!(benches);
