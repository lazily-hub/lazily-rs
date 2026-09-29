//! Consumer simulation conformance testkit.
//!
//! The testkit runs one generated action history through an in-memory reducer
//! and explicitly selected real adapters, then compares canonical observations
//! after every action.  It is intentionally callback based: integrations own
//! their service lifecycle while construction can still reject incomplete or
//! ambiguous topologies before touching a service.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use crate::{ReplayDigest, ReplayValue, canonical_bytes, canonical_digest};

/// Adapter execution boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimConsumerAdapterKind {
    InMemory,
    Postgres,
    Nats,
    ExternalProcess,
}

impl SimConsumerAdapterKind {
    fn real(self) -> bool {
        self != Self::InMemory
    }
}

/// Supported external-process integration port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimConsumerExternalPort {
    Cli,
    Filesystem,
    LocalSocket,
    EditorReplica,
}

/// Determinism declaration for a narrow external boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimConsumerPortDeterminism {
    Deterministic,
    Nondeterministic,
}

/// One narrow external boundary used by every adapter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SimConsumerPort {
    pub id: String,
    pub kind: String,
    pub determinism: SimConsumerPortDeterminism,
    pub stubbed: bool,
}

/// One materialized generated action.
#[derive(Clone, Debug, PartialEq)]
pub struct SimConsumerAction {
    pub id: String,
    pub actor_id: String,
    pub kind: String,
    pub version: String,
    pub payload: ReplayValue,
}

/// An action and its generated model state.
#[derive(Clone, Debug, PartialEq)]
pub struct SimConsumerGeneratedAction {
    pub action: SimConsumerAction,
    pub model_after: ReplayValue,
}

/// A fully materialized deterministic scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct SimConsumerScenario {
    pub seed: [u8; 32],
    pub generator_path: String,
    pub generator_version: String,
    pub initial_model: ReplayValue,
    pub actions: Vec<SimConsumerGeneratedAction>,
}

/// One immutable trace entry from an in-memory execution world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerTraceEntry {
    pub action_id: String,
    pub kind: String,
}

/// Minimal immutable proof that the simulation adapter used the same world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerWorldEvidence {
    pub world_id: String,
    pub steps: u64,
    pub trace: Vec<SimConsumerTraceEntry>,
}

pub type SimConsumerObservation = BTreeMap<String, ReplayValue>;
pub type SimConsumerCallbackResult<T> = Result<T, String>;

/// Integration callbacks. Required callbacks depend on adapter kind and are
/// validated by [`SimConsumerTestkit::new`] before any callback is invoked.
#[derive(Default)]
pub struct SimConsumerCallbacks {
    pub probe: Option<Box<dyn FnMut() -> SimConsumerCallbackResult<()>>>,
    pub reset: Option<Box<dyn FnMut() -> SimConsumerCallbackResult<()>>>,
    pub apply: Option<Box<dyn FnMut(SimConsumerAction) -> SimConsumerCallbackResult<()>>>,
    pub observe: Option<Box<dyn FnMut() -> SimConsumerCallbackResult<SimConsumerObservation>>>,
    pub materialized_history:
        Option<Box<dyn FnMut() -> SimConsumerCallbackResult<Vec<SimConsumerAction>>>>,
    pub world_evidence:
        Option<Box<dyn FnMut() -> SimConsumerCallbackResult<SimConsumerWorldEvidence>>>,
}

/// One configured adapter.
pub struct SimConsumerAdapter {
    pub id: String,
    pub kind: SimConsumerAdapterKind,
    pub service_id: String,
    pub reducer_id: String,
    pub production_reducer_id: String,
    pub protocol_id: String,
    pub external_port: Option<SimConsumerExternalPort>,
    pub ports: Vec<SimConsumerPort>,
    pub callbacks: SimConsumerCallbacks,
}

/// Explicit selection of an external-process adapter and port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerExternalSelection {
    pub adapter_id: String,
    pub port: SimConsumerExternalPort,
}

/// Constructor input.
pub struct SimConsumerTestkitSpec {
    pub simulation_adapter_id: String,
    pub required_real_adapters: Vec<SimConsumerAdapterKind>,
    pub required_external_processes: Vec<SimConsumerExternalSelection>,
    pub adapters: Vec<SimConsumerAdapter>,
}

