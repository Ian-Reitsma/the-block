// Manual binary and JSON codecs for governance data structures.
use crate::treasury::{
    DisbursementStatus, TreasuryBalanceEventKind, TreasuryBalanceSnapshot, TreasuryDisbursement,
};
use crate::{
    ApprovedRelease, ParamKey, Proposal, ProposalStatus, ReleaseAttestation, ReleaseBallot,
    ReleaseVote, Vote, VoteChoice,
};
use foundation_serialization::json::{self, Map, Value};
use std::convert::TryInto;

pub type Result<T> = std::result::Result<T, sled::Error>;

fn codec_error(msg: impl Into<String>) -> sled::Error {
    sled::Error::Unsupported(msg.into().into_boxed_str())
}

#[derive(Default)]
pub struct BinaryWriter {
    buf: Vec<u8>,
}

impl BinaryWriter {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }

    pub fn write_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    pub fn write_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_i64(&mut self, value: i64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u64(bytes.len() as u64);
        self.buf.extend_from_slice(bytes);
    }

    pub fn write_string(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }
}

pub struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BinaryReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.remaining() < len {
            return Err(codec_error("binary decode: unexpected end of input"));
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.data[start..self.pos])
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    pub fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(codec_error(format!(
                "binary decode: invalid bool tag {other}"
            ))),
        }
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let bytes: [u8; 4] = self
            .read_exact(4)?
            .try_into()
            .expect("slice with exact length");
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let bytes: [u8; 8] = self
            .read_exact(8)?
            .try_into()
            .expect("slice with exact length");
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn read_i64(&mut self) -> Result<i64> {
        let bytes: [u8; 8] = self
            .read_exact(8)?
            .try_into()
            .expect("slice with exact length");
        Ok(i64::from_le_bytes(bytes))
    }

    pub fn read_bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u64()? as usize;
        let bytes = self.read_exact(len)?;
        Ok(bytes.to_vec())
    }

    pub fn read_string(&mut self) -> Result<String> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).map_err(|e| codec_error(format!("utf8: {e}")))
    }

    pub fn finish(self) -> Result<()> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err(codec_error("binary decode: trailing bytes"))
        }
    }
}

pub trait BinaryCodec: Sized {
    fn encode(&self, writer: &mut BinaryWriter);
    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self>;
}

impl BinaryCodec for u8 {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u8(*self);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        reader.read_u8()
    }
}

impl BinaryCodec for u32 {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u32(*self);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        reader.read_u32()
    }
}

impl BinaryCodec for u64 {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u64(*self);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        reader.read_u64()
    }
}

impl BinaryCodec for i64 {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_i64(*self);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        reader.read_i64()
    }
}

impl BinaryCodec for bool {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_bool(*self);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        reader.read_bool()
    }
}

impl BinaryCodec for String {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_string(self);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        reader.read_string()
    }
}

impl<T: BinaryCodec> BinaryCodec for Vec<T> {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u64(self.len() as u64);
        for item in self {
            item.encode(writer);
        }
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        let len = reader.read_u64()? as usize;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(T::decode(reader)?);
        }
        Ok(out)
    }
}

impl<T: BinaryCodec> BinaryCodec for Option<T> {
    fn encode(&self, writer: &mut BinaryWriter) {
        match self {
            Some(value) => {
                writer.write_bool(true);
                value.encode(writer);
            }
            None => writer.write_bool(false),
        }
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        if reader.read_bool()? {
            Ok(Some(T::decode(reader)?))
        } else {
            Ok(None)
        }
    }
}

fn param_key_to_tag(key: ParamKey) -> u8 {
    match key {
        ParamKey::SnapshotIntervalSecs => 0,
        ParamKey::ConsumerFeeComfortP90Microunits => 1,
        ParamKey::IndustrialAdmissionMinCapacity => 2,
        ParamKey::FairshareGlobalMax => 3,
        ParamKey::BurstRefillRatePerS => 4,
        ParamKey::BetaStorageSubCt => 5,
        ParamKey::GammaReadSubCt => 6,
        ParamKey::KappaCpuSubCt => 7,
        ParamKey::LambdaBytesOutSubCt => 8,
        ParamKey::ReadSubsidyViewerPercent => 9,
        ParamKey::ReadSubsidyHostPercent => 10,
        ParamKey::ReadSubsidyHardwarePercent => 11,
        ParamKey::ReadSubsidyVerifierPercent => 12,
        ParamKey::ReadSubsidyLiquidityPercent => 13,
        ParamKey::TreasuryPercentCt => 14,
        ParamKey::ProofRebateLimitCt => 15,
        ParamKey::RentRateCtPerByte => 16,
        ParamKey::KillSwitchSubsidyReduction => 17,
        ParamKey::MinerRewardLogisticTarget => 18,
        ParamKey::LogisticSlope => 19,
        ParamKey::MinerHysteresis => 20,
        ParamKey::HeuristicMuMilli => 21,
        ParamKey::FeeFloorWindow => 22,
        ParamKey::FeeFloorPercentile => 23,
        ParamKey::BadgeExpirySecs => 24,
        ParamKey::BadgeIssueUptime => 25,
        ParamKey::BadgeRevokeUptime => 26,
        ParamKey::JurisdictionRegion => 27,
        ParamKey::AiDiagnosticsEnabled => 28,
        ParamKey::KalmanRShort => 29,
        ParamKey::KalmanRMed => 30,
        ParamKey::KalmanRLong => 31,
        ParamKey::SchedulerWeightGossip => 32,
        ParamKey::SchedulerWeightCompute => 33,
        ParamKey::SchedulerWeightStorage => 34,
        ParamKey::RuntimeBackend => 35,
        ParamKey::TransportProvider => 36,
        ParamKey::StorageEnginePolicy => 37,
        ParamKey::BridgeMinBond => 38,
        ParamKey::BridgeDutyReward => 39,
        ParamKey::BridgeFailureSlash => 40,
        ParamKey::BridgeChallengeSlash => 41,
        ParamKey::BridgeDutyWindowSecs => 42,
        ParamKey::DualTokenSettlementEnabled => 43,
        ParamKey::AdReadinessWindowSecs => 44,
        ParamKey::AdReadinessMinUniqueViewers => 45,
        ParamKey::AdReadinessMinHostCount => 46,
        ParamKey::AdReadinessMinProviderCount => 47,
        ParamKey::AdUsePercentileThresholds => 48,
        ParamKey::AdViewerPercentile => 49,
        ParamKey::AdHostPercentile => 50,
        ParamKey::AdProviderPercentile => 51,
        ParamKey::AdEmaSmoothingPpm => 52,
        ParamKey::AdFloorUniqueViewers => 53,
        ParamKey::AdFloorHostCount => 54,
        ParamKey::AdFloorProviderCount => 55,
        ParamKey::AdCapUniqueViewers => 56,
        ParamKey::AdCapHostCount => 57,
        ParamKey::AdCapProviderCount => 58,
        ParamKey::AdPercentileBuckets => 59,
        ParamKey::AdRehearsalEnabled => 60,
        ParamKey::AdRehearsalStabilityWindows => 61,
        ParamKey::EnergyMinStake => 62,
        ParamKey::EnergyOracleTimeoutBlocks => 63,
        ParamKey::EnergySlashingRateBps => 64,

        // Economic Control Laws (Layer 1: Inflation)
        ParamKey::InflationTargetBps => 65,
        ParamKey::InflationControllerGain => 66,
        ParamKey::MinAnnualIssuanceCt => 67,
        ParamKey::MaxAnnualIssuanceCt => 68,

        // Economic Control Laws (Layer 2: Subsidy Allocator)
        ParamKey::StorageUtilTargetBps => 69,
        ParamKey::StorageMarginTargetBps => 70,
        ParamKey::ComputeUtilTargetBps => 71,
        ParamKey::ComputeMarginTargetBps => 72,
        ParamKey::EnergyUtilTargetBps => 73,
        ParamKey::EnergyMarginTargetBps => 74,
        ParamKey::AdUtilTargetBps => 75,
        ParamKey::AdMarginTargetBps => 76,
        ParamKey::SubsidyAllocatorAlpha => 77,
        ParamKey::SubsidyAllocatorBeta => 78,
        ParamKey::SubsidyAllocatorTemperature => 79,
        ParamKey::SubsidyAllocatorDriftRate => 80,

        // Economic Control Laws (Layer 3: Market Multipliers - Storage)
        ParamKey::StorageUtilResponsiveness => 81,
        ParamKey::StorageCostResponsiveness => 82,
        ParamKey::StorageMultiplierFloor => 83,
        ParamKey::StorageMultiplierCeiling => 84,

        // Economic Control Laws (Layer 3: Market Multipliers - Compute)
        ParamKey::ComputeUtilResponsiveness => 85,
        ParamKey::ComputeCostResponsiveness => 86,
        ParamKey::ComputeMultiplierFloor => 87,
        ParamKey::ComputeMultiplierCeiling => 88,

        // Economic Control Laws (Layer 3: Market Multipliers - Energy)
        ParamKey::EnergyUtilResponsiveness => 89,
        ParamKey::EnergyCostResponsiveness => 90,
        ParamKey::EnergyMultiplierFloor => 91,
        ParamKey::EnergyMultiplierCeiling => 92,

        // Economic Control Laws (Layer 3: Market Multipliers - Ad)
        ParamKey::AdUtilResponsiveness => 93,
        ParamKey::AdCostResponsiveness => 94,
        ParamKey::AdMultiplierFloor => 95,
        ParamKey::AdMultiplierCeiling => 96,

        // Economic Control Laws (Layer 4: Ad Market Drift)
        ParamKey::AdPlatformTakeTargetBps => 97,
        ParamKey::AdUserShareTargetBps => 98,
        ParamKey::AdDriftRate => 99,

        // Economic Control Laws (Layer 4: Tariff Controller)
        ParamKey::TariffPublicRevenueTargetBps => 100,
        ParamKey::TariffDriftRate => 101,
        ParamKey::TariffMinBps => 102,
        ParamKey::TariffMaxBps => 103,
    }
}

