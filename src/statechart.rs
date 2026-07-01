//! Full Harel/SCXML state charts — native Rust, conforming to
//! `lazily-spec/docs/state-charts.md`.
//!
//! A chart is **compute, not protocol**: it is never serialized as a distinct
//! wire kind. In this reactive binding the active configuration lives in a
//! [`CellHandle`], so any slot/signal/effect reading [`StateChart::configuration`],
//! [`StateChart::active_leaves`], or [`StateChart::matches`] is invalidated on a
//! real transition; a no-op self-transition is suppressed by the cell's
//! `PartialEq` guard (see the spec's "Self-transitions" section).
//!
//! Implemented subset (per the spec's implementation-status note): compound
//! states, orthogonal (parallel) regions, shallow + deep history, entry/exit/
//! transition actions, named guards, external + internal transitions. Extended
//! state `{"expr": …}` guards and `run` actions are rejected explicitly; `final`
//! states are accepted as leaves without raising completion (`done`) events.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use crate::{CellHandle, Context};

// Variants are constructed only by the feature-gated `parse_state`/`from_json`
// path; the reactive engine matches them but never constructs them without it.
#[cfg_attr(not(feature = "statechart"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Atomic,
    Compound,
    Parallel,
    History(HistoryKind),
    Final,
}

#[cfg_attr(not(feature = "statechart"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryKind {
    Shallow,
    Deep,
}

#[derive(Debug, Clone)]
struct Transition {
    target: String,
    guard: Option<String>,
    action: Vec<String>,
    internal: bool,
}

#[derive(Debug, Clone)]
struct StateDef {
    parent: Option<String>,
    kind: Kind,
    initial: Option<String>,
    default: Option<String>,
    transitions: HashMap<String, Transition>,
    entry: Vec<String>,
    exit: Vec<String>,
}

/// A parsed, immutable chart definition.
#[derive(Debug, Clone)]
pub struct ChartDef {
    states: HashMap<String, StateDef>,
    children: HashMap<String, Vec<String>>,
    order: HashMap<String, usize>,
    depth: HashMap<String, usize>,
    root: String,
}

/// A history recording for a region exited at least once.
#[derive(Debug, Clone)]
enum Recording {
    /// Direct child of the region that was active.
    Shallow(String),
    /// Full active sub-configuration below the region (leaves + ancestors).
    Deep(BTreeSet<String>),
}

/// A reactive full-Harel state chart backed by a configuration cell.
pub struct StateChart {
    def: ChartDef,
    config: CellHandle<BTreeSet<String>>,
    history: RefCell<HashMap<String, Recording>>,
    last_actions: RefCell<Vec<String>>,
}

impl ChartDef {
    /// Parse a chart definition from a `serde_json::Value` of the declarative
    /// form. Returns an error string for malformed charts or unsupported
    /// features (`run` actions, `{"expr": …}` guards).
    #[cfg(feature = "statechart")]
    pub fn from_json(value: &serde_json::Value) -> Result<ChartDef, String> {
        let obj = value
            .as_object()
            .ok_or_else(|| "chart must be a JSON object".to_string())?;
        // Validates `chart.initial` is present; descent uses each compound's
        // own `initial` from the root, so the value itself is not stored.
        let _top_initial = obj
            .get("initial")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "chart.initial is required".to_string())?
            .to_string();

        let states_obj = obj
            .get("states")
            .and_then(|v| v.as_object())
            .ok_or_else(|| "chart.states is required".to_string())?;

        let mut states: HashMap<String, StateDef> = HashMap::new();
        let mut order: HashMap<String, usize> = HashMap::new();
        for (idx, (id, raw)) in states_obj.iter().enumerate() {
            order.insert(id.clone(), idx);
            states.insert(id.clone(), parse_state(id, raw)?);
        }

