use super::GovStore;
use foundation_serialization::{Deserialize, Serialize};

/// Governance-approved authorization to settle outstanding bridge rewards for a
/// relayer. Authorizations may cover multiple claims until the allowance is
/// exhausted or expired.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardClaimApproval {
    /// Unique key minted by governance to reference this approval.
    pub key: String,
    /// Relayer identifier that is permitted to consume the approval.
    pub relayer: String,
    /// Total allowance granted when the approval was issued.
    pub total_amount: u64,
    /// Remaining allowance that can still be claimed.
    pub remaining_amount: u64,
    /// Optional epoch timestamp after which the approval is no longer valid.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Human readable memo or reference for audit dashboards.
    #[serde(default)]
    pub memo: Option<String>,
    /// Timestamp of the most recent claim that consumed this approval.
    #[serde(default)]
    pub last_claimed_at: Option<u64>,
}

impl RewardClaimApproval {
    pub fn new(key: impl Into<String>, relayer: impl Into<String>, amount: u64) -> Self {
        let key = key.into();
        Self {
            key: key.clone(),
            relayer: relayer.into(),
            total_amount: amount,
            remaining_amount: amount,
            expires_at: None,
            memo: None,
            last_claimed_at: None,
        }
    }

    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at
            .map(|deadline| now > deadline)
            .unwrap_or(false)
    }
}

fn store_path() -> String {
    std::env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into())
}

/// Ensure the provided approval key is valid, applies to the relayer, and has
/// sufficient allowance for the requested amount.
pub fn ensure_reward_claim_authorized(
    key: &str,
    relayer: &str,
    amount: u64,
) -> Result<RewardClaimApproval, String> {
    if amount == 0 {
        return Err("claim amount must be non-zero".into());
    }
    let store = GovStore::open(store_path());
    store
        .consume_reward_claim(key, relayer, amount)
        .map_err(|err| err.to_string())
}

/// Return the currently approved reward claim authorizations for observability.
pub fn approved_reward_claims() -> Vec<RewardClaimApproval> {
    let store = GovStore::open(store_path());
    store.reward_claims_snapshot().unwrap_or_default()
}
