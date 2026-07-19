//! `GraphModel` implementations for `lazily`'s three execution models.
//!
//! Each impl is responsible for its own compute-closure bounds and its own
//! read-error convention; the engine sees only `Result<i64, ()>`.

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::model::{GraphModel, Log, Poison, Ref, ScopeModel, log_push};

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
    use lazily::{CellHandle, Context, EffectHandle, SlotHandle, TeardownScope};

    pub struct BasicModel {
        pub ctx: Context,
        runs: Log,
        cleanups: Log,
        poison: Poison,
    }

    fn read_ref(ctx: &Context, node: Ref<BasicModel>) -> Result<i64, ()> {
        match node {
            Ref::Cell(h) => quiet(|| ctx.get_cell(&h)),
            Ref::Slot(h) => quiet(|| ctx.get(&h)),
            Ref::Effect(_) => Err(()),
        }
    }

    /// A read from inside a callback: never unwinds, records the failure.
    fn tracked(ctx: &Context, node: Ref<BasicModel>, poison: &Poison) -> i64 {
        match read_ref(ctx, node) {
            Ok(v) => v,
            Err(()) => {
                poison.store(true, Ordering::SeqCst);
                0
            }
        }
    }

    fn compute(
        reads: &[Ref<BasicModel>],
        offset: i64,
        poison: &Poison,
    ) -> impl Fn(&Context) -> i64 + 'static {
        let reads = reads.to_vec();
        let poison = poison.clone();
        move |c: &Context| {
            let mut acc = offset;
            for r in &reads {
                acc += tracked(c, *r, &poison);
            }
            acc
        }
    }

    fn effect_body(
        name: &str,
        reads: &[Ref<BasicModel>],
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
        fn computed(&self, reads: &[Ref<BasicModel>], offset: i64) -> SlotHandle<i64> {
            self.0.computed(compute(reads, offset, &self.3))
        }
        fn effect(&self, name: &str, reads: &[Ref<BasicModel>]) -> EffectHandle {
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
        type Slot = SlotHandle<i64>;
        type Cell = CellHandle<i64>;
        type Effect = EffectHandle;
        type Scope<'a> = BasicScope<'a>;

        const NAME: &'static str = "Context";

        fn create() -> Self {
            Self {
                ctx: Context::new(),
                runs: Log::default(),
                cleanups: Log::default(),
                poison: Arc::new(AtomicBool::new(false)),
            }
        }

        fn cell(&self, value: i64) -> Self::Cell {
            self.ctx.cell(value)
        }
        fn computed(&self, reads: &[Ref<Self>], offset: i64) -> Self::Slot {
            self.ctx.computed(compute(reads, offset, &self.poison))
        }
        fn effect(&self, name: &str, reads: &[Ref<Self>]) -> Self::Effect {
            self.ctx.effect(effect_body(
                name,
                reads,
                &self.runs,
                &self.cleanups,
                &self.poison,
            ))
        }
        fn read(&self, node: Ref<Self>) -> Result<i64, ()> {
            read_ref(&self.ctx, node)
        }
        fn set_cell(&self, cell: Self::Cell, value: i64) {
            self.ctx.set_cell(&cell, value);
        }
        fn dispose(&self, node: Ref<Self>) {
            match node {
                Ref::Cell(h) => self.ctx.dispose_cell(&h),
                Ref::Slot(h) => self.ctx.dispose_slot(&h),
                Ref::Effect(h) => self.ctx.dispose_effect(&h),
            }
        }
        fn dependent_count(&self, node: Ref<Self>) -> usize {
            match node {
                Ref::Cell(h) => self.ctx.dependent_count(&h),
                Ref::Slot(h) => self.ctx.dependent_count(&h),
                Ref::Effect(h) => self.ctx.dependent_count(&h),
            }
        }
        fn dependency_count(&self, node: Ref<Self>) -> usize {
            match node {
                Ref::Cell(h) => self.ctx.dependency_count(&h),
                Ref::Slot(h) => self.ctx.dependency_count(&h),
                Ref::Effect(h) => self.ctx.dependency_count(&h),
            }
        }
        fn is_effect_active(&self, effect: Self::Effect) -> bool {
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
        CellHandle, EffectHandle, SlotHandle, ThreadSafeContext, ThreadSafeTeardownScope,
    };

    pub struct ThreadSafeModel {
        pub ctx: ThreadSafeContext,
        runs: Log,
        cleanups: Log,
        poison: Poison,
    }

    fn read_ref(ctx: &ThreadSafeContext, node: Ref<ThreadSafeModel>) -> Result<i64, ()> {
        match node {
            Ref::Cell(h) => quiet(|| ctx.get_cell(&h)),
            Ref::Slot(h) => quiet(|| ctx.get(&h)),
            Ref::Effect(_) => Err(()),
        }
    }

    fn tracked(ctx: &ThreadSafeContext, node: Ref<ThreadSafeModel>, poison: &Poison) -> i64 {
        match read_ref(ctx, node) {
            Ok(v) => v,
            Err(()) => {
                poison.store(true, Ordering::SeqCst);
                0
            }
        }
    }

    fn compute(
        reads: &[Ref<ThreadSafeModel>],
        offset: i64,
        poison: &Poison,
    ) -> impl Fn(&ThreadSafeContext) -> i64 + Send + Sync + 'static {
        let reads = reads.to_vec();
        let poison = poison.clone();
        move |c: &ThreadSafeContext| {
            let mut acc = offset;
            for r in &reads {
                acc += tracked(c, *r, &poison);
            }
            acc
        }
    }

    fn effect_body(
        name: &str,
        reads: &[Ref<ThreadSafeModel>],
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
        fn computed(&self, reads: &[Ref<ThreadSafeModel>], offset: i64) -> SlotHandle<i64> {
            self.0.computed(compute(reads, offset, &self.3))
        }
        fn effect(&self, name: &str, reads: &[Ref<ThreadSafeModel>]) -> EffectHandle {
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
        type Slot = SlotHandle<i64>;
        type Cell = CellHandle<i64>;
        type Effect = EffectHandle;
        type Scope<'a> = ThreadSafeScope;

        const NAME: &'static str = "ThreadSafeContext";

        fn create() -> Self {
            Self {
                ctx: ThreadSafeContext::new(),
                runs: Log::default(),
                cleanups: Log::default(),
                poison: Arc::new(AtomicBool::new(false)),
            }
        }

        fn cell(&self, value: i64) -> Self::Cell {
            self.ctx.cell(value)
        }
        fn computed(&self, reads: &[Ref<Self>], offset: i64) -> Self::Slot {
            self.ctx.computed(compute(reads, offset, &self.poison))
        }
        fn effect(&self, name: &str, reads: &[Ref<Self>]) -> Self::Effect {
            self.ctx.effect(effect_body(
                name,
                reads,
                &self.runs,
                &self.cleanups,
                &self.poison,
            ))
        }
        fn read(&self, node: Ref<Self>) -> Result<i64, ()> {
            read_ref(&self.ctx, node)
        }
        fn set_cell(&self, cell: Self::Cell, value: i64) {
            self.ctx.set_cell(&cell, value);
        }
        fn dispose(&self, node: Ref<Self>) {
            match node {
                Ref::Cell(h) => self.ctx.dispose_cell(&h),
                Ref::Slot(h) => self.ctx.dispose_slot(&h),
                Ref::Effect(h) => self.ctx.dispose_effect(&h),
            }
        }
        fn dependent_count(&self, node: Ref<Self>) -> usize {
            match node {
                Ref::Cell(h) => self.ctx.dependent_count(&h),
                Ref::Slot(h) => self.ctx.dependent_count(&h),
                Ref::Effect(h) => self.ctx.dependent_count(&h),
            }
        }
        fn dependency_count(&self, node: Ref<Self>) -> usize {
            match node {
                Ref::Cell(h) => self.ctx.dependency_count(&h),
                Ref::Slot(h) => self.ctx.dependency_count(&h),
                Ref::Effect(h) => self.ctx.dependency_count(&h),
            }
        }
        fn is_effect_active(&self, effect: Self::Effect) -> bool {
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
//
// Added alongside its `dispose_slot`/`dispose_cell`/`scope()` implementation.
