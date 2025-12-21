// Manual binary and JSON codecs for governance data structures.
use super::{
    ApprovedRelease, ParamKey, Proposal, ProposalStatus, ReleaseAttestation, ReleaseBallot,
    ReleaseVote, Vote, VoteChoice,
};
use foundation_serialization::json::{self, Map, Value};
use governance_spec::codec as governance_codec;
pub use governance_spec::codec::{BinaryCodec, BinaryReader, BinaryWriter};
use governance_spec::treasury::{
    DisbursementStatus, TreasuryBalanceEventKind, TreasuryBalanceSnapshot, TreasuryDisbursement,
};

pub type Result<T> = std::result::Result<T, sled::Error>;

fn codec_error(msg: impl Into<String>) -> sled::Error {
    sled::Error::Unsupported(msg.into().into_boxed_str())
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
        // New dynamic readiness params begin at 48
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
    map.insert("amount".into(), Value::Number(disbursement.amount.into()));
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
    let amount = obj
        .get("amount")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury JSON: missing amount"))?;
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
        amount,
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
    map.insert("balance".into(), Value::Number(snapshot.balance.into()));
    map.insert("delta".into(), Value::Number(snapshot.delta.into()));
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
    let balance = obj
        .get("balance")
        .and_then(Value::as_u64)
        .ok_or_else(|| codec_error("treasury balance JSON: missing balance"))?;
    let delta = obj
        .get("delta")
        .and_then(Value::as_i64)
        .ok_or_else(|| codec_error("treasury balance JSON: missing delta"))?;
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
        balance,
        delta,
        recorded_at,
        event,
        disbursement_id,
    })
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
        ParamKey::DualTokenSettlementEnabled => "DualTokenSettlementEnabled",
        ParamKey::AdReadinessWindowSecs => "AdReadinessWindowSecs",
        ParamKey::AdReadinessMinUniqueViewers => "AdReadinessMinUniqueViewers",
        ParamKey::AdReadinessMinHostCount => "AdReadinessMinHostCount",
        ParamKey::AdReadinessMinProviderCount => "AdReadinessMinProviderCount",
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
        ParamKey::AdRehearsalEnabled => "AdRehearsalEnabled",
        ParamKey::AdRehearsalStabilityWindows => "AdRehearsalStabilityWindows",
        ParamKey::EnergyMinStake => "EnergyMinStake",
        ParamKey::EnergyOracleTimeoutBlocks => "EnergyOracleTimeoutBlocks",
        ParamKey::EnergySlashingRateBps => "EnergySlashingRateBps",
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
        "DualTokenSettlementEnabled" => Ok(ParamKey::DualTokenSettlementEnabled),
        "AdReadinessWindowSecs" => Ok(ParamKey::AdReadinessWindowSecs),
        "AdReadinessMinUniqueViewers" => Ok(ParamKey::AdReadinessMinUniqueViewers),
        "AdReadinessMinHostCount" => Ok(ParamKey::AdReadinessMinHostCount),
        "AdReadinessMinProviderCount" => Ok(ParamKey::AdReadinessMinProviderCount),
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
        "AdRehearsalEnabled" => Ok(ParamKey::AdRehearsalEnabled),
        "AdRehearsalStabilityWindows" => Ok(ParamKey::AdRehearsalStabilityWindows),
        "EnergyMinStake" => Ok(ParamKey::EnergyMinStake),
        "EnergyOracleTimeoutBlocks" => Ok(ParamKey::EnergyOracleTimeoutBlocks),
        "EnergySlashingRateBps" => Ok(ParamKey::EnergySlashingRateBps),
        other => Err(codec_error(format!("param key JSON: unknown key {other}"))),
    }
}

pub fn encode_binary<T: BinaryCodec>(value: &T) -> Result<Vec<u8>> {
    governance_codec::encode_binary(value)
}

pub fn decode_binary<T: BinaryCodec>(bytes: &[u8]) -> Result<T> {
    governance_codec::decode_binary(bytes)
}

pub fn json_to_bytes(value: &Value) -> Vec<u8> {
    json::to_vec_value(value)
}

pub fn json_from_bytes(bytes: &[u8]) -> Result<Value> {
    json::value_from_slice(bytes).map_err(|e| codec_error(format!("json decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rehearsal_param_keys_roundtrip() {
        let keys = [
            ParamKey::AdRehearsalEnabled,
            ParamKey::AdRehearsalStabilityWindows,
        ];
        for key in keys {
            let tag = param_key_to_tag(key);
            let decoded = param_key_from_tag(tag).expect("decode tag");
            assert_eq!(decoded, key);
            let string = param_key_to_string(key);
            let parsed = param_key_from_string(string).expect("decode string");
            assert_eq!(parsed, key);
        }
    }
}
