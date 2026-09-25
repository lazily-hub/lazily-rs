//! Backend-neutral durable-owner contracts (`#lzdurablespec`).
//!
//! A durable owner has one authoritative position and one of two state models:
//! append-only event history or a replaceable snapshot. An accepted commit binds
//! the state advance, inbox identity, and outbox effects atomically. External
//! adapters may map that boundary to a database transaction, but SQL, broker
//! subjects, and cache keys never enter this API.
//!
//! [`ProjectionCompleteness::CompleteHistory`] is intentionally distinct from
//! [`ProjectionCompleteness::LatestDurableProjection`]. The former fingerprints
//! every ordered source record. The latter fingerprints only the latest desired
//! value and is suitable for conflating egress, never for proving that history
//! was retained.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ReplayDigest, ReplayProofError, ReplayValue, canonical_digest};

macro_rules! stable_identity {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Stable ", $what, " identity.")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Construct a non-empty stable identity.
            pub fn new(value: impl Into<String>) -> Result<Self, DurableContractError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(DurableContractError::EmptyIdentity { kind: $what });
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

stable_identity!(DurableOwnerId, "owner");
stable_identity!(InboxIdentity, "inbox");
stable_identity!(EffectIdentity, "effect");
stable_identity!(ReceiptIdentity, "receipt");

/// Monotone durable state/event position. Position zero is the empty owner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurablePosition(u64);

impl DurablePosition {
    pub const INITIAL: Self = Self(0);

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    fn checked_next(self) -> Result<Self, DurableContractError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(DurableContractError::PositionOverflow)
    }
}

/// Monotone authority token. Only the current fence may authorize a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FenceToken(u64);

impl FenceToken {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Version of the logical payload schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    pub fn new(value: u32) -> Result<Self, DurableContractError> {
        if value == 0 {
            return Err(DurableContractError::ZeroVersion { kind: "schema" });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Version of the byte codec used for a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodecVersion(u32);

impl CodecVersion {
    pub fn new(value: u32) -> Result<Self, DurableContractError> {
        if value == 0 {
            return Err(DurableContractError::ZeroVersion { kind: "codec" });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Opaque payload bytes paired with explicit logical and physical versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedBytes {
    pub schema_version: SchemaVersion,
    pub codec_version: CodecVersion,
    pub bytes: Vec<u8>,
}

impl VersionedBytes {
    #[must_use]
    pub fn new(
        schema_version: SchemaVersion,
        codec_version: CodecVersion,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            schema_version,
            codec_version,
            bytes: bytes.into(),
        }
    }
}

/// The authoritative state representation selected for an owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableOwnerMode {
    EventHistory,
    Snapshot,
}

/// A position-assigned state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableRecord {
    pub position: DurablePosition,
    pub payload: VersionedBytes,
}

/// State mutation proposed by a single atomic commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableStateMutation {
    AppendEvents(Vec<VersionedBytes>),
    ReplaceSnapshot(VersionedBytes),
}

/// An effect intent persisted before any external publication is attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableEffectIntent {
    pub identity: EffectIdentity,
    pub payload: VersionedBytes,
}

/// A persisted effect with the owner position that accepted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableEffect {
    pub identity: EffectIdentity,
    pub accepted_at: DurablePosition,
    pub payload: VersionedBytes,
}

/// A durable fact reporting the successful external application of an effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableReceipt {
    pub identity: ReceiptIdentity,
    pub effect_identity: EffectIdentity,
    pub outcome: DurableEffectOutcome,
    pub recorded_at: DurablePosition,
    pub fence: FenceToken,
    pub payload: VersionedBytes,
}

/// Terminal effect outcome projected back into the durable owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableEffectOutcome {
    Applied,
    Rejected,
}

/// Receipt proposed either in the owner commit or after external publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableReceiptIntent {
    pub identity: ReceiptIdentity,
    pub effect_identity: EffectIdentity,
    pub outcome: DurableEffectOutcome,
    pub payload: VersionedBytes,
}

/// One proposed owner transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCommit {
    pub owner_id: DurableOwnerId,
    pub expected_position: DurablePosition,
    pub fence: FenceToken,
    /// Digest or canonical bytes of the ingress command. Reuse of the inbox id
    /// with different bytes is an identity conflict, not a duplicate.
    pub inbox_identity: InboxIdentity,
    pub ingress_fingerprint: Vec<u8>,
    pub state: DurableStateMutation,
    pub effects: Vec<DurableEffectIntent>,
    /// Transaction-local successful effects may persist their receipts in the
    /// same atomic boundary as state, inbox, and outbox.
    pub receipts: Vec<DurableReceiptIntent>,
}

