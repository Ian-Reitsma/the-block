use super::{
    codec::{
        balance_history_from_json, balance_history_to_json, decode_binary,
        disbursements_from_json_array, disbursements_to_json_array, encode_binary, json_from_bytes,
        json_to_bytes, param_key_from_string, param_key_to_string, BinaryCodec, BinaryReader,
        BinaryWriter, Result as CodecResult,
    },
    registry, ApprovedRelease, ParamKey, Params, Proposal, ProposalStatus, ReleaseBallot,
    ReleaseVote, RewardClaimApproval, Runtime, Vote, VoteChoice,
};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    governance_webhook, GOV_ACTIVATION_DELAY_SECONDS, GOV_PROPOSALS_PENDING, GOV_ROLLBACK_TOTAL,
    GOV_VOTES_TOTAL, PARAM_CHANGE_ACTIVE, PARAM_CHANGE_PENDING,
};
use concurrency::Lazy;
use foundation_serialization::json::{Map, Number, Value};
use foundation_serialization::{Deserialize, Serialize};
use governance_spec::treasury::{
    mark_cancelled, mark_executed, TreasuryBalanceEventKind, TreasuryBalanceSnapshot,
    TreasuryDisbursement,
};
use governance_spec::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
};
use sled::Config;
use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ACTIVATION_DELAY: u64 = 2;
pub const ROLLBACK_WINDOW_EPOCHS: u64 = 1;
pub const QUORUM: u64 = 1;
const PARAM_HISTORY_LIMIT: usize = 512;
const DID_REVOCATION_HISTORY_LIMIT: usize = 512;
const TREASURY_HISTORY_LIMIT: usize = 1024;
const TREASURY_BALANCE_HISTORY_LIMIT: usize = 2048;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "foundation_serialization::serde")]
pub struct LastActivation {
    pub proposal_id: u64,
    pub key: ParamKey,
    pub old_value: i64,
    pub new_value: i64,
    pub activated_epoch: u64,
}

impl BinaryCodec for LastActivation {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.proposal_id.encode(writer);
        self.key.encode(writer);
        self.old_value.encode(writer);
        self.new_value.encode(writer);
        self.activated_epoch.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            proposal_id: u64::decode(reader)?,
            key: ParamKey::decode(reader)?,
            old_value: i64::decode(reader)?,
            new_value: i64::decode(reader)?,
            activated_epoch: u64::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct ParamChangeRecord {
    key: ParamKey,
    proposal_id: u64,
    epoch: u64,
    old_value: i64,
    new_value: i64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    fee_floor: Option<FeeFloorPolicySnapshot>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    dependency_policy: Option<DependencyPolicySnapshot>,
}

impl BinaryCodec for ParamChangeRecord {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.key.encode(writer);
        self.proposal_id.encode(writer);
        self.epoch.encode(writer);
        self.old_value.encode(writer);
        self.new_value.encode(writer);
        self.fee_floor.encode(writer);
        self.dependency_policy.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            key: ParamKey::decode(reader)?,
            proposal_id: u64::decode(reader)?,
            epoch: u64::decode(reader)?,
            old_value: i64::decode(reader)?,
            new_value: i64::decode(reader)?,
            fee_floor: Option::<FeeFloorPolicySnapshot>::decode(reader)?,
            dependency_policy: Option::<DependencyPolicySnapshot>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct FeeFloorPolicySnapshot {
    window: i64,
    percentile: i64,
}

impl BinaryCodec for FeeFloorPolicySnapshot {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.window.encode(writer);
        self.percentile.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            window: i64::decode(reader)?,
            percentile: i64::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct FeeFloorPolicyRecord {
    epoch: u64,
    proposal_id: u64,
    window: i64,
    percentile: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DependencyPolicySnapshot {
    kind: String,
    allowed: Vec<String>,
}

impl BinaryCodec for DependencyPolicySnapshot {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.kind.encode(writer);
        self.allowed.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            kind: String::decode(reader)?,
            allowed: Vec::<String>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DependencyPolicyRecord {
    pub epoch: u64,
    pub proposal_id: u64,
    pub kind: String,
    pub allowed: Vec<String>,
}

impl BinaryCodec for DependencyPolicyRecord {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.epoch.encode(writer);
        self.proposal_id.encode(writer);
        self.kind.encode(writer);
        self.allowed.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            epoch: u64::decode(reader)?,
            proposal_id: u64::decode(reader)?,
            kind: String::decode(reader)?,
            allowed: Vec::<String>::decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DidRevocationRecord {
    pub address: String,
    pub reason: String,
    pub epoch: u64,
    pub revoked_at: u64,
}

impl BinaryCodec for DidRevocationRecord {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.address.encode(writer);
        self.reason.encode(writer);
        self.epoch.encode(writer);
        self.revoked_at.encode(writer);
    }

    fn decode(reader: &mut BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            address: String::decode(reader)?,
            reason: String::decode(reader)?,
            epoch: u64::decode(reader)?,
            revoked_at: u64::decode(reader)?,
        })
    }
}

#[derive(Clone)]
pub struct GovStore {
    db: Arc<sled::Db>,
    base_path: PathBuf,
}

static GOV_DB_REGISTRY: Lazy<Mutex<HashMap<PathBuf, Weak<sled::Db>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn ser<T: BinaryCodec>(value: &T) -> sled::Result<Vec<u8>> {
    encode_binary(value)
}

fn de<T: BinaryCodec>(bytes: &[u8]) -> sled::Result<T> {
    decode_binary(bytes)
}

fn decode_install_times(bytes: &[u8]) -> sled::Result<Vec<u64>> {
    match de::<Vec<u64>>(bytes) {
        Ok(list) => Ok(list),
        Err(_) => de::<u64>(bytes).map(|single| vec![single]),
    }
}

fn did_revocation_to_json(record: &DidRevocationRecord) -> Value {
    let mut map = Map::new();
    map.insert("address".into(), Value::String(record.address.clone()));
    map.insert("reason".into(), Value::String(record.reason.clone()));
    map.insert("epoch".into(), Value::Number(record.epoch.into()));
    map.insert("revoked_at".into(), Value::Number(record.revoked_at.into()));
    Value::Object(map)
}

fn did_revocation_from_json(value: &Value) -> CodecResult<DidRevocationRecord> {
    let obj = value
        .as_object()
        .ok_or_else(|| sled::Error::Unsupported("did revocation JSON: expected object".into()))?;
    let address = obj
        .get("address")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("did revocation JSON: missing address".into()))?;
    let reason = obj
        .get("reason")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("did revocation JSON: missing reason".into()))?;
    let epoch = obj
        .get("epoch")
        .and_then(Value::as_u64)
        .ok_or_else(|| sled::Error::Unsupported("did revocation JSON: missing epoch".into()))?;
    let revoked_at = obj
        .get("revoked_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            sled::Error::Unsupported("did revocation JSON: missing revoked_at".into())
        })?;
    Ok(DidRevocationRecord {
        address: address.to_string(),
        reason: reason.to_string(),
        epoch,
        revoked_at,
    })
}

fn fee_floor_snapshot_to_json(snapshot: &FeeFloorPolicySnapshot) -> Value {
    let mut map = Map::new();
    map.insert("window".into(), Value::Number(snapshot.window.into()));
    map.insert(
        "percentile".into(),
        Value::Number(snapshot.percentile.into()),
    );
    Value::Object(map)
}

fn fee_floor_snapshot_from_json(value: &Value) -> CodecResult<FeeFloorPolicySnapshot> {
    let obj = value.as_object().ok_or_else(|| {
        sled::Error::Unsupported("fee floor snapshot JSON: expected object".into())
    })?;
    let window = obj.get("window").and_then(Value::as_i64).ok_or_else(|| {
        sled::Error::Unsupported("fee floor snapshot JSON: missing window".into())
    })?;
    let percentile = obj
        .get("percentile")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            sled::Error::Unsupported("fee floor snapshot JSON: missing percentile".into())
        })?;
    Ok(FeeFloorPolicySnapshot { window, percentile })
}

fn dependency_snapshot_to_json(snapshot: &DependencyPolicySnapshot) -> Value {
    let mut map = Map::new();
    map.insert("kind".into(), Value::String(snapshot.kind.clone()));
    map.insert(
        "allowed".into(),
        Value::Array(
            snapshot
                .allowed
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        ),
    );
    Value::Object(map)
}

