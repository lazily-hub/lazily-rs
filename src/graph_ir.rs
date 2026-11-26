//! Graph IR reference interpreter.
//!
//! A definition document ([`lazily-spec` graph IR](https://lazily.dev/schemas/graph-ir.json))
//! parses into an expression tree, lowers onto one [`Context`], and can emit
//! `canonical-source`. Edits add, replace, and remove nodes while the runtime
//! holds the document. This is compute, not a [`Snapshot`](crate::ipc::Snapshot)
//! or Delta.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use serde_json::{Map, Number, Value};

use crate::cell::{Computed, Source};
use crate::merge::MergePolicy;
use crate::{Compute, Context, Effect};

const IR_VERSION: &str = "1";

/// A parsed graph definition. The tree is the document, not a live cell.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphIr {
    id: Option<String>,
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
}

/// One add, replace, or remove, applied in order against a held document.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdit {
    ops: Vec<EditOp>,
}

/// A node in declaration order.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Source(SourceNode),
    Computed(ComputedNode),
    Effect(EffectNode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceNode {
    pub id: String,
    pub init: Value,
    pub policy: Policy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedNode {
    pub id: String,
    pub expr: Expr,
    pub eager: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectNode {
    pub id: String,
    pub body: Stmt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    KeepLatest,
    Sum,
    Max,
    Min,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Value),
    Ref(String),
    Call { op: Op, args: Vec<Expr> },
    Object(Vec<ObjectField>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectField {
    pub key: String,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Neg,
    Not,
    Len,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Min,
    Max,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Get,
    And,
    Or,
    If,
    Array,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Eval(Expr),
    Set {
        target: String,
        expr: Expr,
    },
    Merge {
        target: String,
        expr: Expr,
    },
    Seq(Vec<Stmt>),
    When {
        cond: Expr,
        then: Box<Stmt>,
        else_: Option<Box<Stmt>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    Add(Node),
    Replace(Node),
    Remove(String),
}

/// A derived edge. `from` reads or writes `to`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub kind: EdgeKind,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    Read,
    Write,
}

/// One successful effect run. Removal does not erase history.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRun {
    pub id: String,
    pub result: Value,
}

/// Document plus the context it was lowered onto.
pub struct GraphRuntime {
    cx: Context,
    doc: GraphIr,
    inner: Rc<Inner>,
}

struct Inner {
    bindings: RefCell<HashMap<String, Binding>>,
    frames: RefCell<Vec<BTreeSet<String>>>,
    observed: RefCell<HashMap<String, BTreeSet<String>>>,
    trace: RefCell<Vec<EffectRun>>,
    fault: RefCell<Option<String>>,
}

#[derive(Clone, Copy)]
enum Binding {
    Keep(Source<Value>),
    Sum(Source<Value, JsonSum>),
    Max(Source<Value, JsonMax>),
    Min(Source<Value, JsonMin>),
    And(Source<Value, JsonAnd>),
    Or(Source<Value, JsonOr>),
    Computed(Computed<Value>),
    Effect(Effect),
}

struct JsonSum;
struct JsonMax;
struct JsonMin;
struct JsonAnd;
struct JsonOr;

struct FrameGuard<'a> {
    inner: &'a Inner,
    id: String,
}

impl GraphIr {
    pub fn from_json_str(text: &str) -> Result<Self, String> {
        let value = parse_json(text)?;
        Self::from_json(&value)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let object = as_object(value, "graph document")?;
        allow(object, "graph document", &["kind", "ir", "id", "nodes"])?;
        let kind = require_str(object, "kind", "graph document")?;
        if kind != "graph" {
            return Err(format!(
                "graph document kind must be \"graph\", got `{kind}`"
            ));
        }
        require_version(object, "graph document")?;
        let id = optional_id(object, "id", "graph id")?;
        let raw_nodes = require(object, "nodes", "graph document")?;
        let Some(raw_nodes) = raw_nodes.as_array() else {
            return Err("graph nodes must be an array".into());
        };
        let mut nodes = Vec::with_capacity(raw_nodes.len());
        let mut index = HashMap::new();
        for raw in raw_nodes {
            let node = parse_node(raw)?;
            if index.insert(node.id().to_string(), nodes.len()).is_some() {
                return Err(format!("duplicate node id `{}`", node.id()));
            }
            nodes.push(node);
        }
        let document = Self { id, nodes, index };
        document.check()?;
        Ok(document)
    }

    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("kind".into(), Value::String("graph".into()));
        object.insert("ir".into(), Value::String(IR_VERSION.into()));
        if let Some(id) = &self.id {
            object.insert("id".into(), Value::String(id.clone()));
        }
        object.insert(
            "nodes".into(),
            Value::Array(self.nodes.iter().map(Node::to_json).collect()),
        );
        Value::Object(object)
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.index.get(id).map(|index| &self.nodes[*index])
    }

    /// Static read and write edges, sorted by `(from, kind, to)`.
    pub fn static_edges(&self) -> Vec<Edge> {
        let mut edges = BTreeSet::new();
        for node in &self.nodes {
            let (reads, writes) = node.edge_sets();
            for to in reads {
                edges.insert(Edge {
                    from: node.id().to_string(),
                    kind: EdgeKind::Read,
                    to,
                });
            }
            for to in writes {
                edges.insert(Edge {
                    from: node.id().to_string(),
                    kind: EdgeKind::Write,
                    to,
                });
            }
        }
        edges.into_iter().collect()
    }

    /// Emit the `canonical-source` projection of this document.
    pub fn emit_source(&self) -> String {
        let mut out = match &self.id {
            Some(id) => format!("graph {id} ir {IR_VERSION}\n"),
            None => format!("graph ir {IR_VERSION}\n"),
        };
        for node in &self.nodes {
            out.push_str(&node.emit());
            out.push('\n');
        }
        out
    }

    pub fn check(&self) -> Result<(), String> {
        self.check_refs()?;
        self.check_inits()?;
        self.check_cycles()
    }

    fn check_refs(&self) -> Result<(), String> {
        for node in &self.nodes {
            let (reads, writes) = node.edge_sets();
            for to in reads {
                match self.node(&to) {
                    Some(Node::Source(_) | Node::Computed(_)) => {}
                    Some(Node::Effect(_)) => {
                        return Err(format!(
                            "node `{}` reads effect `{to}`, which has no value",
                            node.id()
                        ));
                    }
                    None => {
                        return Err(format!("node `{}` reads unknown node `{to}`", node.id()));
                    }
                }
            }
            for to in writes {
                match self.node(&to) {
                    Some(Node::Source(_)) => {}
                    Some(_) => {
                        return Err(format!("node `{}` writes non-source `{to}`", node.id()));
                    }
                    None => {
                        return Err(format!("node `{}` writes unknown node `{to}`", node.id()));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_inits(&self) -> Result<(), String> {
        for node in &self.nodes {
            if let Node::Source(source) = node {
                source
                    .policy
                    .check_init(&source.init)
                    .map_err(|error| format!("source `{}`: {error}", source.id))?;
            }
        }
        Ok(())
    }

    fn check_cycles(&self) -> Result<(), String> {
        let mut color = HashMap::new();
        let mut stack = Vec::new();
        for node in &self.nodes {
            self.visit(node.id(), &mut color, &mut stack)?;
        }
        Ok(())
    }

    fn visit<'a>(
        &'a self,
        id: &'a str,
        color: &mut HashMap<&'a str, u8>,
        stack: &mut Vec<&'a str>,
    ) -> Result<(), String> {
        match color.get(id).copied().unwrap_or(0) {
            1 => {
                let start = stack.iter().position(|item| *item == id).unwrap_or(0);
                let mut cycle: Vec<&str> = stack[start..].to_vec();
                cycle.push(id);
                return Err(format!("cycle in read edges: {}", cycle.join(" -> ")));
            }
            2 => return Ok(()),
            _ => {}
        }
        color.insert(id, 1);
        stack.push(id);
        let deps = self.read_ids(id);
        for dep in &deps {
            if let Some(dep_id) = self.node(dep).map(Node::id) {
                self.visit(dep_id, color, stack)?;
            }
        }
        stack.pop();
        color.insert(id, 2);
        Ok(())
    }

    fn read_ids(&self, id: &str) -> Vec<String> {
        self.node(id)
            .map(|node| node.edge_sets().0.into_iter().collect())
            .unwrap_or_default()
    }

    fn apply_edit_op(&mut self, op: &EditOp) -> Result<(), String> {
        match op {
            EditOp::Add(node) => {
                if self.index.contains_key(node.id()) {
                    return Err(format!(
                        "add requires a new id, `{}` already exists",
                        node.id()
                    ));
                }
                self.nodes.push(node.clone());
            }
            EditOp::Replace(node) => {
                let Some(&slot) = self.index.get(node.id()) else {
                    return Err(format!("replace requires an existing node `{}`", node.id()));
                };
                self.nodes[slot] = node.clone();
            }
            EditOp::Remove(id) => {
                let Some(&slot) = self.index.get(id) else {
                    return Err(format!("remove requires an existing node `{id}`"));
                };
                self.nodes.remove(slot);
            }
        }
        self.reindex()?;
        self.check()
    }

    fn reindex(&mut self) -> Result<(), String> {
        self.index.clear();
        for (slot, node) in self.nodes.iter().enumerate() {
            if self.index.insert(node.id().to_string(), slot).is_some() {
                return Err(format!("duplicate node id `{}`", node.id()));
            }
        }
        Ok(())
    }
}

impl GraphEdit {
    pub fn from_json_str(text: &str) -> Result<Self, String> {
        let value = parse_json(text)?;
        Self::from_json(&value)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let object = as_object(value, "graph edit")?;
        allow(object, "graph edit", &["kind", "ir", "ops"])?;
        let kind = require_str(object, "kind", "graph edit")?;
        if kind != "edit" {
            return Err(format!("graph edit kind must be \"edit\", got `{kind}`"));
        }
        require_version(object, "graph edit")?;
        let raw_ops = require(object, "ops", "graph edit")?;
        let Some(raw_ops) = raw_ops.as_array() else {
            return Err("graph edit ops must be an array".into());
        };
        if raw_ops.is_empty() {
            return Err("graph edit ops must not be empty".into());
        }
        let mut ops = Vec::with_capacity(raw_ops.len());
        for raw in raw_ops {
            ops.push(parse_edit_op(raw)?);
        }
        Ok(Self { ops })
    }

    pub fn ops(&self) -> &[EditOp] {
        &self.ops
    }
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Source(node) => &node.id,
            Node::Computed(node) => &node.id,
            Node::Effect(node) => &node.id,
        }
    }

    fn edge_sets(&self) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut reads = BTreeSet::new();
        let mut writes = BTreeSet::new();
        match self {
            Node::Source(_) => {}
            Node::Computed(node) => collect_expr(&node.expr, &mut reads),
            Node::Effect(node) => collect_stmt(&node.body, &mut reads, &mut writes),
        }
        (reads, writes)
    }

    fn to_json(&self) -> Value {
        match self {
            Node::Source(node) => node.to_json(),
            Node::Computed(node) => node.to_json(),
            Node::Effect(node) => node.to_json(),
        }
    }

    fn emit(&self) -> String {
        match self {
            Node::Source(node) => {
                let mut line = format!("source {} = {}", node.id, json_text(&node.init));
                if node.policy != Policy::KeepLatest {
                    line.push_str(" policy ");
                    line.push_str(node.policy.as_str());
                }
                line
            }
            Node::Computed(node) => {
                if node.eager {
                    format!("computed {} eager = {}", node.id, emit_expr(&node.expr))
                } else {
                    format!("computed {} = {}", node.id, emit_expr(&node.expr))
                }
            }
            Node::Effect(node) => format!("effect {} = {}", node.id, emit_stmt(&node.body)),
        }
    }
}

impl SourceNode {
    fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("id".into(), Value::String(self.id.clone()));
        object.insert("kind".into(), Value::String("source".into()));
        object.insert("init".into(), self.init.clone());
        if self.policy != Policy::KeepLatest {
            object.insert("policy".into(), Value::String(self.policy.as_str().into()));
        }
        Value::Object(object)
    }
}

impl ComputedNode {
    fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("id".into(), Value::String(self.id.clone()));
        object.insert("kind".into(), Value::String("computed".into()));
        if self.eager {
            object.insert("eager".into(), Value::Bool(true));
        }
        object.insert("expr".into(), expr_json(&self.expr));
        Value::Object(object)
    }
}

