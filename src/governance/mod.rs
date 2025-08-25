pub mod bicameral;
mod store;
mod params;

pub use bicameral::{Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal};
pub use store::{GovStore, LastActivation, ACTIVATION_DELAY, ROLLBACK_WINDOW_EPOCHS, QUORUM};
pub use params::{ParamSpec, Params, registry};

use serde::{Serialize, Deserialize};

/// Simplified address type reused across governance records.
pub type Address = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ParamKey {
    SnapshotIntervalSecs,
    ConsumerFeeComfortP90Microunits,
    IndustrialAdmissionMinCapacity,
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

