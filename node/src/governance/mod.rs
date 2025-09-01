pub mod bicameral;
mod params;
mod store;

pub use bicameral::{
    Bicameral, CreditIssue, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use params::{registry, ParamSpec, Params, Runtime};
pub use store::{GovStore, LastActivation, ACTIVATION_DELAY, QUORUM, ROLLBACK_WINDOW_EPOCHS};

use serde::{Deserialize, Serialize};

/// Simplified address type reused across governance records.
pub type Address = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ParamKey {
    SnapshotIntervalSecs,
    ConsumerFeeComfortP90Microunits,
    IndustrialAdmissionMinCapacity,
    FairshareGlobalMax,
    BurstRefillRatePerS,
    CreditsDecayLambdaPerHourPpm,
    DailyPayoutCap,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    Open,
    Passed,
    Rejected,
    Activated,
    RolledBack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteChoice {
    Yes,
    No,
    Abstain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub key: ParamKey,
    pub new_value: i64,
    pub min: i64,
    pub max: i64,
    pub proposer: Address,
    pub created_epoch: u64,
    pub vote_deadline_epoch: u64,
    pub activation_epoch: Option<u64>,
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub proposal_id: u64,
    pub voter: Address,
    pub choice: VoteChoice,
    pub weight: u64,
    pub received_at: u64,
}