impl EffectNode {
    fn to_json(&self) -> Value {
        obj(&[
            ("id", Value::String(self.id.clone())),
            ("kind", Value::String("effect".into())),
            ("body", stmt_json(&self.body)),
        ])
    }
}

impl Policy {
    pub fn as_str(self) -> &'static str {
        match self {
            Policy::KeepLatest => "keep-latest",
            Policy::Sum => "sum",
            Policy::Max => "max",
            Policy::Min => "min",
            Policy::And => "and",
            Policy::Or => "or",
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "keep-latest" => Ok(Policy::KeepLatest),
            "sum" => Ok(Policy::Sum),
            "max" => Ok(Policy::Max),
            "min" => Ok(Policy::Min),
            "and" => Ok(Policy::And),
            "or" => Ok(Policy::Or),
            other => Err(format!("unknown source policy `{other}`")),
        }
    }

    fn check_init(self, init: &Value) -> Result<(), String> {
        match self {
            Policy::KeepLatest => Ok(()),
            Policy::Sum | Policy::Max | Policy::Min => as_num(init).map(|_| ()),
            Policy::And | Policy::Or => expect_bool(init).map(|_| ()),
        }
    }
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Neg => "neg",
            Op::Not => "not",
            Op::Len => "len",
            Op::Add => "add",
            Op::Sub => "sub",
            Op::Mul => "mul",
            Op::Div => "div",
            Op::Mod => "mod",
            Op::Min => "min",
            Op::Max => "max",
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Lt => "lt",
            Op::Le => "le",
            Op::Gt => "gt",
            Op::Ge => "ge",
            Op::Concat => "concat",
            Op::Get => "get",
            Op::And => "and",
            Op::Or => "or",
            Op::If => "if",
            Op::Array => "array",
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "neg" => Ok(Op::Neg),
            "not" => Ok(Op::Not),
            "len" => Ok(Op::Len),
            "add" => Ok(Op::Add),
            "sub" => Ok(Op::Sub),
            "mul" => Ok(Op::Mul),
            "div" => Ok(Op::Div),
            "mod" => Ok(Op::Mod),
            "min" => Ok(Op::Min),
            "max" => Ok(Op::Max),
            "eq" => Ok(Op::Eq),
            "ne" => Ok(Op::Ne),
            "lt" => Ok(Op::Lt),
            "le" => Ok(Op::Le),
            "gt" => Ok(Op::Gt),
            "ge" => Ok(Op::Ge),
            "concat" => Ok(Op::Concat),
            "get" => Ok(Op::Get),
            "and" => Ok(Op::And),
            "or" => Ok(Op::Or),
            "if" => Ok(Op::If),
            "array" => Ok(Op::Array),
            other => Err(format!("unknown op `{other}`")),
        }
    }

    fn check_arity(self, len: usize) -> Result<(), String> {
        let ok = match self {
            Op::Neg | Op::Not | Op::Len => len == 1,
            Op::If => len == 3,
            Op::Array => true,
            _ => len == 2,
        };
        if ok {
            Ok(())
        } else {
            Err(format!(
                "op `{}` has the wrong number of arguments",
                self.as_str()
            ))
        }
    }
}

