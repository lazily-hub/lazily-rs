#![cfg(feature = "ipc")]

use lazily::{
    Delta, DeltaApplyStatus, DeltaOp, EdgeSnapshot, IpcMessage, NodeId, NodeSnapshot, NodeState,
    OpKind, PeerId, PeerPermissions, RemoteOp, Snapshot,
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
