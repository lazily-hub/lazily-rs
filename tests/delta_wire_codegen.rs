#![cfg(all(feature = "ffi", feature = "ipc-msgpack", feature = "ipc-binary"))]

//! The generated `delta` wire declarations (`src/generated/delta.rs`,
//! `#lzwiremodel7`) in every codec lazily-rs speaks.
//!
//! `DeltaOp`'s `Serialize` is generated, not derived: `NodeAdd.key` is
//! codec-aware. Self-describing codecs (JSON, msgpack, which opts in through
//! `is_human_readable`) omit an absent key; positional Postcard always writes
//! its option tag so the binary schema stays stable. These tests pin both
//! halves, with Postcard bytes captured from the hand-written implementation the
//! generated one replaced.

use lazily::{DecodeError, Delta, DeltaOp, IpcMessage, NodeId, NodeKey, NodeState, ShmBlobRef};
use serde_json::{Value, json};

/// A non-default backend. `ShmBlobRef` is hand-written (`external` to the
/// generator) and skips `backend: "shm"` in EVERY codec, including positional
/// Postcard, so a default-backend descriptor does not survive a Postcard round
/// trip (`DeserializeUnexpectedEnd`). That predates this lowering and is out of
/// its scope; `arrow` keeps the field on the wire so every codec can round-trip.
fn blob() -> ShmBlobRef {
    serde_json::from_value(json!({
        "offset": 1, "len": 2, "generation": 3, "epoch": 4, "checksum": 5, "backend": "arrow"
    }))
    .unwrap()
}

fn node_add(node: u64, state: NodeState, key: Option<&str>) -> DeltaOp {
    DeltaOp::NodeAdd {
        node: NodeId(node),
        type_tag: "t".into(),
        state,
        key: key.map(|k| NodeKey::new(k).unwrap()),
    }
}

/// Every `DeltaOp` variant, every `IpcValue` and `NodeState` form, and a keyed
/// and an unkeyed `NodeAdd`.
fn every_op() -> Vec<DeltaOp> {
    vec![
        DeltaOp::cell_set(NodeId(1), vec![1, 2]),
        DeltaOp::slot_value_blob(NodeId(2), blob()),
        DeltaOp::invalidate(NodeId(3)),
        node_add(4, NodeState::Opaque, None),
        node_add(5, NodeState::Payload(vec![9]), Some("scores/alice")),
        node_add(6, NodeState::SharedBlob(blob()), None),
        DeltaOp::NodeRemove { node: NodeId(7) },
        DeltaOp::EdgeAdd {
            dependent: NodeId(8),
            dependency: NodeId(9),
        },
        DeltaOp::EdgeRemove {
            dependent: NodeId(10),
            dependency: NodeId(11),
        },
        DeltaOp::queue_push(NodeId(12), vec![3]),
        DeltaOp::queue_pop(NodeId(13)),
        DeltaOp::queue_close(NodeId(u64::MAX)),
    ]
}

fn frame(ops: Vec<DeltaOp>) -> IpcMessage {
    IpcMessage::Delta(Delta::new(0, 1, ops))
}

#[test]
fn every_variant_round_trips_in_every_codec() {
    let all = frame(every_op());
    assert_eq!(
        IpcMessage::decode_json(&all.encode_json().unwrap()).unwrap(),
        all
    );
    assert_eq!(
        IpcMessage::decode_json_intern(&all.encode_json_intern().unwrap()).unwrap(),
        all
    );
    assert_eq!(
        IpcMessage::decode_msgpack(&all.encode_msgpack().unwrap()).unwrap(),
        all
    );
    assert_eq!(
        IpcMessage::decode_binary(&all.encode_binary().unwrap()).unwrap(),
        all
    );
    // One op per frame too, so a variant that only decodes in company is caught.
    for op in every_op() {
        let one = frame(vec![op]);
        assert_eq!(
            IpcMessage::decode_json(&one.encode_json().unwrap()).unwrap(),
            one
        );
        assert_eq!(
            IpcMessage::decode_msgpack(&one.encode_msgpack().unwrap()).unwrap(),
            one
        );
        assert_eq!(
            IpcMessage::decode_binary(&one.encode_binary().unwrap()).unwrap(),
            one
        );
    }
}

fn node_add_body(json: &Value) -> &serde_json::Map<String, Value> {
    json["Delta"]["ops"][0]["NodeAdd"].as_object().unwrap()
}