impl GraphRuntime {
    pub fn load_json(text: &str) -> Result<Self, String> {
        Self::lower(GraphIr::from_json_str(text)?)
    }

    pub fn lower(doc: GraphIr) -> Result<Self, String> {
        doc.check()?;
        let mut runtime = Self::bare(doc);
        runtime.install_document()?;
        Ok(runtime)
    }

    pub fn document(&self) -> &GraphIr {
        &self.doc
    }

    pub fn contains(&self, id: &str) -> bool {
        self.doc.node(id).is_some()
    }

    pub fn is_eager(&self, id: &str) -> Result<bool, String> {
        match self.doc.node(id) {
            Some(Node::Computed(node)) => Ok(node.eager),
            Some(_) => Err(format!("`{id}` is not a computed")),
            None => Err(format!("unknown node `{id}`")),
        }
    }

    pub fn source_policy(&self, id: &str) -> Result<Policy, String> {
        match self.doc.node(id) {
            Some(Node::Source(node)) => Ok(node.policy),
            Some(_) => Err(format!("`{id}` is not a source")),
            None => Err(format!("unknown node `{id}`")),
        }
    }

    pub fn static_edges(&self) -> Vec<Edge> {
        self.doc.static_edges()
    }

    pub fn emit_source(&self) -> String {
        self.doc.emit_source()
    }