/// Validated adapter identity returned as run evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerAdapterEvidence {
    pub adapter_id: String,
    pub kind: SimConsumerAdapterKind,
    pub service_id: String,
    pub reducer_id: String,
    pub production_reducer_id: String,
    pub protocol_id: String,
    pub external_port: Option<SimConsumerExternalPort>,
}

/// Per-action canonical observation digests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerCheckpoint {
    pub step: u64,
    pub action_id: String,
    pub observation_digests: BTreeMap<String, ReplayDigest>,
}

/// Successful run result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerRunResult {
    pub scenario_digest: ReplayDigest,
    pub adapter_ids: Vec<String>,
    pub adapter_evidence: Vec<SimConsumerAdapterEvidence>,
    pub checkpoints: Vec<SimConsumerCheckpoint>,
}

/// Fail-closed construction, execution, or divergence error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConsumerError {
    pub kind: &'static str,
    pub step: Option<u64>,
    pub action_id: Option<Box<str>>,
    pub adapter_id: Option<Box<str>>,
    pub observation_id: Option<Box<str>>,
    message: Box<str>,
}

impl SimConsumerError {
    fn new(kind: &'static str, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            step: None,
            action_id: None,
            adapter_id: None,
            observation_id: None,
            message: message.into(),
        }
    }
    fn at(mut self, step: u64, action: &str, adapter: &str) -> Self {
        self.step = Some(step);
        self.action_id = Some(action.into());
        self.adapter_id = Some(adapter.into());
        self
    }
}

impl fmt::Display for SimConsumerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "consumer simulation conformance failed: {}",
            self.message
        )
    }
}
impl Error for SimConsumerError {}

/// Validated consumer simulation testkit.
pub struct SimConsumerTestkit {
    adapters: Vec<SimConsumerAdapter>,
    baseline: usize,
}

impl SimConsumerTestkit {
    /// Validate a topology without invoking any adapter callback.
    pub fn new(mut spec: SimConsumerTestkitSpec) -> Result<Self, SimConsumerError> {
        if !stable_id(&spec.simulation_adapter_id) {
            return Err(invalid("simulation adapter id is not stable"));
        }
        if spec.required_real_adapters.is_empty() && spec.required_external_processes.is_empty() {
            return Err(invalid("at least one real adapter must be selected"));
        }
        let mut ids = BTreeSet::new();
        for adapter in &spec.adapters {
            validate_adapter(adapter)?;
            if !ids.insert(adapter.id.clone()) {
                return Err(invalid(format!("duplicate adapter {:?}", adapter.id)));
            }
        }
        let baseline = spec
            .adapters
            .iter()
            .position(|a| a.id == spec.simulation_adapter_id)
            .ok_or_else(|| invalid("simulation adapter is not configured"))?;
        if spec.adapters[baseline].kind != SimConsumerAdapterKind::InMemory {
            return Err(invalid("simulation adapter must be in_memory"));
        }
        let mut selected = BTreeSet::from([spec.simulation_adapter_id.clone()]);
        for kind in &spec.required_real_adapters {
            if !matches!(
                kind,
                SimConsumerAdapterKind::Postgres | SimConsumerAdapterKind::Nats
            ) {
                return Err(invalid("required real kind must be postgres or nats"));
            }
            let matches: Vec<_> = spec.adapters.iter().filter(|a| a.kind == *kind).collect();
            if matches.len() != 1 {
                return Err(invalid(format!(
                    "selection {kind:?} resolves to {} adapters",
                    matches.len()
                )));
            }
            selected.insert(matches[0].id.clone());
        }
        let mut external_ids = BTreeSet::new();
        for selection in &spec.required_external_processes {
            if !external_ids.insert(selection.adapter_id.clone()) {
                return Err(invalid("duplicate external selection"));
            }
            let adapter = spec
                .adapters
                .iter()
                .find(|a| a.id == selection.adapter_id)
                .ok_or_else(|| invalid("external selection is unresolved"))?;
            if adapter.kind != SimConsumerAdapterKind::ExternalProcess
                || adapter.external_port != Some(selection.port)
            {
                return Err(invalid("external selection has wrong kind or port"));
            }
            selected.insert(adapter.id.clone());
        }
        if spec.adapters.iter().any(|a| !selected.contains(&a.id)) {
            return Err(invalid("configured adapter was not explicitly selected"));
        }
        let protocol = &spec.adapters[baseline].protocol_id;
        let production = &spec.adapters[baseline].production_reducer_id;
        let ports = port_contract(&spec.adapters[baseline].ports);
        for adapter in &spec.adapters {
            if &adapter.protocol_id != protocol {
                return Err(invalid("adapters do not share a protocol id"));
            }
            if adapter.kind != SimConsumerAdapterKind::ExternalProcess
                && &adapter.production_reducer_id != production
            {
                return Err(invalid(
                    "built-in adapters do not share a production reducer",
                ));
            }
            if port_contract(&adapter.ports) != ports {
                return Err(invalid(
                    "adapters do not expose the same narrow-port contract",
                ));
            }
        }
        spec.adapters.sort_by(|a, b| a.id.cmp(&b.id));
        let baseline = spec
            .adapters
            .iter()
            .position(|a| a.id == spec.simulation_adapter_id)
            .expect("validated baseline");
        Ok(Self {
            adapters: spec.adapters,
            baseline,
        })
    }

