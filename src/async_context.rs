use std::any::Any;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::context::SlotId;

#[cfg(not(feature = "vec_edges"))]
type EdgeVec = smallvec::SmallVec<[SlotId; 4]>;
#[cfg(feature = "vec_edges")]
type EdgeVec = Vec<SlotId>;

fn edge_insert(edges: &mut EdgeVec, id: SlotId) -> bool {
    if edges.contains(&id) {
        false
    } else {
        edges.push(id);
        true
    }
}

fn edge_remove(edges: &mut EdgeVec, id: SlotId) -> bool {
    if let Some(pos) = edges.iter().position(|x| *x == id) {
        edges.swap_remove(pos);
        true
    } else {
        false
    }
}

type AsyncAny = dyn Any + Send + Sync;
type BoxedAsyncFuture = Pin<Box<dyn Future<Output = Arc<AsyncAny>> + Send>>;
type AsyncComputeFn = dyn Fn(AsyncComputeContext) -> BoxedAsyncFuture + Send + Sync;
type AsyncEqualsFn = dyn Fn(&AsyncAny, &AsyncAny) -> bool + Send + Sync;
type BoxedEffectFuture = Pin<Box<dyn Future<Output = Option<BoxedCleanupFn>> + Send>>;
type BoxedCleanupFn = Box<dyn FnOnce() + Send>;
type AsyncEffectFn = dyn Fn(AsyncComputeContext) -> BoxedEffectFuture + Send + Sync;

static NEXT_ASYNC_CONTEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsyncContextId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncSlotStateView {
    None,
    Empty,
    Computing { revision: u64 },
    Resolved,
    Error,
}

#[derive(Debug)]
pub enum AsyncSlotState {
    Empty,
    Computing {
        revision: u64,
        handle: JoinHandle<()>,
    },
    Resolved,
    Error,
}

impl fmt::Display for AsyncSlotState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Empty"),
            Self::Computing { revision, .. } => {
                write!(f, "Computing(revision={revision})")
            }
            Self::Resolved => write!(f, "Resolved"),
            Self::Error => write!(f, "Error"),
        }
    }
}

#[allow(dead_code)]
pub(crate) enum TransitionOutcome {
    Accepted,
    Stale,
    Unchanged,
}

#[allow(dead_code)]
pub(crate) enum InvalidationResult {
    HadInFlight(JoinHandle<()>),
    WasResolved,
    WasError,
    AlreadyEmpty,
}

pub(crate) struct AsyncSlotNode {
    pub(crate) state: AsyncSlotState,
    pub(crate) value: Option<Arc<AsyncAny>>,
    pub(crate) error: Option<Arc<dyn Error + Send + Sync>>,
    pub(crate) revision: u64,
    pub(crate) compute: Arc<AsyncComputeFn>,
    pub(crate) equals: Option<Arc<AsyncEqualsFn>>,
    pub(crate) dependencies: EdgeVec,
    pub(crate) dependents: EdgeVec,
    pub(crate) notifier: Option<watch::Sender<AsyncCompletion>>,
}

#[derive(Clone)]
pub(crate) enum AsyncCompletion {
    Pending,
    Resolved(Arc<dyn Any + Send + Sync>),
    Error(Arc<dyn Error + Send + Sync>),
}

impl AsyncSlotNode {
    pub(crate) fn transition_to_computing(
        &mut self,
        handle: JoinHandle<()>,
    ) -> Option<JoinHandle<()>> {
        let old = std::mem::replace(
            &mut self.state,
            AsyncSlotState::Computing {
                revision: self.revision,
                handle,
            },
        );
        match old {
            AsyncSlotState::Computing { handle, .. } => Some(handle),
            _ => None,
        }
    }

    pub(crate) fn transition_to_resolved(
        &mut self,
        revision: u64,
        value: Arc<AsyncAny>,
    ) -> TransitionOutcome {
        match &self.state {
            AsyncSlotState::Computing {
                revision: current_revision,
                ..
            } if *current_revision == revision => {}
            _ => return TransitionOutcome::Stale,
        }

        let is_new = match (&self.value, &self.equals) {
            (Some(old), Some(eq)) => !eq(old.as_ref(), value.as_ref()),
            _ => true,
        };

        self.state = AsyncSlotState::Resolved;
        self.value = Some(value);
        self.error = None;

        if is_new {
            TransitionOutcome::Accepted
        } else {
            TransitionOutcome::Unchanged
        }
    }

    pub(crate) fn transition_to_error(
        &mut self,
        revision: u64,
        error: Arc<dyn Error + Send + Sync>,
    ) -> TransitionOutcome {
        match &self.state {
            AsyncSlotState::Computing {
                revision: current_revision,
                ..
            } if *current_revision == revision => {}
            _ => return TransitionOutcome::Stale,
        }

        self.state = AsyncSlotState::Error;
        self.error = Some(error);
        self.value = None;
        TransitionOutcome::Accepted
    }

    pub(crate) fn invalidate(&mut self) -> InvalidationResult {
        self.revision += 1;
        match std::mem::replace(&mut self.state, AsyncSlotState::Empty) {
            AsyncSlotState::Computing { handle, .. } => InvalidationResult::HadInFlight(handle),
            AsyncSlotState::Resolved => InvalidationResult::WasResolved,
            AsyncSlotState::Error => InvalidationResult::WasError,
            AsyncSlotState::Empty => InvalidationResult::AlreadyEmpty,
        }
    }

