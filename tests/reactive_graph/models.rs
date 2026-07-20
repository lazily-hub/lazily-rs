//! `GraphModel` implementations for `lazily`'s three execution models.
//!
//! Each impl is responsible for its own compute-closure bounds and its own
//! read-error convention; the engine sees only `Result<i64, ()>`.

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::model::{Computes, GraphModel, Log, Poison, Ref, ScopeModel, count_computes, log_push};

/// Run `f`, converting a panic into `Err(())` with the message suppressed.
///
/// Reading a disposed node panics in every model — that is the library's
/// expression of the corpus's `read_after_dispose`.
pub fn quiet<R>(f: impl FnOnce() -> R) -> Result<R, ()> {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let out = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(prev);
    out.map_err(|_| ())
}

// -- Context ----------------------------------------------------------------

mod basic {
    use super::*;
    use lazily::{CellHandle, Context, EffectHandle, SignalHandle, SlotHandle, TeardownScope};

    pub struct BasicModel {
        pub ctx: Context,
        runs: Log,
        cleanups: Log,
        poison: Poison,
    }

    fn read_ref(ctx: &Context, node: Ref<Context>) -> Result<i64, ()> {
        match node {
            Ref::Cell(h) => quiet(|| ctx.get_cell(&h)),
            Ref::Slot(h) => quiet(|| ctx.get(&h)),
            Ref::Effect(_) => Err(()),
        }
    }

    /// A read from inside a callback: never unwinds, records the failure.
    fn tracked(ctx: &Context, node: Ref<Context>, poison: &Poison) -> i64 {
        match read_ref(ctx, node) {
            Ok(v) => v,
            Err(()) => {
                poison.store(true, Ordering::SeqCst);
                0
            }
        }
    }

    fn compute(
        reads: &[Ref<Context>],
        offset: i64,
        poison: &Poison,
        computes: &Computes,
    ) -> impl Fn(&Context) -> i64 + 'static {
        let reads = reads.to_vec();
        let poison = poison.clone();
        let computes = computes.clone();
        move |c: &Context| {
            // Counted here, inside the body the runtime invokes — see
            // `Computes`. Counting at the construction site instead would make
            // a lazy memo indistinguishable from an eager signal.
            count_computes(&computes);
            let mut acc = offset;
            for r in &reads {
                acc += tracked(c, *r, &poison);
            }
            acc
        }
    }

    fn effect_body(
        name: &str,
        reads: &[Ref<Context>],
        runs: &Log,
        cleanups: &Log,
        poison: &Poison,
    ) -> impl Fn(&Context) -> Box<dyn FnOnce()> + 'static {
        let reads = reads.to_vec();
        let name = name.to_owned();
        let runs = runs.clone();
        let cleanups = cleanups.clone();
        let poison = poison.clone();
        move |c: &Context| {
            for r in &reads {
                tracked(c, *r, &poison);
            }
            log_push(&runs, &name);
            let cleanups = cleanups.clone();
            let name = name.clone();
            Box::new(move || log_push(&cleanups, &name)) as Box<dyn FnOnce()>
        }
    }

    pub struct BasicScope<'a>(TeardownScope<'a>, Log, Log, Poison);

    impl ScopeModel<BasicModel> for BasicScope<'_> {
        fn cell(&self, value: i64) -> CellHandle<i64> {
            self.0.cell(value)
        }
        fn computed(
            &self,
            reads: &[Ref<Context>],
            offset: i64,
            computes: &Computes,
        ) -> SlotHandle<i64> {
            self.0.computed(compute(reads, offset, &self.3, computes))
        }
        fn effect(&self, name: &str, reads: &[Ref<Context>]) -> EffectHandle {
            self.0
                .effect(effect_body(name, reads, &self.1, &self.2, &self.3))
        }
        fn owned(&self) -> usize {
            self.0.len()
        }
        fn disarm(self) {
            self.0.disarm();
        }
    }

    impl GraphModel for BasicModel {
        type Graph = Context;
        type Scope<'a> = BasicScope<'a>;
        type Signal = SignalHandle<i64>;

        const NAME: &'static str = "Context";

        fn create() -> Self {
            Self {
                ctx: Context::new(),
                runs: Log::default(),
                cleanups: Log::default(),
                poison: Arc::new(AtomicBool::new(false)),
            }
        }

        fn graph(&self) -> &Self::Graph {
            &self.ctx
        }

        fn cell(&self, value: i64) -> CellHandle<i64> {
            self.ctx.cell(value)
        }
        fn computed(
            &self,
            reads: &[Ref<Self::Graph>],
            offset: i64,
            computes: &Computes,
        ) -> SlotHandle<i64> {
            self.ctx
                .computed(compute(reads, offset, &self.poison, computes))
        }
        fn effect(&self, name: &str, reads: &[Ref<Self::Graph>]) -> EffectHandle {
            self.ctx.effect(effect_body(
                name,
                reads,
                &self.runs,
                &self.cleanups,
                &self.poison,
            ))
        }
        fn signal(
            &self,
            reads: &[Ref<Self::Graph>],
            offset: i64,
            computes: &Computes,
        ) -> SignalHandle<i64> {
            self.ctx
                .signal(compute(reads, offset, &self.poison, computes))
        }
        fn read_signal(&self, signal: &Self::Signal) -> Result<i64, ()> {
            quiet(|| self.ctx.get_signal(signal))
        }
        fn dispose_signal(&self, signal: &Self::Signal) {
            self.ctx.dispose_signal(signal);
        }
        fn batch(&self, writes: &[(CellHandle<i64>, i64)]) {
            self.ctx.batch(|c| {
                for (h, v) in writes {
                    c.set_cell(h, *v);
                }
            });
        }
        fn read(&self, node: Ref<Self::Graph>) -> Result<i64, ()> {
            read_ref(&self.ctx, node)
        }
        fn set_cell(&self, cell: CellHandle<i64>, value: i64) {
            self.ctx.set_cell(&cell, value);
        }
        fn is_effect_active(&self, effect: EffectHandle) -> bool {
            self.ctx.is_effect_active(&effect)
        }
        fn scope(&self) -> Self::Scope<'_> {
            BasicScope(
                self.ctx.scope(),
                self.runs.clone(),
                self.cleanups.clone(),
                self.poison.clone(),
            )
        }
        fn run_log(&self) -> &Log {
            &self.runs
        }
        fn cleanup_log(&self) -> &Log {
            &self.cleanups
        }
        fn poison(&self) -> &Poison {
            &self.poison
        }
    }
}

