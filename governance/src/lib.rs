pub mod bicameral;
pub mod codec;
pub mod controller;
pub mod kalman;
pub mod params;
pub mod proposals;
pub mod release;
pub mod reward;
pub mod state;
pub mod store;
pub mod treasury;
pub mod variance;

pub use bicameral::{
    Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use params::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
    encode_runtime_backend_policy, encode_storage_engine_policy, encode_transport_provider_policy,
    registry, retune_multipliers, validate_runtime_backend_policy, validate_storage_engine_policy,
    validate_transport_provider_policy, EncryptedUtilization, ParamSpec, Params, Runtime,
    RuntimeAdapter, Utilization, DEFAULT_RUNTIME_BACKEND_POLICY, DEFAULT_STORAGE_ENGINE_POLICY,
    DEFAULT_TRANSPORT_PROVIDER_POLICY, RUNTIME_BACKEND_OPTIONS, STORAGE_ENGINE_OPTIONS,
    TRANSPORT_PROVIDER_OPTIONS,
};
pub use proposals::{validate_dag, Proposal, ProposalStatus, Vote, VoteChoice};
pub use release::{
    approved_releases, ensure_release_authorized, ApprovedRelease, ReleaseAttestation,
    ReleaseBallot, ReleaseVerifier, ReleaseVote,
};
pub use reward::{approved_reward_claims, ensure_reward_claim_authorized, RewardClaimApproval};
pub use state::TreasuryState;
pub use store::{
    DependencyPolicyRecord, DidRevocationRecord, GovStore, LastActivation, TreasuryBalances,
    TreasuryExecutorConfig, TreasuryExecutorError, TreasuryExecutorHandle, ACTIVATION_DELAY,
    QUORUM, ROLLBACK_WINDOW_EPOCHS,
};
pub use treasury::{
    validate_disbursement_payload, DisbursementDetails, DisbursementPayload,
    DisbursementProposalMetadata, DisbursementStatus, DisbursementValidationError,
    SignedExecutionIntent, TreasuryBalanceEventKind, TreasuryBalanceSnapshot,
    TreasuryDisbursement, TreasuryExecutorSnapshot,
};

/// Simplified address type reused across governance records.
pub type Address = String;

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../examples/usage.rs"), "\n```")]
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
    DualTokenSettlementEnabled,
    AdReadinessWindowSecs,
    AdReadinessMinUniqueViewers,
    AdReadinessMinHostCount,
    AdReadinessMinProviderCount,
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
    AdUsePercentileThresholds,
    AdViewerPercentile,
    AdHostPercentile,
    AdProviderPercentile,
    AdEmaSmoothingPpm,
    AdFloorUniqueViewers,
    AdFloorHostCount,
    AdFloorProviderCount,
    AdCapUniqueViewers,
    AdCapHostCount,
    AdCapProviderCount,
    AdPercentileBuckets,
    AdRehearsalEnabled,
    AdRehearsalStabilityWindows,
    EnergyMinStake,
    EnergyOracleTimeoutBlocks,
    EnergySlashingRateBps,
}
