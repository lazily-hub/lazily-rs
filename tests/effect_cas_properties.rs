use std::sync::Mutex;

use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasFailure, EffectIdentity, EffectIntent, EffectPlan,
};
use proptest::prelude::*;

type Core = EffectCasCore<String, u64, u64, u64, &'static str>;

fn intent(key: &str, epoch: u64, expected: u64, payload: u64) -> EffectIntent<String, u64, u64> {
    EffectIntent {
        identity: EffectIdentity::new(format!("{key}-{epoch}")).unwrap(),
        epoch,
        key: key.to_owned(),
        expected,
        plan: EffectPlan::single(payload),
    }
}

fn claim(core: &mut Core, key: &str) -> EffectAttempt<String, u64, u64, u64> {
    match core.claim(&key.to_owned(), 1, 0).1 {
        EffectCasClaim::Claimed(attempt) => attempt,
        other => panic!("expected claim, got {other:?}"),
    }
}

proptest! {
    #[test]
    fn applied_frontier_is_monotone_and_converges_to_latest(epoch_count in 1u64..80) {
        let mut core = Core::new(1);
        let key = "doc".to_owned();
        let mut frontier = None;
        for epoch in 1..=epoch_count {
            core.upsert_desired(intent(&key, epoch, epoch - 1, epoch));
            let attempt = claim(&mut core, &key);
            prop_assert_eq!(
                core.record_applied(&key, &attempt.identity, epoch, epoch).1,
                EffectCasApplied::Completed { applied_through: epoch }
            );
            let next = core.state(&key).unwrap().applied_through;
            prop_assert!(next >= frontier);
            frontier = next;
        }
        prop_assert_eq!(frontier, Some(epoch_count));
    }

    #[test]
    fn stale_attempts_change_no_state(new_generation in 2u64..100) {
        let mut core = Core::new(1);
        let key = "doc".to_owned();
        core.upsert_desired(intent(&key, 1, 0, 1));
        let stale = claim(&mut core, &key);
        core.reconnect(new_generation);
        let before = core.snapshot();
        prop_assert_eq!(
            core.record_applied(&key, &stale.identity, 1, 1).1,
            EffectCasApplied::StaleGeneration { current: new_generation }
        );
        prop_assert_eq!(core.snapshot(), before);
    }
}

struct MemoryAdapter {
    authority: Mutex<u64>,
}

impl CompareAndApply<String, u64, u64, u64> for MemoryAdapter {
    type Error = &'static str;

    fn observe(&self, _key: &String) -> Result<u64, Self::Error> {
        Ok(*self.authority.lock().unwrap())
    }

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<String, u64, u64, u64>,
    ) -> EffectApply<u64, u64, Self::Error> {
        let mut authority = self.authority.lock().unwrap();
        if *authority != attempt.expected {
            return EffectApply::Superseded {
                observed: *authority,
            };
        }
        *authority = attempt.payload;
        EffectApply::Applied {
            receipt: attempt.payload,
            authority: *authority,
        }
    }
}

#[test]
fn adapter_authority_mismatch_mutates_nothing() {
    let adapter = MemoryAdapter {
        authority: Mutex::new(9),
    };
    let mut core = Core::new(1);
    let key = "doc".to_owned();
    core.upsert_desired(intent(&key, 1, 8, 10));
    let attempt = claim(&mut core, &key);
    let before_backend = adapter.observe(&key).unwrap();
    let outcome = adapter.compare_and_apply(&attempt);
    assert_eq!(outcome, EffectApply::Superseded { observed: 9 });
    assert_eq!(adapter.observe(&key).unwrap(), before_backend);
    assert_eq!(
        core.record_outcome(&key, &attempt.identity, outcome, 0).1,
        lazily::EffectCasOutcome::Failure(EffectCasFailure::Superseded)
    );
    assert_eq!(core.state(&key).unwrap().applied_through, None);
}

#[test]
fn at_most_one_attempt_per_key_but_independent_keys_progress() {
    let mut core = Core::new(1);
    let a = "a".to_owned();
    let b = "b".to_owned();
    core.upsert_desired(intent(&a, 1, 0, 1));
    core.upsert_desired(intent(&b, 1, 0, 1));
    claim(&mut core, &a);
    claim(&mut core, &b);
    assert_eq!(core.claim(&a, 1, 0).1, EffectCasClaim::Busy);
    let snapshot = core.snapshot();
    assert!(snapshot.keys[&a].inflight.is_some());
    assert!(snapshot.keys[&b].inflight.is_some());
}
