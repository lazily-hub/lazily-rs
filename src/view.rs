//! Host-neutral view compute document.
//!
//! A view is installed onto one [`Context`]: each signal becomes a [`Source`],
//! each `text` / `hidden` / `disabled` expression becomes a guarded [`Computed`],
//! and the optional chart becomes one [`StateChart`] cell. Semantic events run
//! their effect trees on dispatch. The conformance trace records `set` and
//! `send` only.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::context::ComputeOps;
use crate::{ChartDef, Computed, Context, Effect, Source, StateChart};

const ROLES: &[&str] = &["text", "button", "field", "list", "region"];
const FLOWS: &[&str] = &["row", "column", "stack"];
const ALIGNS: &[&str] = &["start", "center", "end", "stretch"];
const EVENTS: &[&str] = &["activate", "change", "submit"];
const EXPR_OPS: &[&str] = &["lit", "ref", "call", "if", "eq", "concat", "trim"];
const EFFECT_OPS: &[&str] = &["set", "batch", "send"];
const CALLS: &[&str] = &["event", "not", "add", "len"];

#[derive(Clone, Debug, PartialEq)]
enum ViewValue {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
}

struct NodeRec {
    role: String,
    children: Vec<String>,
    flow: Option<String>,
}

/// An installed view document.
pub struct View {
    signals: BTreeMap<String, Source<ViewValue>>,
    exprs: BTreeMap<String, Computed<ViewValue>>,
    nodes: BTreeMap<String, NodeRec>,
    root: String,
    effects: BTreeMap<String, BTreeMap<String, Value>>,
    chart: Option<StateChart>,
    /// Subscription effect. Dropping the handle does not dispose the node.
    host: Effect,
    payload: RefCell<Option<ViewValue>>,
}

