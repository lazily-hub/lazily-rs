#![cfg(feature = "ipc")]

use lazily::{
    Delta, DeltaApplyStatus, DeltaOp, EdgeSnapshot, IpcMessage, NodeId, NodeSnapshot, NodeState,
    OpKind, PeerId, PeerPermissions, RemoteOp, SHM_BLOB_HEADER_LEN, ShmBlobArena,
    ShmBlobArenaError, Snapshot,
};

const PEER_A: PeerId = PeerId(1);
const PEER_B: PeerId = PeerId(2);

#[test]
fn snapshot_round_trips_through_serde() {
    let snapshot = Snapshot::new(
        7,
        vec![
            NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3]),
            NodeSnapshot::opaque(NodeId(2), "opaque-type"),
        ],
        vec![EdgeSnapshot::new(NodeId(2), NodeId(1))],
        vec![NodeId(1), NodeId(2)],
    );

    let json = serde_json::to_string(&IpcMessage::Snapshot(snapshot.clone())).unwrap();
    let back: IpcMessage = serde_json::from_str(&json).unwrap();

    assert_eq!(back, IpcMessage::Snapshot(snapshot));
}

#[test]
fn delta_round_trips_through_serde() {
    let delta = Delta::next(
        41,
        vec![
            DeltaOp::cell_set(NodeId(1), vec![10]),
            DeltaOp::slot_value(NodeId(2), vec![20]),
            DeltaOp::invalidate(NodeId(3)),
            DeltaOp::NodeAdd {
                node: NodeId(4),
                type_tag: "u64".into(),
                state: NodeState::Payload(vec![64]),
            },
            DeltaOp::NodeRemove { node: NodeId(5) },
            DeltaOp::EdgeAdd {
                dependent: NodeId(2),
                dependency: NodeId(1),
            },
            DeltaOp::EdgeRemove {
                dependent: NodeId(3),
                dependency: NodeId(1),
            },
        ],
    );

    let json = serde_json::to_string(&IpcMessage::Delta(delta.clone())).unwrap();
    let back: IpcMessage = serde_json::from_str(&json).unwrap();

    assert_eq!(back, IpcMessage::Delta(delta));
}

#[test]
fn delta_status_accepts_only_sequential_epochs() {
    let next = Delta::next(10, vec![]);
    assert_eq!(next.apply_status(10), DeltaApplyStatus::Apply);
    assert!(next.is_next_after(10));

    let gap = Delta::new(12, 13, vec![]);
    assert_eq!(
        gap.apply_status(10),
        DeltaApplyStatus::ResyncRequired {
            last_epoch: 10,
            base_epoch: 12,
            epoch: 13,
        }
    );

    let non_sequential = Delta::new(10, 12, vec![]);
    assert_eq!(
        non_sequential.apply_status(10),
        DeltaApplyStatus::ResyncRequired {
            last_epoch: 10,
            base_epoch: 10,
            epoch: 12,
        }
    );
}

#[test]
fn snapshot_filter_omits_non_readable_nodes_edges_and_roots() {
    let snapshot = Snapshot::new(
        5,
        vec![
            NodeSnapshot::payload(NodeId(1), "i32", vec![1]),
            NodeSnapshot::payload(NodeId(2), "i32", vec![2]),
            NodeSnapshot::payload(NodeId(3), "i32", vec![3]),
        ],
        vec![
            EdgeSnapshot::new(NodeId(2), NodeId(1)),
            EdgeSnapshot::new(NodeId(3), NodeId(1)),
        ],
        vec![NodeId(1), NodeId(2), NodeId(3)],
    );
    let mut permissions = PeerPermissions::new();
    permissions.allow_many(PEER_A, OpKind::Read, [NodeId(1), NodeId(2)]);
    permissions.allow(PEER_A, RemoteOp::write(NodeId(3)));

    let filtered = snapshot.filter_readable(&permissions, PEER_A);

    assert_eq!(
        filtered.nodes,
        vec![
            NodeSnapshot::payload(NodeId(1), "i32", vec![1]),
            NodeSnapshot::payload(NodeId(2), "i32", vec![2]),
        ]
    );
    assert_eq!(
        filtered.edges,
        vec![EdgeSnapshot::new(NodeId(2), NodeId(1))]
    );
    assert_eq!(filtered.roots, vec![NodeId(1), NodeId(2)]);

    let empty = snapshot.filter_readable(&permissions, PEER_B);
    assert!(empty.nodes.is_empty());
    assert!(empty.edges.is_empty());
    assert!(empty.roots.is_empty());
}