    pub fn observed_reads(&self, id: &str) -> BTreeSet<String> {
        self.inner
            .observed
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn effect_trace(&self) -> Vec<EffectRun> {
        self.inner.trace.borrow().clone()
    }

    pub fn get(&self, id: &str) -> Result<Value, String> {
        let Some(binding) = self.lookup(id) else {
            return Err(format!("unknown node `{id}`"));
        };
        match binding {
            Binding::Effect(_) => Err(format!("`{id}` is an effect and has no value")),
            Binding::Computed(computed) => catch_fault(|| computed.get(&self.cx)),
            other => other.read_untracked(&self.cx),
        }
    }

    /// Replace a source's live value. This is a host input, not a document edit:
    /// `init` and [`Self::emit_source`] stay as declared.
    pub fn write_source(&self, id: &str, value: Value) -> Result<(), String> {
        let Some(binding) = self.lookup(id) else {
            return Err(format!("unknown node `{id}`"));
        };
        let wrote = catch_fault(|| binding.set(id, &self.cx, value));
        if let Some(error) = self.inner.fault.borrow_mut().take() {
            return Err(error);
        }
        wrote?
    }

    pub fn apply(&mut self, edit: &GraphEdit) -> Result<(), String> {
        for (index, op) in edit.ops.iter().enumerate() {
            if let Err(error) = self.apply_op(op) {
                return Err(format!("edit op {index}: {error}"));
            }
        }
        Ok(())
    }

    fn bare(doc: GraphIr) -> Self {
        Self {
            cx: Context::new(),
            doc,
            inner: Rc::new(Inner {
                bindings: RefCell::new(HashMap::new()),
                frames: RefCell::new(Vec::new()),
                observed: RefCell::new(HashMap::new()),
                trace: RefCell::new(Vec::new()),
                fault: RefCell::new(None),
            }),
        }
    }

    fn lookup(&self, id: &str) -> Option<Binding> {
        self.inner.bindings.borrow().get(id).copied()
    }

    fn install_document(&mut self) -> Result<(), String> {
        let nodes = self.doc.nodes.clone();
        for node in &nodes {
            if let Node::Source(source) = node {
                self.insert_source(source)?;
            }
        }
        for node in &nodes {
            if let Node::Computed(computed) = node {
                self.insert_computed(computed);
            }
        }
        for node in &nodes {
            if let Node::Computed(computed) = node
                && computed.eager
            {
                self.materialize_existing(&computed.id)?;
            }
        }
        for node in &nodes {
            if let Node::Effect(effect) = node {
                self.insert_effect(effect)?;
            }
        }
        Ok(())
    }

    fn apply_op(&mut self, op: &EditOp) -> Result<(), String> {
        let mut next = self.doc.clone();
        next.apply_edit_op(op)?;
        self.install_op(op)?;
        self.doc = next;
        Ok(())
    }

    fn install_op(&mut self, op: &EditOp) -> Result<(), String> {
        match op {
            EditOp::Add(node) => self.add_node(node),
            EditOp::Replace(node) => self.replace_node(node),
            EditOp::Remove(id) => self.remove_node(id),
        }
    }

    fn add_node(&mut self, node: &Node) -> Result<(), String> {
        match node {
            Node::Source(source) => self.insert_source(source),
            Node::Computed(computed) => self.add_computed(computed),
            Node::Effect(effect) => self.insert_effect(effect),
        }
    }

    fn replace_node(&mut self, node: &Node) -> Result<(), String> {
        match node {
            Node::Source(source) => self.replace_source(source),
            Node::Computed(computed) => self.replace_computed(computed),
            Node::Effect(effect) => self.replace_effect(effect),
        }
    }

    fn remove_node(&mut self, id: &str) -> Result<(), String> {
        let Some(binding) = self.inner.bindings.borrow_mut().remove(id) else {
            return Err(format!("unknown node `{id}`"));
        };
        self.dispose_binding(binding);
        Ok(())
    }

    fn insert_source(&mut self, node: &SourceNode) -> Result<(), String> {
        let binding = self.create_source(node)?;
        self.inner
            .bindings
            .borrow_mut()
            .insert(node.id.clone(), binding);
        Ok(())
    }

    fn replace_source(&mut self, node: &SourceNode) -> Result<(), String> {
        let binding = self.create_source(node)?;
        let old = self
            .inner
            .bindings
            .borrow_mut()
            .insert(node.id.clone(), binding);
        if let Some(old) = old {
            self.dispose_binding(old);
        }
        Ok(())
    }

    fn create_source(&self, node: &SourceNode) -> Result<Binding, String> {
        node.policy.check_init(&node.init)?;
        let init = node.init.clone();
        Ok(match node.policy {
            Policy::KeepLatest => Binding::Keep(self.cx.source(init)),
            Policy::Sum => Binding::Sum(self.cx.source_with::<JsonSum, Value>(init)),
            Policy::Max => Binding::Max(self.cx.source_with::<JsonMax, Value>(init)),
            Policy::Min => Binding::Min(self.cx.source_with::<JsonMin, Value>(init)),
            Policy::And => Binding::And(self.cx.source_with::<JsonAnd, Value>(init)),
            Policy::Or => Binding::Or(self.cx.source_with::<JsonOr, Value>(init)),
        })
    }

    fn insert_computed(&mut self, node: &ComputedNode) {
        let computed = self.make_computed(node);
        self.inner
            .bindings
            .borrow_mut()
            .insert(node.id.clone(), Binding::Computed(computed));
    }

    fn add_computed(&mut self, node: &ComputedNode) -> Result<(), String> {
        self.insert_computed(node);
        if node.eager
            && let Err(error) = self.materialize_existing(&node.id)
        {
            self.rollback_insert(&node.id);
            return Err(error);
        }
        Ok(())
    }

    fn replace_computed(&mut self, node: &ComputedNode) -> Result<(), String> {
        let computed = self.make_computed(node);
        if node.eager
            && let Err(error) = self.materialize_handle(computed)
        {
            computed.dispose(&self.cx);
            return Err(error);
        }
        let old = self
            .inner
            .bindings
            .borrow_mut()
            .insert(node.id.clone(), Binding::Computed(computed));
        if let Some(old) = old {
            self.dispose_binding(old);
        }
        Ok(())
    }

    fn make_computed(&self, node: &ComputedNode) -> Computed<Value> {
        let inner = Rc::clone(&self.inner);
        let id = node.id.clone();
        let expr = node.expr.clone();
        self.cx.computed(move |cx| {
            let _guard = FrameGuard::enter(&inner, &id);
            match eval_expr(&inner, &expr, cx) {
                Ok(value) => value,
                Err(error) => raise(error),
            }
        })
    }

    fn materialize_existing(&self, id: &str) -> Result<(), String> {
        let Some(Binding::Computed(computed)) = self.lookup(id) else {
            return Err(format!("`{id}` is not a computed"));
        };
        self.materialize_handle(computed)
    }

    fn materialize_handle(&self, computed: Computed<Value>) -> Result<(), String> {
        catch_fault(|| {
            let _ = computed.get(&self.cx);
        })?;
        catch_fault(|| {
            computed.eager(&self.cx);
        })
    }

    fn insert_effect(&mut self, node: &EffectNode) -> Result<(), String> {
        let effect = self.spawn_effect(node)?;
        self.inner
            .bindings
            .borrow_mut()
            .insert(node.id.clone(), Binding::Effect(effect));
        Ok(())
    }

    fn replace_effect(&mut self, node: &EffectNode) -> Result<(), String> {
        let effect = self.spawn_effect(node)?;
        let old = self
            .inner
            .bindings
            .borrow_mut()
            .insert(node.id.clone(), Binding::Effect(effect));
        if let Some(old) = old {
            self.dispose_binding(old);
        }
        Ok(())
    }

    fn spawn_effect(&self, node: &EffectNode) -> Result<Effect, String> {
        self.inner.fault.borrow_mut().take();
        let inner = Rc::clone(&self.inner);
        let id = node.id.clone();
        let body = node.body.clone();
        let effect = self.cx.effect(move |cx| {
            let _guard = FrameGuard::enter(&inner, &id);
            match eval_stmt(&inner, &body, cx) {
                Ok(value) => inner.trace.borrow_mut().push(EffectRun {
                    id: id.clone(),
                    result: value,
                }),
                Err(error) => {
                    inner.fault.borrow_mut().replace(error);
                }
            }
        });
        if let Some(error) = self.inner.fault.borrow_mut().take() {
            effect.dispose(&self.cx);
            return Err(error);
        }
        Ok(effect)
    }

    fn rollback_insert(&mut self, id: &str) {
        if let Some(binding) = self.inner.bindings.borrow_mut().remove(id) {
            self.dispose_binding(binding);
        }
    }

    fn dispose_binding(&self, binding: Binding) {
        match binding {
            Binding::Keep(source) => source.dispose(&self.cx),
            Binding::Sum(source) => source.dispose(&self.cx),
            Binding::Max(source) => source.dispose(&self.cx),
            Binding::Min(source) => source.dispose(&self.cx),
            Binding::And(source) => source.dispose(&self.cx),
            Binding::Or(source) => source.dispose(&self.cx),
            Binding::Computed(computed) => computed.dispose(&self.cx),
            Binding::Effect(effect) => effect.dispose(&self.cx),
        }
    }
}

impl Binding {
    fn read(self, cx: &Compute) -> Result<Value, String> {
        Ok(match self {
            Binding::Keep(source) => source.get(cx),
            Binding::Sum(source) => source.get(cx),
            Binding::Max(source) => source.get(cx),
            Binding::Min(source) => source.get(cx),
            Binding::And(source) => source.get(cx),
            Binding::Or(source) => source.get(cx),
            Binding::Computed(computed) => computed.get(cx),
            Binding::Effect(_) => return Err("effect has no value".into()),
        })
    }

    fn read_untracked(self, cx: &Context) -> Result<Value, String> {
        Ok(match self {
            Binding::Keep(source) => source.get(cx),
            Binding::Sum(source) => source.get(cx),
            Binding::Max(source) => source.get(cx),
            Binding::Min(source) => source.get(cx),
            Binding::And(source) => source.get(cx),
            Binding::Or(source) => source.get(cx),
            Binding::Computed(computed) => computed.get(cx),
            Binding::Effect(_) => return Err("effect has no value".into()),
        })
    }

    fn set(self, id: &str, cx: &Context, value: Value) -> Result<(), String> {
        match self {
            Binding::Keep(source) => source.set(cx, value),
            Binding::Sum(source) => source.set(cx, value),
            Binding::Max(source) => source.set(cx, value),
            Binding::Min(source) => source.set(cx, value),
            Binding::And(source) => source.set(cx, value),
            Binding::Or(source) => source.set(cx, value),
            _ => return Err(format!("`{id}` is not a source")),
        }
        Ok(())
    }

    fn merge(self, id: &str, cx: &Context, value: Value) -> Result<Value, String> {
        match self {
            Binding::Keep(source) => {
                source.merge(cx, value);
                Ok(source.get(cx))
            }
            Binding::Sum(source) => {
                as_num(&value)?;
                source.merge(cx, value);
                Ok(source.get(cx))
            }
            Binding::Max(_) | Binding::Min(_) => {
                as_num(&value)?;
                self.merge_ordered(cx, value)
            }
            Binding::And(source) => {
                expect_bool(&value)?;
                source.merge(cx, value);
                Ok(source.get(cx))
            }
            Binding::Or(source) => {
                expect_bool(&value)?;
                source.merge(cx, value);
                Ok(source.get(cx))
            }
            _ => Err(format!("`{id}` is not a source")),
        }
    }