    pub(crate) fn clear(&mut self) -> Option<JoinHandle<()>> {
        self.revision += 1;
        self.value = None;
        self.error = None;
        match std::mem::replace(&mut self.state, AsyncSlotState::Empty) {
            AsyncSlotState::Computing { handle, .. } => Some(handle),
            _ => None,
        }
    }
}

pub(crate) struct AsyncCellNode {
    pub(crate) value: Arc<AsyncAny>,
    pub(crate) dependents: EdgeVec,
}

pub(crate) struct AsyncEffectNode {
    pub(crate) effect_fn: Arc<AsyncEffectFn>,
    pub(crate) cleanup: Option<BoxedCleanupFn>,
    pub(crate) dependencies: EdgeVec,
    pub(crate) dependents: EdgeVec,
    pub(crate) force_run: bool,
}

#[allow(dead_code)]
pub(crate) enum AsyncNode {
    Slot(AsyncSlotNode),
    Cell(AsyncCellNode),
    Effect(AsyncEffectNode),
}

pub(crate) struct AsyncContextInner {
    pub(crate) nodes: Vec<Option<AsyncNode>>,
    next_id: u64,
    free_ids: Vec<u64>,
    pub(crate) context_id: AsyncContextId,
    batch_depth: usize,
    batched_cells: HashSet<SlotId>,
    pending_async_effects: Vec<SlotId>,
}

impl AsyncContextInner {
    pub(crate) fn alloc_id(&mut self) -> SlotId {
        match self.free_ids.pop() {
            Some(id) => SlotId(id),
            None => {
                let id = SlotId(self.next_id);
                self.next_id += 1;
                id
            }
        }
    }

    fn node_index(id: SlotId) -> Option<usize> {
        usize::try_from(id.0).ok()
    }

    pub(crate) fn get_node(&self, id: SlotId) -> Option<&AsyncNode> {
        Self::node_index(id)
            .and_then(|idx| self.nodes.get(idx))
            .and_then(|opt| opt.as_ref())
    }

    pub(crate) fn get_node_mut(&mut self, id: SlotId) -> Option<&mut AsyncNode> {
        Self::node_index(id)
            .and_then(|idx| self.nodes.get_mut(idx))
            .and_then(|opt| opt.as_mut())
    }

    pub(crate) fn insert_node(&mut self, id: SlotId, node: AsyncNode) {
        let index = Self::node_index(id).expect("SlotId does not fit usize");
        if self.nodes.len() <= index {
            self.nodes.resize_with(index + 1, || None);
        }
        self.nodes[index] = Some(node);
    }
}

pub struct AsyncContext {
    inner: Arc<Mutex<AsyncContextInner>>,
}

pub struct AsyncSlotHandle<T> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<T>,
}

impl<T> Clone for AsyncSlotHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for AsyncSlotHandle<T> {}

pub struct AsyncCellHandle<T> {
    pub(crate) id: SlotId,
    pub(crate) _marker: PhantomData<T>,
}

impl<T> Clone for AsyncCellHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for AsyncCellHandle<T> {}

pub struct AsyncEffectHandle {
    pub(crate) id: SlotId,
}

impl Clone for AsyncEffectHandle {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for AsyncEffectHandle {}

pub struct AsyncComputeContext {
    pub(crate) _context_id: AsyncContextId,
    pub(crate) _node_id: SlotId,
    pub(crate) inner: Arc<Mutex<AsyncContextInner>>,
    pub(crate) dependencies: Arc<Mutex<HashSet<SlotId>>>,
}

impl AsyncComputeContext {
    pub fn get_cell<T>(&self, handle: &AsyncCellHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.dependencies.lock().insert(handle.id);
        let inner = self.inner.lock();
        match inner.get_node(handle.id) {
            Some(AsyncNode::Cell(cell)) => cell
                .value
                .as_ref()
                .downcast_ref::<T>()
                .expect("type mismatch in async compute get_cell")
                .clone(),
            _ => panic!("AsyncCellHandle does not point to a Cell node"),
        }
    }

    pub fn get_async<T>(&self, handle: &AsyncSlotHandle<T>) -> impl Future<Output = T> + Send
    where
        T: Clone + Send + Sync + 'static,
    {
        self.dependencies.lock().insert(handle.id);
        let inner_arc = self.inner.clone();
        async move {
            let ctx = AsyncContext { inner: inner_arc };
            ctx.get_async(handle).await
        }
    }
}

fn spawn_async_compute(ctx: &AsyncContext, slot_id: SlotId) -> watch::Receiver<AsyncCompletion> {
    let inner_arc: Arc<Mutex<AsyncContextInner>> = ctx.inner.clone();
    let mut inner = inner_arc.lock();

    let (compute, context_id, spawn_revision) = match inner.get_node(slot_id) {
        Some(AsyncNode::Slot(slot)) => {
            if let AsyncSlotState::Computing { revision: _, .. } = &slot.state {
                return slot
                    .notifier
                    .as_ref()
                    .expect("computing without notifier")
                    .subscribe();
            }
            (slot.compute.clone(), inner.context_id, slot.revision)
        }
        _ => panic!("spawn_async_compute: not a slot node"),
    };

    let (tx, rx) = watch::channel(AsyncCompletion::Pending);
    let inner_for_compute = inner_arc.clone();
    let tx_clone = tx.clone();

    let deps_arc = Arc::new(Mutex::new(HashSet::new()));
    let deps_for_extract = deps_arc.clone();

    let join_handle = tokio::spawn(async move {
        let compute_ctx = AsyncComputeContext {
            _context_id: context_id,
            _node_id: slot_id,
            inner: inner_for_compute.clone(),
            dependencies: deps_arc,
        };
        let result = compute(compute_ctx).await;
        let deps = deps_for_extract.lock().clone();
        {
            let mut inner = inner_for_compute.lock();
            let current_revision = match inner.get_node(slot_id) {
                Some(AsyncNode::Slot(s)) => s.revision,
                _ => {
                    let _ = tx_clone.send(AsyncCompletion::Error(Arc::new(std::io::Error::other(
                        "slot node removed during compute",
                    ))));
                    return;
                }
            };
            if current_revision != spawn_revision {
                return;
            }
            AsyncContext::update_dependencies(&mut inner, slot_id, &deps);
            if let Some(AsyncNode::Slot(slot)) = inner.get_node_mut(slot_id) {
                slot.transition_to_resolved(spawn_revision, result.clone());
            }
        }
        let _ = tx_clone.send(AsyncCompletion::Resolved(result));
    });

    if let Some(AsyncNode::Slot(slot)) = inner.get_node_mut(slot_id) {
        slot.transition_to_computing(join_handle);
        slot.notifier = Some(tx);
    }

    rx
}

impl Default for AsyncContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncContext {
    pub fn new() -> Self {
        let context_id = AsyncContextId(NEXT_ASYNC_CONTEXT_ID.fetch_add(1, Ordering::Relaxed));
        Self {
            inner: Arc::new(Mutex::new(AsyncContextInner {
                nodes: Vec::new(),
                next_id: 0,
                free_ids: Vec::new(),
                context_id,
                batch_depth: 0,
                batched_cells: HashSet::new(),
                pending_async_effects: Vec::new(),
            })),
        }
    }