#[test]
fn delta_filter_omits_non_readable_ops_without_redaction() {
    let delta = Delta::next(
        8,
        vec![
            DeltaOp::cell_set(NodeId(1), vec![1]),
            DeltaOp::slot_value(NodeId(2), vec![2]),
            DeltaOp::invalidate(NodeId(3)),
            DeltaOp::NodeAdd {
                node: NodeId(4),
                type_tag: "u8".into(),
                state: NodeState::Payload(vec![4]),
            },
            DeltaOp::NodeRemove { node: NodeId(5) },
            DeltaOp::EdgeAdd {
                dependent: NodeId(2),
                dependency: NodeId(1),
            },
            DeltaOp::EdgeRemove {
                dependent: NodeId(3),
                dependency: NodeId(1),
            },
        ],
    );
    let mut permissions = PeerPermissions::new();
    permissions.allow_many(PEER_A, OpKind::Read, [NodeId(1), NodeId(2), NodeId(5)]);

    let filtered = delta.filter_readable(&permissions, PEER_A);

    assert_eq!(
        filtered.ops,
        vec![
            DeltaOp::cell_set(NodeId(1), vec![1]),
            DeltaOp::slot_value(NodeId(2), vec![2]),
            DeltaOp::NodeRemove { node: NodeId(5) },
            DeltaOp::EdgeAdd {
                dependent: NodeId(2),
                dependency: NodeId(1),
            },
        ]
    );
}

#[test]
fn shm_blob_arena_round_trips_payload_by_descriptor() {
    let mut arena = ShmBlobArena::with_capacity(SHM_BLOB_HEADER_LEN + 128).unwrap();
    let blob = arena.write_blob(12, b"large context pack").unwrap();

    assert_eq!(arena.read_blob(blob).unwrap(), b"large context pack");
    assert_eq!(blob.epoch, 12);
    assert_eq!(blob.len, "large context pack".len() as u64);
}

#[test]
fn shm_blob_arena_rejects_oversized_payload() {
    let mut arena = ShmBlobArena::with_capacity(SHM_BLOB_HEADER_LEN + 4).unwrap();
    let err = arena.write_blob(1, b"12345").unwrap_err();

    assert_eq!(err, ShmBlobArenaError::BlobTooLarge { len: 5, max_len: 4 });
}

#[test]
fn shm_blob_arena_wrap_rejects_stale_descriptor() {
    let mut arena = ShmBlobArena::with_capacity((SHM_BLOB_HEADER_LEN * 2) + 8).unwrap();
    let old = arena.write_blob(1, b"old").unwrap();
    let _middle = arena.write_blob(2, b"abcd").unwrap();
    let _new = arena.write_blob(3, b"new").unwrap();

    let err = arena.read_blob(old).unwrap_err();
    assert!(matches!(
        err,
        ShmBlobArenaError::DescriptorMismatch {
            field: "generation"
        } | ShmBlobArenaError::DescriptorMismatch { field: "checksum" }
    ));
}

#[test]
fn shm_blob_arena_rejects_torn_payload() {
    let mut arena = ShmBlobArena::with_capacity(SHM_BLOB_HEADER_LEN + 32).unwrap();
    let blob = arena.write_blob(4, b"payload").unwrap();
    let payload_offset = blob.offset as usize + SHM_BLOB_HEADER_LEN;
    arena.bytes_mut()[payload_offset] ^= 0xff;

    let err = arena.read_blob(blob).unwrap_err();
    assert!(matches!(err, ShmBlobArenaError::ChecksumMismatch { .. }));
}

