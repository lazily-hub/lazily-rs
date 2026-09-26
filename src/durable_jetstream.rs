//! Pull-based NATS JetStream transport subordinate to the Postgres durable owner.
//!
//! The module keeps terminal broker responses behind methods that require the
//! corresponding durable result. Progress and NAK only affect transport
//! ownership; neither can authorize a domain transition.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_nats::jetstream::consumer::{AckPolicy, PullConsumer};
use async_nats::jetstream::message::PublishMessage;
use async_nats::jetstream::{self, AckKind};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::{
    CodecVersion, DurableCommitOutcome, DurableEffectOutcome, DurableReceiptIntent,
    DurableReceiptOutcome, PostgresDurableError, PostgresDurableHost, PostgresDurableUnitOfWork,
    PostgresIngressDisposition, PostgresOutboxClaim, ReceiptIdentity, SchemaVersion,
    VersionedBytes,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JetStreamAdapterError {
    Config(&'static str),
    Draining,
    Codec(String),
    Transport(String),
    Durable(String),
    InboxIdentityMismatch { envelope: String, inbox: String },
    NotPoison,
}

impl std::fmt::Display for JetStreamAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(message) => write!(formatter, "JetStream configuration: {message}"),
            Self::Draining => write!(formatter, "JetStream ingress is draining"),
            Self::Codec(message) => write!(formatter, "JetStream envelope codec: {message}"),
            Self::Transport(message) => write!(formatter, "JetStream transport: {message}"),
            Self::Durable(message) => write!(formatter, "durable authority: {message}"),
            Self::InboxIdentityMismatch { envelope, inbox } => write!(
                formatter,
                "durable inbox identity {inbox:?} does not match envelope identity {envelope:?}"
            ),
            Self::NotPoison => write!(formatter, "delivery contains a valid typed envelope"),
        }
    }
}

impl std::error::Error for JetStreamAdapterError {}

impl From<PostgresDurableError> for JetStreamAdapterError {
    fn from(error: PostgresDurableError) -> Self {
        Self::Durable(error.to_string())
    }
}

fn transport(error: impl std::fmt::Display) -> JetStreamAdapterError {
    JetStreamAdapterError::Transport(error.to_string())
}

/// Versioned portable payload carried by ingress and outbox publications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetStreamEnvelope {
    pub protocol_version: u32,
    pub message_id: String,
    pub schema_version: u32,
    pub codec_version: u32,
    pub payload: Vec<u8>,
}

impl JetStreamEnvelope {
    pub const PROTOCOL_VERSION: u32 = 1;

    pub fn new(
        message_id: impl Into<String>,
        payload: &VersionedBytes,
    ) -> Result<Self, JetStreamAdapterError> {
        let message_id = message_id.into();
        if message_id.is_empty() {
            return Err(JetStreamAdapterError::Config("message id is empty"));
        }
        Ok(Self {
            protocol_version: Self::PROTOCOL_VERSION,
            message_id,
            schema_version: payload.schema_version.get(),
            codec_version: payload.codec_version.get(),
            payload: payload.bytes.clone(),
        })
    }

