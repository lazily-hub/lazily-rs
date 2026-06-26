#![cfg(feature = "ffi")]

use lazily::{
    CrdtOp, CrdtSync, Delta, DeltaOp, EdgeSnapshot, IpcMessage, LazilyFfiBytes,
    LazilyFfiMessageKind, LazilyFfiStatus, NodeId, NodeSnapshot, Snapshot, WireStamp,
    lazily_ffi_bytes_free, lazily_ffi_channel_free, lazily_ffi_channel_len, lazily_ffi_channel_new,
    lazily_ffi_channel_recv_json, lazily_ffi_channel_send_json, lazily_ffi_ipc_message_clone_json,
    lazily_ffi_ipc_message_kind_json, lazily_ffi_ipc_message_validate_json,
};

#[test]
fn ffi_message_helpers_validate_classify_and_clone_ipc_messages() {
    let snapshot = IpcMessage::Snapshot(Snapshot::new(
        3,
        vec![NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3])],
        vec![EdgeSnapshot::new(NodeId(1), NodeId(2))],
        vec![NodeId(1)],
    ));
    let json = serde_json::to_vec(&snapshot).unwrap();

    let mut kind = LazilyFfiMessageKind::Unknown;
    let mut cloned = LazilyFfiBytes::default();

    assert_eq!(
        unsafe { lazily_ffi_ipc_message_validate_json(json.as_ptr(), json.len()) },
        LazilyFfiStatus::Ok
    );
    assert_eq!(
        unsafe { lazily_ffi_ipc_message_kind_json(json.as_ptr(), json.len(), &mut kind) },
        LazilyFfiStatus::Ok
    );
    assert_eq!(kind, LazilyFfiMessageKind::Snapshot);
    assert_eq!(
        unsafe { lazily_ffi_ipc_message_clone_json(json.as_ptr(), json.len(), &mut cloned) },
        LazilyFfiStatus::Ok
    );

    let cloned = unsafe { take_ffi_bytes(cloned) };
    assert_eq!(
        serde_json::from_slice::<IpcMessage>(&cloned).unwrap(),
        snapshot
    );
}

#[test]
fn ffi_classifies_crdt_sync_message_kind() {
    let sync = IpcMessage::CrdtSync(CrdtSync::new(
        vec![(
            1,
            WireStamp {
                wall_time: 9,
                logical: 0,
                peer: 1,
            },
        )],
        vec![CrdtOp::new(
            NodeId(1),
            WireStamp {
                wall_time: 9,
                logical: 0,
                peer: 1,
            },
            vec![1, 2],
        )],
    ));
    let json = serde_json::to_vec(&sync).unwrap();

    let mut kind = LazilyFfiMessageKind::Unknown;
    assert_eq!(
        unsafe { lazily_ffi_ipc_message_validate_json(json.as_ptr(), json.len()) },
        LazilyFfiStatus::Ok
    );
    assert_eq!(
        unsafe { lazily_ffi_ipc_message_kind_json(json.as_ptr(), json.len(), &mut kind) },
        LazilyFfiStatus::Ok
    );
    assert_eq!(kind, LazilyFfiMessageKind::CrdtSync);
}

#[test]
fn ffi_channel_relays_canonical_ipc_message_buffers() {
    let message = IpcMessage::Delta(Delta::next(
        15,
        vec![
            DeltaOp::cell_set(NodeId(1), b"cell".to_vec()),
            DeltaOp::slot_value(NodeId(2), b"slot".to_vec()),
        ],
    ));
    let json = serde_json::to_vec(&message).unwrap();
    let channel = lazily_ffi_channel_new();
    assert!(!channel.is_null());

    let mut queued = 0usize;
    assert_eq!(
        unsafe { lazily_ffi_channel_send_json(channel, json.as_ptr(), json.len()) },
        LazilyFfiStatus::Ok
    );
    assert_eq!(
        unsafe { lazily_ffi_channel_len(channel, &mut queued) },
        LazilyFfiStatus::Ok
    );
    assert_eq!(queued, 1);

    let mut received = LazilyFfiBytes::default();
    assert_eq!(
        unsafe { lazily_ffi_channel_recv_json(channel, &mut received) },
        LazilyFfiStatus::Ok
    );
    let received = unsafe { take_ffi_bytes(received) };
    assert_eq!(
        serde_json::from_slice::<IpcMessage>(&received).unwrap(),
        message
    );

    let mut empty = LazilyFfiBytes::default();
    assert_eq!(
        unsafe { lazily_ffi_channel_recv_json(channel, &mut empty) },
        LazilyFfiStatus::Empty
    );
    assert!(empty.ptr.is_null());
    assert_eq!(empty.len, 0);

    unsafe { lazily_ffi_channel_free(channel) };
}