/// Durable disposition of one inbox identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableInboxRecord {
    pub identity: InboxIdentity,
    pub ingress_fingerprint: Vec<u8>,
    pub committed_through: DurablePosition,
}

/// The authoritative durable image reconstructed after a process crash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableOwnerImage {
    pub owner_id: DurableOwnerId,
    pub mode: DurableOwnerMode,
    pub position: DurablePosition,
    pub fence: FenceToken,
    pub history: Vec<DurableRecord>,
    pub snapshot: Option<DurableRecord>,
    pub inbox: BTreeMap<InboxIdentity, DurableInboxRecord>,
    pub outbox: BTreeMap<EffectIdentity, DurableEffect>,
    pub receipts: BTreeMap<ReceiptIdentity, DurableReceipt>,
}

/// Outcome of an atomic owner commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableCommitOutcome {
    Committed { through: DurablePosition },
    Duplicate { through: DurablePosition },
}

impl DurableCommitOutcome {
    /// A transport delivery may be acknowledged only after one of these durable
    /// outcomes has been observed.
    #[must_use]
    pub const fn ack_is_safe(self) -> bool {
        true
    }

    #[must_use]
    pub const fn through(self) -> DurablePosition {
        match self {
            Self::Committed { through } | Self::Duplicate { through } => through,
        }
    }
}

/// Outcome of recording a terminal effect receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableReceiptOutcome {
    Recorded,
    Duplicate,
}

/// Contract violation detected before any state is mutated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableContractError {
    EmptyIdentity {
        kind: &'static str,
    },
    ZeroVersion {
        kind: &'static str,
    },
    OwnerMismatch,
    PositionOverflow,
    PositionConflict {
        expected: DurablePosition,
        actual: DurablePosition,
    },
    StaleFence {
        attempted: FenceToken,
        current: FenceToken,
    },
    FenceDidNotAdvance {
        attempted: FenceToken,
        current: FenceToken,
    },
    StateModeMismatch,
    EmptyEventBatch,
    InboxIdentityConflict(InboxIdentity),
    EffectIdentityConflict(EffectIdentity),
    UnknownEffect(EffectIdentity),
    ReceiptIdentityConflict(ReceiptIdentity),
    EffectAlreadyReceipted(EffectIdentity),
    ProjectionModeMismatch,
    CorruptImage {
        reason: &'static str,
    },
}

/// Backend-neutral persistence seam. Database adapters supply transaction and
/// scheduling mechanics without changing these outcomes.
pub trait DurableOwnerStore {
    type Error;

    fn load_owner(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<Option<DurableOwnerImage>, Self::Error>;

    fn commit_owner(&mut self, commit: DurableCommit) -> Result<DurableCommitOutcome, Self::Error>;

    fn record_effect_receipt(
        &mut self,
        fence: FenceToken,
        receipt: DurableReceiptIntent,
    ) -> Result<DurableReceiptOutcome, Self::Error>;

    fn advance_owner_fence(&mut self, fence: FenceToken) -> Result<(), Self::Error>;
}

/// In-memory reference authority for the backend-neutral contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableOwnerCore {
    image: DurableOwnerImage,
}

impl DurableOwnerCore {
    #[must_use]
    pub fn new(owner_id: DurableOwnerId, mode: DurableOwnerMode, fence: FenceToken) -> Self {
        Self {
            image: DurableOwnerImage {
                owner_id,
                mode,
                position: DurablePosition::INITIAL,
                fence,
                history: Vec::new(),
                snapshot: None,
                inbox: BTreeMap::new(),
                outbox: BTreeMap::new(),
                receipts: BTreeMap::new(),
            },
        }
    }

