//! Generic causal receipt projection.
//!
//! Receipts record the outcome of a command or effect request keyed by a stable
//! causation id. This is deliberately not a transport ACK plane: `Observed` and
//! `Accepted` are non-terminal progress observations, while `Applied` and
//! `Rejected` are terminal outcomes.

use std::collections::{BTreeMap, BTreeSet};

/// Generic receipt outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ReceiptOutcome {
    /// A peer/process observed the causation request.
    Observed,
    /// A peer/process accepted or queued the request.
    Accepted,
    /// The requested effect/state change was applied.
    Applied,
    /// The requested effect/state change was rejected.
    Rejected,
}

impl ReceiptOutcome {
    /// Whether this outcome completes the causation.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Applied | Self::Rejected)
    }
}

/// One receipt event for a command/effect causation id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CausalReceipt {
    /// Idempotency key for this receipt event.
    pub receipt_id: String,
    /// Stable id of the command/effect request this receipt observes.
    pub causation_id: String,
    /// Peer, process, or subsystem that produced the receipt.
    pub observer: String,
    /// Producer/editor generation.
    pub generation: u64,
    /// Receipt outcome.
    pub outcome: ReceiptOutcome,
    /// Optional human/debug rejection reason.
    pub reason: Option<String>,
    /// Optional hash of the state/payload observed by the receipt.
    pub payload_hash: Option<String>,
}

impl CausalReceipt {
    /// Construct a receipt.
    #[must_use]
    pub fn new(
        receipt_id: impl Into<String>,
        causation_id: impl Into<String>,
        observer: impl Into<String>,
        generation: u64,
        outcome: ReceiptOutcome,
    ) -> Self {
        Self {
            receipt_id: receipt_id.into(),
            causation_id: causation_id.into(),
            observer: observer.into(),
            generation,
            outcome,
            reason: None,
            payload_hash: None,
        }
    }

    /// Construct an `observed` receipt.
    #[must_use]
    pub fn observed(
        receipt_id: impl Into<String>,
        causation_id: impl Into<String>,
        observer: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self::new(
            receipt_id,
            causation_id,
            observer,
            generation,
            ReceiptOutcome::Observed,
        )
    }

    /// Construct an `accepted` receipt.
    #[must_use]
    pub fn accepted(
        receipt_id: impl Into<String>,
        causation_id: impl Into<String>,
        observer: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self::new(
            receipt_id,
            causation_id,
            observer,
            generation,
            ReceiptOutcome::Accepted,
        )
    }

    /// Construct an `applied` receipt.
    #[must_use]
    pub fn applied(
        receipt_id: impl Into<String>,
        causation_id: impl Into<String>,
        observer: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self::new(
            receipt_id,
            causation_id,
            observer,
            generation,
            ReceiptOutcome::Applied,
        )
    }

    /// Construct a `rejected` receipt.
    #[must_use]
    pub fn rejected(
        receipt_id: impl Into<String>,
        causation_id: impl Into<String>,
        observer: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self::new(
            receipt_id,
            causation_id,
            observer,
            generation,
            ReceiptOutcome::Rejected,
        )
    }

    /// Attach a debug reason.
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Attach a payload hash.
    #[must_use]
    pub fn with_payload_hash(mut self, payload_hash: impl Into<String>) -> Self {
        self.payload_hash = Some(payload_hash.into());
        self
    }
}

/// Wire body for the externally-tagged `CausalReceipts` envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CausalReceipts {
    /// Receipt batch.
    pub receipts: Vec<CausalReceipt>,
}

impl CausalReceipts {
    /// Construct a receipt batch.
    #[must_use]
    pub fn new(receipts: impl IntoIterator<Item = CausalReceipt>) -> Self {
        Self {
            receipts: receipts.into_iter().collect(),
        }
    }
}

/// Externally-tagged receipt wire message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReceiptMessage {
    /// Receipt batch envelope.
    CausalReceipts(CausalReceipts),
}

/// Result of applying a receipt to a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptApplyStatus {
    /// Receipt was recorded.
    Recorded,
    /// Receipt id was already seen.
    Duplicate,
    /// Receipt generation does not match the current authority generation.
    StaleGeneration {
        /// Expected current generation.
        expected: u64,
        /// Generation carried by the receipt.
        actual: u64,
    },
    /// A different terminal outcome already exists for this causation id.
    TerminalConflict {
        /// Causation id with conflicting terminal receipts.
        causation_id: String,
        /// Existing terminal outcome.
        existing: ReceiptOutcome,
        /// Incoming conflicting terminal outcome.
        incoming: ReceiptOutcome,
    },
}

/// Folded receipt projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiptProjection {
    receipts_by_id: BTreeMap<String, CausalReceipt>,
    latest_by_causation: BTreeMap<String, CausalReceipt>,
    terminal_by_causation: BTreeMap<String, CausalReceipt>,
    stale_receipt_ids: BTreeSet<String>,
}

