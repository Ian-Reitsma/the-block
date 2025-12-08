#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

/// Governance-controlled incentive parameters shared across the node, CLI, and
/// bridge runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BridgeIncentiveParameters {
    /// Minimum bonded collateral required for a relayer to participate.
    pub min_bond: u64,
    /// Reward credited to a relayer when a duty is completed without dispute.
    pub duty_reward: u64,
    /// Baseline slash applied when a relayer fails a duty (invalid proof,
    /// signature mismatch, etc.).
    pub failure_slash: u64,
    /// Additional slash applied to every signer when a withdrawal bundle is
    /// successfully challenged.
    pub challenge_slash: u64,
    /// Soft deadline for satisfying duties before governance may intervene or
    /// mark the attempt as expired.
    pub duty_window_secs: u64,
}

impl BridgeIncentiveParameters {
    pub const DEFAULT_MIN_BOND: u64 = 50;
    pub const DEFAULT_DUTY_REWARD: u64 = 5;
    pub const DEFAULT_FAILURE_SLASH: u64 = 10;
    pub const DEFAULT_CHALLENGE_SLASH: u64 = 25;
    pub const DEFAULT_DUTY_WINDOW_SECS: u64 = 300;

    pub const fn defaults() -> Self {
        Self {
            min_bond: Self::DEFAULT_MIN_BOND,
            duty_reward: Self::DEFAULT_DUTY_REWARD,
            failure_slash: Self::DEFAULT_FAILURE_SLASH,
            challenge_slash: Self::DEFAULT_CHALLENGE_SLASH,
            duty_window_secs: Self::DEFAULT_DUTY_WINDOW_SECS,
        }
    }
}

impl Default for BridgeIncentiveParameters {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Distinguishes between relayer responsibilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    crate = "foundation_serialization::serde",
    tag = "kind",
    rename_all = "snake_case"
)]
pub enum DutyKind {
    Deposit,
    Withdrawal {
        commitment: [u8; 32],
    },
    Settlement {
        commitment: [u8; 32],
        settlement_chain: String,
        proof_hash: [u8; 32],
    },
}

/// External proof submitted to demonstrate that a withdrawal settled on the
/// destination chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ExternalSettlementProof {
    /// Commitment that ties the settlement to a pending withdrawal.
    pub commitment: [u8; 32],
    /// Destination chain or domain where the withdrawal settled.
    pub settlement_chain: String,
    /// Hash of the settlement artifact (transaction, proof bundle, etc.).
    pub proof_hash: [u8; 32],
    /// Height/slot on the destination chain used for ordering and auditing.
    pub settlement_height: u64,
}

/// Detailed outcome for a duty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    crate = "foundation_serialization::serde",
    tag = "status",
    rename_all = "snake_case"
)]
pub enum DutyStatus {
    Pending,
    Completed {
        reward: u64,
        completed_at: u64,
    },
    Failed {
        penalty: u64,
        failed_at: u64,
        reason: DutyFailureReason,
    },
}

/// Reasons that can trigger slashing for a duty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
pub enum DutyFailureReason {
    InvalidProof,
    BundleMismatch,
    ChallengeAccepted,
    Expired,
    InsufficientBond,
}

impl DutyFailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            DutyFailureReason::InvalidProof => "invalid_proof",
            DutyFailureReason::BundleMismatch => "bundle_mismatch",
            DutyFailureReason::ChallengeAccepted => "challenge_accepted",
            DutyFailureReason::Expired => "expired",
            DutyFailureReason::InsufficientBond => "insufficient_bond",
        }
    }
}

/// Recorded duty entry persisted for governance and operator inspection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DutyRecord {
    pub id: u64,
    pub relayer: String,
    pub asset: String,
    pub user: String,
    pub amount: u64,
    pub assigned_at: u64,
    pub deadline: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub bundle_relayers: Vec<String>,
    pub kind: DutyKind,
    pub status: DutyStatus,
}

impl DutyRecord {
    pub fn is_pending(&self) -> bool {
        matches!(self.status, DutyStatus::Pending)
    }

    pub fn commitment(&self) -> Option<[u8; 32]> {
        match self.kind {
            DutyKind::Withdrawal { commitment } | DutyKind::Settlement { commitment, .. } => {
                Some(commitment)
            }
            DutyKind::Deposit => None,
        }
    }

    pub fn completed_reward(&self) -> u64 {
        match self.status {
            DutyStatus::Completed { reward, .. } => reward,
            _ => 0,
        }
    }

    pub fn penalty(&self) -> u64 {
        match self.status {
            DutyStatus::Failed { penalty, .. } => penalty,
            _ => 0,
        }
    }
}

/// Aggregated accounting metrics maintained per relayer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerAccounting {
    pub bond: u64,
    pub rewards_earned: u64,
    pub rewards_pending: u64,
    pub rewards_claimed: u64,
    pub penalties_applied: u64,
    pub duties_assigned: u64,
    pub duties_completed: u64,
    pub duties_failed: u64,
}

