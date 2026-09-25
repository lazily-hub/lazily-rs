//! PostgreSQL reference host for the backend-neutral durable-owner contract.
//!
//! The adapter deliberately keeps business semantics in [`DurableOwnerCore`].
//! PostgreSQL supplies the serializable transaction, row lock, durable bytes,
//! timer index, and relay claims; it does not become a second reducer.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use postgres::error::SqlState;
use postgres::{Client, Config, IsolationLevel, NoTls, Transaction};
use serde::{Deserialize, Serialize};

use crate::{
    CodecVersion, DurableCommit, DurableCommitOutcome, DurableContractError, DurableEffect,
    DurableEffectOutcome, DurableInboxRecord, DurableOwnerCore, DurableOwnerId, DurableOwnerImage,
    DurableOwnerMode, DurableOwnerStore, DurablePosition, DurableReceipt, DurableReceiptIntent,
    DurableReceiptOutcome, DurableRecord, EffectIdentity, FenceToken, InboxIdentity,
    ReceiptIdentity, SchemaVersion, TimerIdentity, VersionedBytes,
};

/// Idempotent schema for the reference host.
///
/// The canonical owner image is stored as JSONB so the Phase-0 contract remains
/// the one source of truth. Outbox and timer rows are additionally normalized
/// for ordered relay claims and due-timer scans.
pub const POSTGRES_DURABLE_MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS lazily_durable_owner (
    owner_id TEXT PRIMARY KEY,
    mode TEXT NOT NULL CHECK (mode IN ('event_history', 'snapshot')),
    position BIGINT NOT NULL CHECK (position >= 0),
    fence BIGINT NOT NULL CHECK (fence >= 0),
    image_json JSONB NOT NULL,
    projection_version BIGINT NOT NULL DEFAULT 0 CHECK (projection_version >= 0),
    projection_schema_version INTEGER,
    projection_codec_version INTEGER,
    projection_bytes BYTEA,
    projection_fingerprint BYTEA,
    CHECK (
        (projection_version = 0 AND projection_schema_version IS NULL
            AND projection_codec_version IS NULL AND projection_bytes IS NULL
            AND projection_fingerprint IS NULL)
        OR
        (projection_version > 0 AND projection_schema_version > 0
            AND projection_codec_version > 0 AND projection_bytes IS NOT NULL
            AND projection_fingerprint IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS lazily_durable_outbox (
    owner_id TEXT NOT NULL REFERENCES lazily_durable_owner(owner_id) ON DELETE CASCADE,
    effect_id TEXT NOT NULL,
    accepted_at BIGINT NOT NULL CHECK (accepted_at > 0),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    codec_version INTEGER NOT NULL CHECK (codec_version > 0),
    payload BYTEA NOT NULL,
    claimed_by TEXT,
    claim_until_epoch_millis BIGINT,
    receipt_id TEXT,
    PRIMARY KEY (owner_id, effect_id)
);

CREATE INDEX IF NOT EXISTS lazily_durable_outbox_pending
    ON lazily_durable_outbox (accepted_at, owner_id, effect_id)
    WHERE receipt_id IS NULL;

CREATE TABLE IF NOT EXISTS lazily_durable_timer (
    owner_id TEXT NOT NULL REFERENCES lazily_durable_owner(owner_id) ON DELETE CASCADE,
    timer_id TEXT NOT NULL,
    deadline_epoch_millis BIGINT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt >= 0),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    codec_version INTEGER NOT NULL CHECK (codec_version > 0),
    payload BYTEA NOT NULL,
    PRIMARY KEY (owner_id, timer_id)
);

CREATE INDEX IF NOT EXISTS lazily_durable_timer_due
    ON lazily_durable_timer (deadline_epoch_millis, owner_id, timer_id);
"#;

#[derive(Debug)]
pub enum PostgresDurableError {
    Postgres(postgres::Error),
    Codec(serde_json::Error),
    Contract(DurableContractError),
    MissingOwner(DurableOwnerId),
    OwnerAlreadyExists(DurableOwnerId),
    OwnerModeMismatch,
    ProjectionVersion {
        expected: u64,
        actual: u64,
    },
    DuplicateTimerIdentity(TimerIdentity),
    Conversion(&'static str),
    RetryExhausted {
        attempts: u32,
        source: postgres::Error,
    },
}

impl fmt::Display for PostgresDurableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postgres(error) => write!(formatter, "Postgres durable host: {error}"),
            Self::Codec(error) => write!(formatter, "Postgres durable image codec: {error}"),
            Self::Contract(error) => write!(formatter, "durable owner contract: {error:?}"),
            Self::MissingOwner(owner) => write!(formatter, "durable owner {:?} is missing", owner),
            Self::OwnerAlreadyExists(owner) => {
                write!(
                    formatter,
                    "durable owner {:?} already exists with another contract",
                    owner
                )
            }
            Self::OwnerModeMismatch => write!(formatter, "durable owner mode mismatch"),
            Self::ProjectionVersion { expected, actual } => write!(
                formatter,
                "projection version {actual} is not the required next version {expected}"
            ),
            Self::DuplicateTimerIdentity(identity) => {
                write!(
                    formatter,
                    "timer identity {:?} is repeated in one commit",
                    identity
                )
            }
            Self::Conversion(message) => {
                write!(formatter, "durable Postgres conversion: {message}")
            }
            Self::RetryExhausted { attempts, source } => write!(
                formatter,
                "serializable durable transaction failed after {attempts} attempts: {source}"
            ),
        }
    }
}

