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
    pub(crate) in_flight: Option<JoinHandle<()>>,
}

#[allow(dead_code)]
pub(crate) enum AsyncNode {
    Slot(AsyncSlotNode),
    Cell(AsyncCellNode),
    Effect(AsyncEffectNode),
}

pub(crate) struct AsyncContextInner {
    pub(crate) nodes: Vec<Option<AsyncNode>>,
    /// Per-index generation counter, parallel to `nodes`. Bumped every time the
    /// node at an index is disposed and its `SlotId` recycled into `free_ids`
    /// (#lzasyncdispose2). A task spawned for an effect captures the generation
    /// at spawn time and re-checks it before writing cleanup/edges/`in_flight`
    /// back, so a run still in-flight across its `.await` can never alias a
    /// freshly-allocated node that reused the recycled id.
    generations: Vec<u64>,
    next_id: u64,
    free_ids: Vec<u64>,
    pub(crate) context_id: AsyncContextId,
    batch_depth: usize,
    batched_cells: HashSet<SlotId>,
    pending_async_effects: Vec<SlotId>,
    scheduled_async_effects: HashSet<SlotId>,
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

    /// Current generation of the node slot `id` maps to. A never-allocated index
    /// reads as `0`, which matches the generation a freshly-allocated node sees.
    pub(crate) fn generation(&self, id: SlotId) -> u64 {
        Self::node_index(id)
            .and_then(|idx| self.generations.get(idx))
            .copied()
            .unwrap_or(0)
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
        if self.generations.len() <= index {
            self.generations.resize(index + 1, 0);
        }
        self.nodes[index] = Some(node);
    }
}

fn register_dependency_locked(
    inner: &mut AsyncContextInner,
    dependency_id: SlotId,
    dependent_id: SlotId,
) {
    if dependency_id == dependent_id {
        return;
    }
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

/// Test-only async hook installed via [`AsyncContext::__install_window1_hook`]
/// to deterministically exercise the `#k03k` window-1 race in `get_async` (the
/// slot transitioning `Computing -> Resolved` between the lock-free fast-path
/// check and the re-lock). Compiled out of default/release builds.
#[cfg(feature = "instrumentation")]
pub type Window1Hook = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub struct AsyncContext {
    inner: Arc<Mutex<AsyncContextInner>>,
    /// One-shot async seam fired inside `get_async`'s window-1 gap; `take`n on
    /// first fire so it runs exactly once. See [`Window1Hook`].
    #[cfg(feature = "instrumentation")]
    window1_hook: Mutex<Option<Window1Hook>>,
    /// Counts how many times `get_async` returned through the window-1
    /// `Resolved`-after-re-lock arm. Lets tests prove the race arm was taken
    /// rather than the fast path.
    #[cfg(feature = "instrumentation")]
    window1_resolved_hits: AtomicU64,
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

/// A typed handle to an **eager** derived value within an [`AsyncContext`].
///
/// This is the async counterpart to [`crate::SignalHandle`]. It is a memoized
/// backing slot ([`AsyncContext::memo_async`]) plus a small puller effect
/// ([`AsyncContext::effect_async`]) that awaits the slot after every
/// invalidation, so an upstream change eagerly drives the async recompute to
/// completion instead of waiting for the next read. See
/// [`AsyncContext::signal_async`].
pub struct AsyncSignalHandle<T> {
    /// Memoized backing slot that holds the derived value.
    pub(crate) slot: AsyncSlotHandle<T>,
    /// Puller effect that keeps `slot` eagerly materialized.
    pub(crate) effect: AsyncEffectHandle,
}

impl<T> AsyncSignalHandle<T> {
    /// Read this signal's current value if it has resolved, without awaiting.
    ///
    /// Ergonomic alias for [`AsyncContext::get_signal`].
    pub fn get(&self, ctx: &AsyncContext) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        ctx.get_signal(self)
    }

    /// Await this signal's current value, driving recomputation if needed.
    ///
    /// Ergonomic alias for [`AsyncContext::get_signal_async`].
    pub async fn get_async(&self, ctx: &AsyncContext) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        ctx.get_signal_async(self).await
    }

    /// Dispose this signal's eager puller. The backing value remains readable
    /// and reverts to lazy (recomputed on next read) behavior.
    pub fn dispose(&self, ctx: &AsyncContext) {
        ctx.dispose_signal(self);
    }

    /// Check whether this signal's eager puller is still active.
    pub fn is_active(&self, ctx: &AsyncContext) -> bool {
        ctx.is_signal_active(self)
    }
}