pub use basic::BasicModel;

// -- ThreadSafeContext ------------------------------------------------------

#[cfg(feature = "thread-safe")]
mod threadsafe {
    use super::*;
    use lazily::{
        CellHandle, EffectHandle, SlotHandle, ThreadSafeContext, ThreadSafeSignalHandle,
        ThreadSafeTeardownScope,
    };

    pub struct ThreadSafeModel {
        pub ctx: ThreadSafeContext,
        runs: Log,
        cleanups: Log,
        poison: Poison,
    }

    fn read_ref(ctx: &ThreadSafeContext, node: Ref<ThreadSafeContext>) -> Result<i64, ()> {
        match node {
            Ref::Cell(h) => quiet(|| ctx.get_cell(&h)),
            Ref::Slot(h) => quiet(|| ctx.get(&h)),
            Ref::Effect(_) => Err(()),
        }
    }

    fn tracked(ctx: &ThreadSafeContext, node: Ref<ThreadSafeContext>, poison: &Poison) -> i64 {
        match read_ref(ctx, node) {
            Ok(v) => v,
            Err(()) => {
                poison.store(true, Ordering::SeqCst);
                0
            }
        }
    }

    fn compute(
        reads: &[Ref<ThreadSafeContext>],
        offset: i64,
        poison: &Poison,
        computes: &Computes,
    ) -> impl Fn(&ThreadSafeContext) -> i64 + Send + Sync + 'static {
        let reads = reads.to_vec();
        let poison = poison.clone();
        let computes = computes.clone();
        move |c: &ThreadSafeContext| {
            count_computes(&computes);
            let mut acc = offset;
            for r in &reads {
                acc += tracked(c, *r, &poison);
            }
            acc
        }
    }

    fn effect_body(
        name: &str,
        reads: &[Ref<ThreadSafeContext>],
        runs: &Log,
        cleanups: &Log,
        poison: &Poison,
    ) -> impl Fn(&ThreadSafeContext) -> Box<dyn FnOnce() + Send + Sync> + Send + Sync + 'static
    {
        let reads = reads.to_vec();
        let name = name.to_owned();
        let runs = runs.clone();
        let cleanups = cleanups.clone();
        let poison = poison.clone();
        move |c: &ThreadSafeContext| {
            for r in &reads {
                tracked(c, *r, &poison);
            }
            log_push(&runs, &name);
            let cleanups = cleanups.clone();
            let name = name.clone();
            Box::new(move || log_push(&cleanups, &name)) as Box<dyn FnOnce() + Send + Sync>
        }
    }

    /// Owned, not borrowed — see `ThreadSafeContext::scope`. The GAT lifetime is
    /// simply unused here, which is the point: this scope is `Send` and can
    /// outlive the borrow that produced it.
    pub struct ThreadSafeScope(ThreadSafeTeardownScope, Log, Log, Poison);

    impl ScopeModel<ThreadSafeModel> for ThreadSafeScope {
        fn cell(&self, value: i64) -> CellHandle<i64> {
            self.0.cell(value)
        }
        fn computed(
            &self,
            reads: &[Ref<ThreadSafeContext>],
            offset: i64,
            computes: &Computes,
        ) -> SlotHandle<i64> {
            self.0.computed(compute(reads, offset, &self.3, computes))
        }
        fn effect(&self, name: &str, reads: &[Ref<ThreadSafeContext>]) -> EffectHandle {
            self.0
                .effect(effect_body(name, reads, &self.1, &self.2, &self.3))
        }
        fn owned(&self) -> usize {
            self.0.len()
        }
        fn disarm(self) {
            self.0.disarm();
        }
    }

    impl GraphModel for ThreadSafeModel {
        type Graph = ThreadSafeContext;
        type Scope<'a> = ThreadSafeScope;
        type Signal = ThreadSafeSignalHandle<i64>;

        const NAME: &'static str = "ThreadSafeContext";

        fn create() -> Self {
            Self {
                ctx: ThreadSafeContext::new(),
                runs: Log::default(),
                cleanups: Log::default(),
                poison: Arc::new(AtomicBool::new(false)),
            }
        }

        fn graph(&self) -> &Self::Graph {
            &self.ctx
        }

        fn cell(&self, value: i64) -> CellHandle<i64> {
            self.ctx.cell(value)
        }
        fn computed(
            &self,
            reads: &[Ref<Self::Graph>],
            offset: i64,
            computes: &Computes,
        ) -> SlotHandle<i64> {
            self.ctx
                .computed(compute(reads, offset, &self.poison, computes))
        }
        fn effect(&self, name: &str, reads: &[Ref<Self::Graph>]) -> EffectHandle {
            self.ctx.effect(effect_body(
                name,
                reads,
                &self.runs,
                &self.cleanups,
                &self.poison,
            ))
        }
        fn signal(
            &self,
            reads: &[Ref<Self::Graph>],
            offset: i64,
            computes: &Computes,
        ) -> ThreadSafeSignalHandle<i64> {
            self.ctx
                .signal(compute(reads, offset, &self.poison, computes))
        }
        fn read_signal(&self, signal: &Self::Signal) -> Result<i64, ()> {
            quiet(|| self.ctx.get_signal(signal))
        }
        fn dispose_signal(&self, signal: &Self::Signal) {
            self.ctx.dispose_signal(signal);
        }
        fn batch(&self, writes: &[(CellHandle<i64>, i64)]) {
            self.ctx.batch(|c| {
                for (h, v) in writes {
                    c.set_cell(h, *v);
                }
            });
        }
        fn read(&self, node: Ref<Self::Graph>) -> Result<i64, ()> {
            read_ref(&self.ctx, node)
        }
        fn set_cell(&self, cell: CellHandle<i64>, value: i64) {
            self.ctx.set_cell(&cell, value);
        }
        fn is_effect_active(&self, effect: EffectHandle) -> bool {
            self.ctx.is_effect_active(&effect)
        }
        fn scope(&self) -> Self::Scope<'_> {
            ThreadSafeScope(
                self.ctx.scope(),
                self.runs.clone(),
                self.cleanups.clone(),
                self.poison.clone(),
            )
        }
        fn run_log(&self) -> &Log {
            &self.runs
        }
        fn cleanup_log(&self) -> &Log {
            &self.cleanups
        }
        fn poison(&self) -> &Poison {
            &self.poison
        }
    }
}