    /// Execute and compare one generated history.
    pub fn run(
        &mut self,
        scenario: &SimConsumerScenario,
    ) -> Result<SimConsumerRunResult, SimConsumerError> {
        validate_scenario(scenario)?;
        let scenario_digest = canonical_digest(&scenario_value(scenario))
            .map_err(|e| invalid(format!("encode scenario: {e}")))?;
        for adapter in &mut self.adapters {
            if adapter.kind.real() {
                call0(&mut adapter.callbacks.probe, "probe", &adapter.id)?;
            }
            call0(&mut adapter.callbacks.reset, "reset", &adapter.id)?;
            if adapter.kind == SimConsumerAdapterKind::InMemory {
                evidence(&mut adapter.callbacks.world_evidence, &adapter.id)?;
            } else {
                let history = history(&mut adapter.callbacks.materialized_history, &adapter.id)?;
                if !history.is_empty() {
                    return Err(invalid(format!(
                        "adapter {:?} history is not initially empty",
                        adapter.id
                    )));
                }
            }
        }
        let adapter_ids = self.adapters.iter().map(|a| a.id.clone()).collect();
        let adapter_evidence = self
            .adapters
            .iter()
            .map(|a| SimConsumerAdapterEvidence {
                adapter_id: a.id.clone(),
                kind: a.kind,
                service_id: a.service_id.clone(),
                reducer_id: a.reducer_id.clone(),
                production_reducer_id: a.production_reducer_id.clone(),
                protocol_id: a.protocol_id.clone(),
                external_port: a.external_port,
            })
            .collect();
        let mut checkpoints = Vec::new();
        for (index, generated) in scenario.actions.iter().enumerate() {
            let step = (index + 1) as u64;
            let mut observed = Vec::new();
            let mut digests = BTreeMap::new();
            for adapter in &mut self.adapters {
                let before = if adapter.kind == SimConsumerAdapterKind::InMemory {
                    Some(evidence(
                        &mut adapter.callbacks.world_evidence,
                        &adapter.id,
                    )?)
                } else {
                    None
                };
                adapter.callbacks.apply.as_mut().expect("validated apply")(
                    generated.action.clone(),
                )
                .map_err(|e| {
                    callback_error("apply", &adapter.id, e).at(
                        step,
                        &generated.action.id,
                        &adapter.id,
                    )
                })?;
                if let Some(before) = before {
                    let after = evidence(&mut adapter.callbacks.world_evidence, &adapter.id)?;
                    let executed = after
                        .trace
                        .get(before.trace.len()..)
                        .unwrap_or_default()
                        .iter()
                        .any(|entry| {
                            entry.action_id == generated.action.id
                                && entry.kind.starts_with("action_")
                        });
                    if after.world_id != before.world_id || after.steps <= before.steps || !executed
                    {
                        return Err(SimConsumerError::new(
                            "simulation_world_bypass",
                            "in-memory adapter bypassed its simulation world",
                        )
                        .at(step, &generated.action.id, &adapter.id));
                    }
                } else {
                    let actual = history(&mut adapter.callbacks.materialized_history, &adapter.id)?;
                    let expected: Vec<_> = scenario.actions[..=index]
                        .iter()
                        .map(|a| a.action.clone())
                        .collect();
                    if !same_history(&actual, &expected)? {
                        return Err(SimConsumerError::new(
                            "materialized_history_mismatch",
                            format!(
                                "history length {} does not match exact prefix {}",
                                actual.len(),
                                expected.len()
                            ),
                        )
                        .at(step, &generated.action.id, &adapter.id));
                    }
                }
                let values = adapter
                    .callbacks
                    .observe
                    .as_mut()
                    .expect("validated observe")()
                .map_err(|e| {
                    callback_error("observe", &adapter.id, e).at(
                        step,
                        &generated.action.id,
                        &adapter.id,
                    )
                })?;
                if values.is_empty() {
                    return Err(SimConsumerError::new(
                        "empty_observation",
                        "adapter returned no observations",
                    )
                    .at(step, &generated.action.id, &adapter.id));
                }
                let digest = canonical_digest(&observation_value(&values))
                    .map_err(|e| invalid(format!("encode observation: {e}")))?;
                digests.insert(adapter.id.clone(), digest);
                observed.push(values);
            }
            let baseline = &observed[self.baseline];
            for (adapter_index, adapter) in self.adapters.iter().enumerate() {
                if adapter_index == self.baseline {
                    continue;
                }
                let other = &observed[adapter_index];
                let keys: BTreeSet<_> = baseline.keys().chain(other.keys()).collect();
                for key in keys {
                    let equal = match (baseline.get(key), other.get(key)) {
                        (Some(a), Some(b)) => {
                            canonical_bytes(a).map_err(|e| invalid(e.to_string()))?
                                == canonical_bytes(b).map_err(|e| invalid(e.to_string()))?
                        }
                        _ => false,
                    };
                    if !equal {
                        let mut error = SimConsumerError::new(
                            "observation_divergence",
                            "canonical observations differ",
                        )
                        .at(step, &generated.action.id, &adapter.id);
                        error.observation_id = Some(key.clone().into());
                        return Err(error);
                    }
                }
            }
            checkpoints.push(SimConsumerCheckpoint {
                step,
                action_id: generated.action.id.clone(),
                observation_digests: digests,
            });
        }
        Ok(SimConsumerRunResult {
            scenario_digest,
            adapter_ids,
            adapter_evidence,
            checkpoints,
        })
    }
}