impl<T> Clone for AsyncSignalHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for AsyncSignalHandle<T> {}

pub struct AsyncComputeContext {
    pub(crate) _context_id: AsyncContextId,
    pub(crate) _node_id: SlotId,
    /// Generation of `_node_id` captured when this context's run was spawned.
    /// Live dependency-edge writes keyed by `_node_id` are skipped once the
    /// node's current generation diverges — i.e. the node was disposed and its
    /// id potentially recycled mid-run (#lzasyncdispose2).
    pub(crate) _node_gen: u64,
    pub(crate) inner: Arc<Mutex<AsyncContextInner>>,
    pub(crate) dependencies: Arc<Mutex<HashSet<SlotId>>>,
}

impl AsyncComputeContext {
    pub fn get_cell<T>(&self, handle: &AsyncCellHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.dependencies.lock().insert(handle.id);
        let mut inner = self.inner.lock();
        if inner.generation(self._node_id) == self._node_gen {
            register_dependency_locked(&mut inner, handle.id, self._node_id);
        }
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

    pub fn get_async<T>(
        &self,
        handle: &AsyncSlotHandle<T>,
    ) -> impl Future<Output = T> + Send + use<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.dependencies.lock().insert(handle.id);
        {
            let mut inner = self.inner.lock();
            if inner.generation(self._node_id) == self._node_gen {
                register_dependency_locked(&mut inner, handle.id, self._node_id);
            }
        }
        let inner_arc = self.inner.clone();
        // Copy the handle so the returned future does not borrow the `handle`
        // parameter; this keeps the future independent of caller-local handles.
        let handle = *handle;
        async move {
            let ctx = AsyncContext {
                inner: inner_arc,
                #[cfg(feature = "instrumentation")]
                window1_hook: Mutex::new(None),
                #[cfg(feature = "instrumentation")]
                window1_resolved_hits: AtomicU64::new(0),
            };
            ctx.get_async(&handle).await
        }
    }

    /// Await an eager [`AsyncSignalHandle`] from inside a slot/effect callback,
    /// registering its backing slot as a dependency.
    ///
    /// This is the in-callback counterpart to [`AsyncContext::get_signal_async`]
    /// and is what lets async signals be chained or observed by downstream
    /// computeds/effects.
    pub fn get_signal_async<T>(
        &self,
        handle: &AsyncSignalHandle<T>,
    ) -> impl Future<Output = T> + Send + use<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get_async(&handle.slot)
    }
}

