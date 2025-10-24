pub mod bicameral;
mod codec;
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

use concurrency::Lazy;

pub use bicameral::{
    Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use governance_spec::treasury;
pub use governance_spec::treasury::{
    mark_cancelled, mark_executed, DisbursementStatus, TreasuryBalanceEventKind,
    TreasuryBalanceSnapshot, TreasuryDisbursement,
};
pub use governance_spec::{
    approved_reward_claims, ensure_reward_claim_authorized, RewardClaimApproval,
};
pub use params::{registry, retune_multipliers, ParamSpec, Params, Runtime, Utilization};
pub use proposals::{validate_dag, Proposal, ProposalStatus, Vote, VoteChoice};
pub use release::{
    approved_releases, ensure_release_authorized, ApprovedRelease, ReleaseAttestation,
    ReleaseBallot, ReleaseVote,
};
pub use state::TreasuryState;
pub use store::{
    DependencyPolicyRecord, DidRevocationRecord, GovStore, LastActivation, ACTIVATION_DELAY,
    QUORUM, ROLLBACK_WINDOW_EPOCHS,
};
pub use token::{TokenAction, TokenProposal};

/// Simplified address type reused across governance records.
pub type Address = String;

pub static NODE_GOV_STORE: Lazy<GovStore> = Lazy::new(|| {
    let path = std::env::var("TB_GOVERNANCE_DB_PATH").unwrap_or_else(|_| "governance_db".into());
    GovStore::open(path)
});

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../../examples/governance.rs"), "\n```")]
mod governance_example {}
use foundation_serialization::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(crate = "foundation_serialization::serde")]
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
    ReadSubsidyViewerPercent,
    ReadSubsidyHostPercent,
    ReadSubsidyHardwarePercent,
    ReadSubsidyVerifierPercent,
    ReadSubsidyLiquidityPercent,
    TreasuryPercentCt,
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
    RuntimeBackend,
    TransportProvider,
    StorageEnginePolicy,
    BridgeMinBond,
    BridgeDutyReward,
    BridgeFailureSlash,
    BridgeChallengeSlash,
    BridgeDutyWindowSecs,
}
