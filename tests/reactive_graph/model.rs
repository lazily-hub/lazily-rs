//! The execution model abstraction for the reactive-graph conformance corpus
//! (`#lzspecedgeindex`).
//!
//! The corpus describes graph behaviour, not a particular context type, so the
//! runner is parameterised over the execution model rather than written against
//! `Context`. `lazily` ships three execution models and the disposal/teardown
//! contract must hold in all of them — leaks in the thread-safe and async paths
//! are the ones hardest to notice and hardest to reproduce, so those are
//! precisely the ones that need a contract to conform to.
//!
//! Implementations live next to the runner in `reactive_graph_conformance.rs`.
//!
//! ## Why the model builds the closures
//!
//! Trait methods take *data* (`reads`, `offset`, a name) rather than a closure.
//! Each execution model has different bounds on what a compute may capture —
//! `Context` requires only `'static`, `ThreadSafeContext` additionally requires
//! `Send + Sync`, and `AsyncContext` wants an async block — so building the
//! closure inside each impl keeps those bounds where they belong instead of
//! forcing the union of all three onto the runner.
//!
//! ## Why `read` is synchronous
//!
//! The async model blocks on its own runtime internally. Making the trait async
//! would drag an executor into the default-feature `Context` test, which has no
//! tokio dependency; pushing the blocking into the one model that needs it
//! keeps the engine executor-free.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Shared effect run / cleanup log. `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>`
/// because thread-safe compute closures must be `Send + Sync`.
pub type Log = Arc<Mutex<Vec<String>>>;

pub fn log_push(log: &Log, name: &str) {
    log.lock().unwrap().push(name.to_owned());
}

pub fn log_snapshot(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

/// Set when a read performed *inside* a compute or effect callback hit a
/// disposed node.
///
/// The callback must not unwind: `Context` pushes and pops its tracking frame
/// without an RAII guard, so unwinding out of a compute would strand a frame on
/// the thread-local stack and corrupt every later read. Catching inside the
/// callback and recording here keeps the stack balanced while still letting the
/// top-level read surface `read_after_dispose`.
pub type Poison = Arc<AtomicBool>;

/// A handle to a node in whichever execution model is under test.
pub enum Ref<M: GraphModel> {
    Cell(M::Cell),
    Slot(M::Slot),
    Effect(M::Effect),
}

// Derived impls would demand `M: Clone + Copy`, which is not what is wanted:
// the *handles* are Copy, the model is not.
impl<M: GraphModel> Clone for Ref<M> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<M: GraphModel> Copy for Ref<M> {}

/// Nodes owned by a teardown scope, created through it rather than through the
/// context directly.
pub trait ScopeModel<M: GraphModel> {
    fn cell(&self, value: i64) -> M::Cell;
    fn computed(&self, reads: &[Ref<M>], offset: i64) -> M::Slot;
    fn effect(&self, name: &str, reads: &[Ref<M>]) -> M::Effect;
    /// How many nodes the scope currently owns.
    fn owned(&self) -> usize;
    /// Cancel the scope's teardown; ending it afterwards disposes nothing.
    fn disarm(self);
}

/// One of `lazily`'s execution models, as the conformance corpus needs to drive
/// it.
pub trait GraphModel: Sized {
    type Slot: Copy;
    type Cell: Copy;
    type Effect: Copy;
    type Scope<'a>: ScopeModel<Self>
    where
        Self: 'a;

    /// Name used in assertion messages and the per-model divergence ledger.
    const NAME: &'static str;

    fn create() -> Self;

    fn cell(&self, value: i64) -> Self::Cell;
    fn computed(&self, reads: &[Ref<Self>], offset: i64) -> Self::Slot;
    fn effect(&self, name: &str, reads: &[Ref<Self>]) -> Self::Effect;

    /// Read a node's value. `Err` is the corpus's `read_after_dispose`.
    fn read(&self, node: Ref<Self>) -> Result<i64, ()>;
    fn set_cell(&self, cell: Self::Cell, value: i64);

    fn dispose(&self, node: Ref<Self>);
    fn dependent_count(&self, node: Ref<Self>) -> usize;
    fn dependency_count(&self, node: Ref<Self>) -> usize;
    fn is_effect_active(&self, effect: Self::Effect) -> bool;

    fn scope(&self) -> Self::Scope<'_>;

    /// Effects that have run, in order.
    fn run_log(&self) -> &Log;
    /// Effect cleanups that have run, in order.
    fn cleanup_log(&self) -> &Log;
    /// Whether a nested read hit a disposed node since the flag was last reset.
    fn poison(&self) -> &Poison;

    /// Drive the model to quiescence before assertions are evaluated.
    ///
    /// Synchronous models are already quiescent when an op returns, so this
    /// defaults to a no-op. Async effects are *spawned*, so the async model must
    /// let the runtime run them before `observed_by`, `observed_count`, or any
    /// degree assertion can mean anything.
    ///
    /// This changes *when* the corpus's assertions are evaluated, never *what*
    /// they assert: an effect that never runs still fails.
    fn settle(&self) {}
}