    fn merge_ordered(self, cx: &Context, value: Value) -> Result<Value, String> {
        match self {
            Binding::Max(source) => {
                source.merge(cx, value);
                Ok(source.get(cx))
            }
            Binding::Min(source) => {
                source.merge(cx, value);
                Ok(source.get(cx))
            }
            _ => Err("ordered merge on a non-ordered source".into()),
        }
    }
}

impl<'a> FrameGuard<'a> {
    fn enter(inner: &'a Inner, id: &str) -> Self {
        inner.frames.borrow_mut().push(BTreeSet::new());
        Self {
            inner,
            id: id.to_string(),
        }
    }
}

impl Drop for FrameGuard<'_> {
    fn drop(&mut self) {
        let frame = self.inner.frames.borrow_mut().pop();
        if let Some(frame) = frame {
            self.inner
                .observed
                .borrow_mut()
                .insert(self.id.clone(), frame);
        }
    }
}

fn eval_expr(inner: &Inner, expr: &Expr, cx: &Compute) -> Result<Value, String> {
    match expr {
        Expr::Lit(value) => Ok(value.clone()),
        Expr::Ref(id) => read_ref(inner, id, cx),
        Expr::Call { op, args } => eval_call(inner, *op, args, cx),
        Expr::Object(fields) => {
            let mut object = Map::new();
            for field in fields {
                object.insert(field.key.clone(), eval_expr(inner, &field.value, cx)?);
            }
            Ok(Value::Object(object))
        }
    }
}

fn read_ref(inner: &Inner, id: &str, cx: &Compute) -> Result<Value, String> {
    note_read(inner, id)?;
    let binding = inner
        .bindings
        .borrow()
        .get(id)
        .copied()
        .ok_or_else(|| format!("unknown node `{id}`"))?;
    if matches!(binding, Binding::Effect(_)) {
        return Err(format!("`{id}` is an effect and has no value"));
    }
    binding.read(cx)
}

fn note_read(inner: &Inner, id: &str) -> Result<(), String> {
    let mut frames = inner.frames.borrow_mut();
    let Some(frame) = frames.last_mut() else {
        return Err("read outside a node evaluation".into());
    };
    frame.insert(id.to_string());
    Ok(())
}

fn eval_call(inner: &Inner, op: Op, args: &[Expr], cx: &Compute) -> Result<Value, String> {
    match op {
        Op::And => eval_and(inner, args, cx),
        Op::Or => eval_or(inner, args, cx),
        Op::If => eval_if(inner, args, cx),
        Op::Array => {
            let mut items = Vec::with_capacity(args.len());
            for arg in args {
                items.push(eval_expr(inner, arg, cx)?);
            }
            Ok(Value::Array(items))
        }
        other => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_expr(inner, arg, cx)?);
            }
            eval_values(other, &values)
        }
    }
}

fn eval_and(inner: &Inner, args: &[Expr], cx: &Compute) -> Result<Value, String> {
    let (left, right) = pair(args, "and")?;
    let left = expect_bool(&eval_expr(inner, left, cx)?)?;
    if !left {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(expect_bool(&eval_expr(inner, right, cx)?)?))
}

fn eval_or(inner: &Inner, args: &[Expr], cx: &Compute) -> Result<Value, String> {
    let (left, right) = pair(args, "or")?;
    let left = expect_bool(&eval_expr(inner, left, cx)?)?;
    if left {
        return Ok(Value::Bool(true));
    }
    Ok(Value::Bool(expect_bool(&eval_expr(inner, right, cx)?)?))
}

fn eval_if(inner: &Inner, args: &[Expr], cx: &Compute) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("op `if` has the wrong number of arguments".into());
    }
    let cond = expect_bool(&eval_expr(inner, &args[0], cx)?)?;
    if cond {
        eval_expr(inner, &args[1], cx)
    } else {
        eval_expr(inner, &args[2], cx)
    }
}

fn eval_values(op: Op, values: &[Value]) -> Result<Value, String> {
    match op {
        Op::Neg => eval_neg(one(values, "neg")?),
        Op::Not => Ok(Value::Bool(!expect_bool(one(values, "not")?)?)),
        Op::Len => len_value(one(values, "len")?),
        Op::Add => num_bin(NumOp::Add, pair_values(values, "add")?),
        Op::Sub => num_bin(NumOp::Sub, pair_values(values, "sub")?),
        Op::Mul => num_bin(NumOp::Mul, pair_values(values, "mul")?),
        Op::Div => num_bin(NumOp::Div, pair_values(values, "div")?),
        Op::Mod => eval_mod(pair_values(values, "mod")?),
        Op::Min => eval_minmax(false, pair_owned(values, "min")?),
        Op::Max => eval_minmax(true, pair_owned(values, "max")?),
        Op::Eq => {
            let (left, right) = pair_values(values, "eq")?;
            Ok(Value::Bool(left == right))
        }
        Op::Ne => {
            let (left, right) = pair_values(values, "ne")?;
            Ok(Value::Bool(left != right))
        }
        Op::Lt => cmp_bool(values, "lt", std::cmp::Ordering::Less, false),
        Op::Le => cmp_bool(values, "le", std::cmp::Ordering::Greater, true),
        Op::Gt => cmp_bool(values, "gt", std::cmp::Ordering::Greater, false),
        Op::Ge => cmp_bool(values, "ge", std::cmp::Ordering::Less, true),
        Op::Concat => {
            let (left, right) = pair_values(values, "concat")?;
            Ok(Value::String(format!(
                "{}{}",
                expect_str(left)?,
                expect_str(right)?
            )))
        }
        Op::Get => eval_get(pair_owned(values, "get")?),
        Op::And | Op::Or | Op::If | Op::Array => {
            Err(format!("op `{}` is not an eager operator", op.as_str()))
        }
    }
}