fn invalid(message: impl Into<String>) -> SimConsumerError {
    SimConsumerError::new("invalid_configuration", message.into().into_boxed_str())
}
fn callback_error(op: &str, id: &str, message: String) -> SimConsumerError {
    SimConsumerError::new("callback_error", format!("{op} adapter {id:?}: {message}"))
}
fn stable_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-/".contains(&b))
}

fn validate_adapter(a: &SimConsumerAdapter) -> Result<(), SimConsumerError> {
    if !stable_id(&a.id) || !stable_id(&a.protocol_id) || !stable_id(&a.reducer_id) {
        return Err(invalid(
            "adapter needs stable adapter, protocol, and reducer ids",
        ));
    }
    if a.callbacks.reset.is_none() || a.callbacks.apply.is_none() || a.callbacks.observe.is_none() {
        return Err(invalid(format!(
            "adapter {:?} needs reset, apply, and observe callbacks",
            a.id
        )));
    }
    let mut ids = BTreeSet::new();
    for p in &a.ports {
        if !stable_id(&p.id) || !stable_id(&p.kind) || !ids.insert(&p.id) {
            return Err(invalid("ports need unique stable ids and kinds"));
        }
        if p.stubbed && p.determinism == SimConsumerPortDeterminism::Deterministic {
            return Err(invalid("deterministic port cannot be stubbed"));
        }
        if a.kind.real() && p.stubbed {
            return Err(invalid("real adapter cannot stub a port"));
        }
    }
    match a.kind {
        SimConsumerAdapterKind::InMemory => {
            if !stable_id(&a.production_reducer_id)
                || !a.service_id.is_empty()
                || a.external_port.is_some()
                || a.callbacks.probe.is_some()
                || a.callbacks.materialized_history.is_some()
                || a.callbacks.world_evidence.is_none()
            {
                return Err(invalid("invalid in-memory adapter evidence or callbacks"));
            }
        }
        SimConsumerAdapterKind::Postgres | SimConsumerAdapterKind::Nats => {
            if !stable_id(&a.production_reducer_id)
                || a.reducer_id != a.production_reducer_id
                || a.external_port.is_some()
            {
                return Err(invalid("invalid built-in real reducer evidence"));
            }
            validate_real(a)?;
            if a.callbacks.world_evidence.is_some() {
                return Err(invalid("real adapter cannot expose world evidence"));
            }
        }
        SimConsumerAdapterKind::ExternalProcess => {
            if !a.production_reducer_id.is_empty() || a.external_port.is_none() {
                return Err(invalid(
                    "external adapter must have its own reducer and selected port",
                ));
            }
            validate_real(a)?;
            if a.callbacks.world_evidence.is_some() {
                return Err(invalid("real adapter cannot expose world evidence"));
            }
        }
    }
    Ok(())
}
fn validate_real(a: &SimConsumerAdapter) -> Result<(), SimConsumerError> {
    if !stable_id(&a.service_id)
        || a.callbacks.probe.is_none()
        || a.callbacks.materialized_history.is_none()
    {
        return Err(invalid(
            "real adapter needs service id, probe, and materialized history",
        ));
    }
    Ok(())
}
fn port_contract(
    ports: &[SimConsumerPort],
) -> BTreeSet<(String, String, SimConsumerPortDeterminism)> {
    ports
        .iter()
        .map(|p| (p.id.clone(), p.kind.clone(), p.determinism))
        .collect()
}
fn call0(
    callback: &mut Option<Box<dyn FnMut() -> SimConsumerCallbackResult<()>>>,
    op: &str,
    id: &str,
) -> Result<(), SimConsumerError> {
    callback.as_mut().expect("validated callback")().map_err(|e| callback_error(op, id, e))
}
fn evidence(
    callback: &mut Option<Box<dyn FnMut() -> SimConsumerCallbackResult<SimConsumerWorldEvidence>>>,
    id: &str,
) -> Result<SimConsumerWorldEvidence, SimConsumerError> {
    callback.as_mut().expect("validated evidence")()
        .map_err(|e| callback_error("world evidence", id, e))
}
fn history(
    callback: &mut Option<Box<dyn FnMut() -> SimConsumerCallbackResult<Vec<SimConsumerAction>>>>,
    id: &str,
) -> Result<Vec<SimConsumerAction>, SimConsumerError> {
    callback.as_mut().expect("validated history")()
        .map_err(|e| callback_error("materialized history", id, e))
}