impl Default for RelayerAccounting {
    fn default() -> Self {
        Self {
            bond: 0,
            rewards_earned: 0,
            rewards_pending: 0,
            rewards_claimed: 0,
            penalties_applied: 0,
            duties_assigned: 0,
            duties_completed: 0,
            duties_failed: 0,
        }
    }
}

impl RelayerAccounting {
    pub fn credit_bond(&mut self, amount: u64) {
        self.bond = self.bond.saturating_add(amount);
    }

    pub fn debit_bond(&mut self, amount: u64) {
        self.bond = self.bond.saturating_sub(amount);
    }

    pub fn accrue_reward(&mut self, amount: u64) {
        self.rewards_earned = self.rewards_earned.saturating_add(amount);
        self.rewards_pending = self.rewards_pending.saturating_add(amount);
    }

    pub fn apply_penalty(&mut self, amount: u64) {
        self.penalties_applied = self.penalties_applied.saturating_add(amount);
        self.rewards_pending = self
            .rewards_pending
            .saturating_sub(amount.min(self.rewards_pending));
    }

    pub fn mark_claimed(&mut self, amount: u64) {
        let claim = amount.min(self.rewards_pending);
        self.rewards_pending -= claim;
        self.rewards_claimed = self.rewards_claimed.saturating_add(claim);
    }

    pub fn assign_duty(&mut self) {
        self.duties_assigned = self.duties_assigned.saturating_add(1);
    }

    pub fn complete_duty(&mut self) {
        self.duties_completed = self.duties_completed.saturating_add(1);
    }

    pub fn fail_duty(&mut self) {
        self.duties_failed = self.duties_failed.saturating_add(1);
    }
}

/// Deterministically derives the settlement proof digest that relayers must
/// submit when finalising a withdrawal on an external chain. The digest ties
/// together the withdrawal commitment, settlement metadata, and bundle roster so
/// governance can reproduce the attestation without relying on opaque hashes.
pub fn settlement_proof_digest(
    asset: &str,
    commitment: &[u8; 32],
    settlement_chain: &str,
    settlement_height: u64,
    user: &str,
    amount: u64,
    relayers: &[String],
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(asset.as_bytes());
    hasher.update(commitment);
    hasher.update(settlement_chain.as_bytes());
    hasher.update(&settlement_height.to_le_bytes());
    hasher.update(user.as_bytes());
    hasher.update(&amount.to_le_bytes());
    let mut roster: Vec<&str> = relayers.iter().map(|id| id.as_str()).collect();
    roster.sort_unstable();
    roster.dedup();
    for relayer in roster {
        hasher.update(relayer.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_duty_record() {
        let record = DutyRecord {
            id: 7,
            relayer: "relayer-1".into(),
            asset: "native".into(),
            user: "alice".into(),
            amount: 42,
            assigned_at: 12,
            deadline: 90,
            bundle_relayers: vec!["relayer-1".into(), "relayer-2".into()],
            kind: DutyKind::Withdrawal {
                commitment: [1u8; 32],
            },
            status: DutyStatus::Failed {
                penalty: 5,
                failed_at: 88,
                reason: DutyFailureReason::ChallengeAccepted,
            },
        };
        let value = foundation_serialization::json::to_value(&record).expect("serialize duty record");
        let decoded: DutyRecord = foundation_serialization::json::from_value(value).expect("decode duty record");
        assert_eq!(decoded.penalty(), 5);
        assert!(!decoded.is_pending());
    }

    #[test]
    fn settlement_duty_serialization() {
        let record = DutyRecord {
            id: 9,
            relayer: "relayer-9".into(),
            asset: "wrapped-usdc".into(),
            user: "carol".into(),
            amount: 99,
            assigned_at: 33,
            deadline: 333,
            bundle_relayers: vec!["relayer-9".into()],
            kind: DutyKind::Settlement {
                commitment: [9u8; 32],
                settlement_chain: "solana".into(),
                proof_hash: [3u8; 32],
            },
            status: DutyStatus::Completed {
                reward: 12,
                completed_at: 444,
            },
        };
        let value = foundation_serialization::json::to_value(&record).expect("serialize settlement duty");
        let decoded: DutyRecord = foundation_serialization::json::from_value(value).expect("decode settlement duty");
        assert_eq!(decoded.commitment(), Some([9u8; 32]));
        assert_eq!(decoded.completed_reward(), 12);
    }

    #[test]
    fn settlement_proof_digest_orders_relayers() {
        let commitment = [42u8; 32];
        let mut relayers = vec!["r2".to_string(), "r1".to_string(), "r2".to_string()];
        let digest_a =
            settlement_proof_digest("native", &commitment, "solana", 55, "alice", 40, &relayers);
        relayers.reverse();
        let digest_b =
            settlement_proof_digest("native", &commitment, "solana", 55, "alice", 40, &relayers);
        assert_eq!(digest_a, digest_b);
    }
}