        // Derived structure: children, depth, root.
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        let mut root: Option<String> = None;
        for (id, def) in &states {
            match &def.parent {
                Some(p) => children.entry(p.clone()).or_default().push(id.clone()),
                None => {
                    if root.is_some() {
                        return Err("chart has more than one root (parent-less state)".into());
                    }
                    root = Some(id.clone());
                }
            }
        }
        // Sort children by document order for deterministic parallel descent.
        for kids in children.values_mut() {
            kids.sort_by_key(|k| order.get(k).copied().unwrap_or(usize::MAX));
        }
        let root = root.ok_or_else(|| "chart has no root (parent-less state)".to_string())?;

        let mut depth: HashMap<String, usize> = HashMap::new();
        compute_depth(&states, &root, 0, &mut depth);

        Ok(ChartDef {
            states,
            children,
            order,
            depth,
            root,
        })
    }

    fn kind(&self, id: &str) -> Kind {
        self.states.get(id).map(|s| s.kind).unwrap_or(Kind::Atomic)
    }

    fn ancestors_inclusive(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(cid) = cur {
            out.push(cid.clone());
            cur = self.states.get(&cid).and_then(|s| s.parent.clone());
        }
        out
    }

    fn lca(&self, a: &str, b: &str) -> String {
        let anc_a: std::collections::HashSet<String> =
            self.ancestors_inclusive(a).into_iter().collect();
        for cid in self.ancestors_inclusive(b) {
            if anc_a.contains(&cid) {
                return cid;
            }
        }
        self.root.clone()
    }

    fn is_proper_descendant(&self, desc: &str, anc: &str) -> bool {
        desc != anc && self.ancestors_inclusive(desc).iter().any(|x| x == anc)
    }

    fn depth(&self, id: &str) -> usize {
        self.depth.get(id).copied().unwrap_or(0)
    }
}

#[cfg(feature = "statechart")]
fn parse_state(id: &str, raw: &serde_json::Value) -> Result<StateDef, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| format!("state {id} must be an object"))?;
    let parent = obj.get("parent").and_then(|v| v.as_str()).map(String::from);
    let initial = obj
        .get("initial")
        .and_then(|v| v.as_str())
        .map(String::from);
    let default = obj
        .get("default")
        .and_then(|v| v.as_str())
        .map(String::from);

    if obj.get("run").is_some() {
        return Err(format!(
            "state {id} uses `run` actions, which are not supported (rejecting explicitly per spec)"
        ));
    }

    let kind = if let Some(h) = obj.get("history").and_then(|v| v.as_str()) {
        Kind::History(match h {
            "shallow" => HistoryKind::Shallow,
            "deep" => HistoryKind::Deep,
            other => return Err(format!("state {id}: unknown history kind `{other}`")),
        })
    } else if obj.get("parallel").and_then(|v| v.as_bool()) == Some(true) {
        Kind::Parallel
    } else if matches!(obj.get("kind").and_then(|v| v.as_str()), Some("final")) {
        Kind::Final
    } else if obj.contains_key("initial") {
        Kind::Compound
    } else {
        Kind::Atomic
    };

    let entry = parse_action_list(obj.get("entry"))?;
    let exit = parse_action_list(obj.get("exit"))?;

    let mut transitions = HashMap::new();
    if let Some(on) = obj.get("on").and_then(|v| v.as_object()) {
        for (event, raw_t) in on {
            transitions.insert(event.clone(), parse_transition(raw_t)?);
        }
    }

    Ok(StateDef {
        parent,
        kind,
        initial,
        default,
        transitions,
        entry,
        exit,
    })
}

#[cfg(feature = "statechart")]
fn parse_action_list(raw: Option<&serde_json::Value>) -> Result<Vec<String>, String> {
    match raw {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| "action must be a string".into())
            })
            .collect(),
        Some(_) => Err("entry/exit must be an array of strings".into()),
    }
}

