pub mod bicameral;
pub mod controller;
pub mod inflation_cap;
mod kalman;
mod params;
mod proposals;
mod state;
mod store;
mod variance;

pub use bicameral::{
    Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use params::{registry, retune_multipliers, ParamSpec, Params, Runtime, Utilization};
pub use proposals::{validate_dag, Proposal, ProposalStatus, Vote, VoteChoice};
pub use state::TreasuryState;
pub use store::{GovStore, LastActivation, ACTIVATION_DELAY, QUORUM, ROLLBACK_WINDOW_EPOCHS};

/// Simplified address type reused across governance records.
pub type Address = String;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ParamKey {
    SnapshotIntervalSecs,
    ConsumerFeeComfortP90Microunits,
    IndustrialAdmissionMinCapacity,
    FairshareGlobalMax,
    BurstRefillRatePerS,
    BetaStorageSubCt,
    GammaReadSubCt,
    KappaCpuSubCt,
    LambdaBytesOutSubCt,
    RentRateCtPerByte,
    KillSwitchSubsidyReduction,
    MinerRewardLogisticTarget,
    LogisticSlope,
    MinerHysteresis,
    HeuristicMuMilli,
}