#[test]
fn ffi_channel_rejects_invalid_message_bytes() {
    let channel = lazily_ffi_channel_new();
    assert!(!channel.is_null());

    let invalid = b"{\"not\":\"an IpcMessage\"}";
    assert_eq!(
        unsafe { lazily_ffi_channel_send_json(channel, invalid.as_ptr(), invalid.len()) },
        LazilyFfiStatus::InvalidMessage
    );

    unsafe { lazily_ffi_channel_free(channel) };
}

unsafe fn take_ffi_bytes(bytes: LazilyFfiBytes) -> Vec<u8> {
    if bytes.ptr.is_null() {
        assert_eq!(bytes.len, 0);
        return Vec::new();
    }

    // SAFETY: The test only passes buffers returned by lazily FFI functions.
    let out = unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec();
    // SAFETY: The buffer came from lazily FFI and is freed exactly once here.
    unsafe { lazily_ffi_bytes_free(bytes) };
    out
}

#[cfg(feature = "ipc-binary")]
mod binary {
    use super::*;
    use lazily::{
        lazily_ffi_channel_recv_binary, lazily_ffi_channel_send_binary,
        lazily_ffi_ipc_message_clone_binary, lazily_ffi_ipc_message_kind_binary,
        lazily_ffi_ipc_message_validate_binary,
    };