fn eval_stmt(inner: &Inner, stmt: &Stmt, cx: &Compute) -> Result<Value, String> {
    match stmt {
        Stmt::Eval(expr) => eval_expr(inner, expr, cx),
        Stmt::Set { target, expr } => {
            let value = eval_expr(inner, expr, cx)?;
            write_target(inner, target, cx, value, false)
        }
        Stmt::Merge { target, expr } => {
            let value = eval_expr(inner, expr, cx)?;
            write_target(inner, target, cx, value, true)
        }
        Stmt::Seq(steps) => {
            let mut results = Vec::with_capacity(steps.len());
            for step in steps {
                results.push(eval_stmt(inner, step, cx)?);
            }
            Ok(Value::Array(results))
        }
        Stmt::When { cond, then, else_ } => {
            let cond = expect_bool(&eval_expr(inner, cond, cx)?)?;
            if cond {
                eval_stmt(inner, then, cx)
            } else if let Some(else_) = else_ {
                eval_stmt(inner, else_, cx)
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn write_target(
    inner: &Inner,
    id: &str,
    cx: &Compute,
    value: Value,
    merge: bool,
) -> Result<Value, String> {
    let binding = inner
        .bindings
        .borrow()
        .get(id)
        .copied()
        .ok_or_else(|| format!("unknown node `{id}`"))?;
    let ctx = cx.untracked();
    if merge {
        binding.merge(id, ctx, value)
    } else {
        binding.set(id, ctx, value.clone())?;
        binding.read_untracked(ctx)
    }
}

fn eval_neg(value: &Value) -> Result<Value, String> {
    match as_num(value)? {
        Num::Int(integer) => integer
            .checked_neg()
            .map(number_value)
            .ok_or_else(|| "integer overflow".into()),
        Num::Float(float) => canonical_f64(-float),
    }
}

fn eval_mod((left, right): (&Value, &Value)) -> Result<Value, String> {
    let (Num::Int(left), Num::Int(right)) = (as_num(left)?, as_num(right)?) else {
        return Err("mod requires integers".into());
    };
    if right == 0 {
        return Err("division by zero".into());
    }
    Ok(number_value(left % right))
}

fn eval_minmax(max: bool, (left, right): (Value, Value)) -> Result<Value, String> {
    let order = ordering(&left, &right)?;
    let left_wins = if max {
        order != std::cmp::Ordering::Less
    } else {
        order != std::cmp::Ordering::Greater
    };
    Ok(if left_wins { left } else { right })
}

fn eval_get((target, key): (Value, Value)) -> Result<Value, String> {
    match target {
        Value::Array(items) => {
            let index = index_of(&key)?;
            items
                .get(index)
                .cloned()
                .ok_or_else(|| format!("index {index} out of range"))
        }
        Value::Object(map) => {
            let key = expect_str(&key)?.to_string();
            map.get(&key)
                .cloned()
                .ok_or_else(|| format!("missing key `{key}`"))
        }
        other => Err(format!("get requires an array or object, got {other}")),
    }
}

fn cmp_bool(
    values: &[Value],
    op: &str,
    forbidden: std::cmp::Ordering,
    invert: bool,
) -> Result<Value, String> {
    let (left, right) = pair_values(values, op)?;
    let order = ordering(left, right)?;
    let hit = if invert {
        order != forbidden
    } else {
        order == forbidden
    };
    Ok(Value::Bool(hit))
}

#[derive(Clone, Copy)]
enum NumOp {
    Add,
    Sub,
    Mul,
    Div,
}

fn num_bin(op: NumOp, (left, right): (&Value, &Value)) -> Result<Value, String> {
    match (as_num(left)?, as_num(right)?) {
        (Num::Int(left), Num::Int(right)) if !matches!(op, NumOp::Div) => int_bin(op, left, right),
        (left, right) => float_bin(op, left.to_f64(), right.to_f64()),
    }
}

fn int_bin(op: NumOp, left: i64, right: i64) -> Result<Value, String> {
    let value = match op {
        NumOp::Add => left.checked_add(right),
        NumOp::Sub => left.checked_sub(right),
        NumOp::Mul => left.checked_mul(right),
        NumOp::Div => unreachable!("div is not an integer op"),
    };
    value
        .map(number_value)
        .ok_or_else(|| "integer overflow".into())
}

fn float_bin(op: NumOp, left: f64, right: f64) -> Result<Value, String> {
    if matches!(op, NumOp::Div) && right == 0.0 {
        return Err("division by zero".into());
    }
    let value = match op {
        NumOp::Add => left + right,
        NumOp::Sub => left - right,
        NumOp::Mul => left * right,
        NumOp::Div => left / right,
    };
    canonical_f64(value)
}

fn add_numbers(left: &Value, right: &Value) -> Result<Value, String> {
    num_bin(NumOp::Add, (left, right))
}

fn len_value(value: &Value) -> Result<Value, String> {
    let len = match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.len(),
        Value::Object(map) => map.len(),
        other => {
            return Err(format!(
                "len requires a string, array, or object, got {other}"
            ));
        }
    };
    let len = i64::try_from(len).map_err(|_| "length does not fit an integer".to_string())?;
    Ok(number_value(len))
}

fn ordering(left: &Value, right: &Value) -> Result<std::cmp::Ordering, String> {
    match (as_num(left)?, as_num(right)?) {
        (Num::Int(left), Num::Int(right)) => Ok(left.cmp(&right)),
        (left, right) => left
            .to_f64()
            .partial_cmp(&right.to_f64())
            .ok_or_else(|| "non-finite number".into()),
    }
}

#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn to_f64(self) -> f64 {
        match self {
            Num::Int(integer) => integer as f64,
            Num::Float(float) => float,
        }
    }
}

fn as_num(value: &Value) -> Result<Num, String> {
    let Value::Number(number) = value else {
        return Err(format!("expected a number, got {value}"));
    };
    if let Some(integer) = number.as_i64() {
        Ok(Num::Int(integer))
    } else if let Some(float) = number.as_f64() {
        Ok(Num::Float(float))
    } else {
        Err(format!(
            "number {number} is outside the graph IR numeric range"
        ))
    }
}

fn canonical_f64(number: f64) -> Result<Value, String> {
    if !number.is_finite() {
        return Err("non-finite number".into());
    }
    if number.fract() == 0.0 && number >= i64::MIN as f64 && number <= i64::MAX as f64 {
        let integer = number as i64;
        if integer as f64 == number {
            return Ok(number_value(integer));
        }
    }
    Number::from_f64(number)
        .map(Value::Number)
        .ok_or_else(|| "non-finite number".into())
}

fn number_value(integer: i64) -> Value {
    Value::Number(integer.into())
}

fn expect_bool(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(bit) => Ok(*bit),
        other => Err(format!("expected a boolean, got {other}")),
    }
}

fn expect_str(value: &Value) -> Result<&str, String> {
    match value {
        Value::String(text) => Ok(text),
        other => Err(format!("expected a string, got {other}")),
    }
}

fn index_of(value: &Value) -> Result<usize, String> {
    let Num::Int(integer) = as_num(value)? else {
        return Err("array index must be an integer".into());
    };
    if integer < 0 {
        return Err("array index must be >= 0".into());
    }
    usize::try_from(integer).map_err(|_| "array index is too large".into())
}

fn one<'a>(values: &'a [Value], op: &str) -> Result<&'a Value, String> {
    if values.len() == 1 {
        Ok(&values[0])
    } else {
        Err(format!("op `{op}` has the wrong number of arguments"))
    }
}

fn pair<'a>(args: &'a [Expr], op: &str) -> Result<(&'a Expr, &'a Expr), String> {
    if args.len() == 2 {
        Ok((&args[0], &args[1]))
    } else {
        Err(format!("op `{op}` has the wrong number of arguments"))
    }
}

fn pair_values<'a>(values: &'a [Value], op: &str) -> Result<(&'a Value, &'a Value), String> {
    if values.len() == 2 {
        Ok((&values[0], &values[1]))
    } else {
        Err(format!("op `{op}` has the wrong number of arguments"))
    }
}

fn pair_owned(values: &[Value], op: &str) -> Result<(Value, Value), String> {
    if values.len() == 2 {
        Ok((values[0].clone(), values[1].clone()))
    } else {
        Err(format!("op `{op}` has the wrong number of arguments"))
    }
}