fn param_key_from_tag(tag: u8) -> Result<ParamKey> {
    let key = match tag {
        0 => ParamKey::SnapshotIntervalSecs,
        1 => ParamKey::ConsumerFeeComfortP90Microunits,
        2 => ParamKey::IndustrialAdmissionMinCapacity,
        3 => ParamKey::FairshareGlobalMax,
        4 => ParamKey::BurstRefillRatePerS,
        5 => ParamKey::BetaStorageSubCt,
        6 => ParamKey::GammaReadSubCt,
        7 => ParamKey::KappaCpuSubCt,
        8 => ParamKey::LambdaBytesOutSubCt,
        9 => ParamKey::ReadSubsidyViewerPercent,
        10 => ParamKey::ReadSubsidyHostPercent,
        11 => ParamKey::ReadSubsidyHardwarePercent,
        12 => ParamKey::ReadSubsidyVerifierPercent,
        13 => ParamKey::ReadSubsidyLiquidityPercent,
        14 => ParamKey::TreasuryPercentCt,
        15 => ParamKey::ProofRebateLimitCt,
        16 => ParamKey::RentRateCtPerByte,
        17 => ParamKey::KillSwitchSubsidyReduction,
        18 => ParamKey::MinerRewardLogisticTarget,
        19 => ParamKey::LogisticSlope,
        20 => ParamKey::MinerHysteresis,
        21 => ParamKey::HeuristicMuMilli,
        22 => ParamKey::FeeFloorWindow,
        23 => ParamKey::FeeFloorPercentile,
        24 => ParamKey::BadgeExpirySecs,
        25 => ParamKey::BadgeIssueUptime,
        26 => ParamKey::BadgeRevokeUptime,
        27 => ParamKey::JurisdictionRegion,
        28 => ParamKey::AiDiagnosticsEnabled,
        29 => ParamKey::KalmanRShort,
        30 => ParamKey::KalmanRMed,
        31 => ParamKey::KalmanRLong,
        32 => ParamKey::SchedulerWeightGossip,
        33 => ParamKey::SchedulerWeightCompute,
        34 => ParamKey::SchedulerWeightStorage,
        35 => ParamKey::RuntimeBackend,
        36 => ParamKey::TransportProvider,
        37 => ParamKey::StorageEnginePolicy,
        38 => ParamKey::BridgeMinBond,
        39 => ParamKey::BridgeDutyReward,
        40 => ParamKey::BridgeFailureSlash,
        41 => ParamKey::BridgeChallengeSlash,
        42 => ParamKey::BridgeDutyWindowSecs,
        43 => ParamKey::DualTokenSettlementEnabled,
        44 => ParamKey::AdReadinessWindowSecs,
        45 => ParamKey::AdReadinessMinUniqueViewers,
        46 => ParamKey::AdReadinessMinHostCount,
        47 => ParamKey::AdReadinessMinProviderCount,
        48 => ParamKey::AdUsePercentileThresholds,
        49 => ParamKey::AdViewerPercentile,
        50 => ParamKey::AdHostPercentile,
        51 => ParamKey::AdProviderPercentile,
        52 => ParamKey::AdEmaSmoothingPpm,
        53 => ParamKey::AdFloorUniqueViewers,
        54 => ParamKey::AdFloorHostCount,
        55 => ParamKey::AdFloorProviderCount,
        56 => ParamKey::AdCapUniqueViewers,
        57 => ParamKey::AdCapHostCount,
        58 => ParamKey::AdCapProviderCount,
        59 => ParamKey::AdPercentileBuckets,
        60 => ParamKey::AdRehearsalEnabled,
        61 => ParamKey::AdRehearsalStabilityWindows,
        62 => ParamKey::EnergyMinStake,
        63 => ParamKey::EnergyOracleTimeoutBlocks,
        64 => ParamKey::EnergySlashingRateBps,

        // Economic Control Laws (Layer 1: Inflation)
        65 => ParamKey::InflationTargetBps,
        66 => ParamKey::InflationControllerGain,
        67 => ParamKey::MinAnnualIssuanceCt,
        68 => ParamKey::MaxAnnualIssuanceCt,

        // Economic Control Laws (Layer 2: Subsidy Allocator)
        69 => ParamKey::StorageUtilTargetBps,
        70 => ParamKey::StorageMarginTargetBps,
        71 => ParamKey::ComputeUtilTargetBps,
        72 => ParamKey::ComputeMarginTargetBps,
        73 => ParamKey::EnergyUtilTargetBps,
        74 => ParamKey::EnergyMarginTargetBps,
        75 => ParamKey::AdUtilTargetBps,
        76 => ParamKey::AdMarginTargetBps,
        77 => ParamKey::SubsidyAllocatorAlpha,
        78 => ParamKey::SubsidyAllocatorBeta,
        79 => ParamKey::SubsidyAllocatorTemperature,
        80 => ParamKey::SubsidyAllocatorDriftRate,

        // Economic Control Laws (Layer 3: Market Multipliers - Storage)
        81 => ParamKey::StorageUtilResponsiveness,
        82 => ParamKey::StorageCostResponsiveness,
        83 => ParamKey::StorageMultiplierFloor,
        84 => ParamKey::StorageMultiplierCeiling,

        // Economic Control Laws (Layer 3: Market Multipliers - Compute)
        85 => ParamKey::ComputeUtilResponsiveness,
        86 => ParamKey::ComputeCostResponsiveness,
        87 => ParamKey::ComputeMultiplierFloor,
        88 => ParamKey::ComputeMultiplierCeiling,

        // Economic Control Laws (Layer 3: Market Multipliers - Energy)
        89 => ParamKey::EnergyUtilResponsiveness,
        90 => ParamKey::EnergyCostResponsiveness,
        91 => ParamKey::EnergyMultiplierFloor,
        92 => ParamKey::EnergyMultiplierCeiling,

        // Economic Control Laws (Layer 3: Market Multipliers - Ad)
        93 => ParamKey::AdUtilResponsiveness,
        94 => ParamKey::AdCostResponsiveness,
        95 => ParamKey::AdMultiplierFloor,
        96 => ParamKey::AdMultiplierCeiling,

        // Economic Control Laws (Layer 4: Ad Market Drift)
        97 => ParamKey::AdPlatformTakeTargetBps,
        98 => ParamKey::AdUserShareTargetBps,
        99 => ParamKey::AdDriftRate,

        // Economic Control Laws (Layer 4: Tariff Controller)
        100 => ParamKey::TariffPublicRevenueTargetBps,
        101 => ParamKey::TariffDriftRate,
        102 => ParamKey::TariffMinBps,
        103 => ParamKey::TariffMaxBps,

        other => {
            return Err(codec_error(format!(
                "binary decode: unknown ParamKey tag {other}"
            )))
        }
    };
    Ok(key)
}

