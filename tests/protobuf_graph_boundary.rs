#![cfg(feature = "protobuf")]

mod common;

use std::collections::BTreeMap;

use common::Expect;
use lazily::protobuf::{
    BoundaryDecision, GraphBoundaryProjection, PROTOBUF_GRAPH_BOUNDARY_FEATURE,
    wire::{
        self, CapabilityHandshake, CellProjection, CellTextSplice, DerivedProjection, GraphInput,
        GraphSnapshot, ProtocolEnvelope, SurfaceObservation, graph_input, protocol_envelope,
    },
};
use prost::Message;
use serde_json::Value;

const FIXTURE: common::SpecDir = common::SpecDir("protobuf/graph_boundary_traces.json");

fn strings(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .expect("cells must be an object")
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value
                    .as_str()
                    .expect("cell text must be a string")
                    .to_owned(),
            )
        })
        .collect()
}

fn envelope(step: &Value) -> ProtocolEnvelope {
    let body = match step["kind"].as_str().expect("step kind") {
        "cell_text_splice" => protocol_envelope::Body::GraphInput(GraphInput {
            input: Some(graph_input::Input::CellTextSplice(CellTextSplice {
                document_id: step["document_id"]
                    .as_str()
                    .expect("document_id")
                    .to_owned(),
                cell_id: step["cell_id"].as_str().expect("cell_id").to_owned(),
                expected_cell_revision: step["expected_revision"]
                    .as_u64()
                    .expect("expected_revision"),
                local_offset_utf8: step["offset"].as_u64().expect("offset") as u32,
                delete_length_utf8: step["delete_length"].as_u64().expect("delete_length") as u32,
                insert_text: step["insert_text"]
                    .as_str()
                    .expect("insert_text")
                    .to_owned(),
            })),
        }),
        "bootstrap_snapshot" => protocol_envelope::Body::GraphInput(GraphInput {
            input: Some(graph_input::Input::BootstrapSnapshot(GraphSnapshot {
                purpose: wire::SnapshotPurpose::Bootstrap as i32,
                canonical_json: serde_json::to_vec(&step["cells"]).expect("snapshot JSON"),
                logical_hash: String::new(),
            })),
        }),
        "derived_projection" => protocol_envelope::Body::DerivedProjection(DerivedProjection {
            projection_version: step["sequence"].as_u64().expect("sequence"),
            logical_hash: String::new(),
            cells: strings(&step["cells"])
                .into_iter()
                .map(|(cell_id, text)| CellProjection {
                    document_id: "doc".to_owned(),
                    cell_id,
                    revision: 1,
                    text,
                })
                .collect(),
        }),
        "surface_observation" => protocol_envelope::Body::GraphInput(GraphInput {
            input: Some(graph_input::Input::SurfaceObservation(SurfaceObservation {
                surface_id: "fixture".to_owned(),
                kind: wire::SurfaceObservationKind::NativeReload as i32,
                cell_id: step["cell_id"].as_str().expect("cell_id").to_owned(),
                canonical_json_value: Vec::new(),
            })),
        }),
        other => panic!("unknown graph-boundary fixture kind {other}"),
    };

    ProtocolEnvelope {
        protocol_version: 1,
        schema_version: "1.0.0-experimental".to_owned(),
        graph_id: "fixture-graph".to_owned(),
        source_id: "fixture-source".to_owned(),
        source_generation: step["source_generation"]
            .as_u64()
            .expect("source_generation"),
        causal_epoch: step["causal_epoch"].as_u64().expect("causal_epoch"),
        sequence: step["sequence"].as_u64().expect("sequence"),
        correlation_id: format!("fixture-{}", step["sequence"]),
        body: Some(body),
    }
}

fn decision_name(decision: BoundaryDecision) -> &'static str {
    match decision {
        BoundaryDecision::Apply => "apply",
        BoundaryDecision::Bootstrap => "bootstrap",
        BoundaryDecision::Project => "project",
        BoundaryDecision::Observe => "observe",
        BoundaryDecision::Duplicate => "duplicate",
        BoundaryDecision::RejectStale => "reject_stale",
        BoundaryDecision::RejectGap => "reject_gap",
    }
}

#[test]
fn generated_handshake_negotiates_the_optional_feature() {
    let handshake = CapabilityHandshake {
        minimum_protocol_version: 1,
        maximum_protocol_version: 1,
        codecs: vec!["protobuf".to_owned()],
        features: vec![PROTOBUF_GRAPH_BOUNDARY_FEATURE.to_owned()],
    };
    let decoded =
        CapabilityHandshake::decode(handshake.encode_to_vec().as_slice()).expect("round-trip");
    assert_eq!(decoded.codecs, ["protobuf"]);
    assert_eq!(decoded.features, [PROTOBUF_GRAPH_BOUNDARY_FEATURE]);
}

#[test]
fn generated_protobuf_roundtrips_and_replays_canonical_logical_traces() {
    let raw = common::spec_read_to_string(FIXTURE.path()).expect("read protobuf trace fixture");
    let fixture: Value = serde_json::from_str(&raw).expect("parse protobuf trace fixture");

    for (_, id, scenario) in common::scenarios(&FIXTURE.to_string(), &fixture) {
        let scenario = scenario.value();
        let mut projection = GraphBoundaryProjection::default();
        let mut decisions = Vec::new();

        for step in scenario["steps"]
            .as_array()
            .expect("steps must be an array")
        {
            let message = envelope(step);
            let decoded =
                ProtocolEnvelope::decode(message.encode_to_vec().as_slice()).expect("round-trip");
            let decision = projection.admit(&decoded).expect("semantic admission");
            if decision == BoundaryDecision::Bootstrap {
                projection.install_snapshot_cells(strings(&step["cells"]));
            }
            decisions.push(decision_name(decision));
        }

        // Bound to the tracker rather than indexed directly (`#lzrsblockwalk`).
        // All four keys were already asserted below, so this is a routing change
        // and not a coverage change — but until the block was BOUND, rung 0 had
        // no record of it and every rung above was scoped to blocks a runner
        // bound, so a key that stopped being read here would have reported
        // nothing at all.
        let expected = Expect::new(
            FIXTURE.to_string(),
            format!("scenarios[{id}].expect"),
            &scenario["expect"],
        );
        let actual_cells = projection
            .cells()
            .iter()
            .map(|(id, cell)| (id.clone(), cell.text.clone()))
            .collect::<BTreeMap<_, _>>();
        // `cells` is object-valued, so the comparison owes a key-set claim as
        // well as a value claim (`#lzsubblockkeyset`): a cell the corpus adds
        // upstream would otherwise be compared by nothing. The map equality
        // below already fails on a vocabulary difference — `assert_key_set` is
        // what makes that visible to the guard instead of leaving it implied by
        // the shape of a `BTreeMap` comparison.
        expected.assert_key_with("cells", |want| {
            assert_eq!(actual_cells, strings(want), "{id}");
        });
        expected.assert_key_set("cells", actual_cells.keys().cloned());
        expected.assert_key_with("decisions", |want| {
            assert_eq!(
                decisions,
                want.as_array()
                    .expect("decisions")
                    .iter()
                    .map(|value| value.as_str().expect("decision"))
                    .collect::<Vec<_>>(),
                "{id}"
            );
        });
        expected.assert_key(
            "logical_projection",
            projection.logical_projection().to_owned(),
        );
        expected.assert_key("ordinary_snapshot_count", 0u64);
    }
}
