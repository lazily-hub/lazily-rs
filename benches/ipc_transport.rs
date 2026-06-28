#![cfg(feature = "ipc")]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use lazily::{
    EdgeSnapshot, IPC_DEFAULT_QUEUE_CAPACITY, InProcessIpcTransport, IpcControlFrame,
    IpcFramePayload, IpcMessage, IpcTransport, IpcTransportError, NodeSnapshot, Snapshot,
};

const IPC_BACKPRESSURE_DEPTH: usize = 16;
const IPC_TPUT_MESSAGE_SIZE_BYTES: [usize; 3] = [64, 1_024, 16_384];
const IPC_TPUT_ITERS: usize = 256;
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

fn benchmark_round_trip_control(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_transport_control_roundtrip");

    group.bench_function("heartbeats", |b| {
        b.iter(|| {
            let (mut sender, mut receiver) =
                InProcessIpcTransport::pair(1, 2, IPC_DEFAULT_QUEUE_CAPACITY);
            for idx in 0..IPC_TPUT_ITERS {
                let _ = sender.send_control(IpcControlFrame::Heartbeat);
                let _ = receiver.recv_frame().unwrap().unwrap();
                black_box(idx);
            }
        })
    });

    group.bench_function("control_with_retransmit", |b| {
        b.iter(|| {
            let (mut sender, mut receiver) =
                InProcessIpcTransport::pair(1, 2, IPC_DEFAULT_QUEUE_CAPACITY);
            let heartbeat = sender.send_control(IpcControlFrame::Heartbeat).unwrap();
            let _ = receiver.recv_frame().unwrap().unwrap();
            let retransmit = sender.send_frame(lazily::IpcFrame::control(
                1,
                2,
                Some(heartbeat.sequence_id),
                IpcControlFrame::Retransmit {
                    correlation_id: heartbeat.sequence_id,
                },
            ));
            assert!(
                retransmit.is_ok(),
                "transport must accept retransmit control frames"
            );
            let _ = receiver.recv_frame().unwrap().unwrap();
        });
    });

    group.finish();
}

fn benchmark_round_trip_payload(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_transport_payload_roundtrip");
    group.throughput(Throughput::Bytes(1));

    for &size in &IPC_TPUT_MESSAGE_SIZE_BYTES {
        group.bench_with_input(
            BenchmarkId::new("snapshot_payload", size),
            &size,
            |b, payload_size| {
                let payload = make_snapshot(*payload_size);
                let delta = make_delta_chain(IPC_NODE_COUNT);
                b.iter(|| {
                    let (mut sender, mut receiver) =
                        InProcessIpcTransport::pair(1, 2, IPC_DEFAULT_QUEUE_CAPACITY * 16);
                    let _ = sender.send_message(payload.clone());
                    let _ = sender.send_message(delta.clone());
                    let _ = receiver.recv_frame().unwrap().unwrap();
                    let _ = receiver.recv_frame().unwrap().unwrap();
                });
            },
        );
    }

    group.finish();
}

fn benchmark_backpressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_transport_backpressure");

    group.bench_function("bounded_queue", |b| {
        b.iter(|| {
            let (mut sender, _receiver) = InProcessIpcTransport::pair(1, 2, IPC_BACKPRESSURE_DEPTH);
            for seq in 1..=IPC_BACKPRESSURE_DEPTH {
                assert!(sender.send_control(IpcControlFrame::Heartbeat).is_ok());
                black_box(seq);
            }
            assert_eq!(
                sender.send_control(IpcControlFrame::HeartbeatAck),
                Err(IpcTransportError::Backpressure)
            );
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_round_trip_control,
    benchmark_round_trip_payload,
    benchmark_backpressure
);
criterion_main!(benches);