impl BinaryCodec for ParamKey {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u8(param_key_to_tag(*self));
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        param_key_from_tag(reader.read_u8()?)
    }
}

fn proposal_status_to_tag(status: ProposalStatus) -> u8 {
    match status {
        ProposalStatus::Open => 0,
        ProposalStatus::Passed => 1,
        ProposalStatus::Rejected => 2,
        ProposalStatus::Activated => 3,
        ProposalStatus::RolledBack => 4,
    }
}

fn proposal_status_from_tag(tag: u8) -> Result<ProposalStatus> {
    let status = match tag {
        0 => ProposalStatus::Open,
        1 => ProposalStatus::Passed,
        2 => ProposalStatus::Rejected,
        3 => ProposalStatus::Activated,
        4 => ProposalStatus::RolledBack,
        other => {
            return Err(codec_error(format!(
                "binary decode: unknown ProposalStatus tag {other}"
            )))
        }
    };
    Ok(status)
}

impl BinaryCodec for ProposalStatus {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u8(proposal_status_to_tag(*self));
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        proposal_status_from_tag(reader.read_u8()?)
    }
}

fn vote_choice_to_tag(choice: VoteChoice) -> u8 {
    match choice {
        VoteChoice::Yes => 0,
        VoteChoice::No => 1,
        VoteChoice::Abstain => 2,
    }
}

fn vote_choice_from_tag(tag: u8) -> Result<VoteChoice> {
    let choice = match tag {
        0 => VoteChoice::Yes,
        1 => VoteChoice::No,
        2 => VoteChoice::Abstain,
        other => {
            return Err(codec_error(format!(
                "binary decode: unknown VoteChoice tag {other}"
            )))
        }
    };
    Ok(choice)
}

impl BinaryCodec for VoteChoice {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u8(vote_choice_to_tag(*self));
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        vote_choice_from_tag(reader.read_u8()?)
    }
}

fn balance_event_to_tag(event: &TreasuryBalanceEventKind) -> u8 {
    match event {
        TreasuryBalanceEventKind::Accrual => 0,
        TreasuryBalanceEventKind::Queued => 1,
        TreasuryBalanceEventKind::Executed => 2,
        TreasuryBalanceEventKind::Cancelled => 3,
    }
}

fn balance_event_from_tag(tag: u8) -> Result<TreasuryBalanceEventKind> {
    let event = match tag {
        0 => TreasuryBalanceEventKind::Accrual,
        1 => TreasuryBalanceEventKind::Queued,
        2 => TreasuryBalanceEventKind::Executed,
        3 => TreasuryBalanceEventKind::Cancelled,
        other => {
            return Err(codec_error(format!(
                "binary decode: unknown TreasuryBalanceEventKind tag {other}"
            )))
        }
    };
    Ok(event)
}

impl BinaryCodec for TreasuryBalanceEventKind {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u8(balance_event_to_tag(self));
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        balance_event_from_tag(reader.read_u8()?)
    }
}

impl BinaryCodec for Proposal {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.id.encode(writer);
        self.key.encode(writer);
        self.new_value.encode(writer);
        self.min.encode(writer);
        self.max.encode(writer);
        self.proposer.encode(writer);
        self.created_epoch.encode(writer);
        self.vote_deadline_epoch.encode(writer);
        self.activation_epoch.encode(writer);
        self.status.encode(writer);
        self.deps.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            id: u64::decode(reader)?,
            key: ParamKey::decode(reader)?,
            new_value: i64::decode(reader)?,
            min: i64::decode(reader)?,
            max: i64::decode(reader)?,
            proposer: String::decode(reader)?,
            created_epoch: u64::decode(reader)?,
            vote_deadline_epoch: u64::decode(reader)?,
            activation_epoch: Option::<u64>::decode(reader)?,
            status: ProposalStatus::decode(reader)?,
            deps: Vec::<u64>::decode(reader)?,
        })
    }
}

impl BinaryCodec for Vote {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.proposal_id.encode(writer);
        self.voter.encode(writer);
        self.choice.encode(writer);
        self.weight.encode(writer);
        self.received_at.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            proposal_id: u64::decode(reader)?,
            voter: String::decode(reader)?,
            choice: VoteChoice::decode(reader)?,
            weight: u64::decode(reader)?,
            received_at: u64::decode(reader)?,
        })
    }
}

impl BinaryCodec for ReleaseAttestation {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.signer.encode(writer);
        self.signature.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            signer: String::decode(reader)?,
            signature: String::decode(reader)?,
        })
    }
}

impl BinaryCodec for ReleaseVote {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.id.encode(writer);
        self.build_hash.encode(writer);
        self.signatures.encode(writer);
        self.signature_threshold.encode(writer);
        self.signer_set.encode(writer);
        self.proposer.encode(writer);
        self.created_epoch.encode(writer);
        self.vote_deadline_epoch.encode(writer);
        self.activation_epoch.encode(writer);
        self.status.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            id: u64::decode(reader)?,
            build_hash: String::decode(reader)?,
            signatures: Vec::<ReleaseAttestation>::decode(reader)?,
            signature_threshold: u32::decode(reader)?,
            signer_set: Vec::<String>::decode(reader)?,
            proposer: String::decode(reader)?,
            created_epoch: u64::decode(reader)?,
            vote_deadline_epoch: u64::decode(reader)?,
            activation_epoch: Option::<u64>::decode(reader)?,
            status: ProposalStatus::decode(reader)?,
        })
    }
}

impl BinaryCodec for ReleaseBallot {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.proposal_id.encode(writer);
        self.voter.encode(writer);
        self.choice.encode(writer);
        self.weight.encode(writer);
        self.received_at.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            proposal_id: u64::decode(reader)?,
            voter: String::decode(reader)?,
            choice: VoteChoice::decode(reader)?,
            weight: u64::decode(reader)?,
            received_at: u64::decode(reader)?,
        })
    }
}

impl BinaryCodec for ApprovedRelease {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.build_hash.encode(writer);
        self.activated_epoch.encode(writer);
        self.proposer.encode(writer);
        self.signatures.encode(writer);
        self.signature_threshold.encode(writer);
        self.signer_set.encode(writer);
        self.install_times.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            build_hash: String::decode(reader)?,
            activated_epoch: u64::decode(reader)?,
            proposer: String::decode(reader)?,
            signatures: Vec::<ReleaseAttestation>::decode(reader)?,
            signature_threshold: u32::decode(reader)?,
            signer_set: Vec::<String>::decode(reader)?,
            install_times: Vec::<u64>::decode(reader)?,
        })
    }
}

fn disbursement_status_to_tag(status: &DisbursementStatus) -> u8 {
    match status {
        DisbursementStatus::Draft { .. } => 0,
        DisbursementStatus::Voting { .. } => 1,
        DisbursementStatus::Queued { .. } => 2,
        DisbursementStatus::Timelocked { .. } => 3,
        DisbursementStatus::Executed { .. } => 4,
        DisbursementStatus::Finalized { .. } => 5,
        DisbursementStatus::RolledBack { .. } => 6,
    }
}