impl View {
    /// Install `doc` onto `ctx`. `doc` is a `schemas/view.json` document.
    pub fn install(ctx: &Context, doc: &Value) -> Result<Self, String> {
        let obj = doc
            .as_object()
            .ok_or_else(|| "view must be a JSON object".to_string())?;
        allow(obj, &["id", "root", "signals", "nodes", "chart"], "view")?;
        let id = req_str(obj, "id", "view")?;
        if id.is_empty() {
            return Err("view id must be a non-empty string".into());
        }
        let root = req_str(obj, "root", "view")?;
        if root.is_empty() {
            return Err("view root must be a non-empty string".into());
        }

        let signals_obj = req_obj(obj, "signals", "view")?;
        let mut signal_ids = BTreeSet::new();
        let mut initials = BTreeMap::new();
        for (sid, raw) in signals_obj {
            if sid.is_empty() {
                return Err("signal id must be a non-empty string".into());
            }
            let sig = raw
                .as_object()
                .ok_or_else(|| format!("signal `{sid}` must be an object"))?;
            allow(sig, &["initial"], &format!("signal `{sid}`"))?;
            let initial = json_literal(req(sig, "initial", &format!("signal `{sid}`"))?)?;
            signal_ids.insert(sid.clone());
            initials.insert(sid.clone(), initial);
        }

        let has_chart = match obj.get("chart") {
            None => false,
            Some(v) if v.is_object() => true,
            Some(_) => return Err("chart must be a statechart document".into()),
        };
        let chart_def = if has_chart {
            Some(ChartDef::from_json(&obj["chart"])?)
        } else {
            None
        };

        let nodes_obj = req_obj(obj, "nodes", "view")?;
        if nodes_obj.is_empty() {
            return Err("view nodes must contain at least one node".into());
        }
        let mut nodes = BTreeMap::new();
        let mut effects = BTreeMap::new();
        let mut expr_docs: Vec<(String, Value)> = Vec::new();
        for (nid, raw) in nodes_obj {
            if nid.is_empty() {
                return Err("node id must be a non-empty string".into());
            }
            let node = raw
                .as_object()
                .ok_or_else(|| format!("node `{nid}` must be an object"))?;
            allow(
                node,
                &[
                    "role", "children", "layout", "text", "bind", "hidden", "disabled", "effects",
                ],
                &format!("node `{nid}`"),
            )?;
            let role = req_str(node, "role", &format!("node `{nid}`"))?;
            if !ROLES.contains(&role) {
                return Err(format!("unknown role `{role}`"));
            }
            let children = match node.get("children") {
                None => Vec::new(),
                Some(v) => parse_children(nid, v)?,
            };
            let flow = match node.get("layout") {
                None => None,
                Some(v) => parse_layout(nid, v)?,
            };
            if let Some(bind) = node.get("bind") {
                let bind = bind
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("node `{nid}` bind must be a non-empty string"))?;
                if !signal_ids.contains(bind) {
                    return Err(format!("bind names unknown signal `{bind}`"));
                }
            }
            if let Some(text) = node.get("text") {
                let expr = standing_expr(text, &signal_ids)?;
                expr_docs.push((format!("{nid}.text"), expr));
            }
            if let Some(hidden) = node.get("hidden") {
                let expr = standing_bool_or_expr("hidden", hidden, &signal_ids)?;
                expr_docs.push((format!("{nid}.hidden"), expr));
            }
            if let Some(disabled) = node.get("disabled") {
                let expr = standing_bool_or_expr("disabled", disabled, &signal_ids)?;
                expr_docs.push((format!("{nid}.disabled"), expr));
            }
            if let Some(raw_effects) = node.get("effects") {
                let map = raw_effects
                    .as_object()
                    .ok_or_else(|| format!("node `{nid}` effects must be an object"))?;
                allow(map, EVENTS, &format!("node `{nid}` effects"))?;
                let mut stored = BTreeMap::new();
                for (event, effect) in map {
                    validate_effect(effect, &signal_ids, has_chart)?;
                    stored.insert(event.clone(), effect.clone());
                }
                if !stored.is_empty() {
                    effects.insert(nid.clone(), stored);
                }
            }
            nodes.insert(
                nid.clone(),
                NodeRec {
                    role: role.to_string(),
                    children,
                    flow,
                },
            );
        }
        if !nodes.contains_key(root) {
            return Err(format!("root `{root}` is not a node"));
        }
        check_tree(root, &nodes)?;

        let mut signals = BTreeMap::new();
        for (sid, initial) in initials {
            signals.insert(sid, ctx.source(initial));
        }
        for (key, expr) in &expr_docs {
            eval_expr(&signals, None, expr, ctx).map_err(|e| format!("{key}: {e}"))?;
        }
        let mut exprs = BTreeMap::new();
        for (key, expr) in expr_docs {
            let captured = signals.clone();
            let label = key.clone();
            let handle = ctx.computed(move |compute| {
                eval_expr(&captured, None, &expr, compute)
                    .unwrap_or_else(|e| panic!("{label}: {e}"))
            });
            exprs.insert(key, handle);
        }
        let chart = chart_def.map(|def| StateChart::new(ctx, def));
        let host = subscribe(ctx, &signals, &exprs);