fn dependency_snapshot_from_json(value: &Value) -> CodecResult<DependencyPolicySnapshot> {
    let obj = value.as_object().ok_or_else(|| {
        sled::Error::Unsupported("dependency snapshot JSON: expected object".into())
    })?;
    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("dependency snapshot JSON: missing kind".into()))?;
    let allowed = obj
        .get("allowed")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            sled::Error::Unsupported("dependency snapshot JSON: missing allowed".into())
        })?
        .iter()
        .map(|v| {
            v.as_str().map(|s| s.to_string()).ok_or_else(|| {
                sled::Error::Unsupported("dependency snapshot JSON: invalid allowed entry".into())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DependencyPolicySnapshot {
        kind: kind.to_string(),
        allowed,
    })
}

fn dependency_policy_record_to_json(record: &DependencyPolicyRecord) -> Value {
    let mut map = Map::new();
    map.insert("epoch".into(), Value::Number(record.epoch.into()));
    map.insert(
        "proposal_id".into(),
        Value::Number(record.proposal_id.into()),
    );
    map.insert("kind".into(), Value::String(record.kind.clone()));
    map.insert(
        "allowed".into(),
        Value::Array(
            record
                .allowed
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        ),
    );
    Value::Object(map)
}

fn reward_claim_to_json(approval: &RewardClaimApproval) -> Value {
    let mut map = Map::new();
    map.insert("key".into(), Value::String(approval.key.clone()));
    map.insert("relayer".into(), Value::String(approval.relayer.clone()));
    map.insert(
        "total_amount".into(),
        Value::Number(Number::from(approval.total_amount)),
    );
    map.insert(
        "remaining_amount".into(),
        Value::Number(Number::from(approval.remaining_amount)),
    );
    map.insert(
        "expires_at".into(),
        approval
            .expires_at
            .map(|value| Value::Number(Number::from(value)))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "memo".into(),
        approval
            .memo
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "last_claimed_at".into(),
        approval
            .last_claimed_at
            .map(|value| Value::Number(Number::from(value)))
            .unwrap_or(Value::Null),
    );
    Value::Object(map)
}

fn reward_claim_from_json(value: &Value) -> sled::Result<RewardClaimApproval> {
    let obj = value
        .as_object()
        .ok_or_else(|| sled::Error::Unsupported("reward claim JSON: expected object".into()))?;
    let key = obj
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("reward claim JSON: missing key".into()))?;
    let relayer = obj
        .get("relayer")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("reward claim JSON: missing relayer".into()))?;
    let total_amount = obj
        .get("total_amount")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            sled::Error::Unsupported("reward claim JSON: missing total_amount".into())
        })?;
    let remaining_amount = obj
        .get("remaining_amount")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            sled::Error::Unsupported("reward claim JSON: missing remaining_amount".into())
        })?;
    let expires_at = obj.get("expires_at").and_then(Value::as_u64);
    let memo = obj
        .get("memo")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let last_claimed_at = obj.get("last_claimed_at").and_then(Value::as_u64);
    Ok(RewardClaimApproval {
        key: key.to_string(),
        relayer: relayer.to_string(),
        total_amount,
        remaining_amount,
        expires_at,
        memo,
        last_claimed_at,
    })
}

fn dependency_policy_record_from_json(value: &Value) -> CodecResult<DependencyPolicyRecord> {
    let obj = value.as_object().ok_or_else(|| {
        sled::Error::Unsupported("dependency policy JSON: expected object".into())
    })?;
    let epoch = obj
        .get("epoch")
        .and_then(Value::as_u64)
        .ok_or_else(|| sled::Error::Unsupported("dependency policy JSON: missing epoch".into()))?;
    let proposal_id = obj
        .get("proposal_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            sled::Error::Unsupported("dependency policy JSON: missing proposal_id".into())
        })?;
    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("dependency policy JSON: missing kind".into()))?;
    let allowed = obj
        .get("allowed")
        .and_then(Value::as_array)
        .ok_or_else(|| sled::Error::Unsupported("dependency policy JSON: missing allowed".into()))?
        .iter()
        .map(|v| {
            v.as_str().map(|s| s.to_string()).ok_or_else(|| {
                sled::Error::Unsupported("dependency policy JSON: invalid allowed entry".into())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DependencyPolicyRecord {
        epoch,
        proposal_id,
        kind: kind.to_string(),
        allowed,
    })
}

fn fee_floor_record_to_json(record: &FeeFloorPolicyRecord) -> Value {
    let mut map = Map::new();
    map.insert("epoch".into(), Value::Number(record.epoch.into()));
    map.insert(
        "proposal_id".into(),
        Value::Number(record.proposal_id.into()),
    );
    map.insert("window".into(), Value::Number(record.window.into()));
    map.insert("percentile".into(), Value::Number(record.percentile.into()));
    Value::Object(map)
}

fn fee_floor_record_from_json(value: &Value) -> CodecResult<FeeFloorPolicyRecord> {
    let obj = value.as_object().ok_or_else(|| {
        sled::Error::Unsupported("fee floor history JSON: expected object".into())
    })?;
    let epoch = obj
        .get("epoch")
        .and_then(Value::as_u64)
        .ok_or_else(|| sled::Error::Unsupported("fee floor history JSON: missing epoch".into()))?;
    let proposal_id = obj
        .get("proposal_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            sled::Error::Unsupported("fee floor history JSON: missing proposal_id".into())
        })?;
    let window = obj
        .get("window")
        .and_then(Value::as_i64)
        .ok_or_else(|| sled::Error::Unsupported("fee floor history JSON: missing window".into()))?;
    let percentile = obj
        .get("percentile")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            sled::Error::Unsupported("fee floor history JSON: missing percentile".into())
        })?;
    Ok(FeeFloorPolicyRecord {
        epoch,
        proposal_id,
        window,
        percentile,
    })
}

fn param_change_to_json(record: &ParamChangeRecord) -> Value {
    let mut map = Map::new();
    map.insert(
        "key".into(),
        Value::String(param_key_to_string(record.key).into()),
    );
    map.insert(
        "proposal_id".into(),
        Value::Number(record.proposal_id.into()),
    );
    map.insert("epoch".into(), Value::Number(record.epoch.into()));
    map.insert("old_value".into(), Value::Number(record.old_value.into()));
    map.insert("new_value".into(), Value::Number(record.new_value.into()));
    if let Some(snapshot) = &record.fee_floor {
        map.insert("fee_floor".into(), fee_floor_snapshot_to_json(snapshot));
    }
    if let Some(snapshot) = &record.dependency_policy {
        map.insert(
            "dependency_policy".into(),
            dependency_snapshot_to_json(snapshot),
        );
    }
    Value::Object(map)
}

fn param_change_from_json(value: &Value) -> CodecResult<ParamChangeRecord> {
    let obj = value
        .as_object()
        .ok_or_else(|| sled::Error::Unsupported("param change JSON: expected object".into()))?;
    let key = obj
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| sled::Error::Unsupported("param change JSON: missing key".into()))?;
    let proposal_id = obj
        .get("proposal_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| sled::Error::Unsupported("param change JSON: missing proposal_id".into()))?;
    let epoch = obj
        .get("epoch")
        .and_then(Value::as_u64)
        .ok_or_else(|| sled::Error::Unsupported("param change JSON: missing epoch".into()))?;
    let old_value = obj
        .get("old_value")
        .and_then(Value::as_i64)
        .ok_or_else(|| sled::Error::Unsupported("param change JSON: missing old_value".into()))?;
    let new_value = obj
        .get("new_value")
        .and_then(Value::as_i64)
        .ok_or_else(|| sled::Error::Unsupported("param change JSON: missing new_value".into()))?;
    let fee_floor = obj
        .get("fee_floor")
        .map(fee_floor_snapshot_from_json)
        .transpose()?;
    let dependency_policy = obj
        .get("dependency_policy")
        .map(dependency_snapshot_from_json)
        .transpose()?;
    Ok(ParamChangeRecord {
        key: param_key_from_string(key)?,
        proposal_id,
        epoch,
        old_value,
        new_value,
        fee_floor,
        dependency_policy,
    })
}

fn load_json_array<T, F>(path: &Path, parse: F) -> Vec<T>
where
    F: Fn(&Value) -> CodecResult<T>,
{
    if let Ok(bytes) = std::fs::read(path) {
        if let Ok(value) = json_from_bytes(&bytes) {
            if let Some(array) = value.as_array() {
                let mut out = Vec::with_capacity(array.len());
                for entry in array {
                    match parse(entry) {
                        Ok(item) => out.push(item),
                        Err(_) => return Vec::new(),
                    }
                }
                return out;
            }
        }
    }
    Vec::new()
}

fn write_json_array<T, F>(path: &Path, items: &[T], render: F)
where
    F: Fn(&T) -> Value,
{
    let value = Value::Array(items.iter().map(render).collect());
    let bytes = json_to_bytes(&value);
    let _ = std::fs::write(path, bytes);
}

fn normalize_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn derive_base_path(path: &Path) -> PathBuf {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.is_dir() {
            if path.extension().is_some() {
                return path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."));
            }
            return path.to_path_buf();
        }
    }
    if path.extension().is_none() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[cfg(feature = "telemetry")]
