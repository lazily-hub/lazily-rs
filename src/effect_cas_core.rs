//! Generic keyed compare-and-swap effect authority (`#lzeffectcas`).
//!
//! The pure core owns effect identity, keyed single-flight, latest-wins
//! coalescing, retry timing, reconnect fencing, exact receipt matching, and the
//! cursor of an ordered effect plan. Backends remain application-owned: they
//! observe an opaque authority token and execute one atomic compare-and-apply.

use std::collections::BTreeMap;

use crate::EffectIdentity;

/// An ordered, non-empty sequence of idempotent effect payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "Vec<P>"))]
pub struct EffectPlan<P> {
    steps: Vec<P>,
}

impl<P> EffectPlan<P> {
    pub fn new(steps: Vec<P>) -> Result<Self, EffectPlanError> {
        if steps.is_empty() {
            return Err(EffectPlanError::Empty);
        }
        Ok(Self { steps })
    }

    pub fn single(payload: P) -> Self {
        Self {
            steps: vec![payload],
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn steps(&self) -> &[P] {
        &self.steps
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectPlanError {
    Empty,
}

impl std::fmt::Display for EffectPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("an effect plan must contain at least one step"),
        }
    }
}

impl std::error::Error for EffectPlanError {}

impl<P> TryFrom<Vec<P>> for EffectPlan<P> {
    type Error = EffectPlanError;

    fn try_from(steps: Vec<P>) -> Result<Self, Self::Error> {
        Self::new(steps)
    }
}

/// A plan persists as its ordered step list; deserialization re-validates that
/// the list is non-empty.
#[cfg(feature = "serde")]
impl<P: serde::Serialize> serde::Serialize for EffectPlan<P> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.steps.serialize(serializer)
    }
}

impl<P> From<EffectPlan<P>> for Vec<P> {
    fn from(plan: EffectPlan<P>) -> Self {
        plan.steps
    }
}

/// Immutable desired effect. One epoch names exactly one identity and plan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectIntent<K, A, P> {
    pub identity: EffectIdentity,
    pub epoch: u64,
    pub key: K,
    pub expected: A,
    pub plan: EffectPlan<P>,
}

/// Exact identity of one adapter call.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectAttemptIdentity {
    pub effect: EffectIdentity,
    pub generation: u64,
    pub epoch: u64,
    pub step: usize,
    pub attempt: u32,
}

/// A successful step receipt retained as lineage for later plan steps.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectReceipt<R, A> {
    pub attempt: EffectAttemptIdentity,
    pub receipt: R,
    pub authority: A,
}

/// The exact compare-and-apply call handed to an application adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectAttempt<K, A, P, R> {
    pub identity: EffectAttemptIdentity,
    pub key: K,
    pub expected: A,
    pub payload: P,
    pub prior_receipts: Vec<EffectReceipt<R, A>>,
}

/// The only outcomes an application adapter may return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectApply<R, A, E> {
    Applied { receipt: R, authority: A },
    Superseded { observed: A },
    Retryable(E),
    Rejected(E),
}

/// Narrow application-owned atomic authority boundary.
pub trait CompareAndApply<K, A, P, R> {
    type Error;

