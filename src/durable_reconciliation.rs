//! Deterministic complete-history projection and dry-run reconciliation.
//!
//! Projection state is derived only from ordered durable facts. Checkpoints are
//! resumable proof artifacts, while projection/cache reads are advisory and can
//! never authorize a durable-owner transition.

use std::collections::BTreeMap;

use crate::{
    CodecVersion, DurableCommit, DurableContractError, DurableEffectIntent, DurableOwnerId,
    DurablePosition, EffectIdentity, SchemaVersion, VersionedBytes,
};

/// One ordered domain operation from the complete durable history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteHistoryOperation {
    Create(Vec<u8>),
    Amend(Vec<u8>),
    Retract,
}

/// A typed domain event after its versioned durable payload has been decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteHistoryEvent {
    pub position: DurablePosition,
    pub entity_id: String,
    pub operation: CompleteHistoryOperation,
}

/// Restart-safe projection state at an exact durable source frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionCheckpoint {
    pub source_position: DurablePosition,
    pub projection_version: u64,
    pub entries: BTreeMap<String, Vec<u8>>,
}

impl Default for ProjectionCheckpoint {
    fn default() -> Self {
        Self {
            source_position: DurablePosition::INITIAL,
            projection_version: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl ProjectionCheckpoint {
    /// Exact canonical bytes used for drift comparison and repair payloads.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        frame_u64(&mut bytes, self.source_position.get());
        frame_u64(&mut bytes, self.projection_version);
        frame_u64(&mut bytes, self.entries.len() as u64);
        for (entity_id, value) in &self.entries {
            frame_bytes(&mut bytes, entity_id.as_bytes());
            frame_bytes(&mut bytes, value);
        }
        bytes
    }
}

/// Replay failure. All variants fail closed before returning a new checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionReplayError {
    PositionGap {
        expected: DurablePosition,
        actual: DurablePosition,
    },
    ProjectionVersionOverflow,
    EmptyEntityIdentity,
    EntityAlreadyExists(String),
    MissingEntity(String),
}

/// Stateless deterministic projector over a complete, ordered history.
pub struct CompleteHistoryProjector;

impl CompleteHistoryProjector {
    pub fn rebuild(
        history: &[CompleteHistoryEvent],
    ) -> Result<ProjectionCheckpoint, ProjectionReplayError> {
        Self::resume(ProjectionCheckpoint::default(), history)
    }

    pub fn resume(
        mut checkpoint: ProjectionCheckpoint,
        suffix: &[CompleteHistoryEvent],
    ) -> Result<ProjectionCheckpoint, ProjectionReplayError> {
        for event in suffix {
            let expected = checkpoint
                .source_position
                .get()
                .checked_add(1)
                .map(DurablePosition::new)
                .ok_or(ProjectionReplayError::ProjectionVersionOverflow)?;
            if event.position != expected {
                return Err(ProjectionReplayError::PositionGap {
                    expected,
                    actual: event.position,
                });
            }
            if event.entity_id.is_empty() {
                return Err(ProjectionReplayError::EmptyEntityIdentity);
            }
            match &event.operation {
                CompleteHistoryOperation::Create(value) => {
                    if checkpoint
                        .entries
                        .insert(event.entity_id.clone(), value.clone())
                        .is_some()
                    {
                        return Err(ProjectionReplayError::EntityAlreadyExists(
                            event.entity_id.clone(),
                        ));
                    }
                }
                CompleteHistoryOperation::Amend(value) => {
                    let Some(entry) = checkpoint.entries.get_mut(&event.entity_id) else {
                        return Err(ProjectionReplayError::MissingEntity(
                            event.entity_id.clone(),
                        ));
                    };
                    *entry = value.clone();
                }
                CompleteHistoryOperation::Retract => {
                    if checkpoint.entries.remove(&event.entity_id).is_none() {
                        return Err(ProjectionReplayError::MissingEntity(
                            event.entity_id.clone(),
                        ));
                    }
                }
            }
            checkpoint.source_position = event.position;
            checkpoint.projection_version = checkpoint
                .projection_version
                .checked_add(1)
                .ok_or(ProjectionReplayError::ProjectionVersionOverflow)?;
        }
        Ok(checkpoint)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionHealth {
    Healthy,
    Lagging,
    Drifted,
}

/// Projection and cache observations are never transition authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionReadAuthority {
    Advisory,
}

impl ProjectionReadAuthority {
    #[must_use]
    pub const fn may_authorize_transition(self) -> bool {
        false
    }
}

/// Side-effect-free comparison against an authoritative history rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub health: ProjectionHealth,
    pub lag: u64,
    pub drift: bool,
    pub expected: ProjectionCheckpoint,
    pub observed: ProjectionCheckpoint,
    pub read_authority: ProjectionReadAuthority,
}

impl ReconciliationReport {
    #[must_use]
    pub fn dry_run(expected: ProjectionCheckpoint, observed: ProjectionCheckpoint) -> Self {
        let lag = expected
            .source_position
            .get()
            .saturating_sub(observed.source_position.get());
        let drift = expected.canonical_bytes() != observed.canonical_bytes();
        let health = if lag > 0 {
            ProjectionHealth::Lagging
        } else if drift {
            ProjectionHealth::Drifted
        } else {
            ProjectionHealth::Healthy
        };
        Self {
            health,
            lag,
            drift,
            expected,
            observed,
            read_authority: ProjectionReadAuthority::Advisory,
        }
    }

    /// Stable identity for this owner's repair at the rebuilt source frontier.
    pub fn reconciliation_effect_identity(
        &self,
        owner_id: &DurableOwnerId,
    ) -> Result<EffectIdentity, DurableContractError> {
        EffectIdentity::new(format!(
            "reconcile/{}/{}",
            owner_id.as_str(),
            self.expected.source_position.get()
        ))
    }

    /// Attach a stable repair intent to the owner's transactional commit.
    /// Repeating the same source frontier produces byte-identical outbox input.
    pub fn schedule_reconciliation(
        &self,
        owner_id: &DurableOwnerId,
        commit: &mut DurableCommit,
        schema_version: SchemaVersion,
        codec_version: CodecVersion,
    ) -> Result<bool, DurableContractError> {
        if self.health == ProjectionHealth::Healthy {
            return Ok(false);
        }
        let identity = self.reconciliation_effect_identity(owner_id)?;
        let intent = DurableEffectIntent {
            identity: identity.clone(),
            payload: VersionedBytes::new(
                schema_version,
                codec_version,
                self.expected.canonical_bytes(),
            ),
        };
        if commit
            .effects
            .iter()
            .any(|effect| effect.identity == identity)
        {
            return Ok(false);
        }
        commit.effects.push(intent);
        Ok(true)
    }
}

fn frame_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn frame_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    frame_u64(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}