    pub fn versioned_bytes(&self) -> Result<VersionedBytes, JetStreamAdapterError> {
        if self.protocol_version != Self::PROTOCOL_VERSION {
            return Err(JetStreamAdapterError::Codec(format!(
                "unsupported protocol version {}",
                self.protocol_version
            )));
        }
        if self.message_id.is_empty() {
            return Err(JetStreamAdapterError::Codec(
                "message id is empty".to_owned(),
            ));
        }
        let schema_version = SchemaVersion::new(self.schema_version)
            .map_err(|error| JetStreamAdapterError::Codec(format!("{error:?}")))?;
        let codec_version = CodecVersion::new(self.codec_version)
            .map_err(|error| JetStreamAdapterError::Codec(format!("{error:?}")))?;
        Ok(VersionedBytes::new(
            schema_version,
            codec_version,
            self.payload.clone(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetStreamIngressConfig {
    pub stream: String,
    pub subject: String,
    pub durable_consumer: String,
    pub ack_wait: Duration,
    pub max_deliver: i64,
    pub max_ack_pending: i64,
    pub fetch_timeout: Duration,
}

impl JetStreamIngressConfig {
    fn validate(&self) -> Result<(), JetStreamAdapterError> {
        if self.stream.is_empty() || self.subject.is_empty() || self.durable_consumer.is_empty() {
            return Err(JetStreamAdapterError::Config(
                "stream, subject, and durable consumer are required",
            ));
        }
        if self.ack_wait.is_zero()
            || self.fetch_timeout.is_zero()
            || self.max_deliver <= 0
            || self.max_ack_pending <= 0
        {
            return Err(JetStreamAdapterError::Config(
                "ack wait, fetch timeout, delivery budget, and ack-pending bound must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetStreamDeliveryMetadata {
    pub stream: String,
    pub consumer: String,
    pub subject: String,
    pub stream_sequence: u64,
    pub consumer_sequence: u64,
    pub delivery_count: i64,
    pub pending: u64,
}

impl JetStreamDeliveryMetadata {
    #[must_use]
    pub fn delivery_id(&self) -> String {
        format!("{}:{}", self.stream, self.stream_sequence)
    }
}

/// One leased pull delivery. The raw broker message is private so callers
/// cannot ACK or TERM without crossing the durable methods below.
pub struct JetStreamDelivery {
    message: jetstream::Message,
    metadata: JetStreamDeliveryMetadata,
    envelope: Result<JetStreamEnvelope, String>,
    raw_payload: Vec<u8>,
}

impl JetStreamDelivery {
    #[must_use]
    pub fn metadata(&self) -> &JetStreamDeliveryMetadata {
        &self.metadata
    }

    pub fn envelope(&self) -> Result<&JetStreamEnvelope, JetStreamAdapterError> {
        self.envelope
            .as_ref()
            .map_err(|error| JetStreamAdapterError::Codec(error.clone()))
    }

    /// Extend only the broker's AckWait window. No durable state changes.
    pub async fn progress(&self) -> Result<(), JetStreamAdapterError> {
        self.message
            .ack_with(AckKind::Progress)
            .await
            .map_err(transport)
    }

    /// Retry transport delivery immediately or after a bounded broker delay.
    pub async fn retry(self, delay: Option<Duration>) -> Result<(), JetStreamAdapterError> {
        self.message
            .ack_with(AckKind::Nak(delay))
            .await
            .map_err(transport)
    }

    /// Run the authoritative durable operation, then ACK with server
    /// confirmation. An error returns without any ACK and is redelivered.
    pub async fn commit_then_ack(
        self,
        host: &PostgresDurableHost,
        work: PostgresDurableUnitOfWork,
    ) -> Result<DurableCommitOutcome, JetStreamAdapterError> {
        let envelope = self.envelope()?;
        envelope.versioned_bytes()?;
        if work.commit.inbox_identity.as_str() != envelope.message_id {
            return Err(JetStreamAdapterError::InboxIdentityMismatch {
                envelope: envelope.message_id.clone(),
                inbox: work.commit.inbox_identity.as_str().to_owned(),
            });
        }
        let outcome = tokio::task::block_in_place(|| host.commit_unit_of_work(work))?;
        self.message.double_ack().await.map_err(transport)?;
        Ok(outcome)
    }

    /// Persist the poison disposition, publish a stable dead-letter record,
    /// then TERM the broker delivery. Failure at either earlier stage leaves the
    /// delivery un-terminated so redelivery can resume the sequence.
    pub async fn persist_poison_then_term(
        self,
        host: &PostgresDurableHost,
        dead_letters: &JetStreamDeadLetterSink,
        disposed_at_epoch_millis: i64,
    ) -> Result<bool, JetStreamAdapterError> {
        let diagnostic = match &self.envelope {
            Ok(_) => return Err(JetStreamAdapterError::NotPoison),
            Err(error) => error.clone(),
        };
        let disposition = PostgresIngressDisposition {
            transport: "nats-jetstream".to_owned(),
            delivery_id: self.metadata.delivery_id(),
            diagnostic: diagnostic.clone(),
            payload: self.raw_payload.clone(),
            disposed_at_epoch_millis,
        };
        let inserted = tokio::task::block_in_place(|| host.record_ingress_poison(&disposition))?;
        dead_letters
            .publish(&self.metadata, &diagnostic, &self.raw_payload)
            .await?;
        self.message
            .ack_with(AckKind::Term)
            .await
            .map_err(transport)?;
        Ok(inserted)
    }
}

/// Pull consumer with explicit bounds and a local drain switch. Beginning a
/// drain stops new pulls; already returned deliveries remain usable.
pub struct JetStreamIngress {
    consumer: PullConsumer,
    config: JetStreamIngressConfig,
    draining: Arc<AtomicBool>,
}

impl JetStreamIngress {
    pub async fn bind(
        context: jetstream::Context,
        config: JetStreamIngressConfig,
    ) -> Result<Self, JetStreamAdapterError> {
        config.validate()?;
        let stream = context
            .get_or_create_stream(jetstream::stream::Config {
                name: config.stream.clone(),
                subjects: vec![config.subject.clone()],
                ..Default::default()
            })
            .await
            .map_err(transport)?;
        let consumer = stream
            .get_or_create_consumer(
                &config.durable_consumer,
                jetstream::consumer::pull::Config {
                    durable_name: Some(config.durable_consumer.clone()),
                    ack_policy: AckPolicy::Explicit,
                    ack_wait: config.ack_wait,
                    max_deliver: config.max_deliver,
                    max_ack_pending: config.max_ack_pending,
                    filter_subject: config.subject.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(transport)?;
        Ok(Self {
            consumer,
            config,
            draining: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn begin_drain(&self) {
        self.draining.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
    }

    pub async fn next(&self) -> Result<Option<JetStreamDelivery>, JetStreamAdapterError> {
        if self.is_draining() {
            return Ok(None);
        }
        let mut messages = self
            .consumer
            .fetch()
            .max_messages(1)
            .expires(self.config.fetch_timeout)
            .messages()
            .await
            .map_err(transport)?;
        let Some(message) = messages.next().await else {
            return Ok(None);
        };
        let message = message.map_err(transport)?;
        let info = message.info().map_err(transport)?;
        let metadata = JetStreamDeliveryMetadata {
            stream: info.stream.to_owned(),
            consumer: info.consumer.to_owned(),
            subject: message.message.subject.to_string(),
            stream_sequence: info.stream_sequence,
            consumer_sequence: info.consumer_sequence,
            delivery_count: info.delivered,
            pending: info.pending,
        };
        let raw_payload = message.message.payload.to_vec();
        let envelope = serde_json::from_slice::<JetStreamEnvelope>(&raw_payload)
            .map_err(|error| error.to_string())
            .and_then(|envelope| {
                envelope
                    .versioned_bytes()
                    .map(|_| envelope)
                    .map_err(|error| error.to_string())
            });
        Ok(Some(JetStreamDelivery {
            message,
            metadata,
            envelope,
            raw_payload,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetStreamPublication {
    pub stream: String,
    pub sequence: u64,
    pub duplicate: bool,
    pub message_id: String,
}

#[derive(Clone)]
pub struct JetStreamPublisher {
    context: jetstream::Context,
}

impl JetStreamPublisher {
    #[must_use]
    pub fn new(context: jetstream::Context) -> Self {
        Self { context }
    }

    pub async fn publish(
        &self,
        subject: &str,
        envelope: &JetStreamEnvelope,
    ) -> Result<JetStreamPublication, JetStreamAdapterError> {
        if subject.is_empty() {
            return Err(JetStreamAdapterError::Config("publish subject is empty"));
        }
        envelope.versioned_bytes()?;
        let payload = serde_json::to_vec(envelope)
            .map_err(|error| JetStreamAdapterError::Codec(error.to_string()))?;
        self.publish_bytes(subject, &envelope.message_id, payload)
            .await
    }

    async fn publish_bytes(
        &self,
        subject: &str,
        message_id: &str,
        payload: Vec<u8>,
    ) -> Result<JetStreamPublication, JetStreamAdapterError> {
        let ack = self
            .context
            .send_publish(
                subject.to_owned(),
                PublishMessage::build()
                    .payload(payload.into())
                    .message_id(message_id),
            )
            .await
            .map_err(transport)?
            .await
            .map_err(transport)?;
        Ok(JetStreamPublication {
            stream: ack.stream,
            sequence: ack.sequence,
            duplicate: ack.duplicate,
            message_id: message_id.to_owned(),
        })
    }
}

#[derive(Debug, Serialize)]
struct DeadLetterRecord<'a> {
    protocol_version: u32,
    delivery_id: String,
    stream: &'a str,
    consumer: &'a str,
    subject: &'a str,
    delivery_count: i64,
    diagnostic: &'a str,
    payload: &'a [u8],
}

#[derive(Clone)]
pub struct JetStreamDeadLetterSink {
    publisher: JetStreamPublisher,
    subject: String,
}

impl JetStreamDeadLetterSink {
    pub fn new(
        publisher: JetStreamPublisher,
        subject: impl Into<String>,
    ) -> Result<Self, JetStreamAdapterError> {
        let subject = subject.into();
        if subject.is_empty() {
            return Err(JetStreamAdapterError::Config(
                "dead-letter subject is empty",
            ));
        }
        Ok(Self { publisher, subject })
    }

    async fn publish(
        &self,
        metadata: &JetStreamDeliveryMetadata,
        diagnostic: &str,
        payload: &[u8],
    ) -> Result<JetStreamPublication, JetStreamAdapterError> {
        let delivery_id = metadata.delivery_id();
        let record = DeadLetterRecord {
            protocol_version: 1,
            delivery_id: delivery_id.clone(),
            stream: &metadata.stream,
            consumer: &metadata.consumer,
            subject: &metadata.subject,
            delivery_count: metadata.delivery_count,
            diagnostic,
            payload,
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|error| JetStreamAdapterError::Codec(error.to_string()))?;
        self.publisher
            .publish_bytes(&self.subject, &format!("poison.{delivery_id}"), bytes)
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetStreamRelayOutcome {
    pub publication: JetStreamPublication,
    pub receipt: DurableReceiptOutcome,
}

pub struct JetStreamOutboxRelay {
    publisher: JetStreamPublisher,
    subject: String,
}

impl JetStreamOutboxRelay {
    pub fn new(
        publisher: JetStreamPublisher,
        subject: impl Into<String>,
    ) -> Result<Self, JetStreamAdapterError> {
        let subject = subject.into();
        if subject.is_empty() {
            return Err(JetStreamAdapterError::Config("relay subject is empty"));
        }
        Ok(Self { publisher, subject })
    }

    pub async fn publish_claim(
        &self,
        host: &PostgresDurableHost,
        claim: &PostgresOutboxClaim,
        fence: crate::FenceToken,
    ) -> Result<JetStreamRelayOutcome, JetStreamAdapterError> {
        let message_id = format!(
            "{}.{}",
            claim.owner_id.as_str(),
            claim.effect.identity.as_str()
        );
        let envelope = JetStreamEnvelope::new(message_id.clone(), &claim.effect.payload)?;
        let publication = self.publisher.publish(&self.subject, &envelope).await?;
        let receipt_intent = DurableReceiptIntent {
            identity: ReceiptIdentity::new(format!("jetstream.{message_id}"))
                .map_err(|error| JetStreamAdapterError::Durable(format!("{error:?}")))?,
            effect_identity: claim.effect.identity.clone(),
            outcome: DurableEffectOutcome::Applied,
            payload: VersionedBytes::new(
                SchemaVersion::new(1)
                    .map_err(|error| JetStreamAdapterError::Durable(format!("{error:?}")))?,
                CodecVersion::new(1)
                    .map_err(|error| JetStreamAdapterError::Durable(format!("{error:?}")))?,
                format!("{}:{}", publication.stream, publication.sequence),
            ),
        };
        let receipt = tokio::task::block_in_place(|| {
            host.record_publication_receipt(&claim.owner_id, fence, receipt_intent)
        })?;
        Ok(JetStreamRelayOutcome {
            publication,
            receipt,
        })
    }
}

/// Disposable wakeup hint. Durable work existence remains a Postgres query.
pub struct JetStreamWakeupSource {
    client: async_nats::Client,
    subject: String,
    subscriber: tokio::sync::Mutex<async_nats::Subscriber>,
}

impl JetStreamWakeupSource {
    pub async fn subscribe(
        client: async_nats::Client,
        subject: impl Into<String>,
    ) -> Result<Self, JetStreamAdapterError> {
        let subject = subject.into();
        if subject.is_empty() {
            return Err(JetStreamAdapterError::Config("wakeup subject is empty"));
        }
        let subscriber = client.subscribe(subject.clone()).await.map_err(transport)?;
        Ok(Self {
            client,
            subject,
            subscriber: tokio::sync::Mutex::new(subscriber),
        })
    }

    pub async fn wake(&self) -> Result<(), JetStreamAdapterError> {
        self.client
            .publish(self.subject.clone(), Vec::new().into())
            .await
            .map_err(transport)
    }

    pub async fn next(&self) -> Option<()> {
        self.subscriber.lock().await.next().await.map(|_| ())
    }
}
