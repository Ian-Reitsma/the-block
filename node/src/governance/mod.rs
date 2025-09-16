pub mod bicameral;
pub mod controller;
pub mod inflation_cap;
mod kalman;
mod params;
mod proposals;
pub mod release;
mod state;
mod store;
mod token;
mod variance;

pub use bicameral::{
    Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use params::{registry, retune_multipliers, ParamSpec, Params, Runtime, Utilization};
pub use proposals::{validate_dag, Proposal, ProposalStatus, Vote, VoteChoice};
pub use release::{
    approved_releases, ensure_release_authorized, ApprovedRelease, ReleaseAttestation,
    ReleaseBallot, ReleaseVote,
};
pub use state::TreasuryState;
pub use store::{
    DidRevocationRecord, GovStore, LastActivation, ACTIVATION_DELAY, QUORUM, ROLLBACK_WINDOW_EPOCHS,
};
pub use token::{TokenAction, TokenProposal};

/// Simplified address type reused across governance records.
pub type Address = String;

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../../examples/governance.rs"), "\n```")]
mod governance_example {}
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
    FeeFloorWindow,
    FeeFloorPercentile,
    BadgeExpirySecs,
    BadgeIssueUptime,
    BadgeRevokeUptime,
    JurisdictionRegion,
    AiDiagnosticsEnabled,
    KalmanRShort,
    KalmanRMed,
    KalmanRLong,
    SchedulerWeightGossip,
    SchedulerWeightCompute,
    SchedulerWeightStorage,
}