#[test]
fn ipc_messages_can_reference_shared_blobs() {
    let mut arena = ShmBlobArena::with_capacity(SHM_BLOB_HEADER_LEN + 128).unwrap();
    let blob = arena.write_blob(9, b"large slot value").unwrap();
    let snapshot = Snapshot::new(
        9,
        vec![NodeSnapshot::shared_blob(NodeId(7), "text/plain", blob)],
        vec![],
        vec![NodeId(7)],
    );
    let delta = Delta::next(9, vec![DeltaOp::slot_value_blob(NodeId(7), blob)]);

    let snapshot_json = serde_json::to_string(&IpcMessage::Snapshot(snapshot.clone())).unwrap();
    let delta_json = serde_json::to_string(&IpcMessage::Delta(delta.clone())).unwrap();

    assert_eq!(
        serde_json::from_str::<IpcMessage>(&snapshot_json).unwrap(),
        IpcMessage::Snapshot(snapshot)
    );
    assert_eq!(
        serde_json::from_str::<IpcMessage>(&delta_json).unwrap(),
        IpcMessage::Delta(delta)
    );
    assert_eq!(arena.read_blob(blob).unwrap(), b"large slot value");
}

#[test]
fn ipc_message_bytes_are_channel_agnostic_payloads() {
    let message = IpcMessage::Delta(Delta::next(
        15,
        vec![
            DeltaOp::cell_set(NodeId(1), b"cell".to_vec()),
            DeltaOp::slot_value(NodeId(2), b"slot".to_vec()),
        ],
    ));

    let websocket_text_frame = serde_json::to_string(&message).unwrap();
    let webrtc_data_frame = websocket_text_frame.as_bytes().to_vec();
    let ffi_owned_buffer = webrtc_data_frame.clone();

    assert_eq!(
        serde_json::from_str::<IpcMessage>(&websocket_text_frame).unwrap(),
        message
    );
    assert_eq!(
        serde_json::from_slice::<IpcMessage>(&webrtc_data_frame).unwrap(),
        message
    );
    assert_eq!(
        serde_json::from_slice::<IpcMessage>(&ffi_owned_buffer).unwrap(),
        message
    );
}

#[cfg(feature = "ipc-binary")]
mod binary {
    use lazily::{
        DecodeError, Delta, DeltaOp, EdgeSnapshot, IpcMessage, NodeId, NodeSnapshot, Snapshot,
    };

    #[test]
    fn ipc_message_binary_round_trip_snapshot() {
        let snapshot = Snapshot::new(
            7,
            vec![
                NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3]),
                NodeSnapshot::opaque(NodeId(2), "opaque-type"),
            ],
            vec![EdgeSnapshot::new(NodeId(2), NodeId(1))],
            vec![NodeId(1), NodeId(2)],
        );
        let message = IpcMessage::Snapshot(snapshot.clone());

        let encoded = message.encode_binary().unwrap();
        let decoded = IpcMessage::decode_binary(&encoded).unwrap();

        assert_eq!(decoded, message);
    }

    #[test]
    fn ipc_message_binary_round_trip_delta() {
        let delta = Delta::next(
            3,
            vec![
                DeltaOp::cell_set(NodeId(1), vec![10, 20]),
                DeltaOp::slot_value(NodeId(2), vec![30, 40]),
                DeltaOp::invalidate(NodeId(3)),
            ],
        );
        let message = IpcMessage::Delta(delta.clone());

        let encoded = message.encode_binary().unwrap();
        let decoded = IpcMessage::decode_binary(&encoded).unwrap();

        assert_eq!(decoded, message);
    }

    #[test]
    fn ipc_message_binary_rejects_invalid_bytes() {
        let result = IpcMessage::decode_binary(b"garbage");
        assert!(matches!(result, Err(DecodeError::Binary(_))));
    }

    #[test]
    fn ipc_message_binary_is_smaller_than_json() {
        let snapshot = Snapshot::new(
            42,
            vec![NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3, 4])],
            vec![EdgeSnapshot::new(NodeId(1), NodeId(2))],
            vec![NodeId(1)],
        );
        let message = IpcMessage::Snapshot(snapshot);

        let json_len = serde_json::to_vec(&message).unwrap().len();
        let binary_len = message.encode_binary().unwrap().len();

        assert!(
            binary_len < json_len,
            "binary ({binary_len}) should be smaller than json ({json_len})"
        );
    }
}