fn collect_expr(expr: &Expr, reads: &mut BTreeSet<String>) {
    match expr {
        Expr::Lit(_) => {}
        Expr::Ref(id) => {
            reads.insert(id.clone());
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr(arg, reads);
            }
        }
        Expr::Object(fields) => {
            for field in fields {
                collect_expr(&field.value, reads);
            }
        }
    }
}

fn collect_stmt(stmt: &Stmt, reads: &mut BTreeSet<String>, writes: &mut BTreeSet<String>) {
    match stmt {
        Stmt::Eval(expr) => collect_expr(expr, reads),
        Stmt::Set { target, expr } | Stmt::Merge { target, expr } => {
            writes.insert(target.clone());
            collect_expr(expr, reads);
        }
        Stmt::Seq(steps) => {
            for step in steps {
                collect_stmt(step, reads, writes);
            }
        }
        Stmt::When { cond, then, else_ } => {
            collect_expr(cond, reads);
            collect_stmt(then, reads, writes);
            if let Some(else_) = else_ {
                collect_stmt(else_, reads, writes);
            }
        }
    }
}

fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Lit(value) => format!("lit({})", json_text(value)),
        Expr::Ref(id) => format!("ref({id})"),
        Expr::Call { op, args } => {
            let rendered = args.iter().map(emit_expr).collect::<Vec<_>>().join(", ");
            format!("{}({rendered})", op.as_str())
        }
        Expr::Object(fields) => {
            let rendered = fields
                .iter()
                .map(|field| {
                    format!(
                        "{}: {}",
                        json_text(&Value::String(field.key.clone())),
                        emit_expr(&field.value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("object({rendered})")
        }
    }
}

fn emit_stmt(stmt: &Stmt) -> String {
    match stmt {
        Stmt::Eval(expr) => format!("eval({})", emit_expr(expr)),
        Stmt::Set { target, expr } => format!("set({target}, {})", emit_expr(expr)),
        Stmt::Merge { target, expr } => format!("merge({target}, {})", emit_expr(expr)),
        Stmt::Seq(steps) => {
            let rendered = steps.iter().map(emit_stmt).collect::<Vec<_>>().join(", ");
            format!("seq({rendered})")
        }
        Stmt::When { cond, then, else_ } => match else_ {
            Some(else_) => format!(
                "when({}, {}, {})",
                emit_expr(cond),
                emit_stmt(then),
                emit_stmt(else_)
            ),
            None => format!("when({}, {})", emit_expr(cond), emit_stmt(then)),
        },
    }
}

fn expr_json(expr: &Expr) -> Value {
    match expr {
        Expr::Lit(value) => obj(&[("lit", value.clone())]),
        Expr::Ref(id) => obj(&[("ref", Value::String(id.clone()))]),
        Expr::Call { op, args } => obj(&[
            ("op", Value::String(op.as_str().into())),
            ("args", Value::Array(args.iter().map(expr_json).collect())),
        ]),
        Expr::Object(fields) => obj(&[(
            "object",
            Value::Array(
                fields
                    .iter()
                    .map(|field| {
                        obj(&[
                            ("key", Value::String(field.key.clone())),
                            ("value", expr_json(&field.value)),
                        ])
                    })
                    .collect(),
            ),
        )]),
    }
}

fn stmt_json(stmt: &Stmt) -> Value {
    match stmt {
        Stmt::Eval(expr) => obj(&[
            ("do", Value::String("eval".into())),
            ("expr", expr_json(expr)),
        ]),
        Stmt::Set { target, expr } => obj(&[
            ("do", Value::String("set".into())),
            ("target", Value::String(target.clone())),
            ("expr", expr_json(expr)),
        ]),
        Stmt::Merge { target, expr } => obj(&[
            ("do", Value::String("merge".into())),
            ("target", Value::String(target.clone())),
            ("expr", expr_json(expr)),
        ]),
        Stmt::Seq(steps) => obj(&[
            ("do", Value::String("seq".into())),
            ("steps", Value::Array(steps.iter().map(stmt_json).collect())),
        ]),
        Stmt::When { cond, then, else_ } => {
            let mut object = Map::new();
            object.insert("do".into(), Value::String("when".into()));
            object.insert("cond".into(), expr_json(cond));
            object.insert("then".into(), stmt_json(then));
            if let Some(else_) = else_ {
                object.insert("else".into(), stmt_json(else_));
            }
            Value::Object(object)
        }
    }
}

fn parse_node(value: &Value) -> Result<Node, String> {
    let object = as_object(value, "node")?;
    let kind = require_str(object, "kind", "node")?;
    match kind.as_str() {
        "source" => {
            allow(object, "source", &["id", "kind", "init", "policy"])?;
            let id = parse_id(&require_str(object, "id", "source")?, "source id")?;
            let init = require(object, "init", "source")?.clone();
            let policy = match object.get("policy") {
                None => Policy::KeepLatest,
                Some(Value::String(text)) => Policy::parse(text)?,
                Some(other) => {
                    return Err(format!(
                        "source `{id}` policy must be a string, got {other}"
                    ));
                }
            };
            Ok(Node::Source(SourceNode { id, init, policy }))
        }
        "computed" => {
            allow(object, "computed", &["id", "kind", "expr", "eager"])?;
            let id = parse_id(&require_str(object, "id", "computed")?, "computed id")?;
            let eager = match object.get("eager") {
                None => false,
                Some(Value::Bool(bit)) => *bit,
                Some(other) => {
                    return Err(format!(
                        "computed `{id}` eager must be a boolean, got {other}"
                    ));
                }
            };
            let expr = parse_expr(require(object, "expr", "computed")?)?;
            Ok(Node::Computed(ComputedNode { id, expr, eager }))
        }
        "effect" => {
            allow(object, "effect", &["id", "kind", "body"])?;
            let id = parse_id(&require_str(object, "id", "effect")?, "effect id")?;
            let body = parse_stmt(require(object, "body", "effect")?)?;
            Ok(Node::Effect(EffectNode { id, body }))
        }
        other => Err(format!("unknown node kind `{other}`")),
    }
}

fn parse_expr(value: &Value) -> Result<Expr, String> {
    let object = as_object(value, "expression")?;
    allow(
        object,
        "expression",
        &["lit", "ref", "op", "args", "object"],
    )?;
    let forms = [
        object.contains_key("lit"),
        object.contains_key("ref"),
        object.contains_key("op"),
        object.contains_key("object"),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if forms != 1 {
        return Err("expression must be exactly one of lit, ref, op, or object".into());
    }
    if object.contains_key("args") && !object.contains_key("op") {
        return Err("args is only valid on an op expression".into());
    }
    if object.contains_key("lit") {
        return Ok(Expr::Lit(require(object, "lit", "expression")?.clone()));
    }
    if object.contains_key("ref") {
        let id = require_str(object, "ref", "expression")?;
        return Ok(Expr::Ref(parse_id(&id, "ref")?));
    }
    if object.contains_key("object") {
        return parse_object_expr(require(object, "object", "expression")?);
    }
    let op = Op::parse(&require_str(object, "op", "expression")?)?;
    let raw_args = require(object, "args", "op expression")?;
    let Some(raw_args) = raw_args.as_array() else {
        return Err("op args must be an array".into());
    };
    op.check_arity(raw_args.len())?;
    let mut args = Vec::with_capacity(raw_args.len());
    for arg in raw_args {
        args.push(parse_expr(arg)?);
    }
    Ok(Expr::Call { op, args })
}

fn parse_object_expr(value: &Value) -> Result<Expr, String> {
    let Some(fields) = value.as_array() else {
        return Err("object expression must be an array".into());
    };
    let mut parsed = Vec::with_capacity(fields.len());
    let mut seen = BTreeSet::new();
    for field in fields {
        let object = as_object(field, "object field")?;
        allow(object, "object field", &["key", "value"])?;
        let key = require_str(object, "key", "object field")?;
        if !seen.insert(key.clone()) {
            return Err(format!("duplicate object key `{key}`"));
        }
        parsed.push(ObjectField {
            key,
            value: parse_expr(require(object, "value", "object field")?)?,
        });
    }
    Ok(Expr::Object(parsed))
}

fn parse_stmt(value: &Value) -> Result<Stmt, String> {
    let object = as_object(value, "statement")?;
    let verb = require_str(object, "do", "statement")?;
    match verb.as_str() {
        "eval" => {
            allow(object, "eval", &["do", "expr"])?;
            Ok(Stmt::Eval(parse_expr(require(object, "expr", "eval")?)?))
        }
        "set" | "merge" => {
            allow(object, &verb, &["do", "target", "expr"])?;
            let target = parse_id(&require_str(object, "target", &verb)?, "write target")?;
            let expr = parse_expr(require(object, "expr", &verb)?)?;
            if verb == "set" {
                Ok(Stmt::Set { target, expr })
            } else {
                Ok(Stmt::Merge { target, expr })
            }
        }
        "seq" => {
            allow(object, "seq", &["do", "steps"])?;
            let raw = require(object, "steps", "seq")?;
            let Some(raw) = raw.as_array() else {
                return Err("seq steps must be an array".into());
            };
            if raw.is_empty() {
                return Err("seq steps must not be empty".into());
            }
            let mut steps = Vec::with_capacity(raw.len());
            for step in raw {
                steps.push(parse_stmt(step)?);
            }
            Ok(Stmt::Seq(steps))
        }
        "when" => {
            allow(object, "when", &["do", "cond", "then", "else"])?;
            let cond = parse_expr(require(object, "cond", "when")?)?;
            let then = Box::new(parse_stmt(require(object, "then", "when")?)?);
            let else_ = match object.get("else") {
                None => None,
                Some(raw) => Some(Box::new(parse_stmt(raw)?)),
            };
            Ok(Stmt::When { cond, then, else_ })
        }
        other => Err(format!("unknown statement `{other}`")),
    }
}

fn parse_edit_op(value: &Value) -> Result<EditOp, String> {
    let object = as_object(value, "edit op")?;
    let op = require_str(object, "op", "edit op")?;
    match op.as_str() {
        "add" | "replace" => {
            allow(object, &op, &["op", "node"])?;
            let node = parse_node(require(object, "node", &op)?)?;
            if op == "add" {
                Ok(EditOp::Add(node))
            } else {
                Ok(EditOp::Replace(node))
            }
        }
        "remove" => {
            allow(object, "remove", &["op", "id"])?;
            Ok(EditOp::Remove(parse_id(
                &require_str(object, "id", "remove")?,
                "remove id",
            )?))
        }
        other => Err(format!("unknown edit op `{other}`")),
    }
}

fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("graph document is not JSON: {error}"))
}

fn require_version(object: &Map<String, Value>, what: &str) -> Result<(), String> {
    let version = require_str(object, "ir", what)?;
    if version == IR_VERSION {
        Ok(())
    } else {
        Err(format!("unsupported graph IR version `{version}`"))
    }
}

fn optional_id(
    object: &Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<Option<String>, String> {
    match object.get(field) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(parse_id(text, what)?)),
        Some(other) => Err(format!("{what} must be a string, got {other}")),
    }
}