        Ok(Self {
            signals,
            exprs,
            nodes,
            root: root.to_string(),
            effects,
            chart,
            host,
            payload: RefCell::new(None),
        })
    }

    /// The host subscription effect. The handle does not dispose on drop.
    pub fn subscription(&self) -> Effect {
        self.host
    }

    /// Current signal values, keyed by signal id.
    pub fn signals_json(&self, ctx: &Context) -> Value {
        let mut map = Map::new();
        for (id, source) in &self.signals {
            map.insert(id.clone(), to_json(&source.get(ctx)));
        }
        Value::Object(map)
    }

    /// Installed `text` / `hidden` / `disabled` values, keyed `nodeId.field`.
    pub fn expressions_json(&self, ctx: &Context) -> Value {
        let mut map = Map::new();
        for (id, computed) in &self.exprs {
            map.insert(id.clone(), to_json(&computed.get(ctx)));
        }
        Value::Object(map)
    }

    /// Abstract layout in preorder from the root.
    pub fn layout_json(&self) -> Value {
        let mut out = Vec::new();
        walk_layout(&self.root, &self.nodes, &mut out);
        Value::Array(out)
    }

    /// Active chart leaves, sorted, or null when the document has no chart.
    pub fn chart_json(&self, ctx: &Context) -> Value {
        match &self.chart {
            None => Value::Null,
            Some(chart) => {
                let mut leaves = chart.active_leaves(ctx);
                leaves.sort();
                Value::Array(leaves.into_iter().map(Value::String).collect())
            }
        }
    }

    /// Dispatch one semantic event. Unknown nodes and unknown event names are
    /// errors. A known event the node does not handle yields an empty trace.
    pub fn dispatch_json(
        &self,
        ctx: &Context,
        node: &str,
        event: &str,
        value: &Value,
    ) -> Result<Value, String> {
        if !EVENTS.contains(&event) {
            return Err(format!("unknown event `{event}`"));
        }
        if !self.nodes.contains_key(node) {
            return Err(format!("unknown node `{node}`"));
        }
        let Some(effect) = self.effects.get(node).and_then(|map| map.get(event)) else {
            return Ok(Value::Array(Vec::new()));
        };
        let payload = json_literal(value)?;
        *self.payload.borrow_mut() = Some(payload);
        let mut trace = Vec::new();
        let result = self.exec(ctx, effect, &mut trace);
        *self.payload.borrow_mut() = None;
        result?;
        Ok(Value::Array(trace))
    }

    fn exec(&self, ctx: &Context, effect: &Value, trace: &mut Vec<Value>) -> Result<(), String> {
        let obj = effect
            .as_object()
            .ok_or_else(|| "effect must be an object".to_string())?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "effect missing op".to_string())?;
        let args = obj
            .get("args")
            .and_then(Value::as_array)
            .ok_or_else(|| "effect missing args".to_string())?;
        match op {
            "set" => {
                if args.len() != 2 {
                    return Err("set takes a signal id and a value".into());
                }
                let id = args[0]
                    .as_str()
                    .ok_or_else(|| "set signal id must be a string".to_string())?;
                let source = self
                    .signals
                    .get(id)
                    .ok_or_else(|| format!("unknown signal `{id}`"))?;
                let value = self.eval_arg(ctx, &args[1])?;
                source.set(ctx, value.clone());
                trace.push(json!({
                    "op": "set",
                    "signal": id,
                    "value": to_json(&value),
                }));
                Ok(())
            }
            "send" => {
                if args.len() != 1 && args.len() != 2 {
                    return Err("send takes an event name and an optional payload".into());
                }
                let name = args[0]
                    .as_str()
                    .ok_or_else(|| "send event must be a string".to_string())?;
                let chart = self
                    .chart
                    .as_ref()
                    .ok_or_else(|| "send requires a chart".to_string())?;
                let mut record = json!({"op": "send", "event": name});
                if args.len() == 2 {
                    record["payload"] = to_json(&self.eval_arg(ctx, &args[1])?);
                }
                let guards = HashMap::new();
                let _accepted = chart.send(ctx, name, &guards);
                trace.push(record);
                Ok(())
            }
            "batch" => {
                if args.is_empty() {
                    return Err("batch takes one or more effects".into());
                }
                for nested in args {
                    self.exec(ctx, nested, trace)?;
                }
                Ok(())
            }
            other => Err(format!("unknown effect op `{other}`")),
        }
    }

    fn eval_arg(&self, ctx: &Context, value: &Value) -> Result<ViewValue, String> {
        if value.is_object() {
            let payload = self.payload.borrow().clone();
            eval_expr(&self.signals, payload.as_ref(), value, ctx)
        } else {
            json_literal(value)
        }
    }
}

