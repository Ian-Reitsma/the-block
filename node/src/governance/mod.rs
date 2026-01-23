pub mod bicameral;
mod codec;
pub use codec::{decode_binary, encode_binary};
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
    mark_cancelled, mark_executed, DisbursementStatus, SignedExecutionIntent,
    TreasuryBalanceEventKind, TreasuryBalanceSnapshot, TreasuryDisbursement,
    TreasuryExecutorSnapshot,
};
pub use governance_spec::{
    approved_reward_claims, ensure_reward_claim_authorized, RewardClaimApproval,
};
pub use governance_spec::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
};
pub use governance_spec::{
    EnergySettlementChangePayload, EnergySettlementMode, EnergySettlementPayload,
};
pub use governance_spec::{EnergyTimelineEntry, EnergyTimelineEvent, EnergyTimelineFilter};
// Circuit breaker pattern for executor resilience
pub use governance_spec::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use params::{registry, retune_multipliers, ParamSpec, Params, Runtime, Utilization};
pub use proposals::{validate_dag, Proposal, ProposalStatus, Vote, VoteChoice};
pub use release::{
    approved_releases, ensure_release_authorized, ApprovedRelease, ReleaseAttestation,
    ReleaseBallot, ReleaseVote,
};
pub use state::TreasuryState;
pub use store::{
    DependencyPolicyRecord, DidRevocationRecord, EnergySettlementChangeRecord, EnergySlashRecord,
    GovStore, LastActivation, TreasuryExecutorConfig, TreasuryExecutorError,
    TreasuryExecutorHandle, ACTIVATION_DELAY, QUORUM, ROLLBACK_WINDOW_EPOCHS,
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
    BetaStorageSub,
    GammaReadSub,
    KappaCpuSub,
    LambdaBytesOutSub,
    ReadSubsidyViewerPercent,
    ReadSubsidyHostPercent,
    ReadSubsidyHardwarePercent,
    ReadSubsidyVerifierPercent,
    ReadSubsidyLiquidityPercent,
    LaneBasedSettlementEnabled,
    AdReadinessWindowSecs,
    AdReadinessMinUniqueViewers,
    AdReadinessMinHostCount,
    AdReadinessMinProviderCount,
    TreasuryPercent,
    ProofRebateLimit,
    RentRatePerByte,
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
    // Dynamic ad-readiness threshold controls
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
    AdRehearsalContextualEnabled,
    AdRehearsalContextualStabilityWindows,
    AdRehearsalPresenceEnabled,
    AdRehearsalPresenceStabilityWindows,
    EnergyMinStake,
    EnergyOracleTimeoutBlocks,
    EnergySlashingRateBps,
    EnergySettlementMode,
    EnergySettlementQuorumPpm,
    EnergySettlementExpiryBlocks,
    // Lane-based dynamic pricing parameters
    /// Consumer lane maximum transactions per block (service capacity μ)
    LaneConsumerCapacity,
    /// Industrial lane maximum transactions per block (service capacity μ)
    LaneIndustrialCapacity,
    /// Consumer lane congestion sensitivity parameter k
    LaneConsumerCongestionSensitivity,
    /// Industrial lane congestion sensitivity parameter k
    LaneIndustrialCongestionSensitivity,
    /// Minimum industrial/consumer fee ratio (e.g., 50 = 50% premium)
    LaneIndustrialMinPremiumPercent,
    /// Target lane utilization for PI control (e.g., 70 = 70%)
    LaneTargetUtilizationPercent,
    /// Market signal EMA half-life in blocks
    LaneMarketSignalHalfLife,
    /// Market demand multiplier maximum (e.g., 300 = 3x multiplier)
    LaneMarketDemandMaxMultiplierPercent,
    /// Market demand sensitivity (exponential curvature, e.g., 200 = 2.0)
    LaneMarketDemandSensitivityPercent,
    /// PI controller proportional gain Kp (e.g., 10 = 0.1)
    LanePIProportionalGainPercent,
    /// PI controller integral gain Ki (e.g., 1 = 0.01)
    LanePIIntegralGainPercent,
}
