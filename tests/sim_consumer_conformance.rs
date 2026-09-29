mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use lazily::{
    ReplayValue, SimConsumerAction, SimConsumerAdapter, SimConsumerAdapterKind,
    SimConsumerCallbacks, SimConsumerExternalPort, SimConsumerExternalSelection,
    SimConsumerGeneratedAction, SimConsumerPort, SimConsumerPortDeterminism, SimConsumerScenario,
    SimConsumerTestkit, SimConsumerTestkitSpec, SimConsumerTraceEntry, SimConsumerWorldEvidence,
};
use serde_json::Value;

const FIXTURE: common::SpecDir = common::SpecDir("simulation/consumer_testkit.json");

struct State {
    value: i128,
    probes: u64,
    history: Vec<SimConsumerAction>,
    world: SimConsumerWorldEvidence,
}
impl Default for State {
    fn default() -> Self {
        Self {
            value: 0,
            probes: 0,
            history: vec![],
            world: SimConsumerWorldEvidence {
                world_id: String::new(),
                steps: 0,
                trace: vec![],
            },
        }
    }
}

fn s(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be a string"))
        .to_owned()
}
fn port(value: &str) -> SimConsumerExternalPort {
    match value {
        "cli" => SimConsumerExternalPort::Cli,
        "filesystem" => SimConsumerExternalPort::Filesystem,
        "local_socket" => SimConsumerExternalPort::LocalSocket,
        "editor_replica" => SimConsumerExternalPort::EditorReplica,
        other => panic!("unknown external port {other:?}"),
    }
}
fn kind(value: &str) -> SimConsumerAdapterKind {
    match value {
        "in_memory" => SimConsumerAdapterKind::InMemory,
        "postgres" => SimConsumerAdapterKind::Postgres,
        "nats" => SimConsumerAdapterKind::Nats,
        "external_process" => SimConsumerAdapterKind::ExternalProcess,
        other => panic!("unknown adapter kind {other:?}"),
    }
}

fn fixture() -> Value {
    let path = FIXTURE.path();
    let raw = common::spec_read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("parse consumer testkit fixture")
}

fn ports(fixture: &Value, stub_clock: bool) -> Vec<SimConsumerPort> {
    fixture["ports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| SimConsumerPort {
            id: s(p, "id"),
            kind: s(p, "kind"),
            determinism: match p["determinism"].as_str().unwrap() {
                "deterministic" => SimConsumerPortDeterminism::Deterministic,
                "nondeterministic" => SimConsumerPortDeterminism::Nondeterministic,
                other => panic!("unknown determinism {other:?}"),
            },
            stubbed: stub_clock && p["id"] == "logical.clock",
        })
        .collect()
}

fn scenario(fixture: &Value) -> SimConsumerScenario {
    let mut seed = [0_u8; 32];
    let text = fixture["seed"].as_str().unwrap();
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap();
    }
    SimConsumerScenario {
        seed,
        generator_path: s(&fixture["generator"], "path"),
        generator_version: s(&fixture["generator"], "version"),
        initial_model: ReplayValue::Int(0),
        actions: fixture["actions"]
            .as_array()
            .unwrap()
            .iter()
            .scan(0_i128, |model, action| {
                *model += action["payload"].as_i64().unwrap() as i128;
                SimConsumerGeneratedAction {
                    action: SimConsumerAction {
                        id: s(action, "id"),
                        actor_id: s(action, "actor_id"),
                        kind: s(action, "kind"),
                        version: s(action, "version"),
                        payload: ReplayValue::Int(action["payload"].as_i64().unwrap().into()),
                    },
                    model_after: ReplayValue::Int(*model),
                }
                .into()
            })
            .collect(),
    }
}