    pub fn cell<T>(&self, value: T) -> AsyncCellHandle<T>
    where
        T: PartialEq + Clone + Send + Sync + 'static,
    {
        let id;
        {
            let mut inner = self.inner.lock();
            id = inner.alloc_id();
            let node = AsyncCellNode {
                value: Arc::new(value),
                dependents: EdgeVec::new(),
            };
            inner.insert_node(id, AsyncNode::Cell(node));
        }
        AsyncCellHandle {
            id,
            _marker: PhantomData,
        }
    }

    pub fn get_cell<T>(&self, handle: &AsyncCellHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        let inner = self.inner.lock();
        match inner.get_node(handle.id) {
            Some(AsyncNode::Cell(cell)) => cell
                .value
                .as_ref()
                .downcast_ref::<T>()
                .expect("type mismatch in AsyncContext::get_cell")
                .clone(),
            _ => panic!("AsyncCellHandle does not point to a Cell node"),
        }
    }

    pub fn set_cell<T>(&self, handle: &AsyncCellHandle<T>, value: T)
    where
        T: PartialEq + Clone + Send + Sync + 'static,
    {
        let dependents;
        let is_batching;
        {
            let mut inner = self.inner.lock();
            is_batching = inner.batch_depth > 0;
            match inner.get_node_mut(handle.id) {
                Some(AsyncNode::Cell(cell)) => {
                    let changed = !(*cell
                        .value
                        .as_ref()
                        .downcast_ref::<T>()
                        .expect("type mismatch in AsyncContext::set_cell")
                        == value);
                    if changed {
                        cell.value = Arc::new(value);
                        if is_batching {
                            inner.batched_cells.insert(handle.id);
                            return;
                        }
                        dependents = cell.dependents.clone();
                    } else {
                        return;
                    }
                }
                _ => panic!("AsyncCellHandle does not point to a Cell node"),
            }
        }
        for dep_id in &dependents {
            let is_effect = {
                let inner = self.inner.lock();
                matches!(inner.get_node(*dep_id), Some(AsyncNode::Effect(_)))
            };
            if is_effect {
                let mut inner = self.inner.lock();
                if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(*dep_id) {
                    e.force_run = true;
                    inner.pending_async_effects.push(*dep_id);
                }
            } else {
                self.invalidate_async_slot(*dep_id);
            }
        }
        let has_effects = {
            let inner = self.inner.lock();
            !inner.pending_async_effects.is_empty()
        };
        if has_effects {
            self.flush_async_effects();
        }
    }

    pub(crate) fn get_slot_state(&self, id: SlotId) -> AsyncSlotStateView {
        let inner = self.inner.lock();
        match inner.get_node(id) {
            Some(AsyncNode::Slot(slot)) => match &slot.state {
                AsyncSlotState::Empty => AsyncSlotStateView::Empty,
                AsyncSlotState::Computing { revision, .. } => AsyncSlotStateView::Computing {
                    revision: *revision,
                },
                AsyncSlotState::Resolved => AsyncSlotStateView::Resolved,
                AsyncSlotState::Error => AsyncSlotStateView::Error,
            },
            _ => AsyncSlotStateView::None,
        }
    }

    pub(crate) fn get_slot_revision(&self, id: SlotId) -> Option<u64> {
        let inner = self.inner.lock();
        match inner.get_node(id) {
            Some(AsyncNode::Slot(slot)) => Some(slot.revision),
            _ => None,
        }
    }

    pub(crate) fn register_dependency(&self, dependency_id: SlotId, dependent_id: SlotId) {
        if dependency_id == dependent_id {
            return;
        }
        let mut inner = self.inner.lock();
        if let Some(node) = inner.get_node_mut(dependent_id) {
            match node {
                AsyncNode::Slot(s) => {
                    edge_insert(&mut s.dependencies, dependency_id);
                }
                AsyncNode::Effect(e) => {
                    edge_insert(&mut e.dependencies, dependency_id);
                }
                AsyncNode::Cell(_) => {}
            }
        }
        if let Some(node) = inner.get_node_mut(dependency_id) {
            match node {
                AsyncNode::Slot(s) => {
                    edge_insert(&mut s.dependents, dependent_id);
                }
                AsyncNode::Cell(c) => {
                    edge_insert(&mut c.dependents, dependent_id);
                }
                AsyncNode::Effect(_) => {}
            }
        }
    }

