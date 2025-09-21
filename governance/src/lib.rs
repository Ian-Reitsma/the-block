pub mod bicameral;
pub mod controller;
pub mod kalman;
pub mod params;
pub mod proposals;
pub mod release;
pub mod state;
pub mod store;
pub mod variance;

pub use bicameral::{
    Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use params::{
    registry, retune_multipliers, EncryptedUtilization, ParamSpec, Params, Runtime, RuntimeAdapter,
    Utilization,
};
pub use proposals::{validate_dag, Proposal, ProposalStatus, Vote, VoteChoice};
pub use release::{
    approved_releases, ensure_release_authorized, ApprovedRelease, ReleaseAttestation,
    ReleaseBallot, ReleaseVerifier, ReleaseVote,
};
pub use state::TreasuryState;
pub use store::{
    DidRevocationRecord, GovStore, LastActivation, ACTIVATION_DELAY, QUORUM, ROLLBACK_WINDOW_EPOCHS,
};

/// Simplified address type reused across governance records.
pub type Address = String;

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../examples/usage.rs"), "\n```")]
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
    ProofRebateLimitCt,
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