impl ReceiptProjection {
    /// Create an empty projection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one receipt.
    ///
    /// When `current_generation` is `Some`, receipts for any other generation are
    /// retained only as stale ids and do not update the current projection.
    pub fn observe(
        &mut self,
        current_generation: Option<u64>,
        receipt: CausalReceipt,
    ) -> ReceiptApplyStatus {
        if self.receipts_by_id.contains_key(&receipt.receipt_id)
            || self.stale_receipt_ids.contains(&receipt.receipt_id)
        {
            return ReceiptApplyStatus::Duplicate;
        }

        if let Some(expected) = current_generation
            && receipt.generation != expected
        {
            let actual = receipt.generation;
            self.stale_receipt_ids.insert(receipt.receipt_id);
            return ReceiptApplyStatus::StaleGeneration { expected, actual };
        }

        if receipt.outcome.is_terminal()
            && let Some(existing) = self.terminal_by_causation.get(&receipt.causation_id)
            && existing.outcome != receipt.outcome
        {
            return ReceiptApplyStatus::TerminalConflict {
                causation_id: receipt.causation_id,
                existing: existing.outcome,
                incoming: receipt.outcome,
            };
        }

        if receipt.outcome.is_terminal() {
            self.terminal_by_causation
                .entry(receipt.causation_id.clone())
                .or_insert_with(|| receipt.clone());
        }
        self.latest_by_causation
            .insert(receipt.causation_id.clone(), receipt.clone());
        self.receipts_by_id
            .insert(receipt.receipt_id.clone(), receipt);
        ReceiptApplyStatus::Recorded
    }

    /// Latest recorded receipt for a causation id, terminal or non-terminal.
    #[must_use]
    pub fn latest_for(&self, causation_id: &str) -> Option<&CausalReceipt> {
        self.latest_by_causation.get(causation_id)
    }

    /// Terminal receipt for a causation id.
    #[must_use]
    pub fn terminal_for(&self, causation_id: &str) -> Option<&CausalReceipt> {
        self.terminal_by_causation.get(causation_id)
    }

    /// Whether a receipt id has already been seen.
    #[must_use]
    pub fn contains_receipt(&self, receipt_id: &str) -> bool {
        self.receipts_by_id.contains_key(receipt_id) || self.stale_receipt_ids.contains(receipt_id)
    }

    /// Stale receipt ids observed by the projection.
    pub fn stale_receipt_ids(&self) -> impl Iterator<Item = &String> {
        self.stale_receipt_ids.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_outcome_terminality_is_explicit() {
        assert!(!ReceiptOutcome::Observed.is_terminal());
        assert!(!ReceiptOutcome::Accepted.is_terminal());
        assert!(ReceiptOutcome::Applied.is_terminal());
        assert!(ReceiptOutcome::Rejected.is_terminal());
    }

    #[test]
    fn projection_records_nonterminal_and_terminal_receipts() {
        let mut projection = ReceiptProjection::new();
        assert_eq!(
            projection.observe(
                Some(7),
                CausalReceipt::observed("receipt-observed", "patch-123", "editor", 7),
            ),
            ReceiptApplyStatus::Recorded
        );
        assert_eq!(
            projection.observe(
                Some(7),
                CausalReceipt::applied("receipt-applied", "patch-123", "editor", 7)
                    .with_payload_hash("sha256:abc"),
            ),
            ReceiptApplyStatus::Recorded
        );

        assert_eq!(
            projection.latest_for("patch-123").map(|r| r.outcome),
            Some(ReceiptOutcome::Applied)
        );
        assert_eq!(
            projection.terminal_for("patch-123").map(|r| r.outcome),
            Some(ReceiptOutcome::Applied)
        );
    }

    #[test]
    fn stale_generation_does_not_update_projection() {
        let mut projection = ReceiptProjection::new();
        assert_eq!(
            projection.observe(
                Some(7),
                CausalReceipt::rejected("receipt-stale", "patch-123", "editor", 6),
            ),
            ReceiptApplyStatus::StaleGeneration {
                expected: 7,
                actual: 6,
            }
        );

        assert!(projection.terminal_for("patch-123").is_none());
        assert!(projection.contains_receipt("receipt-stale"));
        assert_eq!(
            projection.stale_receipt_ids().cloned().collect::<Vec<_>>(),
            vec!["receipt-stale".to_string()]
        );
    }

    #[test]
    fn duplicate_receipt_id_is_noop() {
        let mut projection = ReceiptProjection::new();
        let receipt = CausalReceipt::accepted("receipt-1", "patch-123", "editor", 7);

        assert_eq!(
            projection.observe(Some(7), receipt.clone()),
            ReceiptApplyStatus::Recorded
        );
        assert_eq!(
            projection.observe(Some(7), receipt),
            ReceiptApplyStatus::Duplicate
        );
    }

    #[test]
    fn conflicting_terminal_receipts_fail_closed() {
        let mut projection = ReceiptProjection::new();
        assert_eq!(
            projection.observe(
                Some(7),
                CausalReceipt::applied("receipt-applied", "patch-123", "editor", 7),
            ),
            ReceiptApplyStatus::Recorded
        );

        assert_eq!(
            projection.observe(
                Some(7),
                CausalReceipt::rejected("receipt-rejected", "patch-123", "editor", 7),
            ),
            ReceiptApplyStatus::TerminalConflict {
                causation_id: "patch-123".to_string(),
                existing: ReceiptOutcome::Applied,
                incoming: ReceiptOutcome::Rejected,
            }
        );
        assert!(!projection.contains_receipt("receipt-rejected"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn receipt_message_uses_externally_tagged_wire_shape() {
        let message =
            ReceiptMessage::CausalReceipts(CausalReceipts::new([CausalReceipt::applied(
                "receipt-applied",
                "patch-123",
                "editor",
                7,
            )]));

        let value = serde_json::to_value(&message).expect("receipt message serializes");
        assert_eq!(value["CausalReceipts"]["receipts"][0]["outcome"], "applied");
        assert_eq!(
            value["CausalReceipts"]["receipts"][0]["reason"],
            serde_json::Value::Null
        );
    }
}