fn key_name(k: ParamKey) -> &'static str {
    match k {
        ParamKey::SnapshotIntervalSecs => "snapshot_interval_secs",
        ParamKey::ConsumerFeeComfortP90Microunits => "consumer_fee_comfort_p90_microunits",
        ParamKey::IndustrialAdmissionMinCapacity => "industrial_admission_min_capacity",
        ParamKey::FairshareGlobalMax => "fairshare_global_max_ppm",
        ParamKey::BurstRefillRatePerS => "burst_refill_rate_per_s_ppm",
        ParamKey::BetaStorageSubCt => "beta_storage_sub_ct",
        ParamKey::GammaReadSubCt => "gamma_read_sub_ct",
        ParamKey::KappaCpuSubCt => "kappa_cpu_sub_ct",
        ParamKey::LambdaBytesOutSubCt => "lambda_bytes_out_sub_ct",
        ParamKey::ReadSubsidyViewerPercent => "read_subsidy_viewer_percent",
        ParamKey::ReadSubsidyHostPercent => "read_subsidy_host_percent",
        ParamKey::ReadSubsidyHardwarePercent => "read_subsidy_hardware_percent",
        ParamKey::ReadSubsidyVerifierPercent => "read_subsidy_verifier_percent",
        ParamKey::ReadSubsidyLiquidityPercent => "read_subsidy_liquidity_percent",
        ParamKey::AdReadinessWindowSecs => "ad_readiness_window_secs",
        ParamKey::AdReadinessMinUniqueViewers => "ad_readiness_min_unique_viewers",
        ParamKey::AdReadinessMinHostCount => "ad_readiness_min_host_count",
        ParamKey::AdReadinessMinProviderCount => "ad_readiness_min_provider_count",
        ParamKey::TreasuryPercentCt => "treasury_percent_ct",
        ParamKey::ProofRebateLimitCt => "proof_rebate_limit_ct",
        ParamKey::RentRateCtPerByte => "rent_rate_ct_per_byte",
        ParamKey::KillSwitchSubsidyReduction => "kill_switch_subsidy_reduction",
        ParamKey::MinerRewardLogisticTarget => "miner_reward_logistic_target",
        ParamKey::LogisticSlope => "logistic_slope_milli",
        ParamKey::MinerHysteresis => "miner_hysteresis",
        ParamKey::HeuristicMuMilli => "heuristic_mu_milli",
        ParamKey::FeeFloorWindow => "fee_floor_window",
        ParamKey::FeeFloorPercentile => "fee_floor_percentile",
        ParamKey::BadgeExpirySecs => "badge_expiry_secs",
        ParamKey::BadgeIssueUptime => "badge_issue_uptime_percent",
        ParamKey::BadgeRevokeUptime => "badge_revoke_uptime_percent",
        ParamKey::JurisdictionRegion => "jurisdiction_region",
        ParamKey::AiDiagnosticsEnabled => "ai_diagnostics_enabled",
        ParamKey::KalmanRShort => "kalman_r_short",
        ParamKey::KalmanRMed => "kalman_r_med",
        ParamKey::KalmanRLong => "kalman_r_long",
        ParamKey::SchedulerWeightGossip => "scheduler_weight_gossip",
        ParamKey::SchedulerWeightCompute => "scheduler_weight_compute",
        ParamKey::SchedulerWeightStorage => "scheduler_weight_storage",
        ParamKey::RuntimeBackend => "runtime_backend_policy",
        ParamKey::TransportProvider => "transport_provider_policy",
        ParamKey::StorageEnginePolicy => "storage_engine_policy",
        ParamKey::BridgeMinBond => "bridge_min_bond",
        ParamKey::BridgeDutyReward => "bridge_duty_reward",
        ParamKey::BridgeFailureSlash => "bridge_failure_slash",
        ParamKey::BridgeChallengeSlash => "bridge_challenge_slash",
        ParamKey::BridgeDutyWindowSecs => "bridge_duty_window_secs",
    }
}

impl GovStore {
    fn did_revocations(&self) -> sled::Tree {
        self.db
            .open_tree("did_revocations")
            .unwrap_or_else(|e| panic!("open did revocation tree: {e}"))
    }

    fn persist_did_revocation(&self, record: &DidRevocationRecord) {
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let path = hist_dir.join("did_revocations.json");
        let mut history = load_json_array(&path, did_revocation_from_json);
        history.push(record.clone());
        if history.len() > DID_REVOCATION_HISTORY_LIMIT {
            history.drain(0..history.len() - DID_REVOCATION_HISTORY_LIMIT);
        }
        write_json_array(&path, &history, did_revocation_to_json);
    }

    fn treasury_disbursement_path(&self) -> PathBuf {
        self.base_path
            .join("governance")
            .join("treasury_disbursements.json")
    }

    fn treasury_balance_path(&self) -> PathBuf {
        self.base_path
            .join("governance")
            .join("treasury_balance.json")
    }

    fn treasury_disbursements_tree(&self) -> sled::Tree {
        self.db
            .open_tree("treasury/disbursements")
            .unwrap_or_else(|e| panic!("open treasury disbursements tree: {e}"))
    }

    fn treasury_balance_tree(&self) -> sled::Tree {
        self.db
            .open_tree("treasury/balance_state")
            .unwrap_or_else(|e| panic!("open treasury balance tree: {e}"))
    }

    fn treasury_balance_history_tree(&self) -> sled::Tree {
        self.db
            .open_tree("treasury/balance_history")
            .unwrap_or_else(|e| panic!("open treasury balance history tree: {e}"))
    }

    fn load_disbursements(&self) -> sled::Result<Vec<TreasuryDisbursement>> {
        let tree = self.treasury_disbursements_tree();
        let mut from_tree = Vec::new();
        for item in tree.iter() {
            let (_, raw) = item?;
            let record: TreasuryDisbursement = de(&raw)?;
            from_tree.push(record);
        }
        if !from_tree.is_empty() {
            from_tree.sort_by_key(|record| record.id);
            return Ok(from_tree);
        }

        let path = self.treasury_disbursement_path();
        match std::fs::read(&path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    Ok(Vec::new())
                } else {
                    let value = json_from_bytes(&bytes).map_err(|e| {
                        sled::Error::Unsupported(
                            format!("decode treasury disbursements: {e}").into(),
                        )
                    })?;
                    let mut decoded = disbursements_from_json_array(&value)?;
                    decoded.sort_by_key(|record| record.id);
                    if !decoded.is_empty() {
                        let _ = self.persist_disbursements(&decoded);
                    }
                    Ok(decoded)
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(sled::Error::Unsupported(
                format!("read treasury disbursements: {err}").into(),
            )),
        }
    }