#[cfg(feature = "thread-safe")]
pub use threadsafe::ThreadSafeModel;

// -- AsyncContext -----------------------------------------------------------

#[cfg(feature = "async")]
mod asynchronous {
    use super::*;
    use lazily::{
        AsyncCellHandle, AsyncComputeContext, AsyncContext, AsyncEffectHandle, AsyncSignalHandle,
        AsyncSlotHandle, AsyncTeardownScope,
    };

    /// The async model owns its runtime and blocks on it inside `read`, so the
    /// engine stays synchronous and the default-feature `Context` test never
    /// needs an executor.
    pub struct AsyncModel {
        pub ctx: AsyncContext,
        rt: tokio::runtime::Runtime,
        runs: Log,
        cleanups: Log,
        poison: Poison,
    }

    /// Computes receive an `AsyncComputeContext`, not an owned `AsyncContext` —
    /// it carries the node id and the generation captured at spawn, which is
    /// what makes dependency registration safe across an await.
    ///
    /// The `use<>` on the return type says the closure captures no lifetime: it
    /// clones everything it needs out of the arguments. Without it Rust 2024
    /// infers a capture of `&Computes` and rejects the temporary counter that
    /// `effect_body` passes.
    fn compute(
        reads: &[Ref<AsyncContext>],
        offset: i64,
        poison: &Poison,
        computes: &Computes,
    ) -> impl Fn(AsyncComputeContext) -> BoxFuture + Send + Sync + 'static + use<> {
        let reads = reads.to_vec();
        let poison = poison.clone();
        let computes = computes.clone();
        move |c: AsyncComputeContext| {
            let reads = reads.clone();
            let poison = poison.clone();
            let computes = computes.clone();
            Box::pin(async move {
                count_computes(&computes);
                let mut acc = offset;
                for r in &reads {
                    match read_in_compute(&c, *r).await {
                        Ok(v) => acc += v,
                        Err(()) => poison.store(true, Ordering::SeqCst),
                    }
                }
                acc
            })
        }
    }

    type BoxFuture = std::pin::Pin<Box<dyn std::future::Future<Output = i64> + Send>>;

    /// A read from inside a compute. Reading a disposed node panics, and
    /// `catch_unwind` cannot span an await point, so the panic is caught around
    /// the two halves separately: building the future, and driving it.
    async fn read_in_compute(c: &AsyncComputeContext, node: Ref<AsyncContext>) -> Result<i64, ()> {
        match node {
            Ref::Cell(h) => quiet(|| c.get_cell(&h)),
            Ref::Slot(h) => match quiet(|| c.get_async(&h)) {
                Ok(fut) => {
                    let prev = std::panic::take_hook();
                    std::panic::set_hook(Box::new(|_| {}));
                    let out = tokio::spawn(fut).await;
                    std::panic::set_hook(prev);
                    out.map_err(|_| ())
                }
                Err(()) => Err(()),
            },
            Ref::Effect(_) => Err(()),
        }
    }

    fn effect_body(
        name: &str,
        reads: &[Ref<AsyncContext>],
        runs: &Log,
        cleanups: &Log,
        poison: &Poison,
    ) -> impl Fn(AsyncComputeContext) -> EffectFuture + Send + Sync + 'static {
        // A private counter: an effect body is not a node the corpus can name in
        // `computes_of`, so its runs must not land on any node's count.
        let uncounted = Computes::default();
        let body = compute(reads, 0, poison, &uncounted);
        let name = name.to_owned();
        let runs = runs.clone();
        let cleanups = cleanups.clone();
        move |c: AsyncComputeContext| {
            let fut = body(c);
            let name = name.clone();
            let runs = runs.clone();
            let cleanups = cleanups.clone();
            Box::pin(async move {
                let _ = fut.await;
                log_push(&runs, &name);
                let cleanup_name = name.clone();
                Some(Box::new(move || log_push(&cleanups, &cleanup_name))
                    as Box<dyn FnOnce() + Send>)
            })
        }
    }

    type EffectFuture = std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<Box<dyn FnOnce() + Send>>> + Send>,
    >;

    /// Carries a runtime `Handle` because effect registration spawns a task, and
    /// `tokio::spawn` panics outside a runtime context. Disposal only calls
    /// `JoinHandle::abort`, which does not need one.
    pub struct AsyncScope(AsyncTeardownScope, Log, Log, Poison, tokio::runtime::Handle);

    impl ScopeModel<AsyncModel> for AsyncScope {
        fn cell(&self, value: i64) -> AsyncCellHandle<i64> {
            let _guard = self.4.enter();
            self.0.cell(value)
        }
        fn computed(
            &self,
            reads: &[Ref<AsyncContext>],
            offset: i64,
            computes: &Computes,
        ) -> AsyncSlotHandle<i64> {
            let _guard = self.4.enter();
            self.0
                .computed_async(compute(reads, offset, &self.3, computes))
        }
        fn effect(&self, name: &str, reads: &[Ref<AsyncContext>]) -> AsyncEffectHandle {
            let _guard = self.4.enter();
            self.0
                .effect_async(effect_body(name, reads, &self.1, &self.2, &self.3))
        }
        fn owned(&self) -> usize {
            self.0.len()
        }
        fn disarm(self) {
            self.0.disarm();
        }
    }

    impl GraphModel for AsyncModel {
        type Graph = AsyncContext;
        type Scope<'a> = AsyncScope;
        type Signal = AsyncSignalHandle<i64>;

        const NAME: &'static str = "AsyncContext";

        fn create() -> Self {
            Self {
                ctx: AsyncContext::new(),
                rt: tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .unwrap(),
                runs: Log::default(),
                cleanups: Log::default(),
                poison: Arc::new(AtomicBool::new(false)),
            }
        }

        fn graph(&self) -> &Self::Graph {
            &self.ctx
        }

        fn cell(&self, value: i64) -> AsyncCellHandle<i64> {
            let _guard = self.rt.enter();
            self.ctx.cell(value)
        }
        fn computed(
            &self,
            reads: &[Ref<Self::Graph>],
            offset: i64,
            computes: &Computes,
        ) -> AsyncSlotHandle<i64> {
            let _guard = self.rt.enter();
            self.ctx
                .computed_async(compute(reads, offset, &self.poison, computes))
        }
        fn effect(&self, name: &str, reads: &[Ref<Self::Graph>]) -> AsyncEffectHandle {
            let _guard = self.rt.enter();
            self.ctx.effect_async(effect_body(
                name,
                reads,
                &self.runs,
                &self.cleanups,
                &self.poison,
            ))
        }
        fn signal(
            &self,
            reads: &[Ref<Self::Graph>],
            offset: i64,
            computes: &Computes,
        ) -> AsyncSignalHandle<i64> {
            let _guard = self.rt.enter();
            self.ctx
                .signal_async(compute(reads, offset, &self.poison, computes))
        }
        /// Awaits rather than snapshotting: `get_signal` returns `Option`, and
        /// treating an unresolved snapshot as a read would let a signal that
        /// never materialized pass as readable.
        fn read_signal(&self, signal: &Self::Signal) -> Result<i64, ()> {
            quiet(|| self.rt.block_on(self.ctx.get_signal_async(signal)))
        }
        fn dispose_signal(&self, signal: &Self::Signal) {
            self.ctx.dispose_signal(signal);
        }
        fn batch(&self, writes: &[(AsyncCellHandle<i64>, i64)]) {
            let _guard = self.rt.enter();
            self.ctx.batch(|c| {
                for (h, v) in writes {
                    c.set_cell(h, *v);
                }
            });
        }
        fn read(&self, node: Ref<Self::Graph>) -> Result<i64, ()> {
            match node {
                Ref::Cell(h) => quiet(|| self.ctx.get_cell(&h)),
                Ref::Slot(h) => quiet(|| self.rt.block_on(self.ctx.get_async(&h))),
                Ref::Effect(_) => Err(()),
            }
        }
        fn set_cell(&self, cell: AsyncCellHandle<i64>, value: i64) {
            let _guard = self.rt.enter();
            self.ctx.set_cell(&cell, value);
        }
        fn is_effect_active(&self, effect: AsyncEffectHandle) -> bool {
            self.ctx.is_async_effect_active(&effect)
        }
        fn scope(&self) -> Self::Scope<'_> {
            let _guard = self.rt.enter();
            AsyncScope(
                self.ctx.scope(),
                self.runs.clone(),
                self.cleanups.clone(),
                self.poison.clone(),
                self.rt.handle().clone(),
            )
        }
        fn run_log(&self) -> &Log {
            &self.runs
        }
        fn cleanup_log(&self) -> &Log {
            &self.cleanups
        }
        fn poison(&self) -> &Poison {
            &self.poison
        }

        /// Async effects are spawned tasks; let them run before the engine
        /// evaluates assertions that depend on an effect having executed.
        fn settle(&self) {
            self.rt.block_on(async {
                for _ in 0..32 {
                    tokio::task::yield_now().await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                for _ in 0..32 {
                    tokio::task::yield_now().await;
                }
            });
        }
    }
}

#[cfg(feature = "async")]
pub use asynchronous::AsyncModel;