    fn observe(&self, key: &K) -> Result<A, Self::Error>;

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<K, A, P, R>,
    ) -> EffectApply<R, A, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectPending<K, A, P, R> {
    pub intent: EffectIntent<K, A, P>,
    pub next_step: usize,
    pub expected: A,
    pub receipts: Vec<EffectReceipt<R, A>>,
    pub next_attempt: u32,
    pub ready_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectRetry<E> {
    pub attempt: EffectAttemptIdentity,
    pub error: E,
    pub retry_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EffectTerminal<R, A, E> {
    Applied {
        identity: EffectIdentity,
        epoch: u64,
        receipts: Vec<EffectReceipt<R, A>>,
    },
    Superseded {
        identity: EffectIdentity,
        epoch: u64,
        observed: Option<A>,
        receipts: Vec<EffectReceipt<R, A>>,
    },
    Rejected {
        identity: EffectIdentity,
        epoch: u64,
        error: E,
        receipts: Vec<EffectReceipt<R, A>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectCasKeyState<K, A, P, R, E> {
    pub pending: Option<EffectPending<K, A, P, R>>,
    pub inflight: Option<EffectAttempt<K, A, P, R>>,
    pub retry: Option<EffectRetry<E>>,
    pub applied_through: Option<u64>,
    pub terminal: Option<EffectTerminal<R, A, E>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectCasSnapshot<K, A, P, R, E> {
    pub generation: u64,
    pub keys: BTreeMap<K, EffectCasKeyState<K, A, P, R, E>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasUpsert {
    Accepted,
    Unchanged,
    AlreadyApplied { applied_through: u64 },
    StaleEpoch { current: u64 },
    EpochConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasClaim<K, A, P, R> {
    Claimed(EffectAttempt<K, A, P, R>),
    Empty,
    Busy,
    Waiting { retry_at: u64 },
    StaleGeneration { current: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasApplied {
    StepApplied { next_step: usize },
    Completed { applied_through: u64 },
    Superseded,
    Duplicate,
    UnknownAttempt,
    StaleGeneration { current: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasFailure {
    Superseded,
    Retrying { retry_at: u64 },
    Rejected,
    UnknownAttempt,
    StaleGeneration { current: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasReconnect {
    Advanced {
        generation: u64,
        requeued: usize,
        superseded: usize,
    },
    Unchanged {
        generation: u64,
    },
    StaleGeneration {
        current: u64,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectCasChange {
    pub state: bool,
}

/// Full-fidelity durable image of an [`EffectCasCore`] (`#lzeffectcasdurable`).
///
/// [`EffectCasSnapshot`] is a reader projection: it omits the retained intent
/// behind an in-flight attempt and the last terminal intent, so it cannot
/// restore the core. This image carries every field the core needs to resume
/// after a crash or in another process: the actor generation, retained intents,
/// plan cursors, logical retry times, receipts, and terminal evidence. Keys are
/// a list rather than a map so a non-string key round-trips through JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectCasDurableState<K, A, P, R, E> {
    pub generation: u64,
    pub keys: Vec<EffectCasDurableKey<K, A, P, R, E>>,
}

/// Durable image of one key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectCasDurableKey<K, A, P, R, E> {
    pub key: K,
    pub pending: Option<EffectPending<K, A, P, R>>,
    pub inflight: Option<EffectCasDurableInflight<K, A, P, R>>,
    pub retry: Option<EffectRetry<E>>,
    pub applied_through: Option<u64>,
    pub terminal: Option<EffectTerminal<R, A, E>>,
    pub terminal_intent: Option<EffectIntent<K, A, P>>,
}

/// Durable image of an in-flight attempt and the retained work behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectCasDurableInflight<K, A, P, R> {
    pub retained: EffectPending<K, A, P, R>,
    pub attempt: EffectAttempt<K, A, P, R>,
}

/// Why a durable image cannot be hydrated. Hydration never repairs an image:
/// a corrupt image is refused so no adapter call runs on invented state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasHydrateError {
    /// Two entries name the same key.
    DuplicateKey,
    /// A retained or terminal intent belongs to a different key.
    KeyMismatch,
    /// A plan cursor points past the end of its plan.
    CursorOutOfRange,
    /// An in-flight attempt does not describe its retained cursor.
    AttemptMismatch,
    /// An in-flight attempt claims an actor generation newer than the image.
    FutureGeneration,
}

impl std::fmt::Display for EffectCasHydrateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateKey => "durable effect-CAS image repeats a key",
            Self::KeyMismatch => "durable effect-CAS intent belongs to another key",
            Self::CursorOutOfRange => "durable effect-CAS plan cursor is out of range",
            Self::AttemptMismatch => {
                "durable effect-CAS attempt does not match its retained cursor"
            }
            Self::FutureGeneration => {
                "durable effect-CAS attempt is newer than the image generation"
            }
        })
    }
}

impl std::error::Error for EffectCasHydrateError {}

#[derive(Debug, Clone)]
struct Inflight<K, A, P, R> {
    retained: EffectPending<K, A, P, R>,
    attempt: EffectAttempt<K, A, P, R>,
}

#[derive(Debug, Clone)]
struct Entry<K, A, P, R, E> {
    pending: Option<EffectPending<K, A, P, R>>,
    inflight: Option<Inflight<K, A, P, R>>,
    retry: Option<EffectRetry<E>>,
    applied_through: Option<u64>,
    terminal: Option<EffectTerminal<R, A, E>>,
    terminal_intent: Option<EffectIntent<K, A, P>>,
}

impl<K, A, P, R, E> Default for Entry<K, A, P, R, E> {
    fn default() -> Self {
        Self {
            pending: None,
            inflight: None,
            retry: None,
            applied_through: None,
            terminal: None,
            terminal_intent: None,
        }
    }
}

/// Pure keyed effect-CAS orchestration authority.
#[derive(Debug, Clone)]
pub struct EffectCasCore<K, A, P, R, E> {
    generation: u64,
    keys: BTreeMap<K, Entry<K, A, P, R, E>>,
}

impl<K, A, P, R, E> EffectCasCore<K, A, P, R, E>
where
    K: Ord + Clone,
    A: Clone + PartialEq,
    P: Clone + PartialEq,
    R: Clone + PartialEq,
    E: Clone + PartialEq,
{
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            keys: BTreeMap::new(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn snapshot(&self) -> EffectCasSnapshot<K, A, P, R, E> {
        EffectCasSnapshot {
            generation: self.generation,
            keys: self
                .keys
                .iter()
                .map(|(key, entry)| {
                    (
                        key.clone(),
                        EffectCasKeyState {
                            pending: entry.pending.clone(),
                            inflight: entry
                                .inflight
                                .as_ref()
                                .map(|inflight| inflight.attempt.clone()),
                            retry: entry.retry.clone(),
                            applied_through: entry.applied_through,
                            terminal: entry.terminal.clone(),
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn state(&self, key: &K) -> Option<EffectCasKeyState<K, A, P, R, E>> {
        self.keys.get(key).map(|entry| EffectCasKeyState {
            pending: entry.pending.clone(),
            inflight: entry
                .inflight
                .as_ref()
                .map(|inflight| inflight.attempt.clone()),
            retry: entry.retry.clone(),
            applied_through: entry.applied_through,
            terminal: entry.terminal.clone(),
        })
    }

    pub fn pending_keys(&self, now: u64) -> Vec<K> {
        self.keys
            .iter()
            .filter(|(_, entry)| {
                entry.inflight.is_none()
                    && entry
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.ready_at.is_none_or(|at| at <= now))
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    pub fn upsert_desired(
        &mut self,
        intent: EffectIntent<K, A, P>,
    ) -> (EffectCasChange, EffectCasUpsert) {
        let key = intent.key.clone();
        let entry = self.keys.entry(key).or_default();
        if let Some(applied_through) = entry.applied_through
            && intent.epoch <= applied_through
        {
            return (
                EffectCasChange::default(),
                EffectCasUpsert::AlreadyApplied { applied_through },
            );
        }

        let pending_intent = entry.pending.as_ref().map(|pending| &pending.intent);
        let inflight_intent = entry
            .inflight
            .as_ref()
            .map(|inflight| &inflight.retained.intent);
        let last_terminal_epoch = entry
            .terminal_intent
            .as_ref()
            .map(|terminal| terminal.epoch);
        let current_epoch = pending_intent
            .map(|current| current.epoch)
            .into_iter()
            .chain(inflight_intent.map(|current| current.epoch))
            .chain(last_terminal_epoch)
            .max();
        if let Some(current) = current_epoch {
            if intent.epoch < current {
                return (
                    EffectCasChange::default(),
                    EffectCasUpsert::StaleEpoch { current },
                );
            }
            if intent.epoch == current {
                let same = pending_intent
                    .into_iter()
                    .chain(inflight_intent)
                    .any(|existing| existing == &intent)
                    || entry
                        .terminal_intent
                        .as_ref()
                        .is_some_and(|terminal| terminal == &intent);
                return (
                    EffectCasChange::default(),
                    if same {
                        EffectCasUpsert::Unchanged
                    } else {
                        EffectCasUpsert::EpochConflict
                    },
                );
            }
        }

        if let Some(superseded) = entry.pending.take() {
            entry.terminal_intent = Some(superseded.intent.clone());
            entry.terminal = Some(EffectTerminal::Superseded {
                identity: superseded.intent.identity,
                epoch: superseded.intent.epoch,
                observed: None,
                receipts: superseded.receipts,
            });
        }
        entry.pending = Some(EffectPending {
            expected: intent.expected.clone(),
            intent,
            next_step: 0,
            receipts: Vec::new(),
            next_attempt: 1,
            ready_at: None,
        });
        entry.retry = None;
        (EffectCasChange { state: true }, EffectCasUpsert::Accepted)
    }

    pub fn claim(
        &mut self,
        key: &K,
        generation: u64,
        now: u64,
    ) -> (EffectCasChange, EffectCasClaim<K, A, P, R>) {
        if generation != self.generation {
            return (
                EffectCasChange::default(),
                EffectCasClaim::StaleGeneration {
                    current: self.generation,
                },
            );
        }
        let Some(entry) = self.keys.get_mut(key) else {
            return (EffectCasChange::default(), EffectCasClaim::Empty);
        };
        if entry.inflight.is_some() {
            return (EffectCasChange::default(), EffectCasClaim::Busy);
        }
        let Some(pending) = entry.pending.as_ref() else {
            return (EffectCasChange::default(), EffectCasClaim::Empty);
        };
        if let Some(retry_at) = pending.ready_at
            && now < retry_at
        {
            return (
                EffectCasChange::default(),
                EffectCasClaim::Waiting { retry_at },
            );
        }

        let pending = entry.pending.take().expect("pending effect checked above");
        let payload = pending.intent.plan.steps[pending.next_step].clone();
        let attempt = EffectAttempt {
            identity: EffectAttemptIdentity {
                effect: pending.intent.identity.clone(),
                generation,
                epoch: pending.intent.epoch,
                step: pending.next_step,
                attempt: pending.next_attempt,
            },
            key: pending.intent.key.clone(),
            expected: pending.expected.clone(),
            payload,
            prior_receipts: pending.receipts.clone(),
        };
        entry.inflight = Some(Inflight {
            retained: pending,
            attempt: attempt.clone(),
        });
        (
            EffectCasChange { state: true },
            EffectCasClaim::Claimed(attempt),
        )
    }

    pub fn record_applied(
        &mut self,
        key: &K,
        attempt: &EffectAttemptIdentity,
        receipt: R,
        authority: A,
    ) -> (EffectCasChange, EffectCasApplied) {
        if attempt.generation != self.generation {
            return (
                EffectCasChange::default(),
                EffectCasApplied::StaleGeneration {
                    current: self.generation,
                },
            );
        }
        let Some(entry) = self.keys.get_mut(key) else {
            return (EffectCasChange::default(), EffectCasApplied::UnknownAttempt);
        };
        if entry.terminal.as_ref().is_some_and(|terminal| {
            terminal_receipts(terminal)
                .iter()
                .any(|known| known.attempt == *attempt && known.receipt == receipt)
        }) {
            return (EffectCasChange::default(), EffectCasApplied::Duplicate);
        }
        if entry
            .inflight
            .as_ref()
            .map(|inflight| &inflight.attempt.identity)
            != Some(attempt)
        {
            return (EffectCasChange::default(), EffectCasApplied::UnknownAttempt);
        }

        let mut inflight = entry.inflight.take().expect("attempt matched in-flight");
        inflight.retained.receipts.push(EffectReceipt {
            attempt: attempt.clone(),
            receipt,
            authority: authority.clone(),
        });
        entry.retry = None;

        if entry
            .pending
            .as_ref()
            .is_some_and(|pending| pending.intent.epoch > inflight.retained.intent.epoch)
        {
            entry.terminal_intent = Some(inflight.retained.intent.clone());
            entry.terminal = Some(EffectTerminal::Superseded {
                identity: inflight.retained.intent.identity,
                epoch: inflight.retained.intent.epoch,
                observed: Some(authority),
                receipts: inflight.retained.receipts,
            });
            return (
                EffectCasChange { state: true },
                EffectCasApplied::Superseded,
            );
        }

        let next_step = inflight.retained.next_step + 1;
        if next_step < inflight.retained.intent.plan.len() {
            inflight.retained.next_step = next_step;
            inflight.retained.expected = authority;
            inflight.retained.ready_at = None;
            inflight.retained.next_attempt = 1;
            entry.pending = Some(inflight.retained);
            return (
                EffectCasChange { state: true },
                EffectCasApplied::StepApplied { next_step },
            );
        }

        let epoch = inflight.retained.intent.epoch;
        entry.applied_through = Some(entry.applied_through.map_or(epoch, |old| old.max(epoch)));
        entry.terminal_intent = Some(inflight.retained.intent.clone());
        entry.terminal = Some(EffectTerminal::Applied {
            identity: inflight.retained.intent.identity,
            epoch,
            receipts: inflight.retained.receipts,
        });
        (
            EffectCasChange { state: true },
            EffectCasApplied::Completed {
                applied_through: entry.applied_through.expect("frontier assigned"),
            },
        )
    }

    pub fn record_superseded(
        &mut self,
        key: &K,
        attempt: &EffectAttemptIdentity,
        observed: A,
    ) -> (EffectCasChange, EffectCasFailure) {
        self.finish_failed(key, attempt, FailureKind::Superseded(observed))
    }

    pub fn record_retryable(
        &mut self,
        key: &K,
        attempt: &EffectAttemptIdentity,
        error: E,
        retry_at: u64,
    ) -> (EffectCasChange, EffectCasFailure) {
        self.finish_failed(key, attempt, FailureKind::Retryable { error, retry_at })
    }

    pub fn record_rejected(
        &mut self,
        key: &K,
        attempt: &EffectAttemptIdentity,
        error: E,
    ) -> (EffectCasChange, EffectCasFailure) {
        self.finish_failed(key, attempt, FailureKind::Rejected(error))
    }

    pub fn record_outcome(
        &mut self,
        key: &K,
        attempt: &EffectAttemptIdentity,
        outcome: EffectApply<R, A, E>,
        retry_at: u64,
    ) -> (EffectCasChange, EffectCasOutcome) {
        match outcome {
            EffectApply::Applied { receipt, authority } => {
                let (change, outcome) = self.record_applied(key, attempt, receipt, authority);
                (change, EffectCasOutcome::Applied(outcome))
            }
            EffectApply::Superseded { observed } => {
                let (change, outcome) = self.record_superseded(key, attempt, observed);
                (change, EffectCasOutcome::Failure(outcome))
            }
            EffectApply::Retryable(error) => {
                let (change, outcome) = self.record_retryable(key, attempt, error, retry_at);
                (change, EffectCasOutcome::Failure(outcome))
            }
            EffectApply::Rejected(error) => {
                let (change, outcome) = self.record_rejected(key, attempt, error);
                (change, EffectCasOutcome::Failure(outcome))
            }
        }
    }

    pub fn reconnect(&mut self, new_generation: u64) -> (EffectCasChange, EffectCasReconnect) {
        if new_generation < self.generation {
            return (
                EffectCasChange::default(),
                EffectCasReconnect::StaleGeneration {
                    current: self.generation,
                },
            );
        }
        if new_generation == self.generation {
            return (
                EffectCasChange::default(),
                EffectCasReconnect::Unchanged {
                    generation: self.generation,
                },
            );
        }

        self.generation = new_generation;
        let mut requeued = 0;
        let mut superseded = 0;
        for entry in self.keys.values_mut() {
            let Some(mut inflight) = entry.inflight.take() else {
                continue;
            };
            if entry
                .pending
                .as_ref()
                .is_some_and(|pending| pending.intent.epoch > inflight.retained.intent.epoch)
            {
                entry.terminal_intent = Some(inflight.retained.intent.clone());
                entry.terminal = Some(EffectTerminal::Superseded {
                    identity: inflight.retained.intent.identity,
                    epoch: inflight.retained.intent.epoch,
                    observed: None,
                    receipts: inflight.retained.receipts,
                });
                superseded += 1;
            } else {
                inflight.retained.next_attempt = inflight.retained.next_attempt.saturating_add(1);
                inflight.retained.ready_at = None;
                entry.pending = Some(inflight.retained);
                requeued += 1;
            }
        }
        (
            EffectCasChange { state: true },
            EffectCasReconnect::Advanced {
                generation: new_generation,
                requeued,
                superseded,
            },
        )
    }

    fn finish_failed(
        &mut self,
        key: &K,
        attempt: &EffectAttemptIdentity,
        kind: FailureKind<A, E>,
    ) -> (EffectCasChange, EffectCasFailure) {
        if attempt.generation != self.generation {
            return (
                EffectCasChange::default(),
                EffectCasFailure::StaleGeneration {
                    current: self.generation,
                },
            );
        }
        let Some(entry) = self.keys.get_mut(key) else {
            return (EffectCasChange::default(), EffectCasFailure::UnknownAttempt);
        };
        if entry
            .inflight
            .as_ref()
            .map(|inflight| &inflight.attempt.identity)
            != Some(attempt)
        {
            return (EffectCasChange::default(), EffectCasFailure::UnknownAttempt);
        }

        let mut inflight = entry.inflight.take().expect("attempt matched in-flight");
        let newer_pending = entry
            .pending
            .as_ref()
            .is_some_and(|pending| pending.intent.epoch > inflight.retained.intent.epoch);
        let identity = inflight.retained.intent.identity.clone();
        let epoch = inflight.retained.intent.epoch;
        match kind {
            FailureKind::Superseded(observed) => {
                entry.retry = None;
                entry.terminal_intent = Some(inflight.retained.intent.clone());
                entry.terminal = Some(EffectTerminal::Superseded {
                    identity,
                    epoch,
                    observed: Some(observed),
                    receipts: inflight.retained.receipts,
                });
                (
                    EffectCasChange { state: true },
                    EffectCasFailure::Superseded,
                )
            }
            FailureKind::Retryable { error, retry_at } if !newer_pending => {
                entry.retry = Some(EffectRetry {
                    attempt: attempt.clone(),
                    error,
                    retry_at,
                });
                inflight.retained.next_attempt = inflight.retained.next_attempt.saturating_add(1);
                inflight.retained.ready_at = Some(retry_at);
                entry.pending = Some(inflight.retained);
                (
                    EffectCasChange { state: true },
                    EffectCasFailure::Retrying { retry_at },
                )
            }
            FailureKind::Retryable { .. } => {
                entry.retry = None;
                entry.terminal_intent = Some(inflight.retained.intent.clone());
                entry.terminal = Some(EffectTerminal::Superseded {
                    identity,
                    epoch,
                    observed: None,
                    receipts: inflight.retained.receipts,
                });
                (
                    EffectCasChange { state: true },
                    EffectCasFailure::Superseded,
                )
            }
            FailureKind::Rejected(error) => {
                entry.retry = None;
                entry.terminal_intent = Some(inflight.retained.intent.clone());
                entry.terminal = Some(EffectTerminal::Rejected {
                    identity,
                    epoch,
                    error,
                    receipts: inflight.retained.receipts,
                });
                (EffectCasChange { state: true }, EffectCasFailure::Rejected)
            }
        }
    }
}

impl<K, A, P, R, E> EffectCasCore<K, A, P, R, E>
where
    K: Ord + Clone,
    A: Clone + PartialEq,
    P: Clone + PartialEq,
    R: Clone + PartialEq,
    E: Clone + PartialEq,
{
    /// Full-fidelity image for crash recovery and cross-process persistence.
    pub fn durable_state(&self) -> EffectCasDurableState<K, A, P, R, E> {
        EffectCasDurableState {
            generation: self.generation,
            keys: self
                .keys
                .iter()
                .map(|(key, entry)| EffectCasDurableKey {
                    key: key.clone(),
                    pending: entry.pending.clone(),
                    inflight: entry
                        .inflight
                        .as_ref()
                        .map(|inflight| EffectCasDurableInflight {
                            retained: inflight.retained.clone(),
                            attempt: inflight.attempt.clone(),
                        }),
                    retry: entry.retry.clone(),
                    applied_through: entry.applied_through,
                    terminal: entry.terminal.clone(),
                    terminal_intent: entry.terminal_intent.clone(),
                })
                .collect(),
        }
    }

    /// Restore a core from [`Self::durable_state`] before the runtime becomes
    /// live. The restored core is observably identical to the one that wrote
    /// the image: the same generation, retained intents, cursors, retry times,
    /// receipts, and frontier. A recovering actor then calls
    /// [`Self::reconnect`] with a newer generation to fence the crashed actor
    /// and replay its retained work under the same effect identity.
    pub fn hydrate(
        state: EffectCasDurableState<K, A, P, R, E>,
    ) -> Result<Self, EffectCasHydrateError> {
        let mut keys = BTreeMap::new();
        for durable in state.keys {
            if let Some(pending) = &durable.pending {
                validate_retained(&durable.key, pending)?;
            }
            if let Some(intent) = &durable.terminal_intent
                && intent.key != durable.key
            {
                return Err(EffectCasHydrateError::KeyMismatch);
            }
            let inflight = match durable.inflight {
                Some(inflight) => {
                    validate_retained(&durable.key, &inflight.retained)?;
                    let attempt = &inflight.attempt;
                    let retained = &inflight.retained;
                    if attempt.identity.generation > state.generation {
                        return Err(EffectCasHydrateError::FutureGeneration);
                    }
                    if attempt.key != durable.key
                        || attempt.identity.effect != retained.intent.identity
                        || attempt.identity.epoch != retained.intent.epoch
                        || attempt.identity.step != retained.next_step
                        || attempt.identity.attempt != retained.next_attempt
                        || attempt.expected != retained.expected
                        || attempt.payload != retained.intent.plan.steps[retained.next_step]
                        || attempt.prior_receipts != retained.receipts
                    {
                        return Err(EffectCasHydrateError::AttemptMismatch);
                    }
                    Some(Inflight {
                        retained: inflight.retained,
                        attempt: inflight.attempt,
                    })
                }
                None => None,
            };
            let entry = Entry {
                pending: durable.pending,
                inflight,
                retry: durable.retry,
                applied_through: durable.applied_through,
                terminal: durable.terminal,
                terminal_intent: durable.terminal_intent,
            };
            if keys.insert(durable.key, entry).is_some() {
                return Err(EffectCasHydrateError::DuplicateKey);
            }
        }
        Ok(Self {
            generation: state.generation,
            keys,
        })
    }
}

fn validate_retained<K: PartialEq, A, P, R>(
    key: &K,
    retained: &EffectPending<K, A, P, R>,
) -> Result<(), EffectCasHydrateError> {
    if retained.intent.key != *key {
        return Err(EffectCasHydrateError::KeyMismatch);
    }
    if retained.next_step >= retained.intent.plan.len() {
        return Err(EffectCasHydrateError::CursorOutOfRange);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectCasOutcome {
    Applied(EffectCasApplied),
    Failure(EffectCasFailure),
}

enum FailureKind<A, E> {
    Superseded(A),
    Retryable { error: E, retry_at: u64 },
    Rejected(E),
}

fn terminal_receipts<R, A, E>(terminal: &EffectTerminal<R, A, E>) -> &[EffectReceipt<R, A>] {
    match terminal {
        EffectTerminal::Applied { receipts, .. }
        | EffectTerminal::Superseded { receipts, .. }
        | EffectTerminal::Rejected { receipts, .. } => receipts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Core = EffectCasCore<&'static str, u64, &'static str, &'static str, &'static str>;

    fn intent(
        id: &str,
        epoch: u64,
        key: &'static str,
        expected: u64,
        steps: Vec<&'static str>,
    ) -> EffectIntent<&'static str, u64, &'static str> {
        EffectIntent {
            identity: EffectIdentity::new(id).unwrap(),
            epoch,
            key,
            expected,
            plan: EffectPlan::new(steps).unwrap(),
        }
    }

    fn claim(
        core: &mut Core,
        key: &'static str,
        generation: u64,
        now: u64,
    ) -> EffectAttempt<&'static str, u64, &'static str, &'static str> {
        match core.claim(&key, generation, now).1 {
            EffectCasClaim::Claimed(attempt) => attempt,
            other => panic!("expected claim, got {other:?}"),
        }
    }

    #[test]
    fn ordered_plan_advances_from_receipt_authority_and_dedupes_terminal_receipt() {
        let mut core = Core::new(7);
        core.upsert_desired(intent(
            "effect-1",
            1,
            "doc",
            10,
            vec!["canonical", "snapshot"],
        ));
        let first = claim(&mut core, "doc", 7, 0);
        assert_eq!(first.expected, 10);
        assert_eq!(
            core.record_applied(&"doc", &first.identity, "r1", 11).1,
            EffectCasApplied::StepApplied { next_step: 1 }
        );
        let second = claim(&mut core, "doc", 7, 0);
        assert_eq!((second.expected, second.payload), (11, "snapshot"));
        assert_eq!(second.prior_receipts.len(), 1);
        assert_eq!(
            core.record_applied(&"doc", &second.identity, "r2", 12).1,
            EffectCasApplied::Completed { applied_through: 1 }
        );
        assert_eq!(
            core.record_applied(&"doc", &second.identity, "r2", 12).1,
            EffectCasApplied::Duplicate
        );
    }

    #[test]
    fn retry_uses_logical_clock_and_newer_desire_wins() {
        let mut core = Core::new(1);
        core.upsert_desired(intent("old", 1, "doc", 1, vec!["write"]));
        let old = claim(&mut core, "doc", 1, 0);
        assert_eq!(
            core.record_retryable(&"doc", &old.identity, "busy", 9).1,
            EffectCasFailure::Retrying { retry_at: 9 }
        );
        assert_eq!(
            core.claim(&"doc", 1, 8).1,
            EffectCasClaim::Waiting { retry_at: 9 }
        );
        core.upsert_desired(intent("new", 2, "doc", 2, vec!["write"]));
        assert_eq!(
            claim(&mut core, "doc", 1, 8).identity.effect.as_str(),
            "new"
        );
    }

    #[test]
    fn stale_attempts_and_authority_mismatch_never_advance_frontier() {
        let mut core = Core::new(2);
        core.upsert_desired(intent("effect", 4, "doc", 8, vec!["write"]));
        let attempt = claim(&mut core, "doc", 2, 0);
        assert_eq!(
            core.record_superseded(&"doc", &attempt.identity, 9).1,
            EffectCasFailure::Superseded
        );
        assert_eq!(core.state(&"doc").unwrap().applied_through, None);
        assert_eq!(
            core.record_applied(&"doc", &attempt.identity, "late", 10).1,
            EffectCasApplied::UnknownAttempt
        );
        assert_eq!(core.state(&"doc").unwrap().applied_through, None);
    }

    #[test]
    fn reconnect_requeues_same_identity_and_fences_old_generation() {
        let mut core = Core::new(3);
        core.upsert_desired(intent("effect", 1, "doc", 1, vec!["write"]));
        let first = claim(&mut core, "doc", 3, 0);
        assert_eq!(
            core.reconnect(4).1,
            EffectCasReconnect::Advanced {
                generation: 4,
                requeued: 1,
                superseded: 0,
            }
        );
        assert_eq!(
            core.record_applied(&"doc", &first.identity, "late", 2).1,
            EffectCasApplied::StaleGeneration { current: 4 }
        );
        let replay = claim(&mut core, "doc", 4, 0);
        assert_eq!(replay.identity.effect, first.identity.effect);
        assert_eq!(replay.identity.attempt, 2);
    }

    #[test]
    fn keys_progress_independently() {
        let mut core = Core::new(1);
        core.upsert_desired(intent("a", 1, "a", 0, vec!["a"]));
        core.upsert_desired(intent("b", 1, "b", 0, vec!["b"]));
        claim(&mut core, "a", 1, 0);
        claim(&mut core, "b", 1, 0);
        assert_eq!(core.claim(&"a", 1, 0).1, EffectCasClaim::Busy);
        assert_eq!(core.snapshot().keys.len(), 2);
    }
}