    /// Reconstruct from the exact durable image left by a prior process.
    pub fn recover(image: DurableOwnerImage) -> Result<Self, DurableContractError> {
        match image.mode {
            DurableOwnerMode::EventHistory => {
                if image.snapshot.is_some() {
                    return Err(DurableContractError::CorruptImage {
                        reason: "event-history owner carries a snapshot",
                    });
                }
                if image.history.len()
                    != usize::try_from(image.position.get()).unwrap_or(usize::MAX)
                    || image
                        .history
                        .iter()
                        .enumerate()
                        .any(|(index, record)| record.position.get() != index as u64 + 1)
                {
                    return Err(DurableContractError::CorruptImage {
                        reason: "event history is not a contiguous prefix through owner position",
                    });
                }
            }
            DurableOwnerMode::Snapshot => {
                if !image.history.is_empty()
                    || image.position == DurablePosition::INITIAL && image.snapshot.is_some()
                    || image.position != DurablePosition::INITIAL
                        && image
                            .snapshot
                            .as_ref()
                            .is_none_or(|record| record.position != image.position)
                {
                    return Err(DurableContractError::CorruptImage {
                        reason: "snapshot state does not match owner position",
                    });
                }
            }
        }
        if image
            .inbox
            .values()
            .any(|record| record.committed_through > image.position)
            || image
                .outbox
                .values()
                .any(|effect| effect.accepted_at > image.position)
            || image.receipts.values().any(|receipt| {
                receipt.recorded_at > image.position
                    || !image.outbox.contains_key(&receipt.effect_identity)
            })
        {
            return Err(DurableContractError::CorruptImage {
                reason: "inbox, outbox, or receipt points beyond the durable image",
            });
        }
        let mut receipt_effects = BTreeSet::new();
        if image
            .receipts
            .values()
            .any(|receipt| !receipt_effects.insert(&receipt.effect_identity))
        {
            return Err(DurableContractError::CorruptImage {
                reason: "more than one terminal receipt names an effect",
            });
        }
        Ok(Self { image })
    }

    #[must_use]
    pub fn image(&self) -> &DurableOwnerImage {
        &self.image
    }

    #[must_use]
    pub fn into_image(self) -> DurableOwnerImage {
        self.image
    }

    #[must_use]
    pub fn pending_effects(&self) -> Vec<&DurableEffect> {
        let receipted = self
            .image
            .receipts
            .values()
            .map(|receipt| &receipt.effect_identity)
            .collect::<BTreeSet<_>>();
        let mut pending = self
            .image
            .outbox
            .values()
            .filter(|effect| !receipted.contains(&effect.identity))
            .collect::<Vec<_>>();
        pending.sort_by(|left, right| {
            (left.accepted_at, &left.identity).cmp(&(right.accepted_at, &right.identity))
        });
        pending
    }

    pub fn commit(
        &mut self,
        commit: DurableCommit,
    ) -> Result<DurableCommitOutcome, DurableContractError> {
        if commit.owner_id != self.image.owner_id {
            return Err(DurableContractError::OwnerMismatch);
        }

        if let Some(previous) = self.image.inbox.get(&commit.inbox_identity) {
            if previous.ingress_fingerprint == commit.ingress_fingerprint {
                return Ok(DurableCommitOutcome::Duplicate {
                    through: previous.committed_through,
                });
            }
            return Err(DurableContractError::InboxIdentityConflict(
                commit.inbox_identity,
            ));
        }

        if commit.fence != self.image.fence {
            return Err(DurableContractError::StaleFence {
                attempted: commit.fence,
                current: self.image.fence,
            });
        }
        if commit.expected_position != self.image.position {
            return Err(DurableContractError::PositionConflict {
                expected: commit.expected_position,
                actual: self.image.position,
            });
        }

        let mut identities = BTreeSet::new();
        for effect in &commit.effects {
            if !identities.insert(&effect.identity)
                || self.image.outbox.contains_key(&effect.identity)
            {
                return Err(DurableContractError::EffectIdentityConflict(
                    effect.identity.clone(),
                ));
            }
        }
        let effect_identities = commit
            .effects
            .iter()
            .map(|effect| &effect.identity)
            .chain(self.image.outbox.keys())
            .collect::<BTreeSet<_>>();
        let mut receipt_identities = BTreeSet::new();
        let mut receipted_effects = self
            .image
            .receipts
            .values()
            .map(|receipt| &receipt.effect_identity)
            .collect::<BTreeSet<_>>();
        for receipt in &commit.receipts {
            if !receipt_identities.insert(&receipt.identity)
                || self.image.receipts.contains_key(&receipt.identity)
            {
                return Err(DurableContractError::ReceiptIdentityConflict(
                    receipt.identity.clone(),
                ));
            }
            if !effect_identities.contains(&receipt.effect_identity) {
                return Err(DurableContractError::UnknownEffect(
                    receipt.effect_identity.clone(),
                ));
            }
            if !receipted_effects.insert(&receipt.effect_identity) {
                return Err(DurableContractError::EffectAlreadyReceipted(
                    receipt.effect_identity.clone(),
                ));
            }
        }

        let mut candidate = self.image.clone();
        match (&candidate.mode, commit.state) {
            (DurableOwnerMode::EventHistory, DurableStateMutation::AppendEvents(events)) => {
                if events.is_empty() {
                    return Err(DurableContractError::EmptyEventBatch);
                }
                for payload in events {
                    candidate.position = candidate.position.checked_next()?;
                    candidate.history.push(DurableRecord {
                        position: candidate.position,
                        payload,
                    });
                }
            }
            (DurableOwnerMode::Snapshot, DurableStateMutation::ReplaceSnapshot(payload)) => {
                candidate.position = candidate.position.checked_next()?;
                candidate.snapshot = Some(DurableRecord {
                    position: candidate.position,
                    payload,
                });
            }
            _ => return Err(DurableContractError::StateModeMismatch),
        }

        for effect in commit.effects {
            candidate.outbox.insert(
                effect.identity.clone(),
                DurableEffect {
                    identity: effect.identity,
                    accepted_at: candidate.position,
                    payload: effect.payload,
                },
            );
        }
        for receipt in commit.receipts {
            candidate.receipts.insert(
                receipt.identity.clone(),
                DurableReceipt {
                    identity: receipt.identity,
                    effect_identity: receipt.effect_identity,
                    outcome: receipt.outcome,
                    recorded_at: candidate.position,
                    fence: candidate.fence,
                    payload: receipt.payload,
                },
            );
        }
        candidate.inbox.insert(
            commit.inbox_identity.clone(),
            DurableInboxRecord {
                identity: commit.inbox_identity,
                ingress_fingerprint: commit.ingress_fingerprint,
                committed_through: candidate.position,
            },
        );

        let through = candidate.position;
        self.image = candidate;
        Ok(DurableCommitOutcome::Committed { through })
    }