fn same_history(
    actual: &[SimConsumerAction],
    expected: &[SimConsumerAction],
) -> Result<bool, SimConsumerError> {
    if actual.len() != expected.len() {
        return Ok(false);
    }
    for (a, b) in actual.iter().zip(expected) {
        if canonical_bytes(&action_value(a)).map_err(|e| invalid(e.to_string()))?
            != canonical_bytes(&action_value(b)).map_err(|e| invalid(e.to_string()))?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
fn validate_scenario(s: &SimConsumerScenario) -> Result<(), SimConsumerError> {
    if !stable_id(&s.generator_path) || !stable_id(&s.generator_version) {
        return Err(invalid("scenario needs stable generator identity"));
    }
    let mut ids = BTreeSet::new();
    for generated in &s.actions {
        let a = &generated.action;
        if !stable_id(&a.id)
            || !stable_id(&a.actor_id)
            || !stable_id(&a.kind)
            || !stable_id(&a.version)
            || !ids.insert(&a.id)
        {
            return Err(invalid(
                "scenario action identities must be unique and stable",
            ));
        }
    }
    Ok(())
}
fn action_value(a: &SimConsumerAction) -> ReplayValue {
    ReplayValue::Record {
        name: "SimConsumerAction".into(),
        fields: vec![
            ("id".into(), a.id.clone().into()),
            ("actor_id".into(), a.actor_id.clone().into()),
            ("kind".into(), a.kind.clone().into()),
            ("version".into(), a.version.clone().into()),
            ("payload".into(), a.payload.clone()),
        ],
    }
}
fn observation_value(o: &SimConsumerObservation) -> ReplayValue {
    ReplayValue::Map(
        o.iter()
            .map(|(k, v)| (k.clone().into(), v.clone()))
            .collect(),
    )
}
fn scenario_value(s: &SimConsumerScenario) -> ReplayValue {
    ReplayValue::Record {
        name: "SimConsumerScenario".into(),
        fields: vec![
            ("seed".into(), ReplayValue::Bytes(s.seed.to_vec())),
            ("generator_path".into(), s.generator_path.clone().into()),
            (
                "generator_version".into(),
                s.generator_version.clone().into(),
            ),
            ("initial_model".into(), s.initial_model.clone()),
            (
                "actions".into(),
                ReplayValue::Seq(
                    s.actions
                        .iter()
                        .map(|g| ReplayValue::Record {
                            name: "SimConsumerGeneratedAction".into(),
                            fields: vec![
                                ("action".into(), action_value(&g.action)),
                                ("model_after".into(), g.model_after.clone()),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    }
}