#[cfg(feature = "statechart")]
fn parse_transition(raw: &serde_json::Value) -> Result<Transition, String> {
    match raw {
        serde_json::Value::String(target) => Ok(Transition {
            target: target.clone(),
            guard: None,
            action: Vec::new(),
            internal: false,
        }),
        serde_json::Value::Object(o) => {
            let target = o
                .get("target")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "transition requires `target`".to_string())?
                .to_string();
            let guard = match o.get("guard") {
                None => None,
                Some(serde_json::Value::String(name)) => Some(name.clone()),
                Some(serde_json::Value::Object(_)) => {
                    return Err(
                        "context-expression `{expr: …}` guards are not supported (rejecting explicitly per spec)"
                            .into(),
                    );
                }
                Some(_) => return Err("guard must be a string".into()),
            };
            let action = parse_action_list(o.get("action"))?;
            let internal = o.get("internal").and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(Transition {
                target,
                guard,
                action,
                internal,
            })
        }
        _ => Err("transition must be a string or object".into()),
    }
}

#[cfg(feature = "statechart")]
fn compute_depth(
    states: &HashMap<String, StateDef>,
    id: &str,
    current: usize,
    out: &mut HashMap<String, usize>,
) {
    out.insert(id.to_string(), current);
    let next: Vec<String> = states
        .iter()
        .filter(|(_, d)| d.parent.as_deref() == Some(id))
        .map(|(k, _)| k.clone())
        .collect();
    for child in next {
        compute_depth(states, &child, current + 1, out);
    }
}

impl StateChart {
    /// Create a chart over `ctx`, entering the initial configuration by
    /// descending from the root via each compound's `initial` (and every region
    /// for parallel states). Initial entry actions are recorded and available
    /// via [`StateChart::last_actions`].
    pub fn new(ctx: &Context, def: ChartDef) -> Self {
        let mut enter = BTreeSet::new();
        let mut actions = Vec::new();
        enter_subtree(&def, &def.root, &mut enter, &mut actions);

        let config = ctx.cell(enter);
        Self {
            def,
            config,
            history: RefCell::new(HashMap::new()),
            last_actions: RefCell::new(actions),
        }
    }

    /// Ordered action names fired by the initial entry or the most recent
    /// [`StateChart::send`] (exit → transition → entry).
    pub fn last_actions(&self) -> Vec<String> {
        self.last_actions.borrow().clone()
    }

    /// The full active configuration (active leaves plus all active ancestors).
    pub fn configuration(&self, ctx: &Context) -> BTreeSet<String> {
        ctx.get_cell(&self.config)
    }

    /// Active atomic leaves, sorted (one per parallel region; one for single-region).
    pub fn active_leaves(&self, ctx: &Context) -> Vec<String> {
        let config = self.configuration(ctx);
        let mut leaves: Vec<String> = config
            .iter()
            .filter(|id| matches!(self.def.kind(id), Kind::Atomic | Kind::Final))
            .cloned()
            .collect();
        leaves.sort();
        leaves
    }

    /// Hierarchical "state-in" predicate: `true` iff `id` is in the active configuration.
    pub fn matches(&self, ctx: &Context, id: &str) -> bool {
        self.configuration(ctx).contains(id)
    }

    /// Send an event. Returns `true` if any transition was taken, `false` if
    /// rejected (configuration unchanged, no actions fired). `guards` resolves
    /// named guards for this send (absent/unknown name → fail-closed `false`).
    pub fn send(&self, ctx: &Context, event: &str, guards: &HashMap<String, bool>) -> bool {
        let config = self.configuration(ctx);

        // 1. Enabled transitions: per active leaf, innermost passing match.
        struct Cand<'a> {
            source: String,
            transition: &'a Transition,
            leaf: String,
        }
        let mut candidates: Vec<Cand> = Vec::new();
        let leaves: Vec<String> = config
            .iter()
            .filter(|id| matches!(self.def.kind(id), Kind::Atomic | Kind::Final))
            .cloned()
            .collect();
        for leaf in &leaves {
            for anc in self.def.ancestors_inclusive(leaf) {
                if let Some(def) = self.def.states.get(&anc)
                    && let Some(t) = def.transitions.get(event)
                    && guard_passes(t, guards)
                {
                    candidates.push(Cand {
                        source: anc.clone(),
                        transition: t,
                        leaf: leaf.clone(),
                    });
                    break; // innermost wins for this leaf's chain
                }
            }
        }