    #[test]
    fn ffi_binary_helpers_validate_classify_and_clone_ipc_messages() {
        let snapshot = IpcMessage::Snapshot(Snapshot::new(
            3,
            vec![NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3])],
            vec![EdgeSnapshot::new(NodeId(1), NodeId(2))],
            vec![NodeId(1)],
        ));
        let binary = postcard::to_allocvec(&snapshot).unwrap();

        let mut kind = LazilyFfiMessageKind::Unknown;
        let mut cloned = LazilyFfiBytes::default();

        assert_eq!(
            unsafe { lazily_ffi_ipc_message_validate_binary(binary.as_ptr(), binary.len()) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(
            unsafe { lazily_ffi_ipc_message_kind_binary(binary.as_ptr(), binary.len(), &mut kind) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(kind, LazilyFfiMessageKind::Snapshot);
        assert_eq!(
            unsafe {
                lazily_ffi_ipc_message_clone_binary(binary.as_ptr(), binary.len(), &mut cloned)
            },
            LazilyFfiStatus::Ok
        );

        let cloned = unsafe { take_ffi_bytes(cloned) };
        assert_eq!(
            postcard::from_bytes::<IpcMessage>(&cloned).unwrap(),
            snapshot
        );
    }

    #[test]
    fn ffi_binary_channel_relays_canonical_ipc_message_buffers() {
        let message = IpcMessage::Delta(Delta::next(
            15,
            vec![
                DeltaOp::cell_set(NodeId(1), b"cell".to_vec()),
                DeltaOp::slot_value(NodeId(2), b"slot".to_vec()),
            ],
        ));
        let binary = postcard::to_allocvec(&message).unwrap();
        let channel = lazily_ffi_channel_new();
        assert!(!channel.is_null());

        let mut queued = 0usize;
        assert_eq!(
            unsafe { lazily_ffi_channel_send_binary(channel, binary.as_ptr(), binary.len()) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(
            unsafe { lazily_ffi_channel_len(channel, &mut queued) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(queued, 1);

        let mut received = LazilyFfiBytes::default();
        assert_eq!(
            unsafe { lazily::lazily_ffi_channel_recv_binary(channel, &mut received) },
            LazilyFfiStatus::Ok
        );
        let received = unsafe { take_ffi_bytes(received) };
        assert_eq!(
            postcard::from_bytes::<IpcMessage>(&received).unwrap(),
            message
        );

        let mut empty = LazilyFfiBytes::default();
        assert_eq!(
            unsafe { lazily_ffi_channel_recv_binary(channel, &mut empty) },
            LazilyFfiStatus::Empty
        );

        unsafe { lazily_ffi_channel_free(channel) };
    }

    #[test]
    fn ffi_binary_channel_rejects_invalid_message_bytes() {
        let channel = lazily_ffi_channel_new();
        assert!(!channel.is_null());

        assert_eq!(
            unsafe { lazily_ffi_channel_send_binary(channel, b"garbage".as_ptr(), 7) },
            LazilyFfiStatus::InvalidMessage
        );

        unsafe { lazily_ffi_channel_free(channel) };
    }
}

#[cfg(feature = "ipc-msgpack")]
mod msgpack {
    use super::*;
    use lazily::{
        lazily_ffi_channel_recv_msgpack, lazily_ffi_channel_send_msgpack,
        lazily_ffi_ipc_message_clone_msgpack, lazily_ffi_ipc_message_kind_msgpack,
        lazily_ffi_ipc_message_validate_msgpack,
    };

    #[test]
    fn ffi_msgpack_helpers_validate_classify_and_clone_ipc_messages() {
        let snapshot = IpcMessage::Snapshot(Snapshot::new(
            3,
            vec![NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3])],
            vec![EdgeSnapshot::new(NodeId(1), NodeId(2))],
            vec![NodeId(1)],
        ));
        let msgpack = snapshot.encode_msgpack().unwrap();

        let mut kind = LazilyFfiMessageKind::Unknown;
        let mut cloned = LazilyFfiBytes::default();

        assert_eq!(
            unsafe { lazily_ffi_ipc_message_validate_msgpack(msgpack.as_ptr(), msgpack.len()) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(
            unsafe {
                lazily_ffi_ipc_message_kind_msgpack(msgpack.as_ptr(), msgpack.len(), &mut kind)
            },
            LazilyFfiStatus::Ok
        );
        assert_eq!(kind, LazilyFfiMessageKind::Snapshot);
        assert_eq!(
            unsafe {
                lazily_ffi_ipc_message_clone_msgpack(msgpack.as_ptr(), msgpack.len(), &mut cloned)
            },
            LazilyFfiStatus::Ok
        );

        let cloned = unsafe { take_ffi_bytes(cloned) };
        assert_eq!(IpcMessage::decode_msgpack(&cloned).unwrap(), snapshot);
    }

    #[test]
    fn ffi_msgpack_channel_relays_canonical_ipc_message_buffers() {
        let message = IpcMessage::Delta(Delta::next(
            15,
            vec![
                DeltaOp::cell_set(NodeId(1), b"cell".to_vec()),
                DeltaOp::slot_value(NodeId(2), b"slot".to_vec()),
            ],
        ));
        let msgpack = message.encode_msgpack().unwrap();
        let channel = lazily_ffi_channel_new();
        assert!(!channel.is_null());

        let mut queued = 0usize;
        assert_eq!(
            unsafe { lazily_ffi_channel_send_msgpack(channel, msgpack.as_ptr(), msgpack.len()) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(
            unsafe { lazily_ffi_channel_len(channel, &mut queued) },
            LazilyFfiStatus::Ok
        );
        assert_eq!(queued, 1);

        let mut received = LazilyFfiBytes::default();
        assert_eq!(
            unsafe { lazily_ffi_channel_recv_msgpack(channel, &mut received) },
            LazilyFfiStatus::Ok
        );
        let received = unsafe { take_ffi_bytes(received) };
        assert_eq!(IpcMessage::decode_msgpack(&received).unwrap(), message);

        unsafe { lazily_ffi_channel_free(channel) };
    }

    #[test]
    fn ffi_channel_can_convert_between_send_and_recv_codecs() {
        let message = IpcMessage::Snapshot(Snapshot::new(
            3,
            vec![NodeSnapshot::payload(NodeId(1), "i32", vec![1, 2, 3])],
            vec![],
            vec![NodeId(1)],
        ));
        let msgpack = message.encode_msgpack().unwrap();
        let channel = lazily_ffi_channel_new();
        assert!(!channel.is_null());

        assert_eq!(
            unsafe { lazily_ffi_channel_send_msgpack(channel, msgpack.as_ptr(), msgpack.len()) },
            LazilyFfiStatus::Ok
        );

        let mut received_json = LazilyFfiBytes::default();
        assert_eq!(
            unsafe { lazily_ffi_channel_recv_json(channel, &mut received_json) },
            LazilyFfiStatus::Ok
        );
        let received_json = unsafe { take_ffi_bytes(received_json) };
        assert_eq!(
            serde_json::from_slice::<IpcMessage>(&received_json).unwrap(),
            message
        );

        unsafe { lazily_ffi_channel_free(channel) };
    }

    #[test]
    fn ffi_msgpack_channel_rejects_invalid_message_bytes() {
        let channel = lazily_ffi_channel_new();
        assert!(!channel.is_null());

        assert_eq!(
            unsafe { lazily_ffi_channel_send_msgpack(channel, b"garbage".as_ptr(), 7) },
            LazilyFfiStatus::InvalidMessage
        );

        unsafe { lazily_ffi_channel_free(channel) };
    }
}