    pub fn record_receipt(
        &mut self,
        fence: FenceToken,
        receipt: DurableReceiptIntent,
    ) -> Result<DurableReceiptOutcome, DurableContractError> {
        if let Some(previous) = self.image.receipts.get(&receipt.identity) {
            if previous.effect_identity == receipt.effect_identity
                && previous.outcome == receipt.outcome
                && previous.payload == receipt.payload
            {
                return Ok(DurableReceiptOutcome::Duplicate);
            }
            return Err(DurableContractError::ReceiptIdentityConflict(
                receipt.identity,
            ));
        }
        if fence != self.image.fence {
            return Err(DurableContractError::StaleFence {
                attempted: fence,
                current: self.image.fence,
            });
        }
        if !self.image.outbox.contains_key(&receipt.effect_identity) {
            return Err(DurableContractError::UnknownEffect(receipt.effect_identity));
        }
        if self
            .image
            .receipts
            .values()
            .any(|known| known.effect_identity == receipt.effect_identity)
        {
            return Err(DurableContractError::EffectAlreadyReceipted(
                receipt.effect_identity,
            ));
        }
        self.image.receipts.insert(
            receipt.identity.clone(),
            DurableReceipt {
                identity: receipt.identity,
                effect_identity: receipt.effect_identity,
                outcome: receipt.outcome,
                recorded_at: self.image.position,
                fence,
                payload: receipt.payload,
            },
        );
        Ok(DurableReceiptOutcome::Recorded)
    }

    pub fn advance_fence(&mut self, fence: FenceToken) -> Result<(), DurableContractError> {
        if fence <= self.image.fence {
            return Err(DurableContractError::FenceDidNotAdvance {
                attempted: fence,
                current: self.image.fence,
            });
        }
        self.image.fence = fence;
        Ok(())
    }

    /// Fingerprint a complete ordered history and its projected observation.
    pub fn complete_history_fingerprint(
        &self,
        projection_schema: SchemaVersion,
        projection_codec: CodecVersion,
        projection: ReplayValue,
    ) -> Result<DurableProjectionFingerprint, DurableFingerprintError> {
        if self.image.mode != DurableOwnerMode::EventHistory {
            return Err(DurableFingerprintError::Contract(
                DurableContractError::ProjectionModeMismatch,
            ));
        }
        let source = ReplayValue::Seq(
            self.image
                .history
                .iter()
                .map(record_value)
                .collect::<Vec<_>>(),
        );
        fingerprint(
            self.image.owner_id.clone(),
            ProjectionCompleteness::CompleteHistory,
            self.image.position,
            projection_schema,
            projection_codec,
            source,
            projection,
        )
    }