impl BinaryCodec for DisbursementStatus {
    fn encode(&self, writer: &mut BinaryWriter) {
        writer.write_u8(disbursement_status_to_tag(self));
        match self {
            DisbursementStatus::Draft { created_at } => {
                created_at.encode(writer);
            }
            DisbursementStatus::Voting {
                vote_deadline_epoch,
            } => {
                vote_deadline_epoch.encode(writer);
            }
            DisbursementStatus::Queued {
                queued_at,
                activation_epoch,
            } => {
                queued_at.encode(writer);
                activation_epoch.encode(writer);
            }
            DisbursementStatus::Timelocked { ready_epoch } => {
                ready_epoch.encode(writer);
            }
            DisbursementStatus::Executed {
                tx_hash,
                executed_at,
            } => {
                tx_hash.encode(writer);
                executed_at.encode(writer);
            }
            DisbursementStatus::Finalized {
                tx_hash,
                executed_at,
                finalized_at,
            } => {
                tx_hash.encode(writer);
                executed_at.encode(writer);
                finalized_at.encode(writer);
            }
            DisbursementStatus::RolledBack {
                reason,
                rolled_back_at,
                prior_tx,
            } => {
                reason.encode(writer);
                rolled_back_at.encode(writer);
                prior_tx.encode(writer);
            }
        }
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        match reader.read_u8()? {
            0 => {
                let created_at = u64::decode(reader)?;
                Ok(DisbursementStatus::Draft { created_at })
            }
            1 => {
                let vote_deadline_epoch = u64::decode(reader)?;
                Ok(DisbursementStatus::Voting {
                    vote_deadline_epoch,
                })
            }
            2 => {
                let queued_at = u64::decode(reader)?;
                let activation_epoch = u64::decode(reader)?;
                Ok(DisbursementStatus::Queued {
                    queued_at,
                    activation_epoch,
                })
            }
            3 => {
                let ready_epoch = u64::decode(reader)?;
                Ok(DisbursementStatus::Timelocked { ready_epoch })
            }
            4 => {
                let tx_hash = String::decode(reader)?;
                let executed_at = u64::decode(reader)?;
                Ok(DisbursementStatus::Executed {
                    tx_hash,
                    executed_at,
                })
            }
            5 => {
                let tx_hash = String::decode(reader)?;
                let executed_at = u64::decode(reader)?;
                let finalized_at = u64::decode(reader)?;
                Ok(DisbursementStatus::Finalized {
                    tx_hash,
                    executed_at,
                    finalized_at,
                })
            }
            6 => {
                let reason = String::decode(reader)?;
                let rolled_back_at = u64::decode(reader)?;
                let prior_tx = Option::<String>::decode(reader)?;
                Ok(DisbursementStatus::RolledBack {
                    reason,
                    rolled_back_at,
                    prior_tx,
                })
            }
            other => Err(codec_error(format!(
                "binary decode: unknown DisbursementStatus tag {other}"
            ))),
        }
    }
}

impl BinaryCodec for TreasuryDisbursement {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.id.encode(writer);
        self.destination.encode(writer);
        self.amount_ct.encode(writer);
        self.memo.encode(writer);
        self.scheduled_epoch.encode(writer);
        self.created_at.encode(writer);
        self.status.encode(writer);
        self.amount_it.encode(writer);
        // Use serde-based encoding for complex nested types
        let proposal_bytes =
            foundation_serialization::binary::encode(&self.proposal).unwrap_or_else(|_| Vec::new());
        (proposal_bytes.len() as u32).encode(writer);
        writer.write_bytes(&proposal_bytes);

        let expected_receipts_bytes =
            foundation_serialization::binary::encode(&self.expected_receipts)
                .unwrap_or_else(|_| Vec::new());
        (expected_receipts_bytes.len() as u32).encode(writer);
        writer.write_bytes(&expected_receipts_bytes);

        let receipts_bytes =
            foundation_serialization::binary::encode(&self.receipts).unwrap_or_else(|_| Vec::new());
        (receipts_bytes.len() as u32).encode(writer);
        writer.write_bytes(&receipts_bytes);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        let id = u64::decode(reader)?;
        let destination = String::decode(reader)?;
        let amount_ct = u64::decode(reader)?;
        let memo = String::decode(reader)?;
        let scheduled_epoch = u64::decode(reader)?;
        let created_at = u64::decode(reader)?;
        let status = DisbursementStatus::decode(reader)?;
        let amount_it = if reader.remaining() >= 8 {
            u64::decode(reader)?
        } else {
            0
        };
        // Decode new fields with backwards compatibility
        let proposal = if reader.remaining() >= 4 {
            let len = u32::decode(reader)? as usize;
            if len > 0 && reader.remaining() >= len {
                let bytes = reader.read_exact(len)?;
                foundation_serialization::binary::decode(bytes).ok()
            } else {
                None
            }
        } else {
            None
        };

