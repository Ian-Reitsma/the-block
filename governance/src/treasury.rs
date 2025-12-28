use foundation_serialization::{json, Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::codec::{BinaryCodec, BinaryWriter, Result as CodecResult};

fn write_bytes(writer: &mut BinaryWriter, data: &[u8]) {
    writer.write_bytes(data);
}

fn read_bytes(reader: &mut crate::codec::BinaryReader<'_>) -> CodecResult<Vec<u8>> {
    reader.read_bytes()
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[allow(dead_code)]
const fn default_vote_window_epochs() -> u64 {
    4
}

#[allow(dead_code)]
const fn default_timelock_epochs() -> u64 {
    2
}

#[allow(dead_code)]
const fn default_rollback_epochs() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct QuorumSpec {
    #[serde(default)]
    pub operators_ppm: u32,
    #[serde(default)]
    pub builders_ppm: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProposalAttachment {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ExpectedReceipt {
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementProposalMetadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub deps: Vec<u64>,
    #[serde(default)]
    pub attachments: Vec<ProposalAttachment>,
    #[serde(default)]
    pub quorum: QuorumSpec,
    #[serde(default = "default_vote_window_epochs")]
    pub vote_window_epochs: u64,
    #[serde(default = "default_timelock_epochs")]
    pub timelock_epochs: u64,
    #[serde(default = "default_rollback_epochs")]
    pub rollback_window_epochs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementDetails {
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub amount: u64,
    #[serde(default)]
    pub memo: String,
    #[serde(default)]
    pub scheduled_epoch: u64,
    #[serde(default)]
    pub expected_receipts: Vec<ExpectedReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementPayload {
    #[serde(default)]
    pub proposal: DisbursementProposalMetadata,
    #[serde(default)]
    pub disbursement: DisbursementDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementReceipt {
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum DisbursementStatus {
    Draft {
        created_at: u64,
    },
    Voting {
        vote_deadline_epoch: u64,
    },
    Queued {
        #[serde(default)]
        queued_at: u64,
        #[serde(default)]
        activation_epoch: u64,
    },
    Timelocked {
        ready_epoch: u64,
    },
    Executed {
        tx_hash: String,
        executed_at: u64,
    },
    Finalized {
        tx_hash: String,
        executed_at: u64,
        finalized_at: u64,
    },
    RolledBack {
        reason: String,
        rolled_back_at: u64,
        #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
        prior_tx: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryDisbursement {
    pub id: u64,
    pub destination: String,
    pub amount: u64,
    pub memo: String,
    pub scheduled_epoch: u64,
    pub created_at: u64,
    pub status: DisbursementStatus,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub proposal: Option<DisbursementProposalMetadata>,
    #[serde(default)]
    pub expected_receipts: Vec<ExpectedReceipt>,
    #[serde(default)]
    pub receipts: Vec<DisbursementReceipt>,
}

impl TreasuryDisbursement {
    pub fn new(
        id: u64,
        destination: String,
        amount: u64,
        memo: String,
        scheduled_epoch: u64,
    ) -> Self {
        Self::from_payload(
            id,
            DisbursementPayload {
                proposal: DisbursementProposalMetadata::default(),
                disbursement: DisbursementDetails {
                    destination,
                    amount,
                    memo,
                    scheduled_epoch,
                    expected_receipts: Vec::new(),
                },
            },
        )
    }

    pub fn from_payload(id: u64, payload: DisbursementPayload) -> Self {
        let created_at = now_ts();
        // Convert empty/default proposal metadata to None to indicate this is a
        // treasury-initiated disbursement that doesn't require governance approval.
        // If the proposal has any non-default values, preserve it for governance workflow.
        let proposal = if payload.proposal == DisbursementProposalMetadata::default() {
            None
        } else {
            Some(payload.proposal)
        };
        Self {
            id,
            destination: payload.disbursement.destination,
            amount: payload.disbursement.amount,
            memo: payload.disbursement.memo,
            scheduled_epoch: payload.disbursement.scheduled_epoch,
            created_at,
            status: DisbursementStatus::Draft { created_at },
            proposal,
            expected_receipts: payload.disbursement.expected_receipts,
            receipts: Vec::new(),
        }
    }
}

pub fn mark_executed(disbursement: &mut TreasuryDisbursement, tx_hash: String) {
    disbursement.status = DisbursementStatus::Executed {
        tx_hash,
        executed_at: now_ts(),
    };
}

pub fn mark_finalized(disbursement: &mut TreasuryDisbursement) {
    if let DisbursementStatus::Executed {
        tx_hash,
        executed_at,
    } = &disbursement.status
    {
        disbursement.status = DisbursementStatus::Finalized {
            tx_hash: tx_hash.clone(),
            executed_at: *executed_at,
            finalized_at: now_ts(),
        };
    }
}

pub fn mark_rolled_back(disbursement: &mut TreasuryDisbursement, reason: String) {
    let prior_tx = match &disbursement.status {
        DisbursementStatus::Executed { tx_hash, .. }
        | DisbursementStatus::Finalized { tx_hash, .. } => Some(tx_hash.clone()),
        _ => None,
    };
    disbursement.status = DisbursementStatus::RolledBack {
        reason,
        rolled_back_at: now_ts(),
        prior_tx,
    };
}

pub fn mark_rolled_back_with_error(
    disbursement: &mut TreasuryDisbursement,
    error: DisbursementError,
) {
    let reason = format!("{:?}", error);
    mark_rolled_back(disbursement, reason);
}

pub fn mark_cancelled(disbursement: &mut TreasuryDisbursement, reason: String) {
    disbursement.status = DisbursementStatus::RolledBack {
        reason,
        rolled_back_at: now_ts(),
        prior_tx: None,
    };
}

#[derive(Debug)]
pub enum DisbursementValidationError {
    EmptyTitle,
    EmptySummary,
    InvalidDestination(String),
    ZeroAmount,
    ZeroScheduledEpoch,
    InvalidQuorum,
    InvalidVoteWindow,
    InvalidTimelock,
    InvalidRollbackWindow,
    ExpectedReceiptsMismatch { expected_total: u64, actual: u64 },
}

/// Explicit error states for treasury execution tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum DisbursementError {
    InsufficientFunds { required: u64, available: u64 },
    InvalidTarget {
        destination: String,
        reason: String,
    },
    StaleProposal {
        proposal_id: u64,
        current_epoch: u64,
        scheduled_epoch: u64,
    },
    DependencyFailed {
        dependency_id: u64,
        reason: String,
    },
    ExecutorBacklog {
        queue_depth: usize,
        threshold: usize,
    },
    NetworkError {
        message: String,
    },
    ValidationFailed {
        reason: String,
    },
}

impl std::fmt::Display for DisbursementValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTitle => write!(f, "disbursement title cannot be empty"),
            Self::EmptySummary => write!(f, "disbursement summary cannot be empty"),
            Self::InvalidDestination(dest) => write!(f, "invalid destination address: {}", dest),
            Self::ZeroAmount => write!(f, "disbursement amount must be greater than zero"),
            Self::ZeroScheduledEpoch => write!(f, "scheduled epoch must be greater than zero"),
            Self::InvalidQuorum => {
                write!(f, "quorum percentages must be between 0 and 1000000 ppm")
            }
            Self::InvalidVoteWindow => write!(f, "vote window must be at least 1 epoch"),
            Self::InvalidTimelock => write!(f, "timelock must be at least 1 epoch"),
            Self::InvalidRollbackWindow => write!(f, "rollback window must be at least 1 epoch"),
            Self::ExpectedReceiptsMismatch {
                expected_total,
                actual,
            } => write!(
                f,
                "expected receipts total {} does not match disbursement amount {}",
                expected_total, actual
            ),
        }
    }
}

impl std::error::Error for DisbursementValidationError {}

/// Validate a disbursement payload before submission
pub fn validate_disbursement_payload(
    payload: &DisbursementPayload,
) -> Result<(), DisbursementValidationError> {
    // Validate proposal metadata
    if payload.proposal.title.trim().is_empty() {
        return Err(DisbursementValidationError::EmptyTitle);
    }

    if payload.proposal.summary.trim().is_empty() {
        return Err(DisbursementValidationError::EmptySummary);
    }

    // Validate quorum (in parts per million, 0-1000000)
    const MAX_PPM: u32 = 1_000_000;
    if payload.proposal.quorum.operators_ppm > MAX_PPM
        || payload.proposal.quorum.builders_ppm > MAX_PPM
    {
        return Err(DisbursementValidationError::InvalidQuorum);
    }

    // Validate windows
    if payload.proposal.vote_window_epochs == 0 {
        return Err(DisbursementValidationError::InvalidVoteWindow);
    }

    if payload.proposal.timelock_epochs == 0 {
        return Err(DisbursementValidationError::InvalidTimelock);
    }

    if payload.proposal.rollback_window_epochs == 0 {
        return Err(DisbursementValidationError::InvalidRollbackWindow);
    }

    // Validate disbursement details
    if payload.disbursement.destination.trim().is_empty() {
        return Err(DisbursementValidationError::InvalidDestination(
            "empty destination".into(),
        ));
    }

    // Validate destination address format (basic check - starts with "tb1" for mainnet)
    if !payload.disbursement.destination.starts_with("tb1") {
        return Err(DisbursementValidationError::InvalidDestination(format!(
            "address must start with 'tb1', got: {}",
            payload.disbursement.destination
        )));
    }

    if payload.disbursement.amount == 0 {
        return Err(DisbursementValidationError::ZeroAmount);
    }

    if payload.disbursement.scheduled_epoch == 0 {
        return Err(DisbursementValidationError::ZeroScheduledEpoch);
    }

    // Validate expected receipts sum matches disbursement amount
    if !payload.disbursement.expected_receipts.is_empty() {
        let total: u64 = payload
            .disbursement
            .expected_receipts
            .iter()
            .map(|r| r.amount)
            .sum();

        if total != payload.disbursement.amount {
            return Err(DisbursementValidationError::ExpectedReceiptsMismatch {
                expected_total: total,
                actual: payload.disbursement.amount,
            });
        }
    }

    Ok(())
}

/// Validate dependency graph for a disbursement payload
pub fn validate_dependencies(
    payload: &DisbursementPayload,
    existing_disbursements: &[TreasuryDisbursement],
) -> Result<(), DisbursementError> {
    for dep_id in &payload.proposal.deps {
        // Find dependency
        let dep = existing_disbursements
            .iter()
            .find(|d| d.id == *dep_id)
            .ok_or_else(|| DisbursementError::DependencyFailed {
                dependency_id: *dep_id,
                reason: format!("Dependency {} not found", dep_id),
            })?;

        // Check dependency is in acceptable state
        match &dep.status {
            DisbursementStatus::Finalized { .. } => {
                // Good - dependency completed successfully
            }
            DisbursementStatus::RolledBack { reason, .. } => {
                return Err(DisbursementError::DependencyFailed {
                    dependency_id: *dep_id,
                    reason: format!("Dependency {} rolled back: {}", dep_id, reason),
                });
            }
            _ => {
                return Err(DisbursementError::DependencyFailed {
                    dependency_id: *dep_id,
                    reason: format!("Dependency {} not yet finalized", dep_id),
                });
            }
        }
    }
    Ok(())
}

/// Parse dependency IDs out of a memo string.
///
/// The parser supports both JSON and key=value styles so tooling that relies on
/// Maximum number of dependencies allowed per disbursement to prevent DOS attacks
const MAX_DEPENDENCIES: usize = 100;

/// memo-based dependency hints can stay in sync. This is the canonical implementation
/// used by the executor, CLI, and explorer layers.
///
/// # Security
/// - Limits dependency count to MAX_DEPENDENCIES (100) to prevent DOS attacks
/// - Deduplicates dependency IDs to prevent repeated processing
/// - Validates memo size before parsing
pub fn parse_dependency_list(memo: &str) -> Vec<u64> {
    let trimmed = memo.trim();

    // Security: Reject excessively large memos (potential DOS)
    if trimmed.is_empty() || trimmed.len() > 8192 {
        return Vec::new();
    }

    let mut deps = Vec::new();

    // Try JSON format first
    if let Ok(json::Value::Object(map)) = json::from_str::<json::Value>(trimmed) {
        if let Some(json::Value::Array(items)) = map.get("depends_on") {
            for item in items.iter().take(MAX_DEPENDENCIES) {
                if let Some(id) = match item {
                    json::Value::Number(num) => num.as_u64(),
                    json::Value::String(text) => text.trim().parse::<u64>().ok(),
                    _ => None,
                } {
                    deps.push(id);
                }
            }
        }
    } else if let Some(rest) = trimmed
        .strip_prefix("depends_on=")
        .or_else(|| trimmed.strip_prefix("depends_on:"))
    {
        // Try key-value format
        for entry in rest.split(',').take(MAX_DEPENDENCIES) {
            if let Ok(id) = entry.trim().parse::<u64>() {
                deps.push(id);
            }
        }
    }

    // Security: Deduplicate to prevent repeated processing
    deps.sort_unstable();
    deps.dedup();
    deps
}

/// Check if a disbursement is eligible for execution at the given epoch
pub fn is_eligible_for_execution(
    disbursement: &TreasuryDisbursement,
    current_epoch: u64,
    existing_disbursements: &[TreasuryDisbursement],
) -> Result<(), DisbursementError> {
    // Check state
    match &disbursement.status {
        DisbursementStatus::Timelocked { ready_epoch } => {
            if current_epoch < *ready_epoch {
                return Err(DisbursementError::StaleProposal {
                    proposal_id: disbursement.id,
                    current_epoch,
                    scheduled_epoch: *ready_epoch,
                });
            }
        }
        _ => {
            return Err(DisbursementError::ValidationFailed {
                reason: format!("Disbursement {} not in Timelocked state", disbursement.id),
            });
        }
    }

    // Check dependencies if proposal metadata exists
    if let Some(proposal) = &disbursement.proposal {
        for dep_id in &proposal.deps {
            let dep = existing_disbursements
                .iter()
                .find(|d| d.id == *dep_id)
                .ok_or_else(|| DisbursementError::DependencyFailed {
                    dependency_id: *dep_id,
                    reason: format!("Dependency {} not found", dep_id),
                })?;

            match &dep.status {
                DisbursementStatus::Finalized { .. } => {}
                DisbursementStatus::RolledBack { reason, .. } => {
                    return Err(DisbursementError::DependencyFailed {
                        dependency_id: *dep_id,
                        reason: format!("Dependency {} rolled back: {}", dep_id, reason),
                    });
                }
                _ => {
                    return Err(DisbursementError::DependencyFailed {
                        dependency_id: *dep_id,
                        reason: format!("Dependency {} not finalized", dep_id),
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_payload() -> DisbursementPayload {
        DisbursementPayload {
            proposal: DisbursementProposalMetadata {
                title: "Test Disbursement".into(),
                summary: "A test disbursement for unit tests".into(),
                deps: vec![],
                attachments: vec![],
                quorum: QuorumSpec {
                    operators_ppm: 670000,
                    builders_ppm: 670000,
                },
                vote_window_epochs: 4,
                timelock_epochs: 2,
                rollback_window_epochs: 1,
            },
            disbursement: DisbursementDetails {
                destination: "tb1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqe4tqx9".into(),
                amount: 100_000,
                memo: "Test payment".into(),
                scheduled_epoch: 1000,
                expected_receipts: vec![],
            },
        }
    }

    #[test]
    fn valid_payload_passes_validation() {
        let payload = valid_payload();
        assert!(validate_disbursement_payload(&payload).is_ok());
    }

    #[test]
    fn empty_title_fails_validation() {
        let mut payload = valid_payload();
        payload.proposal.title = "".into();
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(err, DisbursementValidationError::EmptyTitle));
    }

    #[test]
    fn empty_summary_fails_validation() {
        let mut payload = valid_payload();
        payload.proposal.summary = "".into();
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(err, DisbursementValidationError::EmptySummary));
    }

    #[test]
    fn invalid_quorum_fails_validation() {
        let mut payload = valid_payload();
        payload.proposal.quorum.operators_ppm = 1_000_001;
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(err, DisbursementValidationError::InvalidQuorum));
    }

    #[test]
    fn zero_vote_window_fails_validation() {
        let mut payload = valid_payload();
        payload.proposal.vote_window_epochs = 0;
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(
            err,
            DisbursementValidationError::InvalidVoteWindow
        ));
    }

    #[test]
    fn invalid_destination_fails_validation() {
        let mut payload = valid_payload();
        payload.disbursement.destination = "invalid-address".into();
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(
            err,
            DisbursementValidationError::InvalidDestination(_)
        ));
    }

    #[test]
    fn zero_amount_fails_validation() {
        let mut payload = valid_payload();
        payload.disbursement.amount = 0;
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(err, DisbursementValidationError::ZeroAmount));
    }

    #[test]
    fn expected_receipts_mismatch_fails_validation() {
        let mut payload = valid_payload();
        payload.disbursement.amount = 100_000;
        payload.disbursement.expected_receipts = vec![
            ExpectedReceipt {
                account: "acc1".into(),
                amount: 50_000,
            },
            ExpectedReceipt {
                account: "acc2".into(),
                amount: 40_000, // Total is 90_000, not 100_000
            },
        ];
        let err = validate_disbursement_payload(&payload).unwrap_err();
        assert!(matches!(
            err,
            DisbursementValidationError::ExpectedReceiptsMismatch { .. }
        ));
    }

    #[test]
    fn expected_receipts_matching_passes_validation() {
        let mut payload = valid_payload();
        payload.disbursement.amount = 100_000;
        payload.disbursement.expected_receipts = vec![
            ExpectedReceipt {
                account: "acc1".into(),
                amount: 60_000,
            },
            ExpectedReceipt {
                account: "acc2".into(),
                amount: 40_000,
            },
        ];
        assert!(validate_disbursement_payload(&payload).is_ok());
    }

    #[test]
    fn parse_dependency_list_json() {
        let memo = r#"{"depends_on": [1234, 5678]}"#;
        assert_eq!(parse_dependency_list(memo), vec![1234, 5678]);
    }

    #[test]
    fn parse_dependency_list_key_value() {
        let memo = "depends_on=111,222";
        assert_eq!(parse_dependency_list(memo), vec![111, 222]);
    }

    #[test]
    fn parse_dependency_list_empty() {
        assert!(parse_dependency_list("").is_empty());
        assert!(parse_dependency_list("no deps").is_empty());
    }

    #[test]
    fn parse_dependency_list_deduplicates() {
        // Security: Ensure duplicate IDs are removed
        let memo = r#"{"depends_on": [123, 456, 123, 789, 456]}"#;
        let result = parse_dependency_list(memo);
        assert_eq!(result, vec![123, 456, 789]);
    }

    #[test]
    fn parse_dependency_list_limits_count() {
        // Security: Ensure DOS protection by limiting count
        let mut deps = Vec::new();
        for i in 0..150 {
            deps.push(format!("{}", i));
        }
        let memo = format!("depends_on={}", deps.join(","));
        let result = parse_dependency_list(&memo);
        // Should be limited to MAX_DEPENDENCIES (100), then deduplicated
        assert!(result.len() <= 100);
    }

    #[test]
    fn parse_dependency_list_rejects_huge_memo() {
        // Security: Reject excessively large memos (> 8KB)
        let huge_memo = "depends_on=".to_string() + &"1,".repeat(10000);
        let result = parse_dependency_list(&huge_memo);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_dependency_list_handles_malformed_json() {
        // Security: Ensure malformed JSON doesn't panic
        let memo = r#"{"depends_on": [1, 2, "invalid", null, 3]}"#;
        let result = parse_dependency_list(memo);
        // Should parse valid numbers only
        assert_eq!(result, vec![1, 2, 3]);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SignedExecutionIntent {
    pub disbursement_id: u64,
    pub tx_bytes: Vec<u8>,
    pub tx_hash: String,
    pub staged_at: u64,
    #[serde(default)]
    pub nonce: u64,
}

impl SignedExecutionIntent {
    pub fn new(disbursement_id: u64, tx_bytes: Vec<u8>, tx_hash: String, nonce: u64) -> Self {
        Self {
            disbursement_id,
            tx_bytes,
            tx_hash,
            staged_at: now_ts(),
            nonce,
        }
    }
}

impl BinaryCodec for SignedExecutionIntent {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.disbursement_id.encode(writer);
        write_bytes(writer, &self.tx_bytes);
        self.tx_hash.encode(writer);
        self.staged_at.encode(writer);
        self.nonce.encode(writer);
    }

    fn decode(reader: &mut crate::codec::BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            disbursement_id: u64::decode(reader)?,
            tx_bytes: read_bytes(reader)?,
            tx_hash: String::decode(reader)?,
            staged_at: u64::decode(reader)?,
            nonce: u64::decode(reader).unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryExecutorSnapshot {
    #[serde(default)]
    pub last_tick_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub pending_matured: u64,
    #[serde(default)]
    pub staged_intents: u64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub lease_holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_renewed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_submitted_nonce: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_last_nonce: Option<u64>,
    #[serde(default)]
    pub lease_released: bool,
}

impl TreasuryExecutorSnapshot {
    pub fn record_tick(&mut self, pending: u64, staged: u64) {
        self.last_tick_at = now_ts();
        self.pending_matured = pending;
        self.staged_intents = staged;
    }

    pub fn record_success(&mut self, pending: u64, staged: u64) {
        self.record_tick(pending, staged);
        self.last_success_at = Some(self.last_tick_at);
        self.last_error = None;
        self.last_error_at = None;
    }

    pub fn record_error(&mut self, message: String, pending: u64, staged: u64) {
        self.record_tick(pending, staged);
        self.last_error = Some(message);
        self.last_error_at = Some(self.last_tick_at);
    }

    pub fn record_lease(
        &mut self,
        holder: Option<String>,
        expires_at: Option<u64>,
        renewed_at: Option<u64>,
        last_nonce: Option<u64>,
        released: bool,
    ) {
        self.lease_holder = holder;
        self.lease_expires_at = expires_at;
        self.lease_renewed_at = renewed_at;
        self.lease_last_nonce = last_nonce;
        self.lease_released = released;
    }

    pub fn record_nonce(&mut self, nonce: u64) {
        self.last_submitted_nonce = Some(nonce);
    }
}

impl BinaryCodec for TreasuryExecutorSnapshot {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.last_tick_at.encode(writer);
        self.last_success_at.encode(writer);
        self.last_error_at.encode(writer);
        self.last_error.encode(writer);
        self.pending_matured.encode(writer);
        self.staged_intents.encode(writer);
        self.lease_holder.encode(writer);
        self.lease_expires_at.encode(writer);
        self.lease_renewed_at.encode(writer);
        self.last_submitted_nonce.encode(writer);
        self.lease_last_nonce.encode(writer);
        self.lease_released.encode(writer);
    }

    fn decode(reader: &mut crate::codec::BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            last_tick_at: u64::decode(reader)?,
            last_success_at: Option::<u64>::decode(reader)?,
            last_error_at: Option::<u64>::decode(reader)?,
            last_error: Option::<String>::decode(reader)?,
            pending_matured: u64::decode(reader)?,
            staged_intents: u64::decode(reader)?,
            lease_holder: Option::<String>::decode(reader)?,
            lease_expires_at: Option::<u64>::decode(reader)?,
            lease_renewed_at: Option::<u64>::decode(reader)?,
            last_submitted_nonce: Option::<u64>::decode(reader)?,
            lease_last_nonce: Option::<u64>::decode(reader)?,
            lease_released: bool::decode(reader).unwrap_or(false),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum TreasuryBalanceEventKind {
    Accrual,
    Queued,
    Executed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceSnapshot {
    pub id: u64,
    pub balance: u64,
    pub delta: i64,
    pub recorded_at: u64,
    pub event: TreasuryBalanceEventKind,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub disbursement_id: Option<u64>,
}

impl TreasuryBalanceSnapshot {
    pub fn new(
        id: u64,
        balance: u64,
        delta: i64,
        event: TreasuryBalanceEventKind,
        disbursement_id: Option<u64>,
    ) -> Self {
        Self {
            id,
            balance,
            delta,
            recorded_at: now_ts(),
            event,
            disbursement_id,
        }
    }
}