fn parse_id(text: &str, what: &str) -> Result<String, String> {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return Err(format!("{what} is empty"));
    };
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !(first.is_ascii_alphabetic() || first == '_') || !rest_ok || text.len() > 128 {
        return Err(format!("{what} `{text}` is not a graph IR id"));
    }
    Ok(text.to_string())
}

fn as_object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{what} must be an object"))
}

fn require<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<&'a Value, String> {
    object
        .get(field)
        .ok_or_else(|| format!("{what} requires `{field}`"))
}

fn require_str(object: &Map<String, Value>, field: &str, what: &str) -> Result<String, String> {
    match require(object, field, what)? {
        Value::String(text) => Ok(text.clone()),
        other => Err(format!("{what} `{field}` must be a string, got {other}")),
    }
}

fn allow(object: &Map<String, Value>, what: &str, keys: &[&str]) -> Result<(), String> {
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(format!("{what}: unknown field `{key}`"));
        }
    }
    Ok(())
}

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut object = Map::new();
    for (key, value) in pairs {
        object.insert((*key).to_string(), value.clone());
    }
    Value::Object(object)
}

fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn raise(message: impl Into<String>) -> ! {
    panic!("graph-ir fault: {}", message.into());
}

fn catch_fault<T>(run: impl FnOnce() -> T) -> Result<T, String> {
    match std::panic::catch_unwind(AssertUnwindSafe(run)) {
        Ok(value) => Ok(value),
        Err(payload) => Err(fault_from_panic(payload)),
    }
}

fn fault_from_panic(payload: Box<dyn std::any::Any + Send>) -> String {
    let message = if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else {
        "graph evaluation failed".to_string()
    };
    message
        .strip_prefix("graph-ir fault: ")
        .unwrap_or(&message)
        .to_string()
}

impl MergePolicy<Value> for JsonSum {
    fn merge(old: &Value, op: Value) -> Value {
        match add_numbers(old, &op) {
            Ok(value) => value,
            Err(error) => raise(error),
        }
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = false;
}

impl MergePolicy<Value> for JsonMax {
    fn merge(old: &Value, op: Value) -> Value {
        match ordering(old, &op) {
            Ok(std::cmp::Ordering::Less) => op,
            Ok(_) => old.clone(),
            Err(error) => raise(error),
        }
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

impl MergePolicy<Value> for JsonMin {
    fn merge(old: &Value, op: Value) -> Value {
        match ordering(old, &op) {
            Ok(std::cmp::Ordering::Greater) => op,
            Ok(_) => old.clone(),
            Err(error) => raise(error),
        }
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

impl MergePolicy<Value> for JsonAnd {
    fn merge(old: &Value, op: Value) -> Value {
        match (old, &op) {
            (Value::Bool(left), Value::Bool(right)) => Value::Bool(*left && *right),
            _ => raise("and policy requires booleans"),
        }
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

impl MergePolicy<Value> for JsonOr {
    fn merge(old: &Value, op: Value) -> Value {
        match (old, &op) {
            (Value::Bool(left), Value::Bool(right)) => Value::Bool(*left || *right),
            _ => raise("or policy requires booleans"),
        }
    }
    const COMMUTATIVE: bool = true;
    const IDEMPOTENT: bool = true;
}