        let expected_receipts = if reader.remaining() >= 4 {
            let len = u32::decode(reader)? as usize;
            if len > 0 && reader.remaining() >= len {
                let bytes = reader.read_exact(len)?;
                foundation_serialization::binary::decode(bytes).unwrap_or_default()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        let receipts = if reader.remaining() >= 4 {
            let len = u32::decode(reader)? as usize;
            if len > 0 && reader.remaining() >= len {
                let bytes = reader.read_exact(len)?;
                foundation_serialization::binary::decode(bytes).unwrap_or_default()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Ok(Self {
            id,
            destination,
            amount_ct,
            amount_it,
            memo,
            scheduled_epoch,
            created_at,
            status,
            proposal,
            expected_receipts,
            receipts,
        })
    }
}

impl BinaryCodec for TreasuryBalanceSnapshot {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.id.encode(writer);
        self.balance_ct.encode(writer);
        self.delta_ct.encode(writer);
        self.recorded_at.encode(writer);
        self.event.encode(writer);
        self.disbursement_id.encode(writer);
        self.balance_it.encode(writer);
        self.delta_it.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> Result<Self> {
        Ok(Self {
            id: u64::decode(reader)?,
            balance_ct: u64::decode(reader)?,
            delta_ct: i64::decode(reader)?,
            recorded_at: u64::decode(reader)?,
            event: TreasuryBalanceEventKind::decode(reader)?,
            disbursement_id: Option::<u64>::decode(reader)?,
            balance_it: if reader.remaining() >= 16 {
                u64::decode(reader)?
            } else {
                0
            },
            delta_it: if reader.remaining() >= 8 {
                i64::decode(reader)?
            } else {
                0
            },
        })
    }
}

fn status_value(status: &DisbursementStatus) -> Value {
    match status {
        DisbursementStatus::Draft { created_at } => {
            let mut map = Map::new();
            map.insert("created_at".into(), Value::Number((*created_at).into()));
            Value::Object(map)
        }
        DisbursementStatus::Voting {
            vote_deadline_epoch,
        } => {
            let mut map = Map::new();
            map.insert(
                "vote_deadline_epoch".into(),
                Value::Number((*vote_deadline_epoch).into()),
            );
            Value::Object(map)
        }
        DisbursementStatus::Queued {
            queued_at,
            activation_epoch,
        } => {
            let mut map = Map::new();
            map.insert("queued_at".into(), Value::Number((*queued_at).into()));
            map.insert(
                "activation_epoch".into(),
                Value::Number((*activation_epoch).into()),
            );
            Value::Object(map)
        }
        DisbursementStatus::Timelocked { ready_epoch } => {
            let mut map = Map::new();
            map.insert("ready_epoch".into(), Value::Number((*ready_epoch).into()));
            Value::Object(map)
        }
        DisbursementStatus::Executed {
            tx_hash,
            executed_at,
        } => {
            let mut map = Map::new();
            map.insert("tx_hash".into(), Value::String(tx_hash.clone()));
            map.insert("executed_at".into(), Value::Number((*executed_at).into()));
            Value::Object(map)
        }
        DisbursementStatus::Finalized {
            tx_hash,
            executed_at,
            finalized_at,
        } => {
            let mut map = Map::new();
            map.insert("tx_hash".into(), Value::String(tx_hash.clone()));
            map.insert("executed_at".into(), Value::Number((*executed_at).into()));
            map.insert("finalized_at".into(), Value::Number((*finalized_at).into()));
            Value::Object(map)
        }
        DisbursementStatus::RolledBack {
            reason,
            rolled_back_at,
            prior_tx,
        } => {
            let mut map = Map::new();
            map.insert("reason".into(), Value::String(reason.clone()));
            map.insert(
                "rolled_back_at".into(),
                Value::Number((*rolled_back_at).into()),
            );
            if let Some(tx) = prior_tx {
                map.insert("prior_tx".into(), Value::String(tx.clone()));
            }
            Value::Object(map)
        }
    }
}

fn status_from_value(variant: &str, map: &Map) -> Result<DisbursementStatus> {
    match variant {
        "Draft" => {
            let created_at = map
                .get("created_at")
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing created_at"))?;
            Ok(DisbursementStatus::Draft { created_at })
        }
        "Voting" => {
            let vote_deadline_epoch = map
                .get("vote_deadline_epoch")
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing vote_deadline_epoch"))?;
            Ok(DisbursementStatus::Voting {
                vote_deadline_epoch,
            })
        }
        "Queued" => {
            let queued_at = map.get("queued_at").and_then(Value::as_u64).unwrap_or(0);
            let activation_epoch = map
                .get("activation_epoch")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Ok(DisbursementStatus::Queued {
                queued_at,
                activation_epoch,
            })
        }
        "Timelocked" => {
            let ready_epoch = map
                .get("ready_epoch")
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing ready_epoch"))?;
            Ok(DisbursementStatus::Timelocked { ready_epoch })
        }
        "Executed" => {
            let tx_hash = map
                .get("tx_hash")
                .and_then(Value::as_str)
                .ok_or_else(|| codec_error("treasury JSON: missing tx_hash"))?;
            let executed_at = map
                .get("executed_at")
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing executed_at"))?;
            Ok(DisbursementStatus::Executed {
                tx_hash: tx_hash.to_string(),
                executed_at,
            })
        }
        "Finalized" => {
            let tx_hash = map
                .get("tx_hash")
                .and_then(Value::as_str)
                .ok_or_else(|| codec_error("treasury JSON: missing tx_hash"))?;
            let executed_at = map
                .get("executed_at")
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing executed_at"))?;
            let finalized_at = map
                .get("finalized_at")
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing finalized_at"))?;
            Ok(DisbursementStatus::Finalized {
                tx_hash: tx_hash.to_string(),
                executed_at,
                finalized_at,
            })
        }
        "RolledBack" | "Cancelled" => {
            // Support both old (Cancelled) and new (RolledBack) names for backwards compat
            let reason = map
                .get("reason")
                .and_then(Value::as_str)
                .ok_or_else(|| codec_error("treasury JSON: missing reason"))?;
            let rolled_back_at = map
                .get("rolled_back_at")
                .or_else(|| map.get("cancelled_at"))
                .and_then(Value::as_u64)
                .ok_or_else(|| codec_error("treasury JSON: missing rolled_back_at"))?;
            let prior_tx = map
                .get("prior_tx")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            Ok(DisbursementStatus::RolledBack {
                reason: reason.to_string(),
                rolled_back_at,
                prior_tx,
            })
        }
        // Backwards compatibility for old "Scheduled" variant
        "Scheduled" => Ok(DisbursementStatus::Queued {
            queued_at: 0,
            activation_epoch: 0,
        }),
        other => Err(codec_error(format!(
            "treasury JSON: unknown disbursement status {other}"
        ))),
    }
}

pub fn disbursement_to_json(disbursement: &TreasuryDisbursement) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::Number(disbursement.id.into()));
    map.insert(
        "destination".into(),
        Value::String(disbursement.destination.clone()),
    );
    map.insert(
        "amount_ct".into(),
        Value::Number(disbursement.amount_ct.into()),
    );
    if disbursement.amount_it > 0 {
        map.insert(
            "amount_it".into(),
            Value::Number(disbursement.amount_it.into()),
        );
    }
    map.insert("memo".into(), Value::String(disbursement.memo.clone()));
    map.insert(
        "scheduled_epoch".into(),
        Value::Number(disbursement.scheduled_epoch.into()),
    );
    map.insert(
        "created_at".into(),
        Value::Number(disbursement.created_at.into()),
    );
    let status_variant = match &disbursement.status {
        DisbursementStatus::Draft { .. } => "Draft",
        DisbursementStatus::Voting { .. } => "Voting",
        DisbursementStatus::Queued { .. } => "Queued",
        DisbursementStatus::Timelocked { .. } => "Timelocked",
        DisbursementStatus::Executed { .. } => "Executed",
        DisbursementStatus::Finalized { .. } => "Finalized",
        DisbursementStatus::RolledBack { .. } => "RolledBack",
    };
    let mut status_map = Map::new();
    status_map.insert(status_variant.into(), status_value(&disbursement.status));
    map.insert("status".into(), Value::Object(status_map));

    // Add proposal if present
    if let Some(proposal) = &disbursement.proposal {
        let proposal_json =
            foundation_serialization::json::to_value(proposal).unwrap_or(Value::Object(Map::new()));
        map.insert("proposal".into(), proposal_json);
    }

    // Add expected_receipts if non-empty
    if !disbursement.expected_receipts.is_empty() {
        let receipts_json =
            foundation_serialization::json::to_value(&disbursement.expected_receipts)
                .unwrap_or(Value::Array(vec![]));
        map.insert("expected_receipts".into(), receipts_json);
    }

    // Add receipts if non-empty
    if !disbursement.receipts.is_empty() {
        let receipts_json = foundation_serialization::json::to_value(&disbursement.receipts)
            .unwrap_or(Value::Array(vec![]));
        map.insert("receipts".into(), receipts_json);
    }

    Value::Object(map)
}

pub fn disbursement_from_json(value: &Value) -> Result<TreasuryDisbursement> {
    let obj = value
        .as_object()
        .ok_or_else(|| codec_error("treasury JSON: expected object"))?;
    let id = obj
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury JSON: missing id"))?;
    let destination = obj
        .get("destination")
        .and_then(Value::as_str)
        .ok_or_else(|| codec_error("treasury JSON: missing destination"))?
        .to_string();
    let amount_ct = obj
        .get("amount_ct")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury JSON: missing amount_ct"))?;
    let amount_it = obj.get("amount_it").and_then(Value::as_u64).unwrap_or(0);
    let memo = obj
        .get("memo")
        .and_then(Value::as_str)
        .ok_or_else(|| codec_error("treasury JSON: missing memo"))?
        .to_string();
    let scheduled_epoch = obj
        .get("scheduled_epoch")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury JSON: missing scheduled_epoch"))?;
    let created_at = obj
        .get("created_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury JSON: missing created_at"))?;
    let status_obj = obj
        .get("status")
        .and_then(Value::as_object)
        .ok_or_else(|| codec_error("treasury JSON: missing status"))?;
    if status_obj.len() != 1 {
        return Err(codec_error("treasury JSON: invalid status payload"));
    }
    let (variant, payload) = status_obj.iter().next().unwrap();
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| codec_error("treasury JSON: invalid status value"))?;
    let status = status_from_value(variant, payload_obj)?;

    // Parse optional proposal field
    let proposal = obj
        .get("proposal")
        .and_then(|v| foundation_serialization::json::from_value(v.clone()).ok());

    // Parse expected_receipts (default to empty vec if missing)
    let expected_receipts = obj
        .get("expected_receipts")
        .and_then(|v| foundation_serialization::json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Parse receipts (default to empty vec if missing)
    let receipts = obj
        .get("receipts")
        .and_then(|v| foundation_serialization::json::from_value(v.clone()).ok())
        .unwrap_or_default();

    Ok(TreasuryDisbursement {
        id,
        destination,
        amount_ct,
        amount_it,
        memo,
        scheduled_epoch,
        created_at,
        status,
        proposal,
        expected_receipts,
        receipts,
    })
}