    /// Fingerprint the current authoritative snapshot and its projection.
    pub fn snapshot_fingerprint(
        &self,
        projection_schema: SchemaVersion,
        projection_codec: CodecVersion,
        projection: ReplayValue,
    ) -> Result<DurableProjectionFingerprint, DurableFingerprintError> {
        if self.image.mode != DurableOwnerMode::Snapshot {
            return Err(DurableFingerprintError::Contract(
                DurableContractError::ProjectionModeMismatch,
            ));
        }
        let source = self
            .image
            .snapshot
            .as_ref()
            .map_or(ReplayValue::Null, record_value);
        fingerprint(
            self.image.owner_id.clone(),
            ProjectionCompleteness::SnapshotState,
            self.image.position,
            projection_schema,
            projection_codec,
            source,
            projection,
        )
    }
}

impl DurableOwnerStore for DurableOwnerCore {
    type Error = DurableContractError;

    fn load_owner(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<Option<DurableOwnerImage>, Self::Error> {
        Ok((owner_id == &self.image.owner_id).then(|| self.image.clone()))
    }

    fn commit_owner(&mut self, commit: DurableCommit) -> Result<DurableCommitOutcome, Self::Error> {
        self.commit(commit)
    }

    fn record_effect_receipt(
        &mut self,
        fence: FenceToken,
        receipt: DurableReceiptIntent,
    ) -> Result<DurableReceiptOutcome, Self::Error> {
        self.record_receipt(fence, receipt)
    }

    fn advance_owner_fence(&mut self, fence: FenceToken) -> Result<(), Self::Error> {
        self.advance_fence(fence)
    }
}

/// What durable source a projection fingerprint certifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionCompleteness {
    /// Every ordered event is retained and bound into the fingerprint.
    CompleteHistory,
    /// The current authoritative snapshot is retained and bound.
    SnapshotState,
    /// Only the latest desired egress value is retained; intermediate values may
    /// be superseded. This is not proof of complete history.
    LatestDurableProjection,
}

/// Projection proof bound to owner, position, versions, source, and observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableProjectionFingerprint {
    pub owner_id: DurableOwnerId,
    pub completeness: ProjectionCompleteness,
    pub through: DurablePosition,
    pub schema_version: SchemaVersion,
    pub codec_version: CodecVersion,
    pub source_digest: ReplayDigest,
    pub projection_digest: ReplayDigest,
}