        if candidates.is_empty() {
            *self.last_actions.borrow_mut() = Vec::new();
            return false;
        }

        // 2. Conflict resolution: order by source depth desc, then document order;
        //    take greedily, skipping any whose exit set intersects the taken union.
        candidates.sort_by(|a, b| {
            self.def
                .depth(&b.source)
                .cmp(&self.def.depth(&a.source))
                .then_with(|| {
                    self.def
                        .order
                        .get(&a.source)
                        .cmp(&self.def.order.get(&b.source))
                })
        });

        let mut exit_union: BTreeSet<String> = BTreeSet::new();
        let mut enter_union: BTreeSet<String> = BTreeSet::new();
        let mut taken_transitions: Vec<&Transition> = Vec::new();
        for cand in &candidates {
            let (exit_set, enter_set) =
                self.compute_exit_enter(&cand.source, cand.transition, &cand.leaf, &config);
            if exit_set.intersection(&exit_union).next().is_some() {
                continue; // conflicts with an already-taken transition
            }
            exit_union.extend(exit_set);
            enter_union.extend(enter_set);
            taken_transitions.push(cand.transition);
        }

        if taken_transitions.is_empty() {
            *self.last_actions.borrow_mut() = Vec::new();
            return false;
        }

        // 3. Record history for regions being exited that own a history child.
        let mut history = self.history.borrow_mut();
        for s in &exit_union {
            if let Some(h_child) = history_child_of(&self.def, s) {
                record_region(&self.def, s, h_child, &config, &mut history);
            }
        }
        drop(history);

        // 4. Action trace: exit (innermost-first) → transition → entry (outermost-first).
        let mut actions = Vec::new();
        let mut exit_sorted: Vec<&String> = exit_union.iter().collect();
        exit_sorted.sort_by_key(|s| std::cmp::Reverse(self.def.depth(s)));
        for s in &exit_sorted {
            actions.extend(self.def.states[*s].exit.iter().cloned());
        }
        for t in &taken_transitions {
            actions.extend(t.action.iter().cloned());
        }
        let mut enter_sorted: Vec<&String> = enter_union.iter().collect();
        enter_sorted.sort_by_key(|s| self.def.depth(s));
        for s in &enter_sorted {
            actions.extend(self.def.states[*s].entry.iter().cloned());
        }

        // 5. Apply new configuration.
        let mut new_config = config.clone();
        for s in &exit_union {
            new_config.remove(s);
        }
        for s in &enter_union {
            new_config.insert(s.clone());
        }

