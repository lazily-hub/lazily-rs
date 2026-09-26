//! Lightweight durable wire client with an injected NATS-compatible transport.
//!
//! This module can publish and observe typed envelopes, receipts, and advisory
//! projection metadata. It intentionally exposes no durable-owner transition
//! API: database-backed host features own that authority.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{CodecVersion, SchemaVersion, VersionedBytes};

#[derive(Deserialize)]
struct EnvelopeHeader {
    protocol_version: u32,
}

fn preflight_protocol(encoded: &[u8]) -> Result<(), DurableClientError> {
    let header: EnvelopeHeader = serde_json::from_slice(encoded)
        .map_err(|error| DurableClientError::Codec(error.to_string()))?;
    if header.protocol_version != DurableEnvelope::PROTOCOL_VERSION {
        return Err(DurableClientError::Codec(format!(
            "unsupported protocol version {}",
            header.protocol_version
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableClientError {
    Config(&'static str),
    Codec(String),
    Transport(String),
}

impl std::fmt::Display for DurableClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(message) => write!(formatter, "durable client configuration: {message}"),
            Self::Codec(message) => write!(formatter, "durable client codec: {message}"),
            Self::Transport(message) => write!(formatter, "durable client transport: {message}"),
        }
    }
}

impl std::error::Error for DurableClientError {}

/// The transport-neutral v1 envelope shared with the JetStream host adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableEnvelope {
    pub protocol_version: u32,
    pub message_id: String,
    pub schema_version: u32,
    pub codec_version: u32,
    pub payload: Vec<u8>,
}

impl DurableEnvelope {
    pub const PROTOCOL_VERSION: u32 = 1;

    pub fn new(
        message_id: impl Into<String>,
        payload: &VersionedBytes,
    ) -> Result<Self, DurableClientError> {
        let envelope = Self {
            protocol_version: Self::PROTOCOL_VERSION,
            message_id: message_id.into(),
            schema_version: payload.schema_version.get(),
            codec_version: payload.codec_version.get(),
            payload: payload.bytes.clone(),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), DurableClientError> {
        if self.protocol_version != Self::PROTOCOL_VERSION {
            return Err(DurableClientError::Codec(format!(
                "unsupported protocol version {}",
                self.protocol_version
            )));
        }
        if self.message_id.is_empty() {
            return Err(DurableClientError::Config("message id is empty"));
        }
        SchemaVersion::new(self.schema_version)
            .map_err(|error| DurableClientError::Codec(format!("{error:?}")))?;
        CodecVersion::new(self.codec_version)
            .map_err(|error| DurableClientError::Codec(format!("{error:?}")))?;
        Ok(())
    }

    pub fn versioned_bytes(&self) -> Result<VersionedBytes, DurableClientError> {
        self.validate()?;
        Ok(VersionedBytes::new(
            SchemaVersion::new(self.schema_version).expect("validated schema version"),
            CodecVersion::new(self.codec_version).expect("validated codec version"),
            self.payload.clone(),
        ))
    }
}

/// Minimal raw NATS publish/observe seam. Implementations may wrap a native
/// client, an FFI bridge, or a deterministic test transport.
pub trait CompatibleNatsTransport {
    type Error: std::fmt::Display;

    fn publish(
        &mut self,
        subject: &str,
        message_id: &str,
        payload: &[u8],
    ) -> Result<(), Self::Error>;

    fn try_receive(&mut self, subject: &str) -> Result<Option<Vec<u8>>, Self::Error>;
}

/// Typed client over an injected compatible transport. It never owns a durable
/// transition or turns a broker acknowledgement into a durable receipt.
pub struct DurableClient<T> {
    subject: String,
    transport: T,
}

impl<T: CompatibleNatsTransport> DurableClient<T> {
    pub fn new(subject: impl Into<String>, transport: T) -> Result<Self, DurableClientError> {
        let subject = subject.into();
        if subject.is_empty() {
            return Err(DurableClientError::Config("subject is empty"));
        }
        Ok(Self { subject, transport })
    }

    pub fn publish(&mut self, envelope: &DurableEnvelope) -> Result<(), DurableClientError> {
        envelope.validate()?;
        let encoded = serde_json::to_vec(envelope)
            .map_err(|error| DurableClientError::Codec(error.to_string()))?;
        self.transport
            .publish(&self.subject, &envelope.message_id, &encoded)
            .map_err(|error| DurableClientError::Transport(error.to_string()))
    }

    pub fn try_receive(&mut self) -> Result<Option<DurableEnvelope>, DurableClientError> {
        let Some(encoded) = self
            .transport
            .try_receive(&self.subject)
            .map_err(|error| DurableClientError::Transport(error.to_string()))?
        else {
            return Ok(None);
        };
        // Deserialize only the protocol header first. Serde skips the payload as
        // IgnoredAny, so an unknown version fails before allocating/decoding its
        // byte array into the typed envelope.
        preflight_protocol(&encoded)?;
        let envelope: DurableEnvelope = serde_json::from_slice(&encoded)
            .map_err(|error| DurableClientError::Codec(error.to_string()))?;
        envelope.validate()?;
        Ok(Some(envelope))
    }

    #[must_use]
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    #[must_use]
    pub fn into_transport(self) -> T {
        self.transport
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableDeliveryClassification {
    First,
    Duplicate,
    Conflict,
}

#[must_use]
pub fn classify_durable_delivery(
    previous: Option<&DurableEnvelope>,
    candidate: &DurableEnvelope,
) -> DurableDeliveryClassification {
    match previous {
        None => DurableDeliveryClassification::First,
        Some(previous) if previous == candidate => DurableDeliveryClassification::Duplicate,
        Some(previous) if previous.message_id == candidate.message_id => {
            DurableDeliveryClassification::Conflict
        }
        Some(_) => DurableDeliveryClassification::First,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableReceiptStatus {
    Committed,
    Duplicate,
    Conflict,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableClientReceipt {
    pub protocol_version: u32,
    pub receipt_id: String,
    pub message_id: String,
    pub outcome: DurableReceiptStatus,
    pub owner_position: u64,
}

impl DurableClientReceipt {
    pub const PROTOCOL_VERSION: u32 = 1;

    pub fn validate(&self) -> Result<(), DurableClientError> {
        if self.protocol_version != Self::PROTOCOL_VERSION {
            return Err(DurableClientError::Codec(format!(
                "unsupported protocol version {}",
                self.protocol_version
            )));
        }
        if self.receipt_id.is_empty() {
            return Err(DurableClientError::Config("receipt id is empty"));
        }
        if self.message_id.is_empty() {
            return Err(DurableClientError::Config("message id is empty"));
        }
        Ok(())
    }

    pub fn from_wire(encoded: &[u8]) -> Result<Self, DurableClientError> {
        preflight_protocol(encoded)?;
        let receipt: Self = serde_json::from_slice(encoded)
            .map_err(|error| DurableClientError::Codec(error.to_string()))?;
        receipt.validate()?;
        Ok(receipt)
    }

    #[must_use]
    pub const fn transport_ack_equivalent(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionObservationCompleteness {
    CompleteHistory,
    LatestStateOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableProjectionObservation {
    pub projection_id: String,
    pub source_position: u64,
    pub fingerprint: String,
    pub completeness: ProjectionObservationCompleteness,
    pub may_authorize_transition: bool,
}

impl DurableProjectionObservation {
    pub fn validate(&self) -> Result<(), DurableClientError> {
        if self.projection_id.is_empty() {
            return Err(DurableClientError::Config("projection id is empty"));
        }
        if self.fingerprint.is_empty()
            || !self
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DurableClientError::Codec(
                "projection fingerprint must be non-empty lowercase hex".to_owned(),
            ));
        }
        if self.may_authorize_transition {
            return Err(DurableClientError::Config(
                "client projection may not authorize transitions",
            ));
        }
        Ok(())
    }

    pub fn from_wire(encoded: &[u8]) -> Result<Self, DurableClientError> {
        let observation: Self = serde_json::from_slice(encoded)
            .map_err(|error| DurableClientError::Codec(error.to_string()))?;
        observation.validate()?;
        Ok(observation)
    }

    #[must_use]
    pub fn equivalent_to(&self, other: &Self) -> bool {
        self.validate().is_ok()
            && other.validate().is_ok()
            && self.projection_id == other.projection_id
            && self.source_position == other.source_position
            && self.fingerprint == other.fingerprint
            && self.completeness == other.completeness
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDelivery {
    Buffered,
    Applied,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionOrderObservation {
    pub delivery: ProjectionDelivery,
    pub applied_positions: Vec<u64>,
}

/// Advisory source-position reorder buffer. The output is suitable for a local
/// projection only and cannot authorize a durable transition.
#[derive(Debug, Clone, Default)]
pub struct AdvisoryProjectionOrder {
    applied_through: u64,
    buffered: BTreeSet<u64>,
}

impl AdvisoryProjectionOrder {
    #[must_use]
    pub fn observe(&mut self, source_position: u64) -> ProjectionOrderObservation {
        if source_position <= self.applied_through || self.buffered.contains(&source_position) {
            return ProjectionOrderObservation {
                delivery: ProjectionDelivery::Duplicate,
                applied_positions: Vec::new(),
            };
        }
        if source_position > self.applied_through + 1 {
            self.buffered.insert(source_position);
            return ProjectionOrderObservation {
                delivery: ProjectionDelivery::Buffered,
                applied_positions: Vec::new(),
            };
        }
        let mut applied_positions = vec![source_position];
        self.applied_through = source_position;
        while self.buffered.remove(&(self.applied_through + 1)) {
            self.applied_through += 1;
            applied_positions.push(self.applied_through);
        }
        ProjectionOrderObservation {
            delivery: ProjectionDelivery::Applied,
            applied_positions,
        }
    }

    #[must_use]
    pub const fn may_authorize_transition(&self) -> bool {
        false
    }
}