impl std::error::Error for PostgresDurableError {}

impl From<postgres::Error> for PostgresDurableError {
    fn from(error: postgres::Error) -> Self {
        Self::Postgres(error)
    }
}

impl From<serde_json::Error> for PostgresDurableError {
    fn from(error: serde_json::Error) -> Self {
        Self::Codec(error)
    }
}

impl From<DurableContractError> for PostgresDurableError {
    fn from(error: DurableContractError) -> Self {
        Self::Contract(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostgresRetryPolicy {
    pub max_attempts: u32,
}

impl Default for PostgresRetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 4 }
    }
}

impl PostgresRetryPolicy {
    fn attempts(self) -> u32 {
        self.max_attempts.max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableProjectionUpdate {
    pub version: u64,
    pub payload: VersionedBytes,
    pub fingerprint: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableTimerRecord {
    pub identity: TimerIdentity,
    pub deadline_epoch_millis: i64,
    pub attempt: u32,
    pub payload: VersionedBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableTimerChange {
    Upsert(DurableTimerRecord),
    Cancel(TimerIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresDurableUnitOfWork {
    pub commit: DurableCommit,
    pub projection: Option<DurableProjectionUpdate>,
    pub timers: Vec<DurableTimerChange>,
}

impl From<DurableCommit> for PostgresDurableUnitOfWork {
    fn from(commit: DurableCommit) -> Self {
        Self {
            commit,
            projection: None,
            timers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresOutboxClaim {
    pub owner_id: DurableOwnerId,
    pub effect: DurableEffect,
    pub claimed_by: String,
    pub claim_until_epoch_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProjection {
    pub version: u64,
    pub payload: VersionedBytes,
    pub fingerprint: Vec<u8>,
}

/// Multi-owner reference host. The synchronous `postgres` client is wrapped in
/// `RefCell` only to satisfy the backend-neutral store's read signature; callers
/// still serialize access to one host value.
pub struct PostgresDurableHost {
    client: RefCell<Client>,
    retry_policy: PostgresRetryPolicy,
}

impl PostgresDurableHost {
    pub fn connect(config: &str) -> Result<Self, PostgresDurableError> {
        let config = config
            .parse::<Config>()
            .map_err(PostgresDurableError::Postgres)?;
        let client = config.connect(NoTls)?;
        Ok(Self::from_client(client))
    }

    #[must_use]
    pub fn from_client(client: Client) -> Self {
        Self {
            client: RefCell::new(client),
            retry_policy: PostgresRetryPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_retry_policy(mut self, retry_policy: PostgresRetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    pub fn migrate(&self) -> Result<(), PostgresDurableError> {
        self.client
            .borrow_mut()
            .batch_execute(POSTGRES_DURABLE_MIGRATION)?;
        Ok(())
    }

    /// Create the empty authority row. Repeating the exact same declaration is
    /// idempotent; changing mode or initial fence fails closed.
    pub fn create_owner(
        &self,
        owner_id: DurableOwnerId,
        mode: DurableOwnerMode,
        fence: FenceToken,
    ) -> Result<bool, PostgresDurableError> {
        let image = DurableOwnerCore::new(owner_id.clone(), mode, fence).into_image();
        let stored = serde_json::to_value(StoredImage::from(&image))?;
        let mode_text = mode_text(mode);
        let position = to_i64(image.position.get(), "owner position exceeds BIGINT")?;
        let fence_value = to_i64(fence.get(), "fence exceeds BIGINT")?;
        let mut client = self.client.borrow_mut();
        let inserted = client.execute(
            "INSERT INTO lazily_durable_owner
                (owner_id, mode, position, fence, image_json)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (owner_id) DO NOTHING",
            &[
                &owner_id.as_str(),
                &mode_text,
                &position,
                &fence_value,
                &stored,
            ],
        )?;
        if inserted == 1 {
            return Ok(true);
        }
        let row = client.query_one(
            "SELECT mode, fence FROM lazily_durable_owner WHERE owner_id = $1",
            &[&owner_id.as_str()],
        )?;
        let existing_mode: String = row.get(0);
        let existing_fence: i64 = row.get(1);
        if existing_mode == mode_text && existing_fence == fence_value {
            Ok(false)
        } else {
            Err(PostgresDurableError::OwnerAlreadyExists(owner_id))
        }
    }

    pub fn load_owner(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<Option<DurableOwnerImage>, PostgresDurableError> {
        let row = self.client.borrow_mut().query_opt(
            "SELECT image_json FROM lazily_durable_owner WHERE owner_id = $1",
            &[&owner_id.as_str()],
        )?;
        row.map(|row| decode_image(row.get(0))).transpose()
    }

    pub fn load_projection(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<Option<StoredProjection>, PostgresDurableError> {
        let row = self.client.borrow_mut().query_opt(
            "SELECT projection_version, projection_schema_version,
                    projection_codec_version, projection_bytes, projection_fingerprint
             FROM lazily_durable_owner WHERE owner_id = $1",
            &[&owner_id.as_str()],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let version: i64 = row.get(0);
        if version == 0 {
            return Ok(None);
        }
        let schema: i32 = row.get(1);
        let codec: i32 = row.get(2);
        Ok(Some(StoredProjection {
            version: from_i64(version, "negative projection version")?,
            payload: VersionedBytes::new(
                SchemaVersion::new(to_u32(schema, "invalid projection schema version")?)?,
                CodecVersion::new(to_u32(codec, "invalid projection codec version")?)?,
                row.get::<_, Vec<u8>>(3),
            ),
            fingerprint: row.get(4),
        }))
    }

    pub fn load_timers(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<Vec<DurableTimerRecord>, PostgresDurableError> {
        self.client
            .borrow_mut()
            .query(
                "SELECT timer_id, deadline_epoch_millis, attempt, schema_version,
                        codec_version, payload
                 FROM lazily_durable_timer WHERE owner_id = $1
                 ORDER BY deadline_epoch_millis, timer_id",
                &[&owner_id.as_str()],
            )?
            .into_iter()
            .map(|row| {
                let attempt: i32 = row.get(2);
                let schema: i32 = row.get(3);
                let codec: i32 = row.get(4);
                Ok(DurableTimerRecord {
                    identity: TimerIdentity::new(row.get::<_, String>(0))?,
                    deadline_epoch_millis: row.get(1),
                    attempt: to_u32(attempt, "negative timer attempt")?,
                    payload: VersionedBytes::new(
                        SchemaVersion::new(to_u32(schema, "invalid timer schema version")?)?,
                        CodecVersion::new(to_u32(codec, "invalid timer codec version")?)?,
                        row.get::<_, Vec<u8>>(5),
                    ),
                })
            })
            .collect()
    }

    pub fn commit_unit_of_work(
        &self,
        work: PostgresDurableUnitOfWork,
    ) -> Result<DurableCommitOutcome, PostgresDurableError> {
        let attempts = self.retry_policy.attempts();
        for attempt in 1..=attempts {
            match commit_once(&mut self.client.borrow_mut(), &work) {
                Ok(outcome) => return Ok(outcome),
                Err(PostgresDurableError::Postgres(error))
                    if is_serialization_retry(&error) && attempt < attempts => {}
                Err(PostgresDurableError::Postgres(error)) if is_serialization_retry(&error) => {
                    return Err(PostgresDurableError::RetryExhausted {
                        attempts,
                        source: error,
                    });
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("retry attempts are clamped to at least one")
    }

    /// Claim pending effects in total `(accepted_at, owner_id, effect_id)` order.
    /// The CTE's row locks are deliberately `SKIP LOCKED`; the durable lease
    /// columns, not the transient lock, make a crash retryable.
    pub fn claim_outbox(
        &self,
        worker_id: &str,
        now_epoch_millis: i64,
        claim_until_epoch_millis: i64,
        limit: u32,
    ) -> Result<Vec<PostgresOutboxClaim>, PostgresDurableError> {
        if worker_id.is_empty() {
            return Err(PostgresDurableError::Conversion("worker id is empty"));
        }
        let limit = i64::from(limit);
        let rows = self.client.borrow_mut().query(
            "WITH next AS (
                SELECT owner_id, effect_id
                FROM lazily_durable_outbox
                WHERE receipt_id IS NULL
                  AND (claim_until_epoch_millis IS NULL OR claim_until_epoch_millis <= $1)
                ORDER BY accepted_at, owner_id, effect_id
                FOR UPDATE SKIP LOCKED
                LIMIT $2
             )
             UPDATE lazily_durable_outbox AS outbox
             SET claimed_by = $3, claim_until_epoch_millis = $4
             FROM next
             WHERE outbox.owner_id = next.owner_id AND outbox.effect_id = next.effect_id
             RETURNING outbox.owner_id, outbox.effect_id, outbox.accepted_at,
                       outbox.schema_version, outbox.codec_version, outbox.payload
             ",
            &[
                &now_epoch_millis,
                &limit,
                &worker_id,
                &claim_until_epoch_millis,
            ],
        )?;
        rows.into_iter()
            .map(|row| {
                let accepted_at: i64 = row.get(2);
                let schema: i32 = row.get(3);
                let codec: i32 = row.get(4);
                Ok(PostgresOutboxClaim {
                    owner_id: DurableOwnerId::new(row.get::<_, String>(0))?,
                    effect: DurableEffect {
                        identity: EffectIdentity::new(row.get::<_, String>(1))?,
                        accepted_at: DurablePosition::new(from_i64(
                            accepted_at,
                            "negative outbox position",
                        )?),
                        payload: VersionedBytes::new(
                            SchemaVersion::new(to_u32(schema, "invalid outbox schema version")?)?,
                            CodecVersion::new(to_u32(codec, "invalid outbox codec version")?)?,
                            row.get::<_, Vec<u8>>(5),
                        ),
                    },
                    claimed_by: worker_id.to_owned(),
                    claim_until_epoch_millis,
                })
            })
            .collect()
    }

    pub fn record_publication_receipt(
        &self,
        owner_id: &DurableOwnerId,
        fence: FenceToken,
        receipt: DurableReceiptIntent,
    ) -> Result<DurableReceiptOutcome, PostgresDurableError> {
        let mut client = self.client.borrow_mut();
        let mut transaction = serializable_transaction(&mut client)?;
        let mut core = load_core_for_update(&mut transaction, owner_id)?;
        let outcome = core.record_effect_receipt(fence, receipt.clone())?;
        if outcome == DurableReceiptOutcome::Recorded {
            persist_image(&mut transaction, core.image())?;
            let changed = transaction.execute(
                "UPDATE lazily_durable_outbox
                 SET receipt_id = $3, claimed_by = NULL, claim_until_epoch_millis = NULL
                 WHERE owner_id = $1 AND effect_id = $2 AND receipt_id IS NULL",
                &[
                    &owner_id.as_str(),
                    &receipt.effect_identity.as_str(),
                    &receipt.identity.as_str(),
                ],
            )?;
            if changed != 1 {
                return Err(PostgresDurableError::Conversion(
                    "receipt did not resolve exactly one pending outbox row",
                ));
            }
        }
        transaction.commit()?;
        Ok(outcome)
    }

    pub fn advance_owner_fence(
        &self,
        owner_id: &DurableOwnerId,
        fence: FenceToken,
    ) -> Result<(), PostgresDurableError> {
        let mut client = self.client.borrow_mut();
        let mut transaction = serializable_transaction(&mut client)?;
        let mut core = load_core_for_update(&mut transaction, owner_id)?;
        core.advance_fence(fence)?;
        persist_image(&mut transaction, core.image())?;
        transaction.commit()?;
        Ok(())
    }

    pub fn owner<'host>(&'host self, owner_id: DurableOwnerId) -> PostgresDurableOwner<'host> {
        PostgresDurableOwner {
            host: self,
            owner_id,
        }
    }
}

/// Owner-scoped view implementing the Phase-0 backend-neutral trait.
pub struct PostgresDurableOwner<'host> {
    host: &'host PostgresDurableHost,
    owner_id: DurableOwnerId,
}

impl DurableOwnerStore for PostgresDurableOwner<'_> {
    type Error = PostgresDurableError;

    fn load_owner(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<Option<DurableOwnerImage>, Self::Error> {
        if owner_id != &self.owner_id {
            return Err(DurableContractError::OwnerMismatch.into());
        }
        self.host.load_owner(owner_id)
    }

    fn commit_owner(&mut self, commit: DurableCommit) -> Result<DurableCommitOutcome, Self::Error> {
        if commit.owner_id != self.owner_id {
            return Err(DurableContractError::OwnerMismatch.into());
        }
        self.host.commit_unit_of_work(commit.into())
    }

    fn record_effect_receipt(
        &mut self,
        fence: FenceToken,
        receipt: DurableReceiptIntent,
    ) -> Result<DurableReceiptOutcome, Self::Error> {
        self.host
            .record_publication_receipt(&self.owner_id, fence, receipt)
    }

    fn advance_owner_fence(&mut self, fence: FenceToken) -> Result<(), Self::Error> {
        self.host.advance_owner_fence(&self.owner_id, fence)
    }
}

fn commit_once(
    client: &mut Client,
    work: &PostgresDurableUnitOfWork,
) -> Result<DurableCommitOutcome, PostgresDurableError> {
    validate_timer_changes(&work.timers)?;
    let mut transaction = serializable_transaction(client)?;
    let mut core = load_core_for_update(&mut transaction, &work.commit.owner_id)?;
    let prior_projection_version =
        load_projection_version(&mut transaction, &work.commit.owner_id)?;
    let outcome = core.commit(work.commit.clone())?;
    if let DurableCommitOutcome::Duplicate { .. } = outcome {
        transaction.commit()?;
        return Ok(outcome);
    }

    if let Some(projection) = &work.projection {
        let expected =
            prior_projection_version
                .checked_add(1)
                .ok_or(PostgresDurableError::Conversion(
                    "projection version overflow",
                ))?;
        if projection.version != expected {
            return Err(PostgresDurableError::ProjectionVersion {
                expected,
                actual: projection.version,
            });
        }
    }

    persist_image(&mut transaction, core.image())?;
    persist_projection(
        &mut transaction,
        &work.commit.owner_id,
        work.projection.as_ref(),
    )?;
    persist_new_effects(
        &mut transaction,
        &work.commit.owner_id,
        outcome.through(),
        &work.commit.effects,
    )?;
    persist_local_receipt_marks(
        &mut transaction,
        &work.commit.owner_id,
        &work.commit.receipts,
    )?;
    persist_timer_changes(&mut transaction, &work.commit.owner_id, &work.timers)?;
    transaction.commit()?;
    Ok(outcome)
}

fn serializable_transaction(client: &mut Client) -> Result<Transaction<'_>, postgres::Error> {
    client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
}

fn is_serialization_retry(error: &postgres::Error) -> bool {
    error.code().is_some_and(|code| {
        *code == SqlState::T_R_SERIALIZATION_FAILURE || *code == SqlState::T_R_DEADLOCK_DETECTED
    })
}

fn load_core_for_update(
    transaction: &mut Transaction<'_>,
    owner_id: &DurableOwnerId,
) -> Result<DurableOwnerCore, PostgresDurableError> {
    let row = transaction.query_opt(
        "SELECT image_json FROM lazily_durable_owner WHERE owner_id = $1 FOR UPDATE",
        &[&owner_id.as_str()],
    )?;
    let Some(row) = row else {
        return Err(PostgresDurableError::MissingOwner(owner_id.clone()));
    };
    Ok(DurableOwnerCore::recover(decode_image(row.get(0))?)?)
}

fn load_projection_version(
    transaction: &mut Transaction<'_>,
    owner_id: &DurableOwnerId,
) -> Result<u64, PostgresDurableError> {
    let row = transaction.query_one(
        "SELECT projection_version FROM lazily_durable_owner WHERE owner_id = $1",
        &[&owner_id.as_str()],
    )?;
    from_i64(row.get(0), "negative projection version")
}

fn persist_image(
    transaction: &mut Transaction<'_>,
    image: &DurableOwnerImage,
) -> Result<(), PostgresDurableError> {
    let stored = serde_json::to_value(StoredImage::from(image))?;
    let position = to_i64(image.position.get(), "owner position exceeds BIGINT")?;
    let fence = to_i64(image.fence.get(), "owner fence exceeds BIGINT")?;
    let changed = transaction.execute(
        "UPDATE lazily_durable_owner
         SET position = $2, fence = $3, image_json = $4
         WHERE owner_id = $1",
        &[&image.owner_id.as_str(), &position, &fence, &stored],
    )?;
    if changed != 1 {
        return Err(PostgresDurableError::MissingOwner(image.owner_id.clone()));
    }
    Ok(())
}

fn persist_projection(
    transaction: &mut Transaction<'_>,
    owner_id: &DurableOwnerId,
    projection: Option<&DurableProjectionUpdate>,
) -> Result<(), PostgresDurableError> {
    let Some(projection) = projection else {
        return Ok(());
    };
    let version = to_i64(projection.version, "projection version exceeds BIGINT")?;
    let schema = to_i32(
        projection.payload.schema_version.get(),
        "schema version exceeds INTEGER",
    )?;
    let codec = to_i32(
        projection.payload.codec_version.get(),
        "codec version exceeds INTEGER",
    )?;
    transaction.execute(
        "UPDATE lazily_durable_owner
         SET projection_version = $2, projection_schema_version = $3,
             projection_codec_version = $4, projection_bytes = $5,
             projection_fingerprint = $6
         WHERE owner_id = $1",
        &[
            &owner_id.as_str(),
            &version,
            &schema,
            &codec,
            &projection.payload.bytes,
            &projection.fingerprint,
        ],
    )?;
    Ok(())
}

fn persist_new_effects(
    transaction: &mut Transaction<'_>,
    owner_id: &DurableOwnerId,
    accepted_at: DurablePosition,
    effects: &[crate::DurableEffectIntent],
) -> Result<(), PostgresDurableError> {
    let accepted_at = to_i64(accepted_at.get(), "effect position exceeds BIGINT")?;
    for effect in effects {
        let schema = to_i32(
            effect.payload.schema_version.get(),
            "schema version exceeds INTEGER",
        )?;
        let codec = to_i32(
            effect.payload.codec_version.get(),
            "codec version exceeds INTEGER",
        )?;
        transaction.execute(
            "INSERT INTO lazily_durable_outbox
                (owner_id, effect_id, accepted_at, schema_version, codec_version, payload)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &owner_id.as_str(),
                &effect.identity.as_str(),
                &accepted_at,
                &schema,
                &codec,
                &effect.payload.bytes,
            ],
        )?;
    }
    Ok(())
}

fn persist_local_receipt_marks(
    transaction: &mut Transaction<'_>,
    owner_id: &DurableOwnerId,
    receipts: &[DurableReceiptIntent],
) -> Result<(), PostgresDurableError> {
    for receipt in receipts {
        let changed = transaction.execute(
            "UPDATE lazily_durable_outbox SET receipt_id = $3
             WHERE owner_id = $1 AND effect_id = $2 AND receipt_id IS NULL",
            &[
                &owner_id.as_str(),
                &receipt.effect_identity.as_str(),
                &receipt.identity.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(PostgresDurableError::Conversion(
                "local receipt did not resolve exactly one outbox row",
            ));
        }
    }
    Ok(())
}

fn validate_timer_changes(changes: &[DurableTimerChange]) -> Result<(), PostgresDurableError> {
    let mut seen = BTreeSet::new();
    for change in changes {
        let identity = match change {
            DurableTimerChange::Upsert(timer) => &timer.identity,
            DurableTimerChange::Cancel(identity) => identity,
        };
        if !seen.insert(identity) {
            return Err(PostgresDurableError::DuplicateTimerIdentity(
                identity.clone(),
            ));
        }
    }
    Ok(())
}

fn persist_timer_changes(
    transaction: &mut Transaction<'_>,
    owner_id: &DurableOwnerId,
    changes: &[DurableTimerChange],
) -> Result<(), PostgresDurableError> {
    for change in changes {
        match change {
            DurableTimerChange::Upsert(timer) => {
                let attempt = to_i32(timer.attempt, "timer attempt exceeds INTEGER")?;
                let schema = to_i32(
                    timer.payload.schema_version.get(),
                    "schema version exceeds INTEGER",
                )?;
                let codec = to_i32(
                    timer.payload.codec_version.get(),
                    "codec version exceeds INTEGER",
                )?;
                transaction.execute(
                    "INSERT INTO lazily_durable_timer
                        (owner_id, timer_id, deadline_epoch_millis, attempt,
                         schema_version, codec_version, payload)
                     VALUES ($1, $2, $3, $4, $5, $6, $7)
                     ON CONFLICT (owner_id, timer_id) DO UPDATE SET
                        deadline_epoch_millis = EXCLUDED.deadline_epoch_millis,
                        attempt = EXCLUDED.attempt,
                        schema_version = EXCLUDED.schema_version,
                        codec_version = EXCLUDED.codec_version,
                        payload = EXCLUDED.payload",
                    &[
                        &owner_id.as_str(),
                        &timer.identity.as_str(),
                        &timer.deadline_epoch_millis,
                        &attempt,
                        &schema,
                        &codec,
                        &timer.payload.bytes,
                    ],
                )?;
            }
            DurableTimerChange::Cancel(identity) => {
                transaction.execute(
                    "DELETE FROM lazily_durable_timer WHERE owner_id = $1 AND timer_id = $2",
                    &[&owner_id.as_str(), &identity.as_str()],
                )?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPayload {
    schema_version: u32,
    codec_version: u32,
    bytes: Vec<u8>,
}

impl From<&VersionedBytes> for StoredPayload {
    fn from(payload: &VersionedBytes) -> Self {
        Self {
            schema_version: payload.schema_version.get(),
            codec_version: payload.codec_version.get(),
            bytes: payload.bytes.clone(),
        }
    }
}

impl TryFrom<StoredPayload> for VersionedBytes {
    type Error = PostgresDurableError;

    fn try_from(payload: StoredPayload) -> Result<Self, Self::Error> {
        Ok(Self::new(
            SchemaVersion::new(payload.schema_version)?,
            CodecVersion::new(payload.codec_version)?,
            payload.bytes,
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRecord {
    position: u64,
    payload: StoredPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredInbox {
    identity: String,
    ingress_fingerprint: Vec<u8>,
    committed_through: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEffect {
    identity: String,
    accepted_at: u64,
    payload: StoredPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredReceipt {
    identity: String,
    effect_identity: String,
    outcome: String,
    recorded_at: u64,
    fence: u64,
    payload: StoredPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredImage {
    owner_id: String,
    mode: String,
    position: u64,
    fence: u64,
    history: Vec<StoredRecord>,
    snapshot: Option<StoredRecord>,
    inbox: Vec<StoredInbox>,
    outbox: Vec<StoredEffect>,
    receipts: Vec<StoredReceipt>,
}

impl From<&DurableOwnerImage> for StoredImage {
    fn from(image: &DurableOwnerImage) -> Self {
        Self {
            owner_id: image.owner_id.as_str().to_owned(),
            mode: mode_text(image.mode).to_owned(),
            position: image.position.get(),
            fence: image.fence.get(),
            history: image.history.iter().map(stored_record).collect(),
            snapshot: image.snapshot.as_ref().map(stored_record),
            inbox: image
                .inbox
                .values()
                .map(|record| StoredInbox {
                    identity: record.identity.as_str().to_owned(),
                    ingress_fingerprint: record.ingress_fingerprint.clone(),
                    committed_through: record.committed_through.get(),
                })
                .collect(),
            outbox: image
                .outbox
                .values()
                .map(|effect| StoredEffect {
                    identity: effect.identity.as_str().to_owned(),
                    accepted_at: effect.accepted_at.get(),
                    payload: StoredPayload::from(&effect.payload),
                })
                .collect(),
            receipts: image
                .receipts
                .values()
                .map(|receipt| StoredReceipt {
                    identity: receipt.identity.as_str().to_owned(),
                    effect_identity: receipt.effect_identity.as_str().to_owned(),
                    outcome: match receipt.outcome {
                        DurableEffectOutcome::Applied => "applied",
                        DurableEffectOutcome::Rejected => "rejected",
                    }
                    .to_owned(),
                    recorded_at: receipt.recorded_at.get(),
                    fence: receipt.fence.get(),
                    payload: StoredPayload::from(&receipt.payload),
                })
                .collect(),
        }
    }
}

impl TryFrom<StoredImage> for DurableOwnerImage {
    type Error = PostgresDurableError;

    fn try_from(image: StoredImage) -> Result<Self, Self::Error> {
        let history = image
            .history
            .into_iter()
            .map(durable_record)
            .collect::<Result<Vec<_>, _>>()?;
        let snapshot = image.snapshot.map(durable_record).transpose()?;
        let inbox = image
            .inbox
            .into_iter()
            .map(|record| {
                let identity = InboxIdentity::new(record.identity)?;
                Ok((
                    identity.clone(),
                    DurableInboxRecord {
                        identity,
                        ingress_fingerprint: record.ingress_fingerprint,
                        committed_through: DurablePosition::new(record.committed_through),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, PostgresDurableError>>()?;
        let outbox = image
            .outbox
            .into_iter()
            .map(|effect| {
                let identity = EffectIdentity::new(effect.identity)?;
                Ok((
                    identity.clone(),
                    DurableEffect {
                        identity,
                        accepted_at: DurablePosition::new(effect.accepted_at),
                        payload: effect.payload.try_into()?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, PostgresDurableError>>()?;
        let receipts = image
            .receipts
            .into_iter()
            .map(|receipt| {
                let identity = ReceiptIdentity::new(receipt.identity)?;
                let outcome = match receipt.outcome.as_str() {
                    "applied" => DurableEffectOutcome::Applied,
                    "rejected" => DurableEffectOutcome::Rejected,
                    _ => return Err(PostgresDurableError::Conversion("unknown receipt outcome")),
                };
                Ok((
                    identity.clone(),
                    DurableReceipt {
                        identity,
                        effect_identity: EffectIdentity::new(receipt.effect_identity)?,
                        outcome,
                        recorded_at: DurablePosition::new(receipt.recorded_at),
                        fence: FenceToken::new(receipt.fence),
                        payload: receipt.payload.try_into()?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, PostgresDurableError>>()?;
        Ok(Self {
            owner_id: DurableOwnerId::new(image.owner_id)?,
            mode: parse_mode(&image.mode)?,
            position: DurablePosition::new(image.position),
            fence: FenceToken::new(image.fence),
            history,
            snapshot,
            inbox,
            outbox,
            receipts,
        })
    }
}

fn stored_record(record: &DurableRecord) -> StoredRecord {
    StoredRecord {
        position: record.position.get(),
        payload: StoredPayload::from(&record.payload),
    }
}

fn durable_record(record: StoredRecord) -> Result<DurableRecord, PostgresDurableError> {
    Ok(DurableRecord {
        position: DurablePosition::new(record.position),
        payload: record.payload.try_into()?,
    })
}

fn decode_image(value: serde_json::Value) -> Result<DurableOwnerImage, PostgresDurableError> {
    let stored: StoredImage = serde_json::from_value(value)?;
    stored.try_into()
}

fn mode_text(mode: DurableOwnerMode) -> &'static str {
    match mode {
        DurableOwnerMode::EventHistory => "event_history",
        DurableOwnerMode::Snapshot => "snapshot",
    }
}

fn parse_mode(mode: &str) -> Result<DurableOwnerMode, PostgresDurableError> {
    match mode {
        "event_history" => Ok(DurableOwnerMode::EventHistory),
        "snapshot" => Ok(DurableOwnerMode::Snapshot),
        _ => Err(PostgresDurableError::Conversion(
            "unknown durable owner mode",
        )),
    }
}

fn to_i64(value: u64, message: &'static str) -> Result<i64, PostgresDurableError> {
    i64::try_from(value).map_err(|_| PostgresDurableError::Conversion(message))
}

fn from_i64(value: i64, message: &'static str) -> Result<u64, PostgresDurableError> {
    u64::try_from(value).map_err(|_| PostgresDurableError::Conversion(message))
}

fn to_i32(value: u32, message: &'static str) -> Result<i32, PostgresDurableError> {
    i32::try_from(value).map_err(|_| PostgresDurableError::Conversion(message))
}

fn to_u32(value: i32, message: &'static str) -> Result<u32, PostgresDurableError> {
    u32::try_from(value).map_err(|_| PostgresDurableError::Conversion(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_uses_skip_locked_and_durable_claim_columns() {
        assert!(POSTGRES_DURABLE_MIGRATION.contains("claim_until_epoch_millis"));
        let claim_sql = include_str!("durable_postgres.rs");
        assert!(claim_sql.contains("FOR UPDATE SKIP LOCKED"));
        assert!(claim_sql.contains("ORDER BY accepted_at, owner_id, effect_id"));
    }

    #[test]
    fn retry_policy_never_permits_zero_attempts() {
        assert_eq!(PostgresRetryPolicy { max_attempts: 0 }.attempts(), 1);
        assert_eq!(PostgresRetryPolicy { max_attempts: 3 }.attempts(), 3);
    }

    #[test]
    fn stored_image_round_trips_through_contract_validation() {
        let owner = DurableOwnerId::new("sample-owner").unwrap();
        let image =
            DurableOwnerCore::new(owner, DurableOwnerMode::EventHistory, FenceToken::new(7))
                .into_image();
        let value = serde_json::to_value(StoredImage::from(&image)).unwrap();
        let decoded = decode_image(value).unwrap();
        assert_eq!(decoded, image);
        DurableOwnerCore::recover(decoded).unwrap();
    }
}