    fn persist_disbursements(&self, records: &[TreasuryDisbursement]) -> sled::Result<()> {
        let mut trimmed = records.to_vec();
        trimmed.sort_by_key(|record| record.id);
        if trimmed.len() > TREASURY_HISTORY_LIMIT {
            let drop = trimmed.len() - TREASURY_HISTORY_LIMIT;
            trimmed.drain(0..drop);
        }

        let tree = self.treasury_disbursements_tree();
        let mut existing: Vec<Vec<u8>> = Vec::new();
        for entry in tree.iter() {
            let (k, _) = entry?;
            existing.push(k.to_vec());
        }

        for record in &trimmed {
            let key = ser(&record.id)?;
            tree.insert(&key, ser(record)?)?;
            if let Some(pos) = existing.iter().position(|candidate| candidate == &key) {
                existing.swap_remove(pos);
            }
        }

        for key in existing {
            tree.remove(key)?;
        }

        let value = disbursements_to_json_array(&trimmed);
        let bytes = json_to_bytes(&value);
        let path = self.treasury_disbursement_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, bytes).map_err(|e| {
            sled::Error::Unsupported(format!("write treasury disbursements: {e}").into())
        })
    }

    fn load_balance_history(&self) -> sled::Result<Vec<TreasuryBalanceSnapshot>> {
        let tree = self.treasury_balance_history_tree();
        let mut history = Vec::new();
        for item in tree.iter() {
            let (_, raw) = item?;
            let snapshot: TreasuryBalanceSnapshot = de(&raw)?;
            history.push(snapshot);
        }
        if !history.is_empty() {
            history.sort_by_key(|snap| snap.id);
            return Ok(history);
        }

        let path = self.treasury_balance_path();
        match std::fs::read(&path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    Ok(Vec::new())
                } else {
                    let value = json_from_bytes(&bytes).map_err(|e| {
                        sled::Error::Unsupported(
                            format!("decode treasury balance history: {e}").into(),
                        )
                    })?;
                    let mut decoded = balance_history_from_json(&value)?;
                    decoded.sort_by_key(|snap| snap.id);
                    if !decoded.is_empty() {
                        let _ = self.persist_balance_history(&decoded);
                    }
                    Ok(decoded)
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(sled::Error::Unsupported(
                format!("read treasury balance history: {err}").into(),
            )),
        }
    }

    fn persist_balance_history(&self, history: &[TreasuryBalanceSnapshot]) -> sled::Result<()> {
        let mut trimmed = history.to_vec();
        trimmed.sort_by_key(|snap| snap.id);
        if trimmed.len() > TREASURY_BALANCE_HISTORY_LIMIT {
            let drop = trimmed.len() - TREASURY_BALANCE_HISTORY_LIMIT;
            trimmed.drain(0..drop);
        }

        let tree = self.treasury_balance_history_tree();
        let mut existing: Vec<Vec<u8>> = Vec::new();
        for item in tree.iter() {
            let (k, _) = item?;
            existing.push(k.to_vec());
        }

        for snap in &trimmed {
            let key = ser(&snap.id)?;
            tree.insert(&key, ser(snap)?)?;
            if let Some(pos) = existing.iter().position(|candidate| candidate == &key) {
                existing.swap_remove(pos);
            }
        }

        for key in existing {
            tree.remove(key)?;
        }

        let value = balance_history_to_json(&trimmed);
        let bytes = json_to_bytes(&value);
        let path = self.treasury_balance_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, bytes).map_err(|e| {
            sled::Error::Unsupported(format!("write treasury balance history: {e}").into())
        })?;

        let state = self.treasury_balance_tree();
        if let Some(last) = trimmed.last() {
            state.insert(b"current", ser(&last.balance_ct)?)?;
            state.insert(b"next_snapshot_id", ser(&(last.id.saturating_add(1)))?)?;
        } else {
            state.insert(b"current", ser(&0u64)?)?;
            state.insert(b"next_snapshot_id", ser(&1u64)?)?;
        }
        Ok(())
    }

    fn next_balance_snapshot_id(&self) -> sled::Result<u64> {
        let state = self.treasury_balance_tree();
        let next = state
            .get(b"next_snapshot_id")?
            .map(|raw| de::<u64>(&raw))
            .transpose()?;
        let id = next.unwrap_or(1);
        state.insert(b"next_snapshot_id", ser(&(id.saturating_add(1)))?)?;
        Ok(id)
    }

    fn record_balance_event(
        &self,
        event: TreasuryBalanceEventKind,
        disbursement_id: Option<u64>,
        delta_ct: i64,
    ) -> sled::Result<TreasuryBalanceSnapshot> {
        let current = self.treasury_balance()? as i128;
        let updated = current + i128::from(delta_ct);
        if updated < 0 {
            return Err(sled::Error::Unsupported(
                "treasury balance underflow".into(),
            ));
        }
        let id = self.next_balance_snapshot_id()?;
        let snapshot =
            TreasuryBalanceSnapshot::new(id, updated as u64, delta_ct, event, disbursement_id);
        let mut history = self.load_balance_history()?;
        history.push(snapshot.clone());
        self.persist_balance_history(&history)?;
        Ok(snapshot)
    }

    fn persist_fee_floor_policy(
        &self,
        hist_dir: &Path,
        epoch: u64,
        proposal_id: u64,
        snapshot: FeeFloorPolicySnapshot,
    ) {
        let path = hist_dir.join("fee_floor_policy.json");
        let mut history = load_json_array(&path, fee_floor_record_from_json);
        history.push(FeeFloorPolicyRecord {
            epoch,
            proposal_id,
            window: snapshot.window,
            percentile: snapshot.percentile,
        });
        if history.len() > PARAM_HISTORY_LIMIT {
            history.drain(0..history.len() - PARAM_HISTORY_LIMIT);
        }
        write_json_array(&path, &history, fee_floor_record_to_json);
    }

    fn persist_dependency_policy(
        &self,
        hist_dir: &Path,
        epoch: u64,
        proposal_id: u64,
        snapshot: &DependencyPolicySnapshot,
    ) {
        let path = hist_dir.join("dependency_policy.json");
        let mut history = load_json_array(&path, dependency_policy_record_from_json);
        history.push(DependencyPolicyRecord {
            epoch,
            proposal_id,
            kind: snapshot.kind.clone(),
            allowed: snapshot.allowed.clone(),
        });
        if history.len() > PARAM_HISTORY_LIMIT {
            history.drain(0..history.len() - PARAM_HISTORY_LIMIT);
        }
        write_json_array(&path, &history, dependency_policy_record_to_json);
    }

    fn persist_param_change(
        &self,
        hist_dir: &Path,
        key: ParamKey,
        proposal_id: u64,
        old_value: i64,
        new_value: i64,
        epoch: u64,
        params: &Params,
    ) {
        let fee_snapshot = if matches!(key, ParamKey::FeeFloorWindow | ParamKey::FeeFloorPercentile)
        {
            Some(FeeFloorPolicySnapshot {
                window: params.fee_floor_window,
                percentile: params.fee_floor_percentile,
            })
        } else {
            None
        };

        let dependency_snapshot = match key {
            ParamKey::RuntimeBackend => Some(DependencyPolicySnapshot {
                kind: "runtime_backend".to_string(),
                allowed: decode_runtime_backend_policy(params.runtime_backend_policy),
            }),
            ParamKey::TransportProvider => Some(DependencyPolicySnapshot {
                kind: "transport_provider".to_string(),
                allowed: decode_transport_provider_policy(params.transport_provider_policy),
            }),
            ParamKey::StorageEnginePolicy => Some(DependencyPolicySnapshot {
                kind: "storage_engine".to_string(),
                allowed: decode_storage_engine_policy(params.storage_engine_policy),
            }),
            _ => None,
        };

        let record = ParamChangeRecord {
            key,
            proposal_id,
            epoch,
            old_value,
            new_value,
            fee_floor: fee_snapshot.clone(),
            dependency_policy: dependency_snapshot.clone(),
        };

        let path = hist_dir.join("param_changes.json");
        let mut history = load_json_array(&path, param_change_from_json);
        history.push(record);
        if history.len() > PARAM_HISTORY_LIMIT {
            history.drain(0..history.len() - PARAM_HISTORY_LIMIT);
        }
        write_json_array(&path, &history, param_change_to_json);

        if let Some(snapshot) = fee_snapshot {
            self.persist_fee_floor_policy(hist_dir, epoch, proposal_id, snapshot);
        }

        if let Some(snapshot) = dependency_snapshot {
            self.persist_dependency_policy(hist_dir, epoch, proposal_id, &snapshot);
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Self {
        let normalized = normalize_path(path.as_ref());
        let mut registry = GOV_DB_REGISTRY.lock().unwrap();
        if let Some(existing) = registry.get(&normalized) {
            if let Some(db) = existing.upgrade() {
                let base = derive_base_path(&normalized);
                drop(registry);
                return Self {
                    db,
                    base_path: base,
                };
            }
        }
        registry.remove(&normalized);
        let db_handle = Config::new()
            .path(&normalized)
            .open()
            .unwrap_or_else(|e| panic!("open db: {e}"));
        let db = Arc::new(db_handle);
        registry.insert(normalized.clone(), Arc::downgrade(&db));
        drop(registry);
        let base = derive_base_path(&normalized);
        Self {
            db,
            base_path: base,
        }
    }

    /// Record a DID revocation enforced by governance.
    pub fn revoke_did(&self, address: &str, reason: &str, epoch: u64) -> sled::Result<()> {
        let mut rec = DidRevocationRecord {
            address: address.to_string(),
            reason: reason.to_string(),
            epoch,
            revoked_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let bytes = ser(&rec)?;
        self.did_revocations().insert(address.as_bytes(), bytes)?;
        self.persist_did_revocation(&rec);
        rec.reason.shrink_to_fit();
        Ok(())
    }

    /// Clear a previously recorded DID revocation.
    pub fn clear_did_revocation(&self, address: &str) -> sled::Result<()> {
        self.did_revocations().remove(address.as_bytes())?;
        Ok(())
    }

    /// Determine whether a DID is currently revoked.
    pub fn is_did_revoked(&self, address: &str) -> bool {
        self.did_revocations()
            .get(address.as_bytes())
            .ok()
            .flatten()
            .is_some()
    }

    /// Retrieve recorded DID revocation history for monitoring and explorer use.
    pub fn did_revocation_history(&self) -> sled::Result<Vec<DidRevocationRecord>> {
        let hist_dir = self.base_path.join("governance/history");
        let path = hist_dir.join("did_revocations.json");
        Ok(load_json_array(&path, did_revocation_from_json))
    }

    pub fn dependency_policy_history(&self) -> sled::Result<Vec<DependencyPolicyRecord>> {
        let hist_dir = self.base_path.join("governance/history");
        let path = hist_dir.join("dependency_policy.json");
        Ok(load_json_array(&path, dependency_policy_record_from_json))
    }

    pub fn proposals(&self) -> sled::Tree {
        self.db
            .open_tree("proposals")
            .unwrap_or_else(|e| panic!("open proposals tree: {e}"))
    }
    fn votes(&self, id: u64) -> sled::Tree {
        self.db
            .open_tree(format!("votes/{id}"))
            .unwrap_or_else(|e| panic!("open votes tree: {e}"))
    }
    fn next_id(&self) -> sled::Tree {
        self.db
            .open_tree("next_id")
            .unwrap_or_else(|e| panic!("open next_id tree: {e}"))
    }
    fn active_params(&self) -> sled::Tree {
        self.db
            .open_tree("active_params")
            .unwrap_or_else(|e| panic!("open active_params tree: {e}"))
    }
    fn activation_queue(&self) -> sled::Tree {
        self.db
            .open_tree("activation_queue")
            .unwrap_or_else(|e| panic!("open activation_queue tree: {e}"))
    }
    fn last_activation(&self) -> sled::Tree {
        self.db
            .open_tree("last_activation")
            .unwrap_or_else(|e| panic!("open last_activation tree: {e}"))
    }

    fn release_proposals(&self) -> sled::Tree {
        self.db
            .open_tree("release_proposals")
            .unwrap_or_else(|e| panic!("open release_proposals tree: {e}"))
    }

    fn release_votes(&self, id: u64) -> sled::Tree {
        self.db
            .open_tree(format!("release_votes/{id}"))
            .unwrap_or_else(|e| panic!("open release_votes tree: {e}"))
    }

    fn release_next_id(&self) -> sled::Tree {
        self.db
            .open_tree("release_next_id")
            .unwrap_or_else(|e| panic!("open release_next_id tree: {e}"))
    }

    fn approved_releases(&self) -> sled::Tree {
        self.db
            .open_tree("approved_releases")
            .unwrap_or_else(|e| panic!("open approved_releases tree: {e}"))
    }

    fn release_installs(&self) -> sled::Tree {
        self.db
            .open_tree("release_installs")
            .unwrap_or_else(|e| panic!("open release_installs tree: {e}"))
    }

    fn reward_claims(&self) -> sled::Tree {
        self.db
            .open_tree("reward_claim_approvals")
            .unwrap_or_else(|e| panic!("open reward_claim_approvals tree: {e}"))
    }

    pub fn submit(&self, mut p: Proposal) -> sled::Result<u64> {
        if p.new_value < p.min || p.new_value > p.max {
            return Err(sled::Error::Unsupported("out of bounds".into()));
        }
        // Ensure dependencies exist and graph remains acyclic
        for dep in &p.deps {
            if self.proposals().get(ser(dep)?)?.is_none() {
                return Err(sled::Error::Unsupported("missing dependency".into()));
            }
        }
        let next = self
            .next_id()
            .get("id")?
            .map(|v| de::<u64>(&v))
            .transpose()?
            .unwrap_or(0);
        self.next_id().insert("id", ser(&(next + 1))?)?;
        p.id = next;
        // collect existing proposals for cycle detection
        let mut existing = std::collections::HashMap::new();
        for item in self.proposals().iter() {
            let (k, v) = item?;
            let id: u64 = de(&k)?;
            let prop: Proposal = de(&v)?;
            existing.insert(id, prop);
        }
        if !super::validate_dag(&existing, &p) {
            return Err(sled::Error::Unsupported("cycle".into()));
        }
        self.proposals().insert(ser(&p.id)?, ser(&p)?)?;
        #[cfg(feature = "telemetry")]
        self.update_pending_gauge()?;
        Ok(next)
    }

    pub fn submit_release(&self, mut r: ReleaseVote) -> sled::Result<u64> {
        if r.build_hash.len() != 64 || !r.build_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(sled::Error::Unsupported("invalid release hash".into()));
        }
        if self.is_release_hash_known(&r.build_hash)? {
            return Err(sled::Error::Unsupported(
                "release hash already known".into(),
            ));
        }
        if r.signer_set.is_empty() {
            r.signer_set = crate::provenance::release_signer_hexes();
        }
        if r.signature_threshold == 0 && !r.signer_set.is_empty() {
            r.signature_threshold = r.signer_set.len() as u32;
        }
        if !r.signer_set.is_empty() && r.signature_threshold as usize > r.signer_set.len() {
            return Err(sled::Error::Unsupported(
                "threshold exceeds signer set".into(),
            ));
        }
        let next = self
            .release_next_id()
            .get("id")?
            .map(|v| de::<u64>(&v))
            .transpose()?
            .unwrap_or(0);
        self.release_next_id().insert("id", ser(&(next + 1))?)?;
        r.id = next;
        self.release_proposals().insert(ser(&r.id)?, ser(&r)?)?;
        Ok(next)
    }

    pub fn record_reward_claim(&self, approval: &RewardClaimApproval) -> sled::Result<()> {
        let tree = self.reward_claims();
        let value = reward_claim_to_json(approval);
        let bytes = json_to_bytes(&value);
        tree.insert(approval.key.as_bytes(), bytes)?;
        tree.flush()?;
        Ok(())
    }

    pub fn reward_claim(&self, key: &str) -> sled::Result<Option<RewardClaimApproval>> {
        let tree = self.reward_claims();
        if let Some(raw) = tree.get(key.as_bytes())? {
            let value = json_from_bytes(&raw).map_err(|e| {
                sled::Error::Unsupported(format!("decode reward claim {key}: {e}").into())
            })?;
            let approval = reward_claim_from_json(&value).map_err(|e| {
                sled::Error::Unsupported(format!("decode reward claim {key}: {e}").into())
            })?;
            Ok(Some(approval))
        } else {
            Ok(None)
        }
    }

    pub fn consume_reward_claim(
        &self,
        key: &str,
        relayer: &str,
        amount: u64,
    ) -> sled::Result<RewardClaimApproval> {
        if amount == 0 {
            return Err(sled::Error::Unsupported(
                "reward claim amount must be non-zero".into(),
            ));
        }
        let mut approval = self.reward_claim(key)?.ok_or_else(|| {
            sled::Error::Unsupported(format!("reward claim {key} is not approved").into())
        })?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if approval.is_expired(now) {
            return Err(sled::Error::Unsupported(
                format!("reward claim {key} has expired").into(),
            ));
        }
        if approval.relayer != relayer {
            return Err(sled::Error::Unsupported(
                format!(
                    "reward claim {key} is bound to relayer {}",
                    approval.relayer
                )
                .into(),
            ));
        }
        if approval.remaining_amount < amount {
            return Err(sled::Error::Unsupported(
                format!(
                    "reward claim {key} insufficient allowance: remaining {}, requested {}",
                    approval.remaining_amount, amount
                )
                .into(),
            ));
        }
        approval.remaining_amount -= amount;
        approval.last_claimed_at = Some(now);
        let tree = self.reward_claims();
        if approval.remaining_amount == 0 {
            tree.remove(key.as_bytes())?;
        } else {
            let value = reward_claim_to_json(&approval);
            let bytes = json_to_bytes(&value);
            tree.insert(key.as_bytes(), bytes)?;
        }
        tree.flush()?;
        Ok(approval)
    }

    pub fn reward_claims_snapshot(&self) -> sled::Result<Vec<RewardClaimApproval>> {
        let tree = self.reward_claims();
        let mut approvals = Vec::new();
        for item in tree.iter() {
            let (key, raw) = item?;
            let value = json_from_bytes(&raw).map_err(|e| {
                let key_str = String::from_utf8_lossy(&key);
                sled::Error::Unsupported(format!("decode reward claim {key_str}: {e}").into())
            })?;
            let approval = reward_claim_from_json(&value).map_err(|e| {
                let key_str = String::from_utf8_lossy(&key);
                sled::Error::Unsupported(format!("decode reward claim {key_str}: {e}").into())
            })?;
            approvals.push(approval);
        }
        approvals.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(approvals)
    }

    fn is_release_hash_known(&self, hash: &str) -> sled::Result<bool> {
        if self.approved_releases().get(hash.as_bytes())?.is_some() {
            return Ok(true);
        }
        for item in self.release_proposals().iter() {
            let (_, v) = item?;
            let prop: ReleaseVote = de(&v)?;
            if prop.build_hash == hash {
                return Ok(true);
            }
        }
        Ok(false)
    }

    #[cfg(feature = "telemetry")]
    fn update_pending_gauge(&self) -> sled::Result<()> {
        let mut pending = 0i64;
        for item in self.proposals().iter() {
            let (_, v) = item?;
            let prop: Proposal = de(&v)?;
            if prop.status == ProposalStatus::Open || prop.status == ProposalStatus::Passed {
                pending += 1;
            }
        }
        GOV_PROPOSALS_PENDING.set(pending);
        Ok(())
    }

    pub fn vote(&self, proposal_id: u64, mut v: Vote, current_epoch: u64) -> sled::Result<()> {
        let prop_raw = self
            .proposals()
            .get(ser(&proposal_id)?)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?;
        let prop: Proposal = de(&prop_raw)?;
        if current_epoch >= prop.vote_deadline_epoch {
            return Err(sled::Error::Unsupported("deadline".into()));
        }
        v.received_at = current_epoch;
        self.votes(proposal_id)
            .insert(v.voter.as_bytes(), ser(&v)?)?;
        #[cfg(feature = "telemetry")]
        {
            let choice = match v.choice {
                VoteChoice::Yes => "yes",
                VoteChoice::No => "no",
                VoteChoice::Abstain => "abstain",
            };
            GOV_VOTES_TOTAL
                .ensure_handle_for_label_values(&[choice])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc();
            governance_webhook("vote", proposal_id);
        }
        Ok(())
    }

    pub fn vote_release(&self, proposal_id: u64, mut v: ReleaseBallot) -> sled::Result<()> {
        let prop_key = ser(&proposal_id)?;
        let prop_raw = self
            .release_proposals()
            .get(&prop_key)?
            .ok_or_else(|| sled::Error::Unsupported("missing release proposal".into()))?;
        let prop: ReleaseVote = de(&prop_raw)?;
        if v.received_at > prop.vote_deadline_epoch {
            return Err(sled::Error::Unsupported("deadline".into()));
        }
        v.proposal_id = proposal_id;
        self.release_votes(proposal_id)
            .insert(v.voter.as_bytes(), ser(&v)?)?;
        #[cfg(feature = "telemetry")]
        {
            use crate::telemetry::RELEASE_VOTES_TOTAL;
            let label = match v.choice {
                VoteChoice::Yes => "yes",
                VoteChoice::No => "no",
                VoteChoice::Abstain => "abstain",
            };
            RELEASE_VOTES_TOTAL
                .ensure_handle_for_label_values(&[label])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc_by(v.weight);
        }
        Ok(())
    }

    pub fn tally_and_queue(
        &self,
        proposal_id: u64,
        current_epoch: u64,
    ) -> sled::Result<ProposalStatus> {
        let key = ser(&proposal_id)?;
        let mut prop: Proposal = de(&self
            .proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?)?;
        if prop.status != ProposalStatus::Open {
            return Ok(prop.status);
        }
        if current_epoch < prop.vote_deadline_epoch {
            return Ok(ProposalStatus::Open);
        }
        let votes = self.votes(proposal_id);
        let mut yes = 0u64;
        let mut no = 0u64;
        for v in votes.iter() {
            let (_, raw) = v?;
            let vote: Vote = de(&raw)?;
            match vote.choice {
                VoteChoice::Yes => yes += vote.weight,
                VoteChoice::No => no += vote.weight,
                _ => {}
            }
        }
        if yes >= QUORUM && yes > no {
            prop.status = ProposalStatus::Passed;
            let spec = registry()
                .iter()
                .find(|s| s.key == prop.key)
                .expect("param spec");
            let delay = if spec.timelock_epochs > 0 {
                spec.timelock_epochs
            } else {
                ACTIVATION_DELAY
            };
            let act_epoch = current_epoch + delay;
            prop.activation_epoch = Some(act_epoch);
            let key_epoch = ser(&act_epoch)?;
            let mut list: Vec<u64> = self
                .activation_queue()
                .get(&key_epoch)?
                .map(|v| de(&v))
                .transpose()?
                .unwrap_or_else(|| vec![]);
            list.push(proposal_id);
            self.activation_queue().insert(key_epoch, ser(&list)?)?;
            #[cfg(feature = "telemetry")]
            {
                PARAM_CHANGE_PENDING
                    .ensure_handle_for_label_values(&[key_name(prop.key)])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .set(1);
            }
        } else {
            prop.status = ProposalStatus::Rejected;
            #[cfg(feature = "telemetry")]
            {
                PARAM_CHANGE_PENDING
                    .ensure_handle_for_label_values(&[key_name(prop.key)])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .set(0);
            }
        }
        self.proposals().insert(&key, ser(&prop)?)?;
        #[cfg(feature = "telemetry")]
        self.update_pending_gauge()?;
        Ok(prop.status)
    }

    pub fn tally_release(
        &self,
        proposal_id: u64,
        current_epoch: u64,
    ) -> sled::Result<ProposalStatus> {
        let key = ser(&proposal_id)?;
        let mut prop: ReleaseVote = de(&self
            .release_proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing release proposal".into()))?)?;
        if !prop.is_open() {
            return Ok(prop.status);
        }
        if current_epoch < prop.vote_deadline_epoch {
            return Ok(prop.status);
        }
        let mut yes = 0u64;
        let mut no = 0u64;
        for item in self.release_votes(proposal_id).iter() {
            let (_, raw) = item?;
            let vote: ReleaseBallot = de(&raw)?;
            match vote.choice {
                VoteChoice::Yes => yes += vote.weight,
                VoteChoice::No => no += vote.weight,
                VoteChoice::Abstain => {}
            }
        }
        if ReleaseVote::quorum_met(yes) && yes >= no {
            prop.mark_passed(current_epoch);
            prop.mark_activated(current_epoch);
            self.release_proposals().insert(&key, ser(&prop)?)?;
            let installs: Vec<u64> = self
                .release_installs()
                .get(prop.build_hash.as_bytes())?
                .map(|raw| decode_install_times(&raw))
                .transpose()?
                .unwrap_or_default();
            let record = ApprovedRelease {
                build_hash: prop.build_hash.clone(),
                activated_epoch: current_epoch,
                proposer: prop.proposer.clone(),
                signatures: prop.signatures.clone(),
                signature_threshold: prop.signature_threshold,
                signer_set: prop.signer_set.clone(),
                install_times: installs,
            };
            self.approved_releases()
                .insert(prop.build_hash.as_bytes(), ser(&record)?)?;
            Ok(ProposalStatus::Activated)
        } else if ReleaseVote::quorum_met(no) && no > yes {
            prop.mark_rejected();
            self.release_proposals().insert(&key, ser(&prop)?)?;
            Ok(ProposalStatus::Rejected)
        } else {
            Ok(prop.status)
        }
    }

    pub fn approved_release_hashes(&self) -> sled::Result<Vec<ApprovedRelease>> {
        let mut installs: std::collections::HashMap<String, Vec<u64>> =
            std::collections::HashMap::new();
        for item in self.release_installs().iter() {
            let (hash_bytes, ts_bytes) = item?;
            let hash = String::from_utf8(hash_bytes.to_vec())
                .map_err(|e| sled::Error::Unsupported(format!("utf8: {e}").into()))?;
            let times: Vec<u64> = decode_install_times(&ts_bytes)?;
            installs.insert(hash, times);
        }
        let mut out = Vec::new();
        for item in self.approved_releases().iter() {
            let (_, raw) = item?;
            let mut record: ApprovedRelease = de(&raw)?;
            if let Some(times) = installs.get(&record.build_hash) {
                record.install_times = times.clone();
            }
            out.push(record);
        }
        Ok(out)
    }

    pub fn is_release_hash_approved(&self, hash: &str) -> sled::Result<bool> {
        Ok(self.approved_releases().get(hash.as_bytes())?.is_some())
    }

    pub fn record_release_install(&self, hash: &str) -> sled::Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut installs: Vec<u64> = self
            .release_installs()
            .get(hash.as_bytes())?
            .map(|raw| decode_install_times(&raw))
            .transpose()?
            .unwrap_or_default();
        installs.push(now);
        installs.sort_unstable();
        self.release_installs()
            .insert(hash.as_bytes(), ser(&installs)?)?;
        if let Some(existing) = self
            .approved_releases()
            .get(hash.as_bytes())?
            .map(|raw| de::<ApprovedRelease>(&raw))
            .transpose()?
        {
            let mut updated = existing;
            updated.install_times = installs.clone();
            self.approved_releases()
                .insert(hash.as_bytes(), ser(&updated)?)?;
        }
        #[cfg(feature = "telemetry")]
        {
            use crate::telemetry::RELEASE_INSTALLS_TOTAL;
            RELEASE_INSTALLS_TOTAL.inc();
        }
        Ok(())
    }

    pub fn release_installations(&self) -> sled::Result<Vec<(String, Vec<u64>)>> {
        let mut installs = Vec::new();
        for item in self.release_installs().iter() {
            let (hash_bytes, ts_bytes) = item?;
            let hash = String::from_utf8(hash_bytes.to_vec())
                .map_err(|e| sled::Error::Unsupported(format!("utf8: {e}").into()))?;
            let ts: Vec<u64> = decode_install_times(&ts_bytes)?;
            installs.push((hash, ts));
        }
        Ok(installs)
    }

    pub fn treasury_balance(&self) -> sled::Result<u64> {
        let state = self.treasury_balance_tree();
        if let Some(raw) = state.get(b"current")? {
            return de(&raw);
        }
        let history = self.load_balance_history()?;
        let balance = history.last().map(|snap| snap.balance_ct).unwrap_or(0);
        state.insert(b"current", ser(&balance)?)?;
        if state.get(b"next_snapshot_id")?.is_none() {
            state.insert(b"next_snapshot_id", ser(&1u64)?)?;
        }
        Ok(balance)
    }

    pub fn treasury_balance_history(&self) -> sled::Result<Vec<TreasuryBalanceSnapshot>> {
        self.load_balance_history()
    }

    pub fn record_treasury_accrual(&self, amount_ct: u64) -> sled::Result<TreasuryBalanceSnapshot> {
        if amount_ct == 0 {
            return self.record_balance_event(TreasuryBalanceEventKind::Accrual, None, 0);
        }
        self.record_balance_event(TreasuryBalanceEventKind::Accrual, None, amount_ct as i64)
    }

    pub fn queue_disbursement(
        &self,
        destination: &str,
        amount_ct: u64,
        memo: &str,
        scheduled_epoch: u64,
    ) -> sled::Result<TreasuryDisbursement> {
        let mut records = self.load_disbursements()?;
        let next_id = records
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let record = TreasuryDisbursement::new(
            next_id,
            destination.to_string(),
            amount_ct,
            memo.to_string(),
            scheduled_epoch,
        );
        records.push(record.clone());
        self.persist_disbursements(&records)?;
        self.record_balance_event(TreasuryBalanceEventKind::Queued, Some(record.id), 0)?;
        Ok(record)
    }

    pub fn disbursements(&self) -> sled::Result<Vec<TreasuryDisbursement>> {
        self.load_disbursements()
    }

    pub fn execute_disbursement(
        &self,
        id: u64,
        tx_hash: &str,
    ) -> sled::Result<TreasuryDisbursement> {
        let mut records = self.load_disbursements()?;
        let mut record = None;
        for entry in records.iter_mut() {
            if entry.id == id {
                let balance = self.treasury_balance()?;
                if balance < entry.amount_ct {
                    return Err(sled::Error::Unsupported(
                        format!("treasury balance insufficient for disbursement {id}").into(),
                    ));
                }
                mark_executed(entry, tx_hash.to_string());
                record = Some(entry.clone());
                break;
            }
        }
        if let Some(updated) = record.clone() {
            self.persist_disbursements(&records)?;
            self.record_balance_event(
                TreasuryBalanceEventKind::Executed,
                Some(updated.id),
                -(updated.amount_ct as i64),
            )?;
            Ok(updated)
        } else {
            Err(sled::Error::Unsupported(
                format!("unknown treasury disbursement id {id}").into(),
            ))
        }
    }

    pub fn cancel_disbursement(&self, id: u64, reason: &str) -> sled::Result<TreasuryDisbursement> {
        let mut records = self.load_disbursements()?;
        let mut record = None;
        for entry in records.iter_mut() {
            if entry.id == id {
                mark_cancelled(entry, reason.to_string());
                record = Some(entry.clone());
                break;
            }
        }
        if let Some(updated) = record.clone() {
            self.persist_disbursements(&records)?;
            self.record_balance_event(TreasuryBalanceEventKind::Cancelled, Some(updated.id), 0)?;
            Ok(updated)
        } else {
            Err(sled::Error::Unsupported(
                format!("unknown treasury disbursement id {id}").into(),
            ))
        }
    }

    pub fn activate_ready(
        &self,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        // snapshot current params before applying any changes
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let snap_path = hist_dir.join(format!("{}.json", current_epoch));
        if let Ok(value) = params.to_value() {
            let bytes = json_to_bytes(&value);
            let _ = std::fs::write(&snap_path, bytes);
        }

        let queue = self.activation_queue();
        let mut to_remove = vec![];
        for item in queue.iter() {
            let (k, v) = item?;
            let epoch: u64 = de(&k)?;
            if epoch <= current_epoch {
                let ids: Vec<u64> = de(&v).unwrap_or_else(|_| vec![]);
                for prop_id in ids {
                    let key = ser(&prop_id)?;
                    if let Some(raw) = self.proposals().get(&key)? {
                        let mut prop: Proposal = de(&raw)?;
                        if prop.status == ProposalStatus::Passed {
                            let old = match prop.key {
                                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                                ParamKey::ConsumerFeeComfortP90Microunits => {
                                    params.consumer_fee_comfort_p90_microunits
                                }
                                ParamKey::IndustrialAdmissionMinCapacity => {
                                    params.industrial_admission_min_capacity
                                }
                                ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
                                ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
                                ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
                                ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
                                ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
                                ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
                                ParamKey::ReadSubsidyViewerPercent => {
                                    params.read_subsidy_viewer_percent
                                }
                                ParamKey::ReadSubsidyHostPercent => {
                                    params.read_subsidy_host_percent
                                }
                                ParamKey::ReadSubsidyHardwarePercent => {
                                    params.read_subsidy_hardware_percent
                                }
                                ParamKey::ReadSubsidyVerifierPercent => {
                                    params.read_subsidy_verifier_percent
                                }
                                ParamKey::ReadSubsidyLiquidityPercent => {
                                    params.read_subsidy_liquidity_percent
                                }
                                ParamKey::AdReadinessWindowSecs => params.ad_readiness_window_secs,
                                ParamKey::AdReadinessMinUniqueViewers => {
                                    params.ad_readiness_min_unique_viewers
                                }
                                ParamKey::AdReadinessMinHostCount => {
                                    params.ad_readiness_min_host_count
                                }
                                ParamKey::AdReadinessMinProviderCount => {
                                    params.ad_readiness_min_provider_count
                                }
                                ParamKey::ProofRebateLimitCt => params.proof_rebate_limit_ct,
                                ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
                                ParamKey::KillSwitchSubsidyReduction => {
                                    params.kill_switch_subsidy_reduction
                                }
                                ParamKey::MinerRewardLogisticTarget => {
                                    params.miner_reward_logistic_target
                                }
                                ParamKey::LogisticSlope => params.logistic_slope_milli,
                                ParamKey::MinerHysteresis => params.miner_hysteresis,
                                ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
                                ParamKey::FeeFloorWindow => params.fee_floor_window,
                                ParamKey::FeeFloorPercentile => params.fee_floor_percentile,
                                ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
                                ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
                                ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
                                ParamKey::JurisdictionRegion => params.jurisdiction_region,
                                ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
                                ParamKey::KalmanRShort => params.kalman_r_short,
                                ParamKey::KalmanRMed => params.kalman_r_med,
                                ParamKey::KalmanRLong => params.kalman_r_long,
                                ParamKey::SchedulerWeightGossip => params.scheduler_weight_gossip,
                                ParamKey::SchedulerWeightCompute => params.scheduler_weight_compute,
                                ParamKey::SchedulerWeightStorage => params.scheduler_weight_storage,
                                ParamKey::RuntimeBackend => params.runtime_backend_policy,
                                ParamKey::TransportProvider => params.transport_provider_policy,
                                ParamKey::TreasuryPercentCt => params.treasury_percent_ct,
                                ParamKey::StorageEnginePolicy => params.storage_engine_policy,
                                ParamKey::BridgeMinBond => params.bridge_min_bond,
                                ParamKey::BridgeDutyReward => params.bridge_duty_reward,
                                ParamKey::BridgeFailureSlash => params.bridge_failure_slash,
                                ParamKey::BridgeChallengeSlash => params.bridge_challenge_slash,
                                ParamKey::BridgeDutyWindowSecs => params.bridge_duty_window_secs,
                            };
                            if let Some(spec) = registry().iter().find(|s| s.key == prop.key) {
                                (spec.apply)(prop.new_value, params)
                                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                                (spec.apply_runtime)(prop.new_value, rt)
                                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                            }
                            let last = LastActivation {
                                proposal_id: prop.id,
                                key: prop.key,
                                old_value: old,
                                new_value: prop.new_value,
                                activated_epoch: current_epoch,
                            };
                            self.last_activation().insert("last", ser(&last)?)?;
                            prop.status = ProposalStatus::Activated;
                            self.proposals().insert(&key, ser(&prop)?)?;
                            self.active_params()
                                .insert(ser(&prop.key)?, ser(&prop.new_value)?)?;
                            self.persist_param_change(
                                &hist_dir,
                                prop.key,
                                prop.id,
                                old,
                                prop.new_value,
                                current_epoch,
                                params,
                            );
                            #[cfg(feature = "telemetry")]
                            {
                                PARAM_CHANGE_PENDING
                                    .ensure_handle_for_label_values(&[key_name(prop.key)])
                                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                                    .set(0);
                                PARAM_CHANGE_ACTIVE
                                    .ensure_handle_for_label_values(&[key_name(prop.key)])
                                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                                    .set(prop.new_value);
                                let sched = prop.activation_epoch.unwrap_or(current_epoch);
                                let delay = current_epoch.saturating_sub(sched);
                                GOV_ACTIVATION_DELAY_SECONDS
                                    .ensure_handle_for_label_values(&[key_name(prop.key)])
                                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                                    .observe(delay as f64);
                                governance_webhook("activate", prop.id);
                                if crate::telemetry::should_log("governance") {
                                    let span = crate::log_context!(block = current_epoch);
                                    diagnostics::tracing::info!(
                                        parent: &span,
                                        "gov_param_activated key={:?} new={} old={} epoch={}",
                                        prop.key,
                                        prop.new_value,
                                        old,
                                        current_epoch
                                    );
                                }
                            }
                        }
                    }
                }
                to_remove.push(epoch);
            }
        }
        for e in to_remove {
            queue.remove(ser(&e)?)?;
        }
        #[cfg(feature = "telemetry")]
        self.update_pending_gauge()?;
        Ok(())
    }

    pub fn last_activation_record(&self) -> sled::Result<Option<LastActivation>> {
        match self.last_activation().get("last")? {
            Some(raw) => de(&raw).map(Some),
            None => Ok(None),
        }
    }

    pub fn rollback_last(
        &self,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        if let Some(raw) = self.last_activation().get("last")? {
            let hist_dir = self.base_path.join("governance/history");
            let _ = std::fs::create_dir_all(&hist_dir);
            let last: LastActivation = de(&raw)?;
            if current_epoch > last.activated_epoch + ROLLBACK_WINDOW_EPOCHS {
                return Err(sled::Error::Unsupported("expired".into()));
            }
            if let Some(spec) = registry().iter().find(|s| s.key == last.key) {
                (spec.apply)(last.old_value, params)
                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                (spec.apply_runtime)(last.old_value, rt)
                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
            }
            self.active_params()
                .insert(ser(&last.key)?, ser(&last.old_value)?)?;
            self.persist_param_change(
                &hist_dir,
                last.key,
                last.proposal_id,
                last.new_value,
                last.old_value,
                current_epoch,
                params,
            );
            if let Some(prop_raw) = self.proposals().get(ser(&last.proposal_id)?)? {
                let mut prop: Proposal = de(&prop_raw)?;
                prop.status = ProposalStatus::RolledBack;
                self.proposals().insert(ser(&prop.id)?, ser(&prop)?)?;
            }
            self.last_activation().remove("last")?;
            #[cfg(feature = "telemetry")]
            {
                PARAM_CHANGE_ACTIVE
                    .ensure_handle_for_label_values(&[key_name(last.key)])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .set(last.old_value);
                GOV_ROLLBACK_TOTAL
                    .ensure_handle_for_label_values(&[key_name(last.key)])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                governance_webhook("rollback", last.proposal_id);
            }
            #[cfg(feature = "telemetry")]
            self.update_pending_gauge()?;
            return Ok(());
        }
        Err(sled::Error::ReportableBug("no activation".into()))
    }

    pub fn rollback_proposal(
        &self,
        proposal_id: u64,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        let key = ser(&proposal_id)?;
        let prop_raw = self
            .proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?;
        let mut prop: Proposal = de(&prop_raw)?;
        let act_epoch = prop
            .activation_epoch
            .ok_or_else(|| sled::Error::Unsupported("not activated".into()))?;
        if current_epoch > act_epoch + ROLLBACK_WINDOW_EPOCHS {
            return Err(sled::Error::Unsupported("expired".into()));
        }
        let snap_path = self
            .base_path
            .join("governance/history")
            .join(format!("{}.json", act_epoch));
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let bytes =
            std::fs::read(&snap_path).map_err(|_| sled::Error::Unsupported("snapshot".into()))?;
        let value =
            json_from_bytes(&bytes).map_err(|_| sled::Error::Unsupported("parse".into()))?;
        let prev = Params::deserialize(&value)?;
        *params = prev.clone();
        for spec in registry() {
            let val = match spec.key {
                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                ParamKey::ConsumerFeeComfortP90Microunits => {
                    params.consumer_fee_comfort_p90_microunits
                }
                ParamKey::IndustrialAdmissionMinCapacity => {
                    params.industrial_admission_min_capacity
                }
                ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
                ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
                ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
                ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
                ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
                ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
                ParamKey::ReadSubsidyViewerPercent => params.read_subsidy_viewer_percent,
                ParamKey::ReadSubsidyHostPercent => params.read_subsidy_host_percent,
                ParamKey::ReadSubsidyHardwarePercent => params.read_subsidy_hardware_percent,
                ParamKey::ReadSubsidyVerifierPercent => params.read_subsidy_verifier_percent,
                ParamKey::ReadSubsidyLiquidityPercent => params.read_subsidy_liquidity_percent,
                ParamKey::AdReadinessWindowSecs => params.ad_readiness_window_secs,
                ParamKey::AdReadinessMinUniqueViewers => params.ad_readiness_min_unique_viewers,
                ParamKey::AdReadinessMinHostCount => params.ad_readiness_min_host_count,
                ParamKey::AdReadinessMinProviderCount => params.ad_readiness_min_provider_count,
                ParamKey::ProofRebateLimitCt => params.proof_rebate_limit_ct,
                ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
                ParamKey::KillSwitchSubsidyReduction => params.kill_switch_subsidy_reduction as i64,
                ParamKey::MinerRewardLogisticTarget => params.miner_reward_logistic_target,
                ParamKey::LogisticSlope => params.logistic_slope_milli,
                ParamKey::MinerHysteresis => params.miner_hysteresis,
                ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
                ParamKey::FeeFloorWindow => params.fee_floor_window,
                ParamKey::FeeFloorPercentile => params.fee_floor_percentile,
                ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
                ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
                ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
                ParamKey::JurisdictionRegion => params.jurisdiction_region,
                ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
                ParamKey::KalmanRShort => params.kalman_r_short,
                ParamKey::KalmanRMed => params.kalman_r_med,
                ParamKey::KalmanRLong => params.kalman_r_long,
                ParamKey::SchedulerWeightGossip => params.scheduler_weight_gossip,
                ParamKey::SchedulerWeightCompute => params.scheduler_weight_compute,
                ParamKey::SchedulerWeightStorage => params.scheduler_weight_storage,
                ParamKey::RuntimeBackend => params.runtime_backend_policy,
                ParamKey::TransportProvider => params.transport_provider_policy,
                ParamKey::TreasuryPercentCt => params.treasury_percent_ct,
                ParamKey::StorageEnginePolicy => params.storage_engine_policy,
                ParamKey::BridgeMinBond => params.bridge_min_bond,
                ParamKey::BridgeDutyReward => params.bridge_duty_reward,
                ParamKey::BridgeFailureSlash => params.bridge_failure_slash,
                ParamKey::BridgeChallengeSlash => params.bridge_challenge_slash,
                ParamKey::BridgeDutyWindowSecs => params.bridge_duty_window_secs,
            };
            (spec.apply_runtime)(val, rt)
                .map_err(|_| sled::Error::Unsupported("apply_runtime".into()))?;
            self.active_params().insert(ser(&spec.key)?, ser(&val)?)?;
        }
        let reverted_val = match prop.key {
            ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
            ParamKey::ConsumerFeeComfortP90Microunits => params.consumer_fee_comfort_p90_microunits,
            ParamKey::IndustrialAdmissionMinCapacity => params.industrial_admission_min_capacity,
            ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
            ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
            ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
            ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
            ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
            ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
            ParamKey::ReadSubsidyViewerPercent => params.read_subsidy_viewer_percent,
            ParamKey::ReadSubsidyHostPercent => params.read_subsidy_host_percent,
            ParamKey::ReadSubsidyHardwarePercent => params.read_subsidy_hardware_percent,
            ParamKey::ReadSubsidyVerifierPercent => params.read_subsidy_verifier_percent,
            ParamKey::ReadSubsidyLiquidityPercent => params.read_subsidy_liquidity_percent,
            ParamKey::AdReadinessWindowSecs => params.ad_readiness_window_secs,
            ParamKey::AdReadinessMinUniqueViewers => params.ad_readiness_min_unique_viewers,
            ParamKey::AdReadinessMinHostCount => params.ad_readiness_min_host_count,
            ParamKey::AdReadinessMinProviderCount => params.ad_readiness_min_provider_count,
            ParamKey::ProofRebateLimitCt => params.proof_rebate_limit_ct,
            ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
            ParamKey::KillSwitchSubsidyReduction => params.kill_switch_subsidy_reduction as i64,
            ParamKey::MinerRewardLogisticTarget => params.miner_reward_logistic_target,
            ParamKey::LogisticSlope => params.logistic_slope_milli,
            ParamKey::MinerHysteresis => params.miner_hysteresis,
            ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
            ParamKey::FeeFloorWindow => params.fee_floor_window,
            ParamKey::FeeFloorPercentile => params.fee_floor_percentile,
            ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
            ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
            ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
            ParamKey::JurisdictionRegion => params.jurisdiction_region,
            ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
            ParamKey::KalmanRShort => params.kalman_r_short,
            ParamKey::KalmanRMed => params.kalman_r_med,
            ParamKey::KalmanRLong => params.kalman_r_long,
            ParamKey::SchedulerWeightGossip => params.scheduler_weight_gossip,
            ParamKey::SchedulerWeightCompute => params.scheduler_weight_compute,
            ParamKey::SchedulerWeightStorage => params.scheduler_weight_storage,
            ParamKey::RuntimeBackend => params.runtime_backend_policy,
            ParamKey::TransportProvider => params.transport_provider_policy,
            ParamKey::TreasuryPercentCt => params.treasury_percent_ct,
            ParamKey::StorageEnginePolicy => params.storage_engine_policy,
            ParamKey::BridgeMinBond => params.bridge_min_bond,
            ParamKey::BridgeDutyReward => params.bridge_duty_reward,
            ParamKey::BridgeFailureSlash => params.bridge_failure_slash,
            ParamKey::BridgeChallengeSlash => params.bridge_challenge_slash,
            ParamKey::BridgeDutyWindowSecs => params.bridge_duty_window_secs,
        };
        self.persist_param_change(
            &hist_dir,
            prop.key,
            prop.id,
            prop.new_value,
            reverted_val,
            current_epoch,
            params,
        );
        prop.status = ProposalStatus::RolledBack;
        self.proposals().insert(key, ser(&prop)?)?;
        #[cfg(feature = "telemetry")]
        {
            GOV_ROLLBACK_TOTAL
                .ensure_handle_for_label_values(&[key_name(prop.key)])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc();
            governance_webhook("rollback", proposal_id);
            self.update_pending_gauge()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

    fn open_store() -> (GovStore, sys::tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("gov.db");
        let store = GovStore::open(&path);
        (store, dir)
    }

    #[test]
    fn reward_claim_roundtrip_persists_records() {
        let (store, dir) = open_store();
        let db_path = dir.path().join("gov.db");
        let approval = RewardClaimApproval::new("claim-a", "relayer-a", 90);
        store
            .record_reward_claim(&approval)
            .expect("record approval");

        let fetched = store
            .reward_claim("claim-a")
            .expect("read approval")
            .expect("approval present");
        assert_eq!(fetched, approval);

        let snapshot = store.reward_claims_snapshot().expect("snapshot approvals");
        assert_eq!(snapshot, vec![approval.clone()]);

        drop(store);
        let reopened = GovStore::open(&db_path);
        let persisted = reopened
            .reward_claim("claim-a")
            .expect("read approval")
            .expect("approval present");
        assert_eq!(persisted, approval);
    }

    #[test]
    fn reward_claim_consumption_updates_allowance_and_removes_entry() {
        let (store, dir) = open_store();
        let db_path = dir.path().join("gov.db");
        let approval = RewardClaimApproval::new("claim-b", "relayer-b", 120);
        store
            .record_reward_claim(&approval)
            .expect("record approval");

        let updated = store
            .consume_reward_claim("claim-b", "relayer-b", 20)
            .expect("consume allowance");
        assert_eq!(updated.remaining_amount, 100);

        let stored = store
            .reward_claim("claim-b")
            .expect("read approval")
            .expect("approval present");
        assert_eq!(stored.remaining_amount, 100);

        let finalized = store
            .consume_reward_claim("claim-b", "relayer-b", 100)
            .expect("consume remaining allowance");
        assert_eq!(finalized.remaining_amount, 0);
        assert!(store
            .reward_claim("claim-b")
            .expect("read approval")
            .is_none());

        store
            .record_reward_claim(&RewardClaimApproval::new("claim-c", "relayer-c", 30))
            .expect("record second approval");
        let err = store
            .consume_reward_claim("claim-c", "relayer-x", 5)
            .expect_err("relayer mismatch should fail");
        assert!(err.to_string().contains("bound to relayer"));

        drop(store);
        let reopened = GovStore::open(&db_path);
        let err = reopened
            .consume_reward_claim("claim-c", "relayer-c", 50)
            .expect_err("insufficient allowance should fail");
        assert!(err.to_string().contains("insufficient allowance"));
    }
}
