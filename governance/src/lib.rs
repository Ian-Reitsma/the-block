#![allow(
    clippy::new_without_default,
    clippy::implicit_saturating_sub,
    clippy::should_implement_trait,
    clippy::redundant_closure,
    clippy::manual_range_contains,
    clippy::excessive_precision,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_cast,
    clippy::unwrap_or_default,
    clippy::for_kv_map
)]

pub mod access_control;
pub mod authorization;
pub mod bicameral;
pub mod circuit_breaker;
#[cfg(test)]
mod circuit_breaker_integration_test;
pub mod codec;
pub mod controller;
pub mod disbursement_auth;
pub mod energy_params;
pub mod kalman;
pub mod params;
pub mod proposals;
pub mod release;
pub mod reward;
pub mod state;
pub mod store;
pub mod store_auth_helpers;
pub mod treasury;
pub mod treasury_deps;
pub mod variance;

pub use access_control::{
    AuthContext, AuthError, AuthNonceTracker, AuthorizedCall,
    OperatorRegistry as AccessOperatorRegistry, Role as AccessRole,
};
pub use bicameral::{
    Bicameral, Governance as BicameralGovernance, House, Proposal as BicameralProposal,
};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use energy_params::{
    EnergySettlementChangePayload, EnergySettlementMode, EnergySettlementPayload,
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
    DependencyPolicyRecord, DidRevocationRecord, EnergySettlementChangeRecord, EnergySlashRecord,
    EnergyTimelineEntry, EnergyTimelineEvent, EnergyTimelineFilter, GovStore, LastActivation,
    TreasuryBalances, TreasuryExecutorConfig, TreasuryExecutorError, TreasuryExecutorHandle,
    ACTIVATION_DELAY, QUORUM, ROLLBACK_WINDOW_EPOCHS,
};
pub use treasury::{
    parse_dependency_list, validate_disbursement_payload, DisbursementDetails, DisbursementPayload,
    DisbursementProposalMetadata, DisbursementStatus, DisbursementValidationError,
    SignedExecutionIntent, TreasuryBalanceEventKind, TreasuryBalanceSnapshot, TreasuryDisbursement,
    TreasuryExecutorSnapshot,
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
    EnergySettlementMode,
    EnergySettlementQuorumPpm,
    EnergySettlementExpiryBlocks,

    // ===== Economic Control Laws =====

    // Layer 1: Inflation Controller
    InflationTargetBps,
    InflationControllerGain,
    MinAnnualIssuanceCt,
    MaxAnnualIssuanceCt,

    // Layer 2: Subsidy Allocator
    StorageUtilTargetBps,
    StorageMarginTargetBps,
    ComputeUtilTargetBps,
    ComputeMarginTargetBps,
    EnergyUtilTargetBps,
    EnergyMarginTargetBps,
    AdUtilTargetBps,
    AdMarginTargetBps,
    SubsidyAllocatorAlpha,
    SubsidyAllocatorBeta,
    SubsidyAllocatorTemperature,
    SubsidyAllocatorDriftRate,

    // Layer 3: Market Multipliers - Storage
    StorageUtilResponsiveness,
    StorageCostResponsiveness,
    StorageMultiplierFloor,
    StorageMultiplierCeiling,

    // Layer 3: Market Multipliers - Compute
    ComputeUtilResponsiveness,
    ComputeCostResponsiveness,
    ComputeMultiplierFloor,
    ComputeMultiplierCeiling,

    // Layer 3: Market Multipliers - Energy
    EnergyUtilResponsiveness,
    EnergyCostResponsiveness,
    EnergyMultiplierFloor,
    EnergyMultiplierCeiling,

    // Layer 3: Market Multipliers - Ad
    AdUtilResponsiveness,
    AdCostResponsiveness,
    AdMultiplierFloor,
    AdMultiplierCeiling,

    // Layer 4: Ad Market Drift
    AdPlatformTakeTargetBps,
    AdUserShareTargetBps,
    AdDriftRate,

    // Layer 4: Tariff Controller
    TariffPublicRevenueTargetBps,
    TariffDriftRate,
    TariffMinBps,
    TariffMaxBps,
}