#[test]
fn self_describing_codecs_omit_an_absent_key_and_keep_a_present_one() {
    let unkeyed = frame(vec![node_add(4, NodeState::Opaque, None)]);
    let keyed = frame(vec![node_add(4, NodeState::Opaque, Some("a/b"))]);

    let json: Value = serde_json::from_slice(&unkeyed.encode_json().unwrap()).unwrap();
    assert!(!node_add_body(&json).contains_key("key"), "{json}");
    let json: Value = serde_json::from_slice(&keyed.encode_json().unwrap()).unwrap();
    assert_eq!(node_add_body(&json)["key"], json!("a/b"));

    let msgpack: Value = rmp_serde::from_slice(&unkeyed.encode_msgpack().unwrap()).unwrap();
    assert!(!node_add_body(&msgpack).contains_key("key"), "{msgpack}");
    let msgpack: Value = rmp_serde::from_slice(&keyed.encode_msgpack().unwrap()).unwrap();
    assert_eq!(node_add_body(&msgpack)["key"], json!("a/b"));
}

#[test]
fn postcard_always_writes_the_key_option_tag() {
    // Bytes from the hand-written `Serialize` this lowering replaced: tag 0x01
    // (Delta), epochs 0 and 1, one op, tag 0x03 (NodeAdd), node 4, "t", state
    // tag 0x02 (Opaque), then the key's option tag. Omitting it misaligns every
    // following field.
    let unkeyed = frame(vec![node_add(4, NodeState::Opaque, None)]);
    assert_eq!(
        unkeyed.encode_binary().unwrap(),
        [0x01, 0x00, 0x01, 0x01, 0x03, 0x04, 0x01, b't', 0x02, 0x00]
    );
    let keyed = frame(vec![node_add(4, NodeState::Opaque, Some("a/b"))]);
    assert_eq!(
        keyed.encode_binary().unwrap(),
        [
            0x01, 0x00, 0x01, 0x01, 0x03, 0x04, 0x01, b't', 0x02, 0x01, 0x03, b'a', b'/', b'b'
        ]
    );
    // An unkeyed NodeAdd followed by another op: the following op must decode
    // from the right offset.
    let pair = frame(vec![
        node_add(4, NodeState::Opaque, None),
        DeltaOp::invalidate(NodeId(5)),
    ]);
    assert_eq!(
        IpcMessage::decode_binary(&pair.encode_binary().unwrap()).unwrap(),
        pair
    );
}

#[test]
fn an_explicit_null_key_reads_as_absent() {
    let wire = json!({"Delta": {"base_epoch": 0, "epoch": 1, "ops": [
        {"NodeAdd": {"node": 4, "type_tag": "t", "state": "Opaque", "key": null}}
    ]}});
    let expected = frame(vec![node_add(4, NodeState::Opaque, None)]);
    assert_eq!(
        IpcMessage::decode_json(&serde_json::to_vec(&wire).unwrap()).unwrap(),
        expected
    );
    assert_eq!(
        IpcMessage::decode_msgpack(&rmp_serde::to_vec(&wire).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn closed_records_refuse_unknown_keys() {
    // The generated decoder is strict on `additionalProperties: false`. The
    // hand-written one ignored all three of these keys in JSON and msgpack.
    for (label, wire) in [
        (
            "op body",
            json!({"Delta": {"base_epoch": 0, "epoch": 1, "ops": [
                {"Invalidate": {"node": 1, "extra": 2}}
            ]}}),
        ),
        (
            "NodeAdd body",
            json!({"Delta": {"base_epoch": 0, "epoch": 1, "ops": [
                {"NodeAdd": {"node": 1, "type_tag": "t", "state": "Opaque", "extra": 2}}
            ]}}),
        ),
        (
            "Delta",
            json!({"Delta": {"base_epoch": 0, "epoch": 1, "ops": [], "extra": 2}}),
        ),
    ] {
        match IpcMessage::decode_json(&serde_json::to_vec(&wire).unwrap()) {
            Err(DecodeError::Json(err)) => {
                assert!(
                    err.to_string().contains("unknown field `extra`"),
                    "{label}: {err}"
                )
            }
            other => panic!("{label}: json decoded {other:?}"),
        }
        match IpcMessage::decode_msgpack(&rmp_serde::to_vec(&wire).unwrap()) {
            Err(DecodeError::Msgpack(err)) => {
                assert!(
                    err.to_string().contains("unknown field `extra`"),
                    "{label}: {err}"
                )
            }
            other => panic!("{label}: msgpack decoded {other:?}"),
        }
    }
}