pub fn balance_snapshot_to_json(snapshot: &TreasuryBalanceSnapshot) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::Number(snapshot.id.into()));
    map.insert(
        "balance_ct".into(),
        Value::Number(snapshot.balance_ct.into()),
    );
    map.insert("delta_ct".into(), Value::Number(snapshot.delta_ct.into()));
    map.insert(
        "balance_it".into(),
        Value::Number(snapshot.balance_it.into()),
    );
    map.insert("delta_it".into(), Value::Number(snapshot.delta_it.into()));
    map.insert(
        "recorded_at".into(),
        Value::Number(snapshot.recorded_at.into()),
    );
    let event_str = match snapshot.event {
        TreasuryBalanceEventKind::Accrual => "Accrual",
        TreasuryBalanceEventKind::Queued => "Queued",
        TreasuryBalanceEventKind::Executed => "Executed",
        TreasuryBalanceEventKind::Cancelled => "Cancelled",
    };
    map.insert("event".into(), Value::String(event_str.into()));
    if let Some(id) = snapshot.disbursement_id {
        map.insert("disbursement_id".into(), Value::Number(id.into()));
    }
    Value::Object(map)
}

pub fn balance_snapshot_from_json(value: &Value) -> Result<TreasuryBalanceSnapshot> {
    let obj = value
        .as_object()
        .ok_or_else(|| codec_error("treasury balance JSON: expected object"))?;
    let id = obj
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury balance JSON: missing id"))?;
    let balance_ct = obj
        .get("balance_ct")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury balance JSON: missing balance_ct"))?;
    let delta_ct = obj
        .get("delta_ct")
        .and_then(Value::as_i64)
        .ok_or_else(|| codec_error("treasury balance JSON: missing delta_ct"))?;
    let balance_it = obj.get("balance_it").and_then(Value::as_u64).unwrap_or(0);
    let delta_it = obj.get("delta_it").and_then(Value::as_i64).unwrap_or(0);
    let recorded_at = obj
        .get("recorded_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury balance JSON: missing recorded_at"))?;
    let event = match obj
        .get("event")
        .and_then(Value::as_str)
        .ok_or_else(|| codec_error("treasury balance JSON: missing event"))?
    {
        "Accrual" => TreasuryBalanceEventKind::Accrual,
        "Queued" => TreasuryBalanceEventKind::Queued,
        "Executed" => TreasuryBalanceEventKind::Executed,
        "Cancelled" => TreasuryBalanceEventKind::Cancelled,
        other => {
            return Err(codec_error(format!(
                "treasury balance JSON: unknown event {other}"
            )))
        }
    };
    let disbursement_id = obj.get("disbursement_id").and_then(Value::as_u64);
    Ok(TreasuryBalanceSnapshot {
        id,
        balance_ct,
        delta_ct,
        balance_it,
        delta_it,
        recorded_at,
        event,
        disbursement_id,
    })
}

pub fn encode_binary<T: BinaryCodec>(value: &T) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    value.encode(&mut writer);
    Ok(writer.into_inner())
}

pub fn decode_binary<T: BinaryCodec>(bytes: &[u8]) -> Result<T> {
    let mut reader = BinaryReader::new(bytes);
    let value = T::decode(&mut reader)?;
    reader.finish()?;
    Ok(value)
}

pub fn disbursements_to_json_array(records: &[TreasuryDisbursement]) -> Value {
    Value::Array(records.iter().map(disbursement_to_json).collect())
}

pub fn disbursements_from_json_array(value: &Value) -> Result<Vec<TreasuryDisbursement>> {
    let arr = value
        .as_array()
        .ok_or_else(|| codec_error("treasury JSON: expected array"))?;
    arr.iter().map(disbursement_from_json).collect()
}

pub fn balance_history_to_json(history: &[TreasuryBalanceSnapshot]) -> Value {
    Value::Array(history.iter().map(balance_snapshot_to_json).collect())
}

pub fn balance_history_from_json(value: &Value) -> Result<Vec<TreasuryBalanceSnapshot>> {
    let arr = value
        .as_array()
        .ok_or_else(|| codec_error("treasury balance JSON: expected array"))?;
    arr.iter().map(balance_snapshot_from_json).collect()
}