fn adapter(fixture: &Value, block: &Value) -> (SimConsumerAdapter, Rc<RefCell<State>>) {
    let adapter_kind = kind(block["kind"].as_str().unwrap());
    let execution = block["execution_mode"].as_str().unwrap().to_owned();
    let history_mode = block["history_mode"].as_str().unwrap().to_owned();
    let clock_stub = match block["clock_stub"].as_str().unwrap() {
        "stubbed" => true,
        "none" => false,
        other => panic!("unknown clock_stub {other:?}"),
    };
    if !matches!(execution.as_str(), "sim_world" | "real" | "bypass") {
        panic!("unknown execution mode {execution:?}");
    }
    if !matches!(history_mode.as_str(), "none" | "exact" | "empty") {
        panic!("unknown history mode {history_mode:?}");
    }
    let bias = block["delta_bias"].as_i64().unwrap() as i128;
    let state = Rc::new(RefCell::new(State::default()));
    let reset_state = Rc::clone(&state);
    let apply_state = Rc::clone(&state);
    let observe_state = Rc::clone(&state);
    let evidence_state = Rc::clone(&state);
    let history_state = Rc::clone(&state);
    let probe_state = Rc::clone(&state);
    let id = s(block, "id");
    let world_id = format!("{id}.world");
    let mut callbacks = SimConsumerCallbacks {
        reset: Some(Box::new(move || {
            let probes = reset_state.borrow().probes;
            *reset_state.borrow_mut() = State {
                probes,
                world: SimConsumerWorldEvidence {
                    world_id: world_id.clone(),
                    steps: 0,
                    trace: vec![],
                },
                ..State::default()
            };
            Ok(())
        })),
        apply: Some(Box::new(move |action| {
            let delta = action
                .payload
                .as_int()
                .ok_or_else(|| "payload is not int".to_owned())?;
            let mut state = apply_state.borrow_mut();
            state.value += delta + bias;
            if adapter_kind != SimConsumerAdapterKind::InMemory && history_mode == "exact" {
                state.history.push(action.clone());
            }
            if adapter_kind == SimConsumerAdapterKind::InMemory && execution == "sim_world" {
                state.world.steps += 1;
                state.world.trace.push(SimConsumerTraceEntry {
                    action_id: action.id,
                    kind: "action_execute".into(),
                });
            }
            Ok(())
        })),
        observe: Some(Box::new(move || {
            Ok(BTreeMap::from([(
                "consumer.value".into(),
                ReplayValue::Int(observe_state.borrow().value),
            )]))
        })),
        ..SimConsumerCallbacks::default()
    };
    if adapter_kind == SimConsumerAdapterKind::InMemory {
        callbacks.world_evidence =
            Some(Box::new(move || Ok(evidence_state.borrow().world.clone())));
    } else {
        callbacks.probe = Some(Box::new(move || {
            probe_state.borrow_mut().probes += 1;
            Ok(())
        }));
        callbacks.materialized_history =
            Some(Box::new(move || Ok(history_state.borrow().history.clone())));
    }
    (
        SimConsumerAdapter {
            id,
            kind: adapter_kind,
            service_id: s(block, "service_id"),
            reducer_id: s(block, "reducer_id"),
            production_reducer_id: s(block, "production_reducer_id"),
            protocol_id: s(block, "protocol_id"),
            external_port: block
                .get("external_port")
                .map(|v| port(v.as_str().unwrap())),
            ports: ports(fixture, clock_stub),
            callbacks,
        },
        state,
    )
}