        *self.last_actions.borrow_mut() = actions;
        if new_config != config {
            ctx.set_cell(&self.config, new_config);
        }
        true
    }

    fn compute_exit_enter(
        &self,
        source: &str,
        transition: &Transition,
        leaf: &str,
        config: &BTreeSet<String>,
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let target = &transition.target;
        let internal = transition.internal
            && (target == source || self.def.is_proper_descendant(target, source));
        let lca = if internal {
            source.to_string()
        } else {
            self.def.lca(leaf, target)
        };

        // Exit set: active proper-descendants of the lca.
        let exit_set: BTreeSet<String> = config
            .iter()
            .filter(|s| self.def.is_proper_descendant(s, &lca))
            .cloned()
            .collect();

        // Enter set.
        let mut enter: BTreeSet<String> = BTreeSet::new();
        if matches!(self.def.kind(target), Kind::History(_)) {
            let region = self.def.states[target]
                .parent
                .clone()
                .unwrap_or_else(|| self.def.root.clone());
            for s in path_below(&self.def, &lca, &region) {
                enter.insert(s);
            }
            self.restore_via_history(target, &region, &mut enter);
        } else {
            for s in path_below(&self.def, &lca, target) {
                enter.insert(s);
            }
            let mut entry_actions = Vec::new();
            enter_subtree(&self.def, target, &mut enter, &mut entry_actions);
        }

        (exit_set, enter)
    }

    fn restore_via_history(&self, hist: &str, region: &str, enter: &mut BTreeSet<String>) {
        let history = self.history.borrow();
        match history.get(hist) {
            Some(Recording::Shallow(child)) => {
                let child = child.clone();
                drop(history);
                enter.insert(child.clone());
                let mut tmp = Vec::new();
                enter_subtree(&self.def, &child, enter, &mut tmp);
            }
            Some(Recording::Deep(set)) => {
                for s in set {
                    enter.insert(s.clone());
                }
            }
            None => {
                drop(history);
                // First entry: descend via `default`, else the region's `initial`.
                let start = self.def.states[hist]
                    .default
                    .clone()
                    .or_else(|| self.def.states[region].initial.clone());
                if let Some(start) = start {
                    for s in path_below(&self.def, region, &start) {
                        enter.insert(s);
                    }
                    let mut tmp = Vec::new();
                    enter_subtree(&self.def, &start, enter, &mut tmp);
                }
            }
        }
    }
}

fn guard_passes(t: &Transition, guards: &HashMap<String, bool>) -> bool {
    match &t.guard {
        None => true,
        Some(name) => guards.get(name).copied().unwrap_or(false), // fail-closed
    }
}

/// Enter `state` and its default descendants, recording entry actions top-down.
fn enter_subtree(
    def: &ChartDef,
    state: &str,
    enter: &mut BTreeSet<String>,
    actions: &mut Vec<String>,
) {
    enter.insert(state.to_string());
    collect_entry(def, state, actions);
    match def.kind(state) {
        Kind::Atomic | Kind::Final | Kind::History(_) => {}
        Kind::Compound => {
            if let Some(init) = def.states[state].initial.as_deref() {
                enter_subtree(def, init, enter, actions);
            }
        }
        Kind::Parallel => {
            for region in def.children.get(state).cloned().unwrap_or_default() {
                enter_subtree(def, &region, enter, actions);
            }
        }
    }
}

fn collect_entry(def: &ChartDef, state: &str, actions: &mut Vec<String>) {
    actions.extend(def.states[state].entry.iter().cloned());
}

/// Path from just-below `lca` down to `target` (exclusive lca, inclusive target).
fn path_below(def: &ChartDef, lca: &str, target: &str) -> Vec<String> {
    let mut chain = def.ancestors_inclusive(target); // [target, ..., root]
    let idx = chain.iter().position(|x| x == lca).unwrap_or(chain.len());
    chain.truncate(idx); // drop lca and above
    chain.reverse(); // [child-of-lca, ..., target]
    chain
}

fn history_child_of(def: &ChartDef, region: &str) -> Option<String> {
    def.children.get(region).and_then(|kids| {
        kids.iter()
            .find(|k| matches!(def.kind(k), Kind::History(_)))
            .cloned()
    })
}

fn record_region(
    def: &ChartDef,
    region: &str,
    hist_child: String,
    config: &BTreeSet<String>,
    history: &mut HashMap<String, Recording>,
) {
    let kind = match def.kind(&hist_child) {
        Kind::History(h) => h,
        _ => return,
    };
    match kind {
        HistoryKind::Shallow => {
            // Record the direct child of `region` that was active.
            let child = def
                .children
                .get(region)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .find(|c| config.contains(c) && !matches!(def.kind(c), Kind::History(_)));
            if let Some(c) = child {
                history.insert(hist_child, Recording::Shallow(c));
            }
        }
        HistoryKind::Deep => {
            // Record every active state strictly below `region`.
            let set: BTreeSet<String> = config
                .iter()
                .filter(|s| def.is_proper_descendant(s, region))
                .cloned()
                .collect();
            history.insert(hist_child, Recording::Deep(set));
        }
    }
}