pub fn param_key_to_string(key: ParamKey) -> &'static str {
    match key {
        ParamKey::SnapshotIntervalSecs => "SnapshotIntervalSecs",
        ParamKey::ConsumerFeeComfortP90Microunits => "ConsumerFeeComfortP90Microunits",
        ParamKey::IndustrialAdmissionMinCapacity => "IndustrialAdmissionMinCapacity",
        ParamKey::FairshareGlobalMax => "FairshareGlobalMax",
        ParamKey::BurstRefillRatePerS => "BurstRefillRatePerS",
        ParamKey::BetaStorageSubCt => "BetaStorageSubCt",
        ParamKey::GammaReadSubCt => "GammaReadSubCt",
        ParamKey::KappaCpuSubCt => "KappaCpuSubCt",
        ParamKey::LambdaBytesOutSubCt => "LambdaBytesOutSubCt",
        ParamKey::ReadSubsidyViewerPercent => "ReadSubsidyViewerPercent",
        ParamKey::ReadSubsidyHostPercent => "ReadSubsidyHostPercent",
        ParamKey::ReadSubsidyHardwarePercent => "ReadSubsidyHardwarePercent",
        ParamKey::ReadSubsidyVerifierPercent => "ReadSubsidyVerifierPercent",
        ParamKey::ReadSubsidyLiquidityPercent => "ReadSubsidyLiquidityPercent",
        ParamKey::DualTokenSettlementEnabled => "DualTokenSettlementEnabled",
        ParamKey::AdReadinessWindowSecs => "AdReadinessWindowSecs",
        ParamKey::AdReadinessMinUniqueViewers => "AdReadinessMinUniqueViewers",
        ParamKey::AdReadinessMinHostCount => "AdReadinessMinHostCount",
        ParamKey::AdReadinessMinProviderCount => "AdReadinessMinProviderCount",
        ParamKey::AdRehearsalEnabled => "AdRehearsalEnabled",
        ParamKey::AdRehearsalStabilityWindows => "AdRehearsalStabilityWindows",
        ParamKey::AdUsePercentileThresholds => "AdUsePercentileThresholds",
        ParamKey::AdViewerPercentile => "AdViewerPercentile",
        ParamKey::AdHostPercentile => "AdHostPercentile",
        ParamKey::AdProviderPercentile => "AdProviderPercentile",
        ParamKey::AdEmaSmoothingPpm => "AdEmaSmoothingPpm",
        ParamKey::AdFloorUniqueViewers => "AdFloorUniqueViewers",
        ParamKey::AdFloorHostCount => "AdFloorHostCount",
        ParamKey::AdFloorProviderCount => "AdFloorProviderCount",
        ParamKey::AdCapUniqueViewers => "AdCapUniqueViewers",
        ParamKey::AdCapHostCount => "AdCapHostCount",
        ParamKey::AdCapProviderCount => "AdCapProviderCount",
        ParamKey::AdPercentileBuckets => "AdPercentileBuckets",
        ParamKey::EnergyMinStake => "EnergyMinStake",
        ParamKey::EnergyOracleTimeoutBlocks => "EnergyOracleTimeoutBlocks",
        ParamKey::EnergySlashingRateBps => "EnergySlashingRateBps",
        ParamKey::TreasuryPercentCt => "TreasuryPercentCt",
        ParamKey::ProofRebateLimitCt => "ProofRebateLimitCt",
        ParamKey::RentRateCtPerByte => "RentRateCtPerByte",
        ParamKey::KillSwitchSubsidyReduction => "KillSwitchSubsidyReduction",
        ParamKey::MinerRewardLogisticTarget => "MinerRewardLogisticTarget",
        ParamKey::LogisticSlope => "LogisticSlope",
        ParamKey::MinerHysteresis => "MinerHysteresis",
        ParamKey::HeuristicMuMilli => "HeuristicMuMilli",
        ParamKey::FeeFloorWindow => "FeeFloorWindow",
        ParamKey::FeeFloorPercentile => "FeeFloorPercentile",
        ParamKey::BadgeExpirySecs => "BadgeExpirySecs",
        ParamKey::BadgeIssueUptime => "BadgeIssueUptime",
        ParamKey::BadgeRevokeUptime => "BadgeRevokeUptime",
        ParamKey::JurisdictionRegion => "JurisdictionRegion",
        ParamKey::AiDiagnosticsEnabled => "AiDiagnosticsEnabled",
        ParamKey::KalmanRShort => "KalmanRShort",
        ParamKey::KalmanRMed => "KalmanRMed",
        ParamKey::KalmanRLong => "KalmanRLong",
        ParamKey::SchedulerWeightGossip => "SchedulerWeightGossip",
        ParamKey::SchedulerWeightCompute => "SchedulerWeightCompute",
        ParamKey::SchedulerWeightStorage => "SchedulerWeightStorage",
        ParamKey::RuntimeBackend => "RuntimeBackend",
        ParamKey::TransportProvider => "TransportProvider",
        ParamKey::StorageEnginePolicy => "StorageEnginePolicy",
        ParamKey::BridgeMinBond => "BridgeMinBond",
        ParamKey::BridgeDutyReward => "BridgeDutyReward",
        ParamKey::BridgeFailureSlash => "BridgeFailureSlash",
        ParamKey::BridgeChallengeSlash => "BridgeChallengeSlash",
        ParamKey::BridgeDutyWindowSecs => "BridgeDutyWindowSecs",

        // Economic Control Laws (Layer 1: Inflation)
        ParamKey::InflationTargetBps => "InflationTargetBps",
        ParamKey::InflationControllerGain => "InflationControllerGain",
        ParamKey::MinAnnualIssuanceCt => "MinAnnualIssuanceCt",
        ParamKey::MaxAnnualIssuanceCt => "MaxAnnualIssuanceCt",

        // Economic Control Laws (Layer 2: Subsidy Allocator)
        ParamKey::StorageUtilTargetBps => "StorageUtilTargetBps",
        ParamKey::StorageMarginTargetBps => "StorageMarginTargetBps",
        ParamKey::ComputeUtilTargetBps => "ComputeUtilTargetBps",
        ParamKey::ComputeMarginTargetBps => "ComputeMarginTargetBps",
        ParamKey::EnergyUtilTargetBps => "EnergyUtilTargetBps",
        ParamKey::EnergyMarginTargetBps => "EnergyMarginTargetBps",
        ParamKey::AdUtilTargetBps => "AdUtilTargetBps",
        ParamKey::AdMarginTargetBps => "AdMarginTargetBps",
        ParamKey::SubsidyAllocatorAlpha => "SubsidyAllocatorAlpha",
        ParamKey::SubsidyAllocatorBeta => "SubsidyAllocatorBeta",
        ParamKey::SubsidyAllocatorTemperature => "SubsidyAllocatorTemperature",
        ParamKey::SubsidyAllocatorDriftRate => "SubsidyAllocatorDriftRate",

        // Economic Control Laws (Layer 3: Market Multipliers - Storage)
        ParamKey::StorageUtilResponsiveness => "StorageUtilResponsiveness",
        ParamKey::StorageCostResponsiveness => "StorageCostResponsiveness",
        ParamKey::StorageMultiplierFloor => "StorageMultiplierFloor",
        ParamKey::StorageMultiplierCeiling => "StorageMultiplierCeiling",

        // Economic Control Laws (Layer 3: Market Multipliers - Compute)
        ParamKey::ComputeUtilResponsiveness => "ComputeUtilResponsiveness",
        ParamKey::ComputeCostResponsiveness => "ComputeCostResponsiveness",
        ParamKey::ComputeMultiplierFloor => "ComputeMultiplierFloor",
        ParamKey::ComputeMultiplierCeiling => "ComputeMultiplierCeiling",

        // Economic Control Laws (Layer 3: Market Multipliers - Energy)
        ParamKey::EnergyUtilResponsiveness => "EnergyUtilResponsiveness",
        ParamKey::EnergyCostResponsiveness => "EnergyCostResponsiveness",
        ParamKey::EnergyMultiplierFloor => "EnergyMultiplierFloor",
        ParamKey::EnergyMultiplierCeiling => "EnergyMultiplierCeiling",

        // Economic Control Laws (Layer 3: Market Multipliers - Ad)
        ParamKey::AdUtilResponsiveness => "AdUtilResponsiveness",
        ParamKey::AdCostResponsiveness => "AdCostResponsiveness",
        ParamKey::AdMultiplierFloor => "AdMultiplierFloor",
        ParamKey::AdMultiplierCeiling => "AdMultiplierCeiling",

        // Economic Control Laws (Layer 4: Ad Market Drift)
        ParamKey::AdPlatformTakeTargetBps => "AdPlatformTakeTargetBps",
        ParamKey::AdUserShareTargetBps => "AdUserShareTargetBps",
        ParamKey::AdDriftRate => "AdDriftRate",

        // Economic Control Laws (Layer 4: Tariff Controller)
        ParamKey::TariffPublicRevenueTargetBps => "TariffPublicRevenueTargetBps",
        ParamKey::TariffDriftRate => "TariffDriftRate",
        ParamKey::TariffMinBps => "TariffMinBps",
        ParamKey::TariffMaxBps => "TariffMaxBps",
    }
}