    fn update_dependencies(
        inner: &mut AsyncContextInner,
        node_id: SlotId,
        new_deps: &HashSet<SlotId>,
    ) {
        let old_deps = match inner.get_node(node_id) {
            Some(AsyncNode::Slot(s)) => s.dependencies.iter().copied().collect::<HashSet<_>>(),
            _ => return,
        };
        for old_id in old_deps.difference(new_deps) {
            if let Some(AsyncNode::Slot(s)) = inner.get_node_mut(*old_id) {
                edge_remove(&mut s.dependents, node_id);
            }
            if let Some(AsyncNode::Cell(c)) = inner.get_node_mut(*old_id) {
                edge_remove(&mut c.dependents, node_id);
            }
        }
        if let Some(AsyncNode::Slot(s)) = inner.get_node_mut(node_id) {
            s.dependencies = new_deps.iter().copied().collect();
        }
        for new_id in new_deps {
            if let Some(AsyncNode::Slot(s)) = inner.get_node_mut(*new_id) {
                edge_insert(&mut s.dependents, node_id);
            }
            if let Some(AsyncNode::Cell(c)) = inner.get_node_mut(*new_id) {
                edge_insert(&mut c.dependents, node_id);
            }
        }
    }

    fn invalidate_async_slot(&self, id: SlotId) {
        let slot_dependents;
        let effect_dependents;
        let old_notifier;
        let in_flight_handle;
        {
            let mut inner = self.inner.lock();
            match inner.get_node_mut(id) {
                Some(AsyncNode::Slot(slot)) => {
                    let result = slot.invalidate();
                    in_flight_handle = match result {
                        InvalidationResult::HadInFlight(handle) => Some(handle),
                        _ => None,
                    };
                    old_notifier = slot.notifier.take();
                    let all_dependents = slot.dependents.clone();
                    let mut sd = EdgeVec::new();
                    let mut ed = EdgeVec::new();
                    for d in &all_dependents {
                        match inner.get_node(*d) {
                            Some(AsyncNode::Effect(_)) => ed.push(*d),
                            _ => sd.push(*d),
                        }
                    }
                    slot_dependents = sd;
                    effect_dependents = ed;
                }
                _ => return,
            }
        }
        if let Some(handle) = in_flight_handle {
            handle.abort();
        }
        drop(old_notifier);
        for dep_id in &effect_dependents {
            let mut inner = self.inner.lock();
            if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(*dep_id) {
                e.force_run = true;
                inner.pending_async_effects.push(*dep_id);
            }
        }
        for dep_id in slot_dependents {
            self.invalidate_async_slot(dep_id);
        }
        if !effect_dependents.is_empty() {
            self.flush_async_effects();
        }
    }

    pub fn computed_async<T, F, Fut>(&self, compute: F) -> AsyncSlotHandle<T>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        self.slot_async_with_equals(compute, None)
    }

