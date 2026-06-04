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

type AsyncAny = dyn Any + Send + Sync;
type BoxedAsyncFuture = Pin<Box<dyn Future<Output = Arc<AsyncAny>> + Send>>;
type AsyncComputeFn = dyn Fn() -> BoxedAsyncFuture + Send + Sync;
type AsyncEqualsFn = dyn Fn(&AsyncAny, &AsyncAny) -> bool + Send + Sync;

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

#[allow(dead_code)]
pub(crate) struct AsyncSlotNode {
    pub(crate) state: AsyncSlotState,
    pub(crate) value: Option<Arc<AsyncAny>>,
    pub(crate) error: Option<Arc<dyn Error + Send + Sync>>,
    pub(crate) revision: u64,
    pub(crate) compute: Arc<AsyncComputeFn>,
    pub(crate) equals: Option<Arc<AsyncEqualsFn>>,
    pub(crate) dependencies: EdgeVec,
    pub(crate) dependents: EdgeVec,
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

#[allow(dead_code)]
pub(crate) struct AsyncEffectNode {
    pub(crate) _dependencies: EdgeVec,
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

pub struct AsyncComputeContext<'a> {
    pub(crate) _context_id: AsyncContextId,
    pub(crate) _node_id: SlotId,
    pub(crate) inner: &'a Arc<Mutex<AsyncContextInner>>,
    pub(crate) dependencies: HashSet<SlotId>,
}

impl AsyncComputeContext<'_> {
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
                .expect("type mismatch in async compute get_cell")
                .clone(),
            _ => panic!("AsyncCellHandle does not point to a Cell node"),
        }
    }
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
        {
            let mut inner = self.inner.lock();
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
                        dependents = cell.dependents.clone();
                    } else {
                        return;
                    }
                }
                _ => panic!("AsyncCellHandle does not point to a Cell node"),
            }
        }
        let _ = dependents;
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
                    edge_insert(&mut e._dependencies, dependency_id);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    fn stub_compute() -> BoxedAsyncFuture {
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
}