pub fn param_key_from_string(value: &str) -> Result<ParamKey> {
    match value {
        "SnapshotIntervalSecs" => Ok(ParamKey::SnapshotIntervalSecs),
        "ConsumerFeeComfortP90Microunits" => Ok(ParamKey::ConsumerFeeComfortP90Microunits),
        "IndustrialAdmissionMinCapacity" => Ok(ParamKey::IndustrialAdmissionMinCapacity),
        "FairshareGlobalMax" => Ok(ParamKey::FairshareGlobalMax),
        "BurstRefillRatePerS" => Ok(ParamKey::BurstRefillRatePerS),
        "BetaStorageSubCt" => Ok(ParamKey::BetaStorageSubCt),
        "GammaReadSubCt" => Ok(ParamKey::GammaReadSubCt),
        "KappaCpuSubCt" => Ok(ParamKey::KappaCpuSubCt),
        "LambdaBytesOutSubCt" => Ok(ParamKey::LambdaBytesOutSubCt),
        "ReadSubsidyViewerPercent" => Ok(ParamKey::ReadSubsidyViewerPercent),
        "ReadSubsidyHostPercent" => Ok(ParamKey::ReadSubsidyHostPercent),
        "ReadSubsidyHardwarePercent" => Ok(ParamKey::ReadSubsidyHardwarePercent),
        "ReadSubsidyVerifierPercent" => Ok(ParamKey::ReadSubsidyVerifierPercent),
        "ReadSubsidyLiquidityPercent" => Ok(ParamKey::ReadSubsidyLiquidityPercent),
        "DualTokenSettlementEnabled" => Ok(ParamKey::DualTokenSettlementEnabled),
        "AdReadinessWindowSecs" => Ok(ParamKey::AdReadinessWindowSecs),
        "AdReadinessMinUniqueViewers" => Ok(ParamKey::AdReadinessMinUniqueViewers),
        "AdReadinessMinHostCount" => Ok(ParamKey::AdReadinessMinHostCount),
        "AdReadinessMinProviderCount" => Ok(ParamKey::AdReadinessMinProviderCount),
        "AdRehearsalEnabled" => Ok(ParamKey::AdRehearsalEnabled),
        "AdRehearsalStabilityWindows" => Ok(ParamKey::AdRehearsalStabilityWindows),
        "AdUsePercentileThresholds" => Ok(ParamKey::AdUsePercentileThresholds),
        "AdViewerPercentile" => Ok(ParamKey::AdViewerPercentile),
        "AdHostPercentile" => Ok(ParamKey::AdHostPercentile),
        "AdProviderPercentile" => Ok(ParamKey::AdProviderPercentile),
        "AdEmaSmoothingPpm" => Ok(ParamKey::AdEmaSmoothingPpm),
        "AdFloorUniqueViewers" => Ok(ParamKey::AdFloorUniqueViewers),
        "AdFloorHostCount" => Ok(ParamKey::AdFloorHostCount),
        "AdFloorProviderCount" => Ok(ParamKey::AdFloorProviderCount),
        "AdCapUniqueViewers" => Ok(ParamKey::AdCapUniqueViewers),
        "AdCapHostCount" => Ok(ParamKey::AdCapHostCount),
        "AdCapProviderCount" => Ok(ParamKey::AdCapProviderCount),
        "AdPercentileBuckets" => Ok(ParamKey::AdPercentileBuckets),
        "EnergyMinStake" => Ok(ParamKey::EnergyMinStake),
        "EnergyOracleTimeoutBlocks" => Ok(ParamKey::EnergyOracleTimeoutBlocks),
        "EnergySlashingRateBps" => Ok(ParamKey::EnergySlashingRateBps),
        "TreasuryPercentCt" => Ok(ParamKey::TreasuryPercentCt),
        "ProofRebateLimitCt" => Ok(ParamKey::ProofRebateLimitCt),
        "RentRateCtPerByte" => Ok(ParamKey::RentRateCtPerByte),
        "KillSwitchSubsidyReduction" => Ok(ParamKey::KillSwitchSubsidyReduction),
        "MinerRewardLogisticTarget" => Ok(ParamKey::MinerRewardLogisticTarget),
        "LogisticSlope" => Ok(ParamKey::LogisticSlope),
        "MinerHysteresis" => Ok(ParamKey::MinerHysteresis),
        "HeuristicMuMilli" => Ok(ParamKey::HeuristicMuMilli),
        "FeeFloorWindow" => Ok(ParamKey::FeeFloorWindow),
        "FeeFloorPercentile" => Ok(ParamKey::FeeFloorPercentile),
        "BadgeExpirySecs" => Ok(ParamKey::BadgeExpirySecs),
        "BadgeIssueUptime" => Ok(ParamKey::BadgeIssueUptime),
        "BadgeRevokeUptime" => Ok(ParamKey::BadgeRevokeUptime),
        "JurisdictionRegion" => Ok(ParamKey::JurisdictionRegion),
        "AiDiagnosticsEnabled" => Ok(ParamKey::AiDiagnosticsEnabled),
        "KalmanRShort" => Ok(ParamKey::KalmanRShort),
        "KalmanRMed" => Ok(ParamKey::KalmanRMed),
        "KalmanRLong" => Ok(ParamKey::KalmanRLong),
        "SchedulerWeightGossip" => Ok(ParamKey::SchedulerWeightGossip),
        "SchedulerWeightCompute" => Ok(ParamKey::SchedulerWeightCompute),
        "SchedulerWeightStorage" => Ok(ParamKey::SchedulerWeightStorage),
        "RuntimeBackend" => Ok(ParamKey::RuntimeBackend),
        "TransportProvider" => Ok(ParamKey::TransportProvider),
        "StorageEnginePolicy" => Ok(ParamKey::StorageEnginePolicy),
        "BridgeMinBond" => Ok(ParamKey::BridgeMinBond),
        "BridgeDutyReward" => Ok(ParamKey::BridgeDutyReward),
        "BridgeFailureSlash" => Ok(ParamKey::BridgeFailureSlash),
        "BridgeChallengeSlash" => Ok(ParamKey::BridgeChallengeSlash),
        "BridgeDutyWindowSecs" => Ok(ParamKey::BridgeDutyWindowSecs),

        // Economic Control Laws (Layer 1: Inflation)
        "InflationTargetBps" => Ok(ParamKey::InflationTargetBps),
        "InflationControllerGain" => Ok(ParamKey::InflationControllerGain),
        "MinAnnualIssuanceCt" => Ok(ParamKey::MinAnnualIssuanceCt),
        "MaxAnnualIssuanceCt" => Ok(ParamKey::MaxAnnualIssuanceCt),

        // Economic Control Laws (Layer 2: Subsidy Allocator)
        "StorageUtilTargetBps" => Ok(ParamKey::StorageUtilTargetBps),
        "StorageMarginTargetBps" => Ok(ParamKey::StorageMarginTargetBps),
        "ComputeUtilTargetBps" => Ok(ParamKey::ComputeUtilTargetBps),
        "ComputeMarginTargetBps" => Ok(ParamKey::ComputeMarginTargetBps),
        "EnergyUtilTargetBps" => Ok(ParamKey::EnergyUtilTargetBps),
        "EnergyMarginTargetBps" => Ok(ParamKey::EnergyMarginTargetBps),
        "AdUtilTargetBps" => Ok(ParamKey::AdUtilTargetBps),
        "AdMarginTargetBps" => Ok(ParamKey::AdMarginTargetBps),
        "SubsidyAllocatorAlpha" => Ok(ParamKey::SubsidyAllocatorAlpha),
        "SubsidyAllocatorBeta" => Ok(ParamKey::SubsidyAllocatorBeta),
        "SubsidyAllocatorTemperature" => Ok(ParamKey::SubsidyAllocatorTemperature),
        "SubsidyAllocatorDriftRate" => Ok(ParamKey::SubsidyAllocatorDriftRate),

        // Economic Control Laws (Layer 3: Market Multipliers - Storage)
        "StorageUtilResponsiveness" => Ok(ParamKey::StorageUtilResponsiveness),
        "StorageCostResponsiveness" => Ok(ParamKey::StorageCostResponsiveness),
        "StorageMultiplierFloor" => Ok(ParamKey::StorageMultiplierFloor),
        "StorageMultiplierCeiling" => Ok(ParamKey::StorageMultiplierCeiling),

        // Economic Control Laws (Layer 3: Market Multipliers - Compute)
        "ComputeUtilResponsiveness" => Ok(ParamKey::ComputeUtilResponsiveness),
        "ComputeCostResponsiveness" => Ok(ParamKey::ComputeCostResponsiveness),
        "ComputeMultiplierFloor" => Ok(ParamKey::ComputeMultiplierFloor),
        "ComputeMultiplierCeiling" => Ok(ParamKey::ComputeMultiplierCeiling),

        // Economic Control Laws (Layer 3: Market Multipliers - Energy)
        "EnergyUtilResponsiveness" => Ok(ParamKey::EnergyUtilResponsiveness),
        "EnergyCostResponsiveness" => Ok(ParamKey::EnergyCostResponsiveness),
        "EnergyMultiplierFloor" => Ok(ParamKey::EnergyMultiplierFloor),
        "EnergyMultiplierCeiling" => Ok(ParamKey::EnergyMultiplierCeiling),

        // Economic Control Laws (Layer 3: Market Multipliers - Ad)
        "AdUtilResponsiveness" => Ok(ParamKey::AdUtilResponsiveness),
        "AdCostResponsiveness" => Ok(ParamKey::AdCostResponsiveness),
        "AdMultiplierFloor" => Ok(ParamKey::AdMultiplierFloor),
        "AdMultiplierCeiling" => Ok(ParamKey::AdMultiplierCeiling),

        // Economic Control Laws (Layer 4: Ad Market Drift)
        "AdPlatformTakeTargetBps" => Ok(ParamKey::AdPlatformTakeTargetBps),
        "AdUserShareTargetBps" => Ok(ParamKey::AdUserShareTargetBps),
        "AdDriftRate" => Ok(ParamKey::AdDriftRate),

        // Economic Control Laws (Layer 4: Tariff Controller)
        "TariffPublicRevenueTargetBps" => Ok(ParamKey::TariffPublicRevenueTargetBps),
        "TariffDriftRate" => Ok(ParamKey::TariffDriftRate),
        "TariffMinBps" => Ok(ParamKey::TariffMinBps),
        "TariffMaxBps" => Ok(ParamKey::TariffMaxBps),

        other => Err(codec_error(format!("param key JSON: unknown key {other}"))),
    }
}

pub fn json_to_bytes(value: &Value) -> Vec<u8> {
    json::to_vec_value(value)
}

pub fn json_from_bytes(bytes: &[u8]) -> Result<Value> {
    json::value_from_slice(bytes).map_err(|e| codec_error(format!("json decode: {e}")))
}