    pub fn memo_async<T, F, Fut>(&self, compute: F) -> AsyncSlotHandle<T>
    where
        T: PartialEq + Clone + Send + Sync + 'static,
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let equals: Arc<AsyncEqualsFn> = Arc::new(|old: &AsyncAny, new: &AsyncAny| -> bool {
            let old_val = old.downcast_ref::<T>();
            let new_val = new.downcast_ref::<T>();
            match (old_val, new_val) {
                (Some(o), Some(n)) => o == n,
                _ => false,
            }
        });
        self.slot_async_with_equals(compute, Some(equals))
    }

    fn slot_async_with_equals<T, F, Fut>(
        &self,
        compute: F,
        equals: Option<Arc<AsyncEqualsFn>>,
    ) -> AsyncSlotHandle<T>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let compute_arc: Arc<AsyncComputeFn> = Arc::new(move |ctx| {
            let fut = compute(ctx);
            Box::pin(async move { Arc::new(fut.await) as Arc<AsyncAny> })
        });
        let id;
        {
            let mut inner = self.inner.lock();
            id = inner.alloc_id();
            let node = AsyncSlotNode {
                state: AsyncSlotState::Empty,
                value: None,
                error: None,
                revision: 0,
                compute: compute_arc,
                equals,
                dependencies: EdgeVec::new(),
                dependents: EdgeVec::new(),
                notifier: None,
            };
            inner.insert_node(id, AsyncNode::Slot(node));
        }
        AsyncSlotHandle {
            id,
            _marker: PhantomData,
        }
    }

    pub async fn get_async<T>(&self, handle: &AsyncSlotHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        let mut recv = {
            let inner = self.inner.lock();
            match inner.get_node(handle.id) {
                Some(AsyncNode::Slot(slot)) => match &slot.state {
                    AsyncSlotState::Resolved => {
                        let val = slot.value.as_ref().expect("resolved without value");
                        return val
                            .downcast_ref::<T>()
                            .expect("type mismatch in get_async")
                            .clone();
                    }
                    AsyncSlotState::Computing { .. } => slot
                        .notifier
                        .as_ref()
                        .expect("computing without notifier")
                        .subscribe(),
                    AsyncSlotState::Error | AsyncSlotState::Empty => {
                        drop(inner);
                        spawn_async_compute(self, handle.id)
                    }
                },
                _ => panic!("AsyncSlotHandle does not point to a Slot node"),
            }
        };

        loop {
            if recv.changed().await.is_err() {
                panic!("get_async: notifier dropped unexpectedly");
            }
            let completion = recv.borrow_and_update().clone();
            match completion {
                AsyncCompletion::Resolved(val) => {
                    return val
                        .downcast_ref::<T>()
                        .expect("type mismatch in get_async completion")
                        .clone();
                }
                AsyncCompletion::Error(_err) => {
                    recv = spawn_async_compute(self, handle.id);
                    continue;
                }
                AsyncCompletion::Pending => continue,
            }
        }
    }

    pub fn batch<F, R>(&self, run: F) -> R
    where
        F: FnOnce(&AsyncContext) -> R,
    {
        {
            let mut inner = self.inner.lock();
            inner.batch_depth += 1;
        }
        let result = run(self);
        {
            let mut inner = self.inner.lock();
            inner.batch_depth -= 1;
            if inner.batch_depth == 0 {
                let batched = inner.batched_cells.drain().collect::<Vec<_>>();
                drop(inner);
                for cell_id in batched {
                    let dependents = {
                        let inner = self.inner.lock();
                        match inner.get_node(cell_id) {
                            Some(AsyncNode::Cell(c)) => c.dependents.clone(),
                            _ => EdgeVec::new(),
                        }
                    };
                    for dep_id in dependents {
                        self.invalidate_async_slot(dep_id);
                    }
                }
            }
        }
        result
    }

    pub fn effect_async<F, Fut, C>(&self, effect: F) -> AsyncEffectHandle
    where
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<C>> + Send + 'static,
        C: FnOnce() + Send + 'static,
    {
        let id;
        {
            let mut inner = self.inner.lock();
            id = inner.alloc_id();
            let effect_fn: Arc<AsyncEffectFn> = Arc::new(move |ctx| {
                let fut = effect(ctx);
                Box::pin(async move { fut.await.map(|c| Box::new(c) as BoxedCleanupFn) })
            });
            let node = AsyncEffectNode {
                effect_fn,
                cleanup: None,
                dependencies: EdgeVec::new(),
                dependents: EdgeVec::new(),
                force_run: true,
            };
            inner.insert_node(id, AsyncNode::Effect(node));
            inner.pending_async_effects.push(id);
        }
        let handle = AsyncEffectHandle { id };
        self.flush_async_effects();
        handle
    }

    pub fn dispose_async_effect(&self, handle: &AsyncEffectHandle) {
        {
            let mut inner = self.inner.lock();
            inner.pending_async_effects.retain(|&id| id != handle.id);
            match inner.get_node_mut(handle.id) {
                Some(AsyncNode::Effect(e)) => {
                    let deps = e.dependencies.clone();
                    for dep_id in &deps {
                        match inner.get_node_mut(*dep_id) {
                            Some(AsyncNode::Slot(s)) => {
                                edge_remove(&mut s.dependents, handle.id);
                            }
                            Some(AsyncNode::Cell(c)) => {
                                edge_remove(&mut c.dependents, handle.id);
                            }
                            _ => {}
                        }
                    }
                }
                _ => return,
            };
            let index = usize::try_from(handle.id.0).ok();
            if let Some(idx) = index
                && idx < inner.nodes.len()
            {
                inner.nodes[idx] = None;
                inner.free_ids.push(handle.id.0);
            }
        }
    }

    fn flush_async_effects(&self) {
        let effect_ids: Vec<SlotId>;
        {
            let mut inner = self.inner.lock();
            effect_ids = inner.pending_async_effects.drain(..).collect();
        }
        let ctx_inner = self.inner.clone();
        for effect_id in effect_ids {
            let should_run = {
                let inner = self.inner.lock();
                match inner.get_node(effect_id) {
                    Some(AsyncNode::Effect(e)) => e.force_run,
                    _ => false,
                }
            };
            if !should_run {
                continue;
            }
            {
                let mut inner = self.inner.lock();
                if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id) {
                    e.force_run = false;
                }
            }
            let effect_fn = {
                let inner = self.inner.lock();
                match inner.get_node(effect_id) {
                    Some(AsyncNode::Effect(e)) => Some(e.effect_fn.clone()),
                    _ => None,
                }
            };
            if let Some(fn_arc) = effect_fn {
                let inner_for_ctx = ctx_inner.clone();
                tokio::spawn(async move {
                    {
                        let mut inner = inner_for_ctx.lock();
                        if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id)
                            && let Some(cleanup) = e.cleanup.take()
                        {
                            drop(inner);
                            cleanup();
                        }
                    }
                    let deps_arc = Arc::new(Mutex::new(HashSet::new()));
                    let deps_for_extract = deps_arc.clone();
                    let compute_ctx = AsyncComputeContext {
                        _context_id: {
                            let inner = inner_for_ctx.lock();
                            inner.context_id
                        },
                        _node_id: effect_id,
                        inner: inner_for_ctx.clone(),
                        dependencies: deps_arc,
                    };
                    let cleanup = fn_arc(compute_ctx).await;
                    let deps = deps_for_extract.lock().clone();
                    {
                        let mut inner = inner_for_ctx.lock();
                        AsyncContext::update_effect_dependencies(&mut inner, effect_id, &deps);
                        if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id) {
                            e.cleanup = cleanup;
                        }
                    }
                });
            }
        }
    }

    fn update_effect_dependencies(
        inner: &mut AsyncContextInner,
        effect_id: SlotId,
        new_deps: &HashSet<SlotId>,
    ) {
        let old_deps = match inner.get_node(effect_id) {
            Some(AsyncNode::Effect(e)) => e.dependencies.iter().copied().collect::<HashSet<_>>(),
            _ => return,
        };
        for old_id in old_deps.difference(new_deps) {
            match inner.get_node_mut(*old_id) {
                Some(AsyncNode::Slot(s)) => {
                    edge_remove(&mut s.dependents, effect_id);
                }
                Some(AsyncNode::Cell(c)) => {
                    edge_remove(&mut c.dependents, effect_id);
                }
                _ => {}
            }
        }
        if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id) {
            e.dependencies = new_deps.iter().copied().collect();
        }
        for new_id in new_deps {
            match inner.get_node_mut(*new_id) {
                Some(AsyncNode::Slot(s)) => {
                    edge_insert(&mut s.dependents, effect_id);
                }
                Some(AsyncNode::Cell(c)) => {
                    edge_insert(&mut c.dependents, effect_id);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use tokio::runtime::Runtime;

    fn stub_compute(_ctx: AsyncComputeContext) -> BoxedAsyncFuture {
        Box::pin(async { Arc::new(()) as Arc<AsyncAny> })
    }

    fn make_slot_node(revision: u64) -> AsyncSlotNode {
        AsyncSlotNode {
            state: AsyncSlotState::Empty,
            value: None,
            error: None,
            revision,
            compute: Arc::new(stub_compute),
            equals: None,
            dependencies: EdgeVec::new(),
            dependents: EdgeVec::new(),
            notifier: None,
        }
    }

    fn make_slot_node_with_memo(revision: u64, value: Option<Arc<AsyncAny>>) -> AsyncSlotNode {
        AsyncSlotNode {
            state: AsyncSlotState::Empty,
            value,
            error: None,
            revision,
            compute: Arc::new(stub_compute),
            equals: Some(Arc::new(|old: &AsyncAny, new: &AsyncAny| -> bool {
                let old_val = old.downcast_ref::<i32>();
                let new_val = new.downcast_ref::<i32>();
                match (old_val, new_val) {
                    (Some(o), Some(n)) => o == n,
                    _ => false,
                }
            })),
            dependencies: EdgeVec::new(),
            dependents: EdgeVec::new(),
            notifier: None,
        }
    }

    #[test]
    fn async_slot_state_starts_empty() {
        let ctx = AsyncContext::new();
        let id;
        {
            let mut inner = ctx.inner.lock();
            id = inner.alloc_id();
            inner.insert_node(id, AsyncNode::Slot(make_slot_node(0)));
        }
        let state = ctx.get_slot_state(id);
        assert!(matches!(state, AsyncSlotStateView::Empty));
    }

    #[test]
    fn empty_to_computing_transition() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async {});
        let mut node = make_slot_node(0);
        let old = node.transition_to_computing(handle);
        assert!(old.is_none());
        assert!(matches!(
            node.state,
            AsyncSlotState::Computing { revision: 0, .. }
        ));
    }

    #[test]
    fn computing_to_resolved_transition() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async {});
        let mut node = make_slot_node(0);
        node.transition_to_computing(handle);
        let result = node.transition_to_resolved(0, Arc::new(42i32));
        assert!(matches!(result, TransitionOutcome::Accepted));
        assert!(matches!(node.state, AsyncSlotState::Resolved));
        assert_eq!(
            node.value.as_ref().unwrap().downcast_ref::<i32>().unwrap(),
            &42
        );
    }

    #[test]
    fn computing_to_error_transition() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async {});
        let mut node = make_slot_node(0);
        node.transition_to_computing(handle);
        let err: Arc<dyn Error + Send + Sync> = Arc::new(std::io::Error::other("test error"));
        let result = node.transition_to_error(0, err);
        assert!(matches!(result, TransitionOutcome::Accepted));
        assert!(matches!(node.state, AsyncSlotState::Error));
        assert!(node.error.is_some());
        assert!(node.value.is_none());
    }

    #[test]
    fn stale_completion_is_rejected() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async {});
        let mut node = make_slot_node(1);
        node.transition_to_computing(handle);
        let result = node.transition_to_resolved(0, Arc::new(42i32));
        assert!(matches!(result, TransitionOutcome::Stale));
    }

    #[test]
    fn computing_to_computing_stale_returns_old_handle() {
        let rt = Runtime::new().unwrap();
        let handle1 = rt.spawn(async {});
        let handle2 = rt.spawn(async {});
        let mut node = make_slot_node(0);
        node.transition_to_computing(handle1);
        node.revision = 1;
        let old = node.transition_to_computing(handle2);
        assert!(old.is_some());
        assert!(matches!(
            node.state,
            AsyncSlotState::Computing { revision: 1, .. }
        ));
    }

    #[test]
    fn resolved_to_computing_via_invalidation() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async {});
        let mut node = make_slot_node(0);
        node.transition_to_computing(handle);
        node.transition_to_resolved(0, Arc::new(42i32));
        assert!(matches!(node.state, AsyncSlotState::Resolved));

        let result = node.invalidate();
        assert!(matches!(result, InvalidationResult::WasResolved));
        assert!(matches!(node.state, AsyncSlotState::Empty));
        assert_eq!(node.revision, 1);
    }

    #[test]
    fn error_to_computing_via_invalidation() {
        let mut node = AsyncSlotNode {
            state: AsyncSlotState::Error,
            value: None,
            error: Some(Arc::new(std::io::Error::other("test"))),
            revision: 0,
            compute: Arc::new(stub_compute),
            equals: None,
            dependencies: EdgeVec::new(),
            dependents: EdgeVec::new(),
            notifier: None,
        };
        let result = node.invalidate();
        assert!(matches!(result, InvalidationResult::WasError));
        assert!(matches!(node.state, AsyncSlotState::Empty));
        assert_eq!(node.revision, 1);
    }

    #[test]
    fn clear_aborts_in_flight() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async { std::future::pending::<()>().await });
        let mut node = make_slot_node(0);
        node.transition_to_computing(handle);
        let old_handle = node.clear();
        assert!(old_handle.is_some());
        old_handle.unwrap().abort();
        assert!(matches!(node.state, AsyncSlotState::Empty));
        assert!(node.value.is_none());
        assert_eq!(node.revision, 1);
    }

    #[test]
    fn memo_unchanged_transition() {
        let rt = Runtime::new().unwrap();
        let handle = rt.spawn(async {});
        let mut node = make_slot_node_with_memo(0, Some(Arc::new(42i32)));
        node.transition_to_computing(handle);
        let result = node.transition_to_resolved(0, Arc::new(42i32));
        assert!(matches!(result, TransitionOutcome::Unchanged));
    }

    #[test]
    fn async_context_cell_basic() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(10i32);
        assert_eq!(ctx.get_cell(&cell), 10);
        ctx.set_cell(&cell, 20);
        assert_eq!(ctx.get_cell(&cell), 20);
    }

    #[test]
    fn async_context_cell_noop_on_equal() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(10i32);
        ctx.set_cell(&cell, 10);
        assert_eq!(ctx.get_cell(&cell), 10);
    }

    #[test]
    fn async_context_id_unique() {
        let ctx1 = AsyncContext::new();
        let ctx2 = AsyncContext::new();
        let id1 = ctx1.inner.lock().context_id;
        let id2 = ctx2.inner.lock().context_id;
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn computed_async_basic() {
        let ctx = AsyncContext::new();
        let slot = ctx.computed_async(|_ctx| async move { 42i32 });
        let val = ctx.get_async(&slot).await;
        assert_eq!(val, 42);
    }

    #[tokio::test]
    async fn computed_async_reads_cell() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(10i32);
        let slot = ctx.computed_async(move |ctx| {
            let val = ctx.get_cell(&cell);
            async move { val + 1 }
        });
        let val = ctx.get_async(&slot).await;
        assert_eq!(val, 11);
    }

    #[tokio::test]
    async fn computed_async_cached() {
        let ctx = AsyncContext::new();
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        let slot = ctx.computed_async(move |_| {
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                42i32
            }
        });
        let v1 = ctx.get_async(&slot).await;
        let v2 = ctx.get_async(&slot).await;
        assert_eq!(v1, 42);
        assert_eq!(v2, 42);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn computed_async_invalidation() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let slot = ctx.computed_async(move |ctx| {
            let val = ctx.get_cell(&cell);
            async move { val * 2 }
        });
        assert_eq!(ctx.get_async(&slot).await, 2);
        ctx.set_cell(&cell, 5);
        assert_eq!(ctx.get_async(&slot).await, 10);
    }

    #[tokio::test]
    async fn memo_async_suppresses_equal() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        let slot = ctx.memo_async(move |ctx| {
            let val = ctx.get_cell(&cell);
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                val / val
            }
        });
        assert_eq!(ctx.get_async(&slot).await, 1);
        ctx.set_cell(&cell, 2);
        assert_eq!(ctx.get_async(&slot).await, 1);
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn batch_defers_invalidation() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let slot = ctx.computed_async(move |ctx| {
            let val = ctx.get_cell(&cell);
            async move { val * 10 }
        });
        assert_eq!(ctx.get_async(&slot).await, 10);
        ctx.batch(|ctx| {
            ctx.set_cell(&cell, 2);
            ctx.set_cell(&cell, 3);
        });
        assert_eq!(ctx.get_async(&slot).await, 30);
    }

    #[tokio::test]
    async fn concurrent_get_async_deduplicates() {
        let ctx = AsyncContext::new();
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        let slot = ctx.computed_async(move |_| {
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                99i32
            }
        });
        let (v1, v2) = tokio::join!(ctx.get_async(&slot), ctx.get_async(&slot));
        assert_eq!(v1, 99);
        assert_eq!(v2, 99);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn async_slot_reads_async_slot() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(5i32);
        let base = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            async move { v + 10 }
        });
        let derived = ctx.computed_async(move |ctx| {
            let base_handle = base;
            async move {
                let v = ctx.get_async(&base_handle).await;
                v * 2
            }
        });
        assert_eq!(ctx.get_async(&derived).await, 30);
    }

    #[tokio::test]
    async fn async_chain_invalidation() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let cell_clone = cell;
        let base = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell_clone);
            async move { v + 10 }
        });
        let derived = ctx.computed_async(move |ctx| {
            let bh = base;
            async move {
                let v = ctx.get_async(&bh).await;
                v * 2
            }
        });
        assert_eq!(ctx.get_async(&derived).await, 22);
        ctx.set_cell(&cell_clone, 3);
        assert_eq!(ctx.get_async(&derived).await, 26);
    }

    #[tokio::test]
    async fn async_chain_three_levels() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let a = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            async move { v + 1 }
        });
        let b = ctx.computed_async(move |ctx| {
            let ah = a;
            async move { ctx.get_async(&ah).await + 1 }
        });
        let c = ctx.computed_async(move |ctx| {
            let bh = b;
            async move { ctx.get_async(&bh).await + 1 }
        });
        assert_eq!(ctx.get_async(&c).await, 4);
        ctx.set_cell(&cell, 10);
        assert_eq!(ctx.get_async(&c).await, 13);
    }

    #[tokio::test]
    async fn async_dependency_tracks_slot_edges() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(3i32);
        let slot = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            async move { v * 2 }
        });
        let _ = ctx.get_async(&slot).await;
        {
            let inner = ctx.inner.lock();
            if let Some(AsyncNode::Slot(s)) = inner.get_node(slot.id) {
                assert!(s.dependencies.contains(&cell.id));
            }
        }
        {
            let inner = ctx.inner.lock();
            if let Some(AsyncNode::Cell(c)) = inner.get_node(cell.id) {
                assert!(c.dependents.contains(&slot.id));
            }
        }
    }

    #[tokio::test]
    async fn async_dependency_updates_on_rerun() {
        let ctx = AsyncContext::new();
        let cell_a = ctx.cell(1i32);
        let cell_b = ctx.cell(100i32);
        let flag = ctx.cell(true);
        let slot = ctx.computed_async(move |ctx| {
            let f = ctx.get_cell(&flag);
            let v = if f {
                ctx.get_cell(&cell_a)
            } else {
                ctx.get_cell(&cell_b)
            };
            async move { v }
        });
        assert_eq!(ctx.get_async(&slot).await, 1);
        {
            let inner = ctx.inner.lock();
            if let Some(AsyncNode::Slot(s)) = inner.get_node(slot.id) {
                assert!(s.dependencies.contains(&cell_a.id));
                assert!(!s.dependencies.contains(&cell_b.id));
            }
        }
        ctx.set_cell(&flag, false);
        assert_eq!(ctx.get_async(&slot).await, 100);
        {
            let inner = ctx.inner.lock();
            if let Some(AsyncNode::Slot(s)) = inner.get_node(slot.id) {
                assert!(!s.dependencies.contains(&cell_a.id));
                assert!(s.dependencies.contains(&cell_b.id));
            }
        }
    }

    #[tokio::test]
    async fn invalidation_aborts_in_flight() {
        let ctx = Arc::new(AsyncContext::new());
        let cell = ctx.cell(1i32);
        let compute_count = Arc::new(AtomicU64::new(0));
        let count_clone = compute_count.clone();
        let slot = ctx.computed_async(move |ctx| {
            let _v = ctx.get_cell(&cell);
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                42i32
            }
        });
        let ctx_clone = ctx.clone();
        let handle = tokio::spawn(async move { ctx_clone.get_async(&slot).await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        ctx.set_cell(&cell, 2);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let val = ctx.get_async(&slot).await;
        assert_eq!(val, 42);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn stale_revision_prevents_publish() {
        let ctx = Arc::new(AsyncContext::new());
        let cell = ctx.cell(1i32);
        let slot = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            async move { v + 1 }
        });
        let ctx1 = ctx.clone();
        let h1 = tokio::spawn(async move { ctx1.get_async(&slot).await });
        let v1 = h1.await.unwrap();
        assert_eq!(v1, 2);
        let state = ctx.get_slot_state(slot.id);
        assert!(matches!(state, AsyncSlotStateView::Resolved));
        ctx.set_cell(&cell, 10);
        let state = ctx.get_slot_state(slot.id);
        assert!(matches!(state, AsyncSlotStateView::Empty));
        let v2 = ctx.get_async(&slot).await;
        assert_eq!(v2, 11);
    }

    #[tokio::test]
    async fn dropping_one_waiter_does_not_cancel_shared_compute() {
        let ctx = AsyncContext::new();
        let compute_count = Arc::new(AtomicU64::new(0));
        let count_clone = compute_count.clone();
        let slot = ctx.computed_async(move |_| {
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                99i32
            }
        });
        let (v2, v3) = tokio::join!(ctx.get_async(&slot), ctx.get_async(&slot));
        assert_eq!(v2, 99);
        assert_eq!(v3, 99);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn effect_async_runs_on_creation() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(10i32);
        let result = Arc::new(Mutex::new(0i32));
        let result_clone = result.clone();
        ctx.effect_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            let r = result_clone.clone();
            async move {
                *r.lock() = v;
                None::<fn()>
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(*result.lock(), 10);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn effect_async_reruns_on_cell_change() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        ctx.effect_async(move |ctx| {
            let _v = ctx.get_cell(&cell);
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                None::<fn()>
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(count.load(Ordering::Relaxed), 1);
        ctx.set_cell(&cell, 2);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(count.load(Ordering::Relaxed) >= 2);
    }

    #[tokio::test]
    async fn effect_async_cleanup_runs_on_rerun() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let cleanup_count = Arc::new(AtomicU64::new(0));
        let cleanup_clone = cleanup_count.clone();
        ctx.effect_async(move |ctx| {
            let _v = ctx.get_cell(&cell);
            let c = cleanup_clone.clone();
            async move {
                Some(move || {
                    c.fetch_add(1, Ordering::Relaxed);
                })
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 0);
        ctx.set_cell(&cell, 2);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(cleanup_count.load(Ordering::Relaxed) >= 1);
    }

    #[tokio::test]
    async fn dispose_async_effect_removes_it() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        let handle = ctx.effect_async(move |ctx| {
            let _v = ctx.get_cell(&cell);
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                None::<fn()>
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let after_first = count.load(Ordering::Relaxed);
        assert!(after_first >= 1);
        ctx.dispose_async_effect(&handle);
        ctx.set_cell(&cell, 2);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(count.load(Ordering::Relaxed), after_first);
    }
}