fn subscribe(
    ctx: &Context,
    signals: &BTreeMap<String, Source<ViewValue>>,
    exprs: &BTreeMap<String, Computed<ViewValue>>,
) -> Effect {
    if !exprs.is_empty() {
        let handles: Vec<Computed<ViewValue>> = exprs.values().copied().collect();
        ctx.effect(move |compute| {
            for handle in &handles {
                let _value = handle.get(compute);
            }
        })
    } else if !signals.is_empty() {
        let handles: Vec<Source<ViewValue>> = signals.values().copied().collect();
        ctx.effect(move |compute| {
            for handle in &handles {
                let _value = handle.get(compute);
            }
        })
    } else {
        ctx.effect(|_compute| {})
    }
}

fn walk_layout(id: &str, nodes: &BTreeMap<String, NodeRec>, out: &mut Vec<Value>) {
    let node = &nodes[id];
    out.push(json!({
        "id": id,
        "role": node.role,
        "children": node.children,
        "flow": node.flow,
    }));
    for child in &node.children {
        walk_layout(child, nodes, out);
    }
}

fn check_tree(root: &str, nodes: &BTreeMap<String, NodeRec>) -> Result<(), String> {
    let mut stack = HashSet::new();
    let mut done = HashSet::new();
    walk_tree(root, nodes, &mut stack, &mut done)?;
    if done.len() != nodes.len() {
        return Err("a node is not reachable from root".into());
    }
    Ok(())
}

fn walk_tree(
    id: &str,
    nodes: &BTreeMap<String, NodeRec>,
    stack: &mut HashSet<String>,
    done: &mut HashSet<String>,
) -> Result<(), String> {
    if stack.contains(id) {
        return Err(format!("children cycle at `{id}`"));
    }
    if done.contains(id) {
        return Err(format!("node `{id}` has more than one parent"));
    }
    stack.insert(id.to_string());
    for child in &nodes[id].children {
        if !nodes.contains_key(child) {
            return Err(format!("children name unknown node `{child}`"));
        }
        walk_tree(child, nodes, stack, done)?;
    }
    stack.remove(id);
    done.insert(id.to_string());
    Ok(())
}

fn parse_children(nid: &str, value: &Value) -> Result<Vec<String>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| format!("node `{nid}` children must be an array"))?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for child in arr {
        let child = child
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("node `{nid}` children must be non-empty strings"))?;
        if !seen.insert(child.to_string()) {
            return Err(format!("node `{nid}` repeats child `{child}`"));
        }
        out.push(child.to_string());
    }
    Ok(out)
}

fn parse_layout(nid: &str, value: &Value) -> Result<Option<String>, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("node `{nid}` layout must be an object"))?;
    allow(obj, &["flow", "align", "grow"], &format!("node `{nid}` layout"))?;
    let flow = match obj.get("flow") {
        None => None,
        Some(v) => {
            let flow = v
                .as_str()
                .ok_or_else(|| format!("node `{nid}` layout.flow must be a string"))?;
            if !FLOWS.contains(&flow) {
                return Err(format!("unknown flow `{flow}`"));
            }
            Some(flow.to_string())
        }
    };
    if let Some(align) = obj.get("align") {
        let align = align
            .as_str()
            .ok_or_else(|| format!("node `{nid}` layout.align must be a string"))?;
        if !ALIGNS.contains(&align) {
            return Err(format!("unknown align `{align}`"));
        }
    }
    if let Some(grow) = obj.get("grow") {
        let grow = grow
            .as_i64()
            .ok_or_else(|| format!("node `{nid}` layout.grow must be an integer"))?;
        if grow < 0 {
            return Err("layout.grow must be >= 0".into());
        }
    }
    Ok(flow)
}

fn standing_expr(value: &Value, signals: &BTreeSet<String>) -> Result<Value, String> {
    if let Some(text) = value.as_str() {
        return Ok(json!({"op": "lit", "args": [text]}));
    }
    if value.is_object() {
        validate_expr(value, signals, false)?;
        return Ok(value.clone());
    }
    Err("text must be a string or an expression".into())
}