fn spawn_async_compute(ctx: &AsyncContext, slot_id: SlotId) -> watch::Receiver<AsyncCompletion> {
    let inner_arc: Arc<Mutex<AsyncContextInner>> = ctx.inner.clone();
    let mut inner = inner_arc.lock();

    let (compute, context_id, spawn_revision) = match inner.get_node(slot_id) {
        Some(AsyncNode::Slot(slot)) => {
            if let AsyncSlotState::Computing { .. } = &slot.state {
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
    let slot_gen = inner.generation(slot_id);

    let join_handle = tokio::spawn(async move {
        let compute_ctx = AsyncComputeContext {
            _context_id: context_id,
            _node_id: slot_id,
            _node_gen: slot_gen,
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
                generations: Vec::new(),
                next_id: 0,
                free_ids: Vec::new(),
                context_id,
                batch_depth: 0,
                batched_cells: HashSet::new(),
                pending_async_effects: Vec::new(),
                scheduled_async_effects: HashSet::new(),
            })),
            #[cfg(feature = "instrumentation")]
            window1_hook: Mutex::new(None),
            #[cfg(feature = "instrumentation")]
            window1_resolved_hits: AtomicU64::new(0),
        }
    }

    /// Install a one-shot async seam fired inside `get_async`'s window-1 gap.
    /// Used by tests to deterministically resolve a slot in the window between
    /// the fast-path `get()` check and the re-lock, forcing the `#k03k`
    /// `Resolved`-after-re-lock arm. Test-only (`instrumentation` feature).
    #[cfg(feature = "instrumentation")]
    pub fn __install_window1_hook(&self, hook: Window1Hook) {
        *self.window1_hook.lock() = Some(hook);
    }

    /// Number of `get_async` returns that went through the window-1
    /// `Resolved`-after-re-lock arm. Test-only (`instrumentation` feature).
    #[cfg(feature = "instrumentation")]
    pub fn __window1_resolved_hits(&self) -> u64 {
        self.window1_resolved_hits.load(Ordering::Relaxed)
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
                Self::schedule_async_effect(&mut inner, *dep_id);
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
        let mut inner = self.inner.lock();
        register_dependency_locked(&mut inner, dependency_id, dependent_id);
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
            Self::schedule_async_effect(&mut inner, *dep_id);
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

    pub fn get<T>(&self, handle: &AsyncSlotHandle<T>) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let inner = self.inner.lock();
        match inner.get_node(handle.id) {
            Some(AsyncNode::Slot(slot)) => match &slot.state {
                AsyncSlotState::Resolved => {
                    let val = slot.value.as_ref().expect("resolved without value");
                    Some(
                        val.downcast_ref::<T>()
                            .expect("type mismatch in get")
                            .clone(),
                    )
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub async fn get_async<T>(&self, handle: &AsyncSlotHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        // Outer loop re-resolves from authoritative slot state. Two concurrency
        // windows make a single straight-line pass insufficient (#k03k):
        //   1. The slot can transition `Computing -> Resolved` between the
        //      `get()` fast-path check (which drops the lock) and the re-lock
        //      below — so `Resolved` is reachable here and must be read, not
        //      treated as unreachable.
        //   2. The notifier's `watch` senders can all drop without a final
        //      `Resolved` send when an in-flight compute is superseded by a
        //      newer revision (`spawn_revision` mismatch -> early return) or the
        //      slot is invalidated. A dropped notifier therefore means "the
        //      world changed", not a fatal error: restart and re-observe.
        loop {
            // Fast path: value already published.
            if let Some(val) = self.get(handle) {
                return val;
            }

            // Test-only seam (compiled out of default/release builds): fire a
            // one-shot hook in the window-1 gap so a test can resolve the slot
            // between the fast-path check above and the re-lock below,
            // deterministically reaching the `#k03k` Resolved arm.
            #[cfg(feature = "instrumentation")]
            {
                let hook = self.window1_hook.lock().take();
                if let Some(hook) = hook {
                    hook().await;
                }
            }

            let mut recv = {
                let inner = self.inner.lock();
                match inner.get_node(handle.id) {
                    Some(AsyncNode::Slot(slot)) => match &slot.state {
                        AsyncSlotState::Computing { .. } => slot
                            .notifier
                            .as_ref()
                            .expect("computing without notifier")
                            .subscribe(),
                        AsyncSlotState::Error | AsyncSlotState::Empty => {
                            drop(inner);
                            spawn_async_compute(self, handle.id)
                        }
                        AsyncSlotState::Resolved => {
                            // Window (1): resolved since the `get()` check.
                            #[cfg(feature = "instrumentation")]
                            self.window1_resolved_hits.fetch_add(1, Ordering::Relaxed);
                            let val = slot.value.as_ref().expect("resolved without value");
                            return val
                                .downcast_ref::<T>()
                                .expect("type mismatch in get_async")
                                .clone();
                        }
                    },
                    _ => panic!("AsyncSlotHandle does not point to a Slot node"),
                }
            };

            'await_completion: loop {
                if recv.changed().await.is_err() {
                    // Window (2): notifier dropped (compute superseded or slot
                    // invalidated). Re-resolve from current slot state instead
                    // of panicking.
                    break 'await_completion;
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
                        continue 'await_completion;
                    }
                    AsyncCompletion::Pending => continue 'await_completion,
                }
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
                in_flight: None,
            };
            inner.insert_node(id, AsyncNode::Effect(node));
            Self::schedule_async_effect(&mut inner, id);
        }
        let handle = AsyncEffectHandle { id };
        self.flush_async_effects();
        handle
    }

    pub fn dispose_async_effect(&self, handle: &AsyncEffectHandle) {
        let (cleanup, in_flight) = {
            let mut inner = self.inner.lock();
            inner.pending_async_effects.retain(|&id| id != handle.id);
            inner.scheduled_async_effects.remove(&handle.id);
            let (cleanup, in_flight) = match inner.get_node_mut(handle.id) {
                Some(AsyncNode::Effect(e)) => {
                    let deps = e.dependencies.clone();
                    let prior_cleanup = e.cleanup.take();
                    let prior_in_flight = e.in_flight.take();
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
                    (prior_cleanup, prior_in_flight)
                }
                _ => return,
            };
            let index = usize::try_from(handle.id.0).ok();
            if let Some(idx) = index
                && idx < inner.nodes.len()
            {
                inner.nodes[idx] = None;
                // Bump the generation BEFORE recycling the id so any task still
                // in-flight for this effect fails its generation re-check and
                // cannot write into the node a future `alloc_id` reuses here
                // (#lzasyncdispose2).
                if idx < inner.generations.len() {
                    inner.generations[idx] += 1;
                }
                inner.free_ids.push(handle.id.0);
            }
            (cleanup, in_flight)
        };
        if let Some(in_flight) = in_flight {
            in_flight.abort();
        }
        if let Some(cleanup) = cleanup {
            cleanup();
        }
    }

    // -- Signal API --------------------------------------------------------

    /// Create an **eager** async derived value that drives its recomputation to
    /// completion the instant one of its dependencies is invalidated.
    ///
    /// This is the [`AsyncContext`] counterpart to [`Context::signal`]. Where
    /// [`computed_async`](Self::computed_async)/[`memo_async`](Self::memo_async)
    /// is lazy (re-resolved on the next `get_async`), a signal is eager: a
    /// puller effect awaits the backing slot after every invalidation, so by the
    /// time the spawned recompute finishes the signal already holds its new
    /// value without anyone reading it.
    ///
    /// The signal is backed by a memoized slot, so a recomputation that yields
    /// an equal value (via `PartialEq`) does not invalidate downstream
    /// dependents. Recomputation is pull-based and therefore glitch-free.
    ///
    /// Because resolution is asynchronous, eager materialization completes on
    /// the runtime rather than synchronously within the invalidating
    /// `set_cell`/`batch` call. Use [`get_signal`](Self::get_signal) for a
    /// non-blocking snapshot or [`get_signal_async`](Self::get_signal_async) to
    /// await the up-to-date value.
    pub fn signal_async<T, F, Fut>(&self, compute: F) -> AsyncSignalHandle<T>
    where
        T: PartialEq + Clone + Send + Sync + 'static,
        F: Fn(AsyncComputeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let slot = self.memo_async(compute);
        // Eager puller: awaits the backing slot after every invalidation. The
        // synchronous part of `get_async` registers the slot as a dependency of
        // this effect, so a later invalidation reschedules the puller.
        let effect = self.effect_async(move |ctx: AsyncComputeContext| {
            let fut = ctx.get_async(&slot);
            async move {
                let _ = fut.await;
                None::<fn()>
            }
        });
        AsyncSignalHandle { slot, effect }
    }

    /// Read a signal's current value if it has resolved, without awaiting.
    pub fn get_signal<T: Clone + Send + Sync + 'static>(
        &self,
        handle: &AsyncSignalHandle<T>,
    ) -> Option<T> {
        self.get(&handle.slot)
    }

    /// Await a signal's current value, driving recomputation if needed.
    pub async fn get_signal_async<T: Clone + Send + Sync + 'static>(
        &self,
        handle: &AsyncSignalHandle<T>,
    ) -> T {
        self.get_async(&handle.slot).await
    }

    /// Dispose a signal's eager puller.
    ///
    /// Stops eager recomputation; the backing value remains readable and
    /// reverts to lazy (recomputed on next read) behavior.
    pub fn dispose_signal<T>(&self, handle: &AsyncSignalHandle<T>) {
        self.dispose_async_effect(&handle.effect);
    }

    /// Check whether a signal's eager puller is still active.
    pub fn is_signal_active<T>(&self, handle: &AsyncSignalHandle<T>) -> bool {
        let inner = self.inner.lock();
        matches!(inner.get_node(handle.effect.id), Some(AsyncNode::Effect(_)))
    }

    fn schedule_async_effect(inner: &mut AsyncContextInner, id: SlotId) {
        if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(id) {
            e.force_run = true;
        }
        if inner.scheduled_async_effects.insert(id) {
            inner.pending_async_effects.push(id);
        }
    }

    fn flush_async_effects(&self) {
        let effect_ids: Vec<SlotId>;
        {
            let mut inner = self.inner.lock();
            effect_ids = inner.pending_async_effects.drain(..).collect();
            inner.scheduled_async_effects.clear();
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
                // Abort any prior still-running task so the re-run does not race
                // it (#lzasyncrerunabort). Pre-fix the spawn below overwrote
                // `in_flight` without aborting, so a dependency that changed again
                // mid-`.await` left the old task running concurrently — double
                // execution plus a leaked (overwritten) cleanup. The cleanup of a
                // *completed* prior run still lives in `e.cleanup` and is drained
                // inside the spawned task below; aborting only cancels an
                // unfinished `.await`, never an already-stored cleanup.
                {
                    let mut inner = self.inner.lock();
                    if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id)
                        && let Some(prior) = e.in_flight.take()
                    {
                        prior.abort();
                    }
                }
                let inner_for_ctx = ctx_inner.clone();
                // Capture the generation of this effect at spawn time. Every
                // write keyed by `effect_id` below re-checks it so a run still
                // in-flight after a concurrent `dispose_async_effect` (which
                // bumps the generation and recycles the id) can never alias a
                // freshly-allocated node that reused the id (#lzasyncdispose2).
                let effect_gen = {
                    let inner = self.inner.lock();
                    inner.generation(effect_id)
                };
                let join = tokio::spawn(async move {
                    {
                        let mut inner = inner_for_ctx.lock();
                        if inner.generation(effect_id) == effect_gen
                            && let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id)
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
                        _node_gen: effect_gen,
                        inner: inner_for_ctx.clone(),
                        dependencies: deps_arc,
                    };
                    let cleanup = fn_arc(compute_ctx).await;
                    let deps = deps_for_extract.lock().clone();
                    {
                        let mut inner = inner_for_ctx.lock();
                        if inner.generation(effect_id) == effect_gen {
                            AsyncContext::update_effect_dependencies(&mut inner, effect_id, &deps);
                            if let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id) {
                                e.cleanup = cleanup;
                            }
                        } else {
                            // The effect was disposed (and its id possibly
                            // recycled) while this run was in-flight. Never write
                            // cleanup/edges into the aliased node; instead run
                            // THIS run's own cleanup so its side effects are
                            // still undone rather than leaked (#lzasyncdispose2).
                            drop(inner);
                            if let Some(cleanup) = cleanup {
                                cleanup();
                            }
                        }
                    }
                });
                let mut inner = self.inner.lock();
                if inner.generation(effect_id) == effect_gen
                    && let Some(AsyncNode::Effect(e)) = inner.get_node_mut(effect_id)
                {
                    e.in_flight = Some(join);
                }
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

    // #lzasyncdispose2: disposing an effect bumps the per-index generation and
    // recycles the SlotId; the next allocation reuses the id but sees the
    // bumped generation, which is what lets an in-flight stale run detect that
    // it no longer owns the node.
    #[tokio::test]
    async fn dispose_bumps_generation_then_id_recycles_fresh() {
        let ctx = Arc::new(AsyncContext::new());
        let cell = ctx.cell(1i32);
        let effect_a = ctx.effect_async(move |c| {
            let _v = c.get_cell(&cell);
            async move { None::<fn()> }
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let a_id = effect_a.id;
        let g0 = ctx.inner.lock().generation(a_id);

        // Allocate B's dependency BEFORE dispose so the recycled id goes to the
        // effect (free_ids LIFO), not to this cell.
        let cell_b = ctx.cell(2i32);
        ctx.dispose_async_effect(&effect_a);
        let g1 = ctx.inner.lock().generation(a_id);
        assert_eq!(g1, g0 + 1, "dispose must bump the node generation");

        // Reuse the recycled id with a fresh effect; the bumped generation
        // sticks so any task still holding `g0` can detect the reuse.
        let effect_b = ctx.effect_async(move |c| {
            let _v = c.get_cell(&cell_b);
            async move { None::<fn()> }
        });
        assert_eq!(
            effect_b.id, a_id,
            "free_ids LIFO should recycle A's id for B"
        );
        assert_eq!(
            ctx.inner.lock().generation(effect_b.id),
            g1,
            "recycled node keeps the bumped generation",
        );
    }

    // #lzasyncdispose2: a run still in-flight after its effect was disposed (and
    // its id recycled to a NEW effect) must not write its edges/cleanup into the
    // aliased node. `dispose`'s `abort()` is the first defense; this guards the
    // window where the run already passed its `.await` and `abort()` lost the
    // race. We exercise that second defense directly by replaying a stale-
    // generation compute context against a recycled id.
    #[tokio::test]
    async fn stale_generation_context_does_not_alias_recycled_effect() {
        let ctx = Arc::new(AsyncContext::new());
        let cell = ctx.cell(1i32);
        let effect_a = ctx.effect_async(move |c| {
            let _v = c.get_cell(&cell);
            async move { None::<fn()> }
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let a_id = effect_a.id;

        // Capture a compute context exactly as A's in-flight run would hold it.
        // Each lock is its own statement so the (non-reentrant) guards do not
        // overlap.
        let ctx_id = ctx.inner.lock().context_id;
        let a_gen = ctx.inner.lock().generation(a_id);
        let stale_ctx = AsyncComputeContext {
            _context_id: ctx_id,
            _node_id: a_id,
            _node_gen: a_gen,
            inner: ctx.inner.clone(),
            dependencies: Arc::new(Mutex::new(HashSet::new())),
        };

        // Allocate B's dependency BEFORE dispose so the recycled id (free_ids
        // LIFO) goes to the effect, then dispose A and allocate B reusing the id.
        let cell_b = ctx.cell(99i32);
        ctx.dispose_async_effect(&effect_a);
        let effect_b = ctx.effect_async(move |c| {
            let _v = c.get_cell(&cell_b);
            async move { None::<fn()> }
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(
            effect_b.id, a_id,
            "B must reuse A's recycled id for the test"
        );

        // A's stale run replays a dependency read on `cell`. Pre-fix this wrote
        // an edge `cell -> a_id` and `a_id.dependencies += cell`, aliasing B.
        let _ = stale_ctx.get_cell(&cell);

        let inner = ctx.inner.lock();
        match inner.get_node(a_id) {
            Some(AsyncNode::Effect(e)) => {
                assert!(
                    e.dependencies.contains(&cell_b.id),
                    "B's real dependency must stay intact",
                );
                assert!(
                    !e.dependencies.contains(&cell.id),
                    "stale-generation write must not alias B with A's old dependency",
                );
            }
            _ => panic!("recycled node should be B's effect"),
        }
        if let Some(AsyncNode::Cell(c)) = inner.get_node(cell.id) {
            assert!(
                !c.dependents.contains(&a_id),
                "`cell` must not gain a phantom dependent via the stale write",
            );
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
    async fn effect_rerun_aborts_prior_inflight() {
        // Re-invalidating an effect_async dependency while the effect body is
        // still .awaiting must abort the prior run, not spawn a second concurrent
        // one. Pre-#lzasyncrerunabort the prior in_flight was overwritten without
        // abort: the effect body completed twice and the overwritten run's
        // cleanup was leaked (never invoked).
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let done_count = Arc::new(AtomicU64::new(0));
        let cleanup_count = Arc::new(AtomicU64::new(0));
        let done_clone = done_count.clone();
        let cleanup_clone = cleanup_count.clone();
        let handle = ctx.effect_async(move |ctx| {
            let _v = ctx.get_cell(&cell);
            let d = done_clone.clone();
            let c = cleanup_clone.clone();
            async move {
                // Started; yield to the scheduler so the body is genuinely
                // in-flight at the sleep when the dependency flips.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                // Past the await = the run completed (only reached if NOT aborted).
                d.fetch_add(1, Ordering::Relaxed);
                let c = c.clone();
                Some(move || {
                    c.fetch_add(1, Ordering::Relaxed);
                })
            }
        });

        // First run is now parked in its sleep await.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        ctx.set_cell(&cell, 2); // re-invalidate mid-flight
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        // Exactly one completed run; the aborted prior run never passed its await.
        assert_eq!(
            done_count.load(Ordering::Relaxed),
            1,
            "prior in-flight effect must be aborted on re-run, not double-executed"
        );

        // The single surviving cleanup must fire on dispose (not be leaked).
        ctx.dispose_async_effect(&handle);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            cleanup_count.load(Ordering::Relaxed),
            1,
            "the surviving run's cleanup must run exactly once on dispose"
        );
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

    #[test]
    fn sync_get_returns_none_for_empty_slot() {
        let ctx = AsyncContext::new();
        let slot = ctx.computed_async(|_| async { 42i32 });
        assert!(ctx.get(&slot).is_none());
    }

    #[tokio::test]
    async fn sync_get_returns_some_after_resolve() {
        let ctx = AsyncContext::new();
        let slot = ctx.computed_async(|_| async { 42i32 });
        let val = ctx.get_async(&slot).await;
        assert_eq!(val, 42);
        assert_eq!(ctx.get(&slot), Some(42));
    }

    #[tokio::test]
    async fn sync_get_returns_none_after_invalidation() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(1i32);
        let slot = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            async move { v * 2 }
        });
        let _ = ctx.get_async(&slot).await;
        assert_eq!(ctx.get(&slot), Some(2));
        ctx.set_cell(&cell, 5);
        assert!(ctx.get(&slot).is_none());
    }

    #[tokio::test]
    async fn sync_get_avoids_spawn_overhead() {
        let ctx = AsyncContext::new();
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        let slot = ctx.computed_async(move |_| {
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                99i32
            }
        });
        let v1 = ctx.get_async(&slot).await;
        assert_eq!(v1, 99);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        let v2 = ctx.get(&slot);
        assert_eq!(v2, Some(99));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn sync_get_with_memo_returns_cached() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(3i32);
        let slot = ctx.memo_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            async move { v.abs() }
        });
        assert_eq!(ctx.get_async(&slot).await, 3);
        assert_eq!(ctx.get(&slot), Some(3));
    }

    #[tokio::test]
    async fn get_async_uses_sync_fast_path() {
        let ctx = AsyncContext::new();
        let cell = ctx.cell(10i32);
        let count = Arc::new(AtomicU64::new(0));
        let count_clone = count.clone();
        let slot = ctx.computed_async(move |ctx| {
            let v = ctx.get_cell(&cell);
            let c = count_clone.clone();
            async move {
                c.fetch_add(1, Ordering::Relaxed);
                v + 1
            }
        });
        let v1 = ctx.get_async(&slot).await;
        assert_eq!(v1, 11);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        let v2 = ctx.get_async(&slot).await;
        assert_eq!(v2, 11);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn async_schedule_effect_dedupes_pending_queue() {
        let ctx = AsyncContext::new();
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let cell = ctx.cell(0i32);
        let effect = ctx.effect_async(move |ctx| {
            let _ = ctx.get_cell(&cell);
            async { None::<fn()> }
        });
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(20)).await });
        {
            let mut inner = ctx.inner.lock();
            AsyncContext::schedule_async_effect(&mut inner, effect.id);
            AsyncContext::schedule_async_effect(&mut inner, effect.id);
            AsyncContext::schedule_async_effect(&mut inner, effect.id);
            let count = inner
                .pending_async_effects
                .iter()
                .filter(|&&id| id == effect.id)
                .count();
            assert_eq!(
                count, 1,
                "pending_async_effects must dedupe the same effect id; got {:?}",
                inner.pending_async_effects
            );
            assert!(inner.scheduled_async_effects.contains(&effect.id));
        }
        ctx.flush_async_effects();
        {
            let inner = ctx.inner.lock();
            assert!(
                !inner.scheduled_async_effects.contains(&effect.id),
                "flush must clear scheduled_async_effects"
            );
        }
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(20)).await });
    }
}