impl DurableProjectionFingerprint {
    /// Fingerprint conflating latest-value egress. Keeping this constructor
    /// separate makes it impossible to label one as complete history by accident.
    pub fn latest_durable_projection(
        owner_id: DurableOwnerId,
        through: DurablePosition,
        latest: &VersionedBytes,
        projection: ReplayValue,
    ) -> Result<Self, DurableFingerprintError> {
        fingerprint(
            owner_id,
            ProjectionCompleteness::LatestDurableProjection,
            through,
            latest.schema_version,
            latest.codec_version,
            payload_value(latest),
            projection,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DurableFingerprintError {
    Contract(DurableContractError),
    Encoding(ReplayProofError),
}

impl From<ReplayProofError> for DurableFingerprintError {
    fn from(error: ReplayProofError) -> Self {
        Self::Encoding(error)
    }
}

fn payload_value(payload: &VersionedBytes) -> ReplayValue {
    ReplayValue::Record {
        name: "VersionedBytes".to_owned(),
        fields: vec![
            (
                "schema_version".to_owned(),
                ReplayValue::Int(i128::from(payload.schema_version.get())),
            ),
            (
                "codec_version".to_owned(),
                ReplayValue::Int(i128::from(payload.codec_version.get())),
            ),
            (
                "bytes".to_owned(),
                ReplayValue::Bytes(payload.bytes.clone()),
            ),
        ],
    }
}

fn record_value(record: &DurableRecord) -> ReplayValue {
    ReplayValue::Record {
        name: "DurableRecord".to_owned(),
        fields: vec![
            (
                "position".to_owned(),
                ReplayValue::Int(i128::from(record.position.get())),
            ),
            ("payload".to_owned(), payload_value(&record.payload)),
        ],
    }
}

#[allow(clippy::too_many_arguments)]
fn fingerprint(
    owner_id: DurableOwnerId,
    completeness: ProjectionCompleteness,
    through: DurablePosition,
    schema_version: SchemaVersion,
    codec_version: CodecVersion,
    source: ReplayValue,
    projection: ReplayValue,
) -> Result<DurableProjectionFingerprint, DurableFingerprintError> {
    Ok(DurableProjectionFingerprint {
        owner_id,
        completeness,
        through,
        schema_version,
        codec_version,
        source_digest: canonical_digest(&source)?,
        projection_digest: canonical_digest(&projection)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T>(result: Result<T, DurableContractError>) -> T {
        result.unwrap()
    }

    fn payload(value: &str) -> VersionedBytes {
        VersionedBytes::new(id(SchemaVersion::new(1)), id(CodecVersion::new(1)), value)
    }

    fn commit(owner: &DurableOwnerId, inbox: &str, position: u64, value: &str) -> DurableCommit {
        DurableCommit {
            owner_id: owner.clone(),
            expected_position: DurablePosition::new(position),
            fence: FenceToken::new(1),
            inbox_identity: id(InboxIdentity::new(inbox)),
            ingress_fingerprint: inbox.as_bytes().to_vec(),
            state: DurableStateMutation::AppendEvents(vec![payload(value)]),
            effects: Vec::new(),
            receipts: Vec::new(),
        }
    }

    #[test]
    fn duplicate_inbox_is_resolved_before_new_fence_and_position_checks() {
        let owner = id(DurableOwnerId::new("owner"));
        let mut core = DurableOwnerCore::new(
            owner.clone(),
            DurableOwnerMode::EventHistory,
            FenceToken::new(1),
        );
        let accepted = commit(&owner, "message-1", 0, "one");
        assert_eq!(
            core.commit(accepted.clone()),
            Ok(DurableCommitOutcome::Committed {
                through: DurablePosition::new(1)
            })
        );
        core.advance_fence(FenceToken::new(2)).unwrap();
        assert_eq!(
            core.commit(accepted),
            Ok(DurableCommitOutcome::Duplicate {
                through: DurablePosition::new(1)
            })
        );
        assert_eq!(core.image.history.len(), 1);
    }

    #[test]
    fn complete_history_and_latest_projection_are_different_proofs() {
        let owner = id(DurableOwnerId::new("owner"));
        let mut left = DurableOwnerCore::new(
            owner.clone(),
            DurableOwnerMode::EventHistory,
            FenceToken::new(1),
        );
        let mut right = left.clone();
        left.commit(commit(&owner, "left-1", 0, "1")).unwrap();
        left.commit(commit(&owner, "left-2", 1, "0")).unwrap();
        right.commit(commit(&owner, "right-1", 0, "0")).unwrap();
        right.commit(commit(&owner, "right-2", 1, "0")).unwrap();

        let observation = ReplayValue::Int(0);
        let complete_left = left
            .complete_history_fingerprint(
                id(SchemaVersion::new(1)),
                id(CodecVersion::new(1)),
                observation.clone(),
            )
            .unwrap();
        let complete_right = right
            .complete_history_fingerprint(
                id(SchemaVersion::new(1)),
                id(CodecVersion::new(1)),
                observation.clone(),
            )
            .unwrap();
        assert_eq!(
            complete_left.projection_digest,
            complete_right.projection_digest
        );
        assert_ne!(complete_left.source_digest, complete_right.source_digest);

        let latest = payload("0");
        let latest_left = DurableProjectionFingerprint::latest_durable_projection(
            owner.clone(),
            DurablePosition::new(2),
            &latest,
            observation.clone(),
        )
        .unwrap();
        let latest_right = DurableProjectionFingerprint::latest_durable_projection(
            owner,
            DurablePosition::new(2),
            &latest,
            observation,
        )
        .unwrap();
        assert_eq!(latest_left, latest_right);
        assert_ne!(complete_left.completeness, latest_left.completeness);
    }

    #[test]
    fn recovery_rejects_a_gap_in_authoritative_history() {
        let owner = id(DurableOwnerId::new("owner"));
        let mut core = DurableOwnerCore::new(
            owner.clone(),
            DurableOwnerMode::EventHistory,
            FenceToken::new(1),
        );
        core.commit(commit(&owner, "message-1", 0, "one")).unwrap();
        let mut image = core.into_image();
        image.history[0].position = DurablePosition::new(2);
        assert_eq!(
            DurableOwnerCore::recover(image),
            Err(DurableContractError::CorruptImage {
                reason: "event history is not a contiguous prefix through owner position"
            })
        );
    }
}