fn standing_bool_or_expr(
    field: &str,
    value: &Value,
    signals: &BTreeSet<String>,
) -> Result<Value, String> {
    if let Some(flag) = value.as_bool() {
        return Ok(json!({"op": "lit", "args": [flag]}));
    }
    if value.is_object() {
        validate_expr(value, signals, false)?;
        return Ok(value.clone());
    }
    Err(format!("{field} must be a boolean or an expression"))
}

fn validate_expr(expr: &Value, signals: &BTreeSet<String>, in_effect: bool) -> Result<(), String> {
    let obj = expr
        .as_object()
        .ok_or_else(|| "expression must be an object".to_string())?;
    allow(obj, &["op", "args"], "expression")?;
    let op = req_str(obj, "op", "expression")?;
    if !EXPR_OPS.contains(&op) {
        return Err(format!("unknown expression op `{op}`"));
    }
    let args = req(obj, "args", "expression")?
        .as_array()
        .ok_or_else(|| "expression args must be an array".to_string())?;
    match op {
        "lit" => {
            if args.len() != 1 {
                return Err("lit takes one literal".into());
            }
            json_literal(&args[0])?;
        }
        "ref" => {
            if args.len() != 1 {
                return Err("ref takes one signal id".into());
            }
            let id = args[0]
                .as_str()
                .ok_or_else(|| "ref argument must be a string".to_string())?;
            if !signals.contains(id) {
                return Err(format!("unknown signal `{id}`"));
            }
        }
        "call" => validate_call(args, signals, in_effect)?,
        "if" => {
            if args.len() != 3 {
                return Err("if takes three arguments".into());
            }
            for arg in args {
                validate_arg(arg, signals, in_effect)?;
            }
        }
        "eq" => {
            if args.len() != 2 {
                return Err("eq takes two arguments".into());
            }
            for arg in args {
                validate_arg(arg, signals, in_effect)?;
            }
        }
        "concat" => {
            for arg in args {
                validate_arg(arg, signals, in_effect)?;
            }
        }
        "trim" => {
            if args.len() != 1 {
                return Err("trim takes one value".into());
            }
            validate_arg(&args[0], signals, in_effect)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn validate_call(args: &[Value], signals: &BTreeSet<String>, in_effect: bool) -> Result<(), String> {
    if args.is_empty() {
        return Err("call takes a name".into());
    }
    let name = args[0]
        .as_str()
        .ok_or_else(|| "call name must be a string".to_string())?;
    if !CALLS.contains(&name) {
        return Err(format!("unknown call `{name}`"));
    }
    let rest = &args[1..];
    match name {
        "event" => {
            if !rest.is_empty() {
                return Err("event takes no arguments".into());
            }
            if !in_effect {
                return Err("event is illegal outside an effect".into());
            }
        }
        "not" => {
            if rest.len() != 1 {
                return Err("not takes one boolean".into());
            }
            validate_arg(&rest[0], signals, in_effect)?;
        }
        "add" => {
            if rest.len() < 2 {
                return Err("add takes two or more integers".into());
            }
            for arg in rest {
                validate_arg(arg, signals, in_effect)?;
            }
        }
        "len" => {
            if rest.len() != 1 {
                return Err("len takes one string".into());
            }
            validate_arg(&rest[0], signals, in_effect)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn validate_arg(value: &Value, signals: &BTreeSet<String>, in_effect: bool) -> Result<(), String> {
    if value.is_object() {
        validate_expr(value, signals, in_effect)
    } else {
        json_literal(value).map(|_| ())
    }
}

fn validate_effect(effect: &Value, signals: &BTreeSet<String>, has_chart: bool) -> Result<(), String> {
    let obj = effect
        .as_object()
        .ok_or_else(|| "effect must be an object".to_string())?;
    allow(obj, &["op", "args"], "effect")?;
    let op = req_str(obj, "op", "effect")?;
    if !EFFECT_OPS.contains(&op) {
        return Err(format!("unknown effect op `{op}`"));
    }
    let args = req(obj, "args", "effect")?
        .as_array()
        .ok_or_else(|| "effect args must be an array".to_string())?;
    match op {
        "set" => {
            if args.len() != 2 {
                return Err("set takes a signal id and a value".into());
            }
            let id = args[0]
                .as_str()
                .ok_or_else(|| "set signal id must be a string".to_string())?;
            if !signals.contains(id) {
                return Err(format!("unknown signal `{id}`"));
            }
            validate_arg(&args[1], signals, true)?;
        }
        "send" => {
            if !has_chart {
                return Err("send requires a chart".into());
            }
            if args.len() != 1 && args.len() != 2 {
                return Err("send takes an event name and an optional payload".into());
            }
            if args[0].as_str().is_none() {
                return Err("send event must be a string".into());
            }
            if args.len() == 2 {
                validate_arg(&args[1], signals, true)?;
            }
        }
        "batch" => {
            if args.is_empty() {
                return Err("batch takes one or more effects".into());
            }
            for nested in args {
                validate_effect(nested, signals, has_chart)?;
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn eval_expr<C: ComputeOps>(
    signals: &BTreeMap<String, Source<ViewValue>>,
    payload: Option<&ViewValue>,
    expr: &Value,
    ctx: &C,
) -> Result<ViewValue, String> {
    let obj = expr
        .as_object()
        .ok_or_else(|| "expression must be an object".to_string())?;
    let op = obj
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "expression missing op".to_string())?;
    let args = obj
        .get("args")
        .and_then(Value::as_array)
        .ok_or_else(|| "expression missing args".to_string())?;
    match op {
        "lit" => {
            if args.len() != 1 {
                return Err("lit takes one literal".into());
            }
            json_literal(&args[0])
        }
        "ref" => {
            if args.len() != 1 {
                return Err("ref takes one signal id".into());
            }
            let id = args[0]
                .as_str()
                .ok_or_else(|| "ref argument must be a string".to_string())?;
            let source = signals
                .get(id)
                .ok_or_else(|| format!("unknown signal `{id}`"))?;
            Ok(source.get(ctx))
        }
        "call" => eval_call(signals, payload, args, ctx),
        "if" => {
            if args.len() != 3 {
                return Err("if takes three arguments".into());
            }
            match eval_arg(signals, payload, &args[0], ctx)? {
                ViewValue::Bool(true) => eval_arg(signals, payload, &args[1], ctx),
                ViewValue::Bool(false) => eval_arg(signals, payload, &args[2], ctx),
                _ => Err("if condition must be a boolean".into()),
            }
        }
        "eq" => {
            if args.len() != 2 {
                return Err("eq takes two arguments".into());
            }
            let left = eval_arg(signals, payload, &args[0], ctx)?;
            let right = eval_arg(signals, payload, &args[1], ctx)?;
            Ok(ViewValue::Bool(left == right))
        }
        "concat" => {
            let mut text = String::new();
            for arg in args {
                text.push_str(&stringify(&eval_arg(signals, payload, arg, ctx)?));
            }
            Ok(ViewValue::Str(text))
        }
        "trim" => {
            if args.len() != 1 {
                return Err("trim takes one value".into());
            }
            let text = stringify(&eval_arg(signals, payload, &args[0], ctx)?);
            Ok(ViewValue::Str(text.trim().to_string()))
        }
        other => Err(format!("unknown expression op `{other}`")),
    }
}

fn eval_call<C: ComputeOps>(
    signals: &BTreeMap<String, Source<ViewValue>>,
    payload: Option<&ViewValue>,
    args: &[Value],
    ctx: &C,
) -> Result<ViewValue, String> {
    if args.is_empty() {
        return Err("call takes a name".into());
    }
    let name = args[0]
        .as_str()
        .ok_or_else(|| "call name must be a string".to_string())?;
    let rest = &args[1..];
    match name {
        "event" => {
            if !rest.is_empty() {
                return Err("event takes no arguments".into());
            }
            payload
                .cloned()
                .ok_or_else(|| "event is illegal outside an effect".to_string())
        }
        "not" => {
            if rest.len() != 1 {
                return Err("not takes one boolean".into());
            }
            match eval_arg(signals, payload, &rest[0], ctx)? {
                ViewValue::Bool(flag) => Ok(ViewValue::Bool(!flag)),
                _ => Err("not expects a boolean".into()),
            }
        }
        "add" => {
            if rest.len() < 2 {
                return Err("add takes two or more integers".into());
            }
            let mut sum: i64 = 0;
            for arg in rest {
                match eval_arg(signals, payload, arg, ctx)? {
                    ViewValue::Int(n) => {
                        sum = sum
                            .checked_add(n)
                            .ok_or_else(|| "add overflow".to_string())?;
                    }
                    _ => return Err("add expects integers".into()),
                }
            }
            Ok(ViewValue::Int(sum))
        }
        "len" => {
            if rest.len() != 1 {
                return Err("len takes one string".into());
            }
            match eval_arg(signals, payload, &rest[0], ctx)? {
                ViewValue::Str(text) => Ok(ViewValue::Int(text.chars().count() as i64)),
                _ => Err("len expects a string".into()),
            }
        }
        other => Err(format!("unknown call `{other}`")),
    }
}

fn eval_arg<C: ComputeOps>(
    signals: &BTreeMap<String, Source<ViewValue>>,
    payload: Option<&ViewValue>,
    value: &Value,
    ctx: &C,
) -> Result<ViewValue, String> {
    if value.is_object() {
        eval_expr(signals, payload, value, ctx)
    } else {
        json_literal(value)
    }
}

fn json_literal(value: &Value) -> Result<ViewValue, String> {
    match value {
        Value::Null => Ok(ViewValue::Null),
        Value::Bool(flag) => Ok(ViewValue::Bool(*flag)),
        Value::Number(number) => {
            let int = number
                .as_i64()
                .ok_or_else(|| format!("non-integer number {number}"))?;
            Ok(ViewValue::Int(int))
        }
        Value::String(text) => Ok(ViewValue::Str(text.clone())),
        _ => Err("value must be null, boolean, integer, or string".into()),
    }
}

fn to_json(value: &ViewValue) -> Value {
    match value {
        ViewValue::Null => Value::Null,
        ViewValue::Bool(flag) => Value::Bool(*flag),
        ViewValue::Int(int) => Value::Number((*int).into()),
        ViewValue::Str(text) => Value::String(text.clone()),
    }
}

fn stringify(value: &ViewValue) -> String {
    match value {
        ViewValue::Null => String::new(),
        ViewValue::Bool(true) => "true".into(),
        ViewValue::Bool(false) => "false".into(),
        ViewValue::Int(int) => int.to_string(),
        ViewValue::Str(text) => text.clone(),
    }
}

fn allow(obj: &Map<String, Value>, keys: &[&str], what: &str) -> Result<(), String> {
    for key in obj.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(format!("{what} has unknown field `{key}`"));
        }
    }
    Ok(())
}

fn req<'a>(obj: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a Value, String> {
    obj.get(key)
        .ok_or_else(|| format!("{what} missing `{key}`"))
}

fn req_str<'a>(obj: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a str, String> {
    req(obj, key, what)?
        .as_str()
        .ok_or_else(|| format!("{what} `{key}` must be a string"))
}

fn req_obj<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    what: &str,
) -> Result<&'a Map<String, Value>, String> {
    req(obj, key, what)?
        .as_object()
        .ok_or_else(|| format!("{what} `{key}` must be an object"))
}