#[test]
fn consumer_testkit_conformance() {
    let fixture = fixture();
    assert_eq!(fixture["kind"], "ConsumerSimulationTestkit");
    let materialized = scenario(&fixture);
    for (index, id, view) in common::scenarios(&FIXTURE.to_string(), &fixture) {
        let sc = view.value();
        let mut states = BTreeMap::new();
        let mut adapters = Vec::new();
        for block in sc["adapters"].as_array().unwrap() {
            let (adapter, state) = adapter(&fixture, block);
            states.insert(adapter.id.clone(), state);
            adapters.push(adapter);
        }
        let required_real_adapters = sc["required_real_adapters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| kind(v.as_str().unwrap()))
            .collect();
        let required_external_processes = sc["required_external_processes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| SimConsumerExternalSelection {
                adapter_id: s(v, "adapter_id"),
                port: port(v["port"].as_str().unwrap()),
            })
            .collect();
        let mut kit = SimConsumerTestkit::new(SimConsumerTestkitSpec {
            simulation_adapter_id: s(sc, "simulation_adapter_id"),
            required_real_adapters,
            required_external_processes,
            adapters,
        })
        .unwrap_or_else(|e| panic!("{id}: construct: {e}"));
        let expected = &sc["expected"];
        let expected_guard =
            common::Expect::new(FIXTURE, format!("scenarios[{index}].expected"), expected);
        let outcome = expected["outcome"].as_str().unwrap();
        match outcome {
            "success" => {
                let result = kit
                    .run(&materialized)
                    .unwrap_or_else(|e| panic!("{id}: {e}"));
                expected_guard.assert_key("outcome", "success");
                expected_guard.assert_key("adapter_ids", serde_json::json!(result.adapter_ids));
                let expected_ids: Vec<_> = expected["adapter_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(
                    result
                        .adapter_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    expected_ids,
                    "{id}"
                );
                let expected_steps: Vec<_> = expected["checkpoint_steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
                    .collect();
                assert_eq!(
                    result
                        .checkpoints
                        .iter()
                        .map(|c| c.step)
                        .collect::<Vec<_>>(),
                    expected_steps,
                    "{id}"
                );
                expected_guard.assert_key(
                    "checkpoint_steps",
                    serde_json::json!(
                        result
                            .checkpoints
                            .iter()
                            .map(|c| c.step)
                            .collect::<Vec<_>>()
                    ),
                );
                if let Some(actions) = expected.get("checkpoint_action_ids") {
                    let actions: Vec<_> = actions
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect();
                    assert_eq!(
                        result
                            .checkpoints
                            .iter()
                            .map(|c| c.action_id.as_str())
                            .collect::<Vec<_>>(),
                        actions,
                        "{id}"
                    );
                    expected_guard.assert_key(
                        "checkpoint_action_ids",
                        serde_json::json!(
                            result
                                .checkpoints
                                .iter()
                                .map(|c| c.action_id.as_str())
                                .collect::<Vec<_>>()
                        ),
                    );
                }
                assert!(!result.scenario_digest.as_bytes().is_empty());
                for evidence in &result.adapter_evidence {
                    let wanted = sc["adapters"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|a| a["id"] == evidence.adapter_id)
                        .unwrap();
                    assert_eq!(
                        evidence.protocol_id,
                        wanted["protocol_id"].as_str().unwrap()
                    );
                    assert_eq!(evidence.reducer_id, wanted["reducer_id"].as_str().unwrap());
                    assert_eq!(
                        evidence.production_reducer_id,
                        wanted["production_reducer_id"].as_str().unwrap()
                    );
                    assert_eq!(evidence.service_id, wanted["service_id"].as_str().unwrap());
                }
                for state in states.values() {
                    assert_eq!(state.borrow().value, 6);
                }
                for (adapter_id, state) in &states {
                    if adapter_id != "memory" {
                        assert_eq!(state.borrow().probes, 1);
                    }
                }
                assert_eq!(expected["checkpoint_values"], serde_json::json!([1, 3, 6]));
                expected_guard.assert_key("checkpoint_values", serde_json::json!([1, 3, 6]));
                match id.as_str() {
                    "selected_real_adapters_match_every_checkpoint" => {
                        assert_eq!(
                            expected["observation_relation"],
                            "all_equal_at_every_checkpoint"
                        );
                        assert_eq!(
                            expected["materialized_history_relation"],
                            "exact_prefix_at_every_checkpoint"
                        );
                        assert_eq!(expected["probe_relation"], "every_real_adapter_once");
                        expected_guard
                            .assert_key("observation_relation", "all_equal_at_every_checkpoint");
                        expected_guard.assert_key(
                            "materialized_history_relation",
                            "exact_prefix_at_every_checkpoint",
                        );
                        expected_guard.assert_key("probe_relation", "every_real_adapter_once");
                    }
                    "selected_external_process_preserves_independent_reducer_identity" => {
                        let evidence = result
                            .adapter_evidence
                            .iter()
                            .find(|e| {
                                e.adapter_id == expected["external_adapter_id"].as_str().unwrap()
                            })
                            .unwrap();
                        assert_eq!(
                            evidence.external_port,
                            Some(port(expected["external_port"].as_str().unwrap()))
                        );
                        assert_eq!(
                            evidence.protocol_id,
                            expected["external_protocol_id"].as_str().unwrap()
                        );
                        assert_eq!(
                            evidence.reducer_id,
                            expected["external_reducer_id"].as_str().unwrap()
                        );
                        assert_eq!(
                            evidence.production_reducer_id,
                            expected["external_production_reducer_id"].as_str().unwrap()
                        );
                        expected_guard
                            .assert_key("external_adapter_id", evidence.adapter_id.clone());
                        expected_guard.assert_key(
                            "external_port",
                            match evidence.external_port.expect("external port") {
                                SimConsumerExternalPort::Cli => "cli",
                                SimConsumerExternalPort::Filesystem => "filesystem",
                                SimConsumerExternalPort::LocalSocket => "local_socket",
                                SimConsumerExternalPort::EditorReplica => "editor_replica",
                            },
                        );
                        expected_guard
                            .assert_key("external_protocol_id", evidence.protocol_id.clone());
                        expected_guard
                            .assert_key("external_reducer_id", evidence.reducer_id.clone());
                        expected_guard.assert_key(
                            "external_production_reducer_id",
                            evidence.production_reducer_id.clone(),
                        );
                    }
                    other => panic!("unknown successful scenario {other:?}"),
                }
            }
            "observation_divergence"
            | "materialized_history_mismatch"
            | "simulation_world_bypass" => {
                let error = kit.run(&materialized).expect_err(&id);
                expected_guard.assert_key("outcome", error.kind);
                expected_guard.assert_key("step", error.step.expect("localized step"));
                expected_guard.assert_key(
                    "action_id",
                    error.action_id.as_deref().expect("localized action"),
                );
                expected_guard.assert_key(
                    "adapter_id",
                    error.adapter_id.as_deref().expect("localized adapter"),
                );
                assert_eq!(error.kind, outcome, "{id}");
                assert_eq!(error.step, Some(expected["step"].as_u64().unwrap()), "{id}");
                assert_eq!(
                    error.action_id.as_deref(),
                    expected["action_id"].as_str(),
                    "{id}"
                );
                assert_eq!(
                    error.adapter_id.as_deref(),
                    expected["adapter_id"].as_str(),
                    "{id}"
                );
                if outcome == "observation_divergence" {
                    expected_guard.assert_key(
                        "observation_id",
                        error
                            .observation_id
                            .as_deref()
                            .expect("localized observation"),
                    );
                    assert_eq!(
                        error.observation_id.as_deref(),
                        expected["observation_id"].as_str(),
                        "{id}"
                    );
                }
                if outcome == "materialized_history_mismatch" {
                    expected_guard.assert_key("expected_prefix_length", 1_u64);
                    expected_guard.assert_key("actual_prefix_length", 0_u64);
                    assert_eq!(expected["expected_prefix_length"], 1);
                    assert_eq!(expected["actual_prefix_length"], 0);
                }
            }
            other => panic!("unknown expected outcome {other:?}"),
        }
    }
}

fn valid_pair() -> SimConsumerTestkitSpec {
    let fixture = fixture();
    let scenario = &fixture["scenarios"][0];
    let mut adapters = Vec::new();
    for block in scenario["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| matches!(v["kind"].as_str().unwrap(), "in_memory" | "postgres"))
    {
        adapters.push(adapter(&fixture, block).0);
    }
    SimConsumerTestkitSpec {
        simulation_adapter_id: "memory".into(),
        required_real_adapters: vec![SimConsumerAdapterKind::Postgres],
        required_external_processes: vec![],
        adapters,
    }
}

#[test]
fn constructor_rejection_matrix_is_callback_free() {
    type MutateSpec = Box<dyn Fn(&mut SimConsumerTestkitSpec)>;
    let mut cases: Vec<(&str, MutateSpec)> = vec![
        (
            "no selection",
            Box::new(|s| s.required_real_adapters.clear()),
        ),
        (
            "missing simulation",
            Box::new(|s| s.simulation_adapter_id = "missing".into()),
        ),
        (
            "duplicate adapter",
            Box::new(|s| s.adapters[1].id = s.adapters[0].id.clone()),
        ),
        (
            "protocol mismatch",
            Box::new(|s| s.adapters[1].protocol_id = "other.protocol".into()),
        ),
        (
            "port mismatch",
            Box::new(|s| s.adapters[1].ports.pop().map(drop).unwrap()),
        ),
        (
            "real stub",
            Box::new(|s| s.adapters[1].ports[1].stubbed = true),
        ),
        (
            "missing callback",
            Box::new(|s| s.adapters[1].callbacks.probe = None),
        ),
        (
            "wrong reducer",
            Box::new(|s| s.adapters[1].reducer_id = "other.reducer".into()),
        ),
    ];
    for (name, mutate) in cases.drain(..) {
        let mut spec = valid_pair();
        mutate(&mut spec);
        assert!(SimConsumerTestkit::new(spec).is_err(), "{name}");
    }
}
