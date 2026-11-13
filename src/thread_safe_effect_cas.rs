//! `Send + Sync` reactive shell for [`EffectCasCore`](crate::EffectCasCore).

use std::sync::{Arc, Mutex};

use crate::cell::Computed;
use crate::effect_cas_core::{
    EffectApply, EffectAttemptIdentity, EffectCasApplied, EffectCasChange, EffectCasClaim,
    EffectCasCore, EffectCasFailure, EffectCasOutcome, EffectCasReconnect, EffectCasSnapshot,
    EffectCasUpsert, EffectIntent,
};
use crate::thread_safe::ThreadSafeContext;

type Core<K, A, P, R, E> = EffectCasCore<K, A, P, R, E>;
type SharedCore<K, A, P, R, E> = Arc<Mutex<Core<K, A, P, R, E>>>;
type Snapshot<K, A, P, R, E> = EffectCasSnapshot<K, A, P, R, E>;

struct Inner<K, A, P, R, E> {
    core: SharedCore<K, A, P, R, E>,
    state: Computed<Snapshot<K, A, P, R, E>>,
}

/// Thread-safe reactive keyed effect-CAS authority.
pub struct ThreadSafeEffectCas<K, A, P, R, E> {
    inner: Arc<Inner<K, A, P, R, E>>,
}

impl<K, A, P, R, E> Clone for ThreadSafeEffectCas<K, A, P, R, E> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, A, P, R, E> ThreadSafeEffectCas<K, A, P, R, E>
where
    K: Ord + Clone + Send + Sync + 'static,
    A: Clone + PartialEq + Send + Sync + 'static,
    P: Clone + PartialEq + Send + Sync + 'static,
    R: Clone + PartialEq + Send + Sync + 'static,
    E: Clone + PartialEq + Send + Sync + 'static,
{
    pub fn new(ctx: &ThreadSafeContext, generation: u64) -> Self {
        let core = Arc::new(Mutex::new(EffectCasCore::new(generation)));
        let state = {
            let core = Arc::clone(&core);
            ctx.computed(move |_| core.lock().expect("effect-CAS core").snapshot())
        };
        Self {
            inner: Arc::new(Inner { core, state }),
        }
    }

    fn apply(&self, ctx: &ThreadSafeContext, change: EffectCasChange) {
        if change.state {
            ctx.clear(&self.inner.state);
        }
    }

    pub fn generation(&self) -> u64 {
        self.inner
            .core
            .lock()
            .expect("effect-CAS core")
            .generation()
    }

    pub fn snapshot(&self, ctx: &ThreadSafeContext) -> Snapshot<K, A, P, R, E> {
        ctx.get(&self.inner.state)
    }

    pub fn state_handle(&self) -> Computed<Snapshot<K, A, P, R, E>> {
        self.inner.state
    }

    pub fn pending_keys(&self, now: u64) -> Vec<K> {
        self.inner
            .core
            .lock()
            .expect("effect-CAS core")
            .pending_keys(now)
    }

    pub fn upsert_desired(
        &self,
        ctx: &ThreadSafeContext,
        intent: EffectIntent<K, A, P>,
    ) -> EffectCasUpsert {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .upsert_desired(intent);
        self.apply(ctx, change);
        outcome
    }

    pub fn claim(
        &self,
        ctx: &ThreadSafeContext,
        key: &K,
        generation: u64,
        now: u64,
    ) -> EffectCasClaim<K, A, P, R> {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .claim(key, generation, now);
        self.apply(ctx, change);
        outcome
    }

    pub fn record_applied(
        &self,
        ctx: &ThreadSafeContext,
        key: &K,
        attempt: &EffectAttemptIdentity,
        receipt: R,
        authority: A,
    ) -> EffectCasApplied {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .record_applied(key, attempt, receipt, authority);
        self.apply(ctx, change);
        outcome
    }

    pub fn record_outcome(
        &self,
        ctx: &ThreadSafeContext,
        key: &K,
        attempt: &EffectAttemptIdentity,
        outcome: EffectApply<R, A, E>,
        retry_at: u64,
    ) -> EffectCasOutcome {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .record_outcome(key, attempt, outcome, retry_at);
        self.apply(ctx, change);
        outcome
    }

    pub fn record_superseded(
        &self,
        ctx: &ThreadSafeContext,
        key: &K,
        attempt: &EffectAttemptIdentity,
        observed: A,
    ) -> EffectCasFailure {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .record_superseded(key, attempt, observed);
        self.apply(ctx, change);
        outcome
    }

    pub fn record_retryable(
        &self,
        ctx: &ThreadSafeContext,
        key: &K,
        attempt: &EffectAttemptIdentity,
        error: E,
        retry_at: u64,
    ) -> EffectCasFailure {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .record_retryable(key, attempt, error, retry_at);
        self.apply(ctx, change);
        outcome
    }

    pub fn record_rejected(
        &self,
        ctx: &ThreadSafeContext,
        key: &K,
        attempt: &EffectAttemptIdentity,
        error: E,
    ) -> EffectCasFailure {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .record_rejected(key, attempt, error);
        self.apply(ctx, change);
        outcome
    }

    pub fn reconnect(&self, ctx: &ThreadSafeContext, generation: u64) -> EffectCasReconnect {
        let (change, outcome) = self
            .inner
            .core
            .lock()
            .expect("effect-CAS core")
            .reconnect(generation);
        self.apply(ctx, change);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EffectIdentity, EffectPlan};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn invalidates_after_unlock_only_for_state_changes() {
        let ctx = ThreadSafeContext::new();
        let effects = ThreadSafeEffectCas::<&str, u64, &str, &str, &str>::new(&ctx, 1);
        let runs = Arc::new(AtomicUsize::new(0));
        let _effect = {
            let runs = Arc::clone(&runs);
            let state = effects.state_handle();
            ctx.effect(move |compute| {
                compute.get(&state);
                runs.fetch_add(1, Ordering::SeqCst);
            })
        };
        let intent = EffectIntent {
            identity: EffectIdentity::new("shell-effect").unwrap(),
            epoch: 1,
            key: "doc",
            expected: 1,
            plan: EffectPlan::single("write"),
        };
        assert_eq!(
            effects.upsert_desired(&ctx, intent.clone()),
            EffectCasUpsert::Accepted
        );
        assert_eq!(
            effects.upsert_desired(&ctx, intent),
            EffectCasUpsert::Unchanged
        );
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }
}
