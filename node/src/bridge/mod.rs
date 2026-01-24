#![forbid(unsafe_code)]

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    BRIDGE_CHALLENGES_TOTAL, BRIDGE_DISPUTE_OUTCOMES_TOTAL, BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL,
    BRIDGE_REWARD_CLAIMS_TOTAL, BRIDGE_SETTLEMENT_RESULTS_TOTAL,
};
use crate::{governance, simple_db::names, SimpleDb};
use bridge_types::{
    settlement_proof_digest, BridgeIncentiveParameters, DutyFailureReason, DutyKind, DutyRecord,
    DutyStatus, ExternalSettlementProof, RelayerAccounting,
};
use bridges::codec::Error as CodecError;
use bridges::relayer::RelayerSet;
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header as LightHeader, Proof},
    token_bridge::AssetSnapshot,
    Bridge as ExternalBridge, BridgeConfig, PendingWithdrawal, RelayerBundle, TokenBridge,
};
use concurrency::Lazy;
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::json::{self, Number, Value};
use foundation_serialization::Serialize;
use sled::{Config as SledConfig, Db as SledDb};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryFrom;
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_KEY: &str = "bridge/state";
const RECEIPT_RETENTION: usize = 512;
const CHALLENGE_RETENTION: usize = 256;
const SLASH_RETENTION: usize = 512;
const DUTY_RETENTION: usize = 1024;
const REWARD_CLAIM_RETENTION: usize = 256;
const REWARD_ACCRUAL_RETENTION: usize = 1024;
const SETTLEMENT_RETENTION: usize = 256;
const DISPUTE_HISTORY_RETENTION: usize = 2048;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(feature = "telemetry")]
fn telemetry_record_settlement(result: &'static str, reason: &'static str) {
    if let Ok(handle) =
        BRIDGE_SETTLEMENT_RESULTS_TOTAL.ensure_handle_for_label_values(&[result, reason])
    {
        handle.inc();
    }
}

#[cfg(not(feature = "telemetry"))]
fn telemetry_record_settlement(_: &'static str, _: &'static str) {}

fn telemetry_record_settlement_success() {
    telemetry_record_settlement("success", "ok");
}

fn telemetry_record_settlement_failure(reason: &'static str) {
    telemetry_record_settlement("failure", reason);
}

#[cfg(feature = "telemetry")]
fn telemetry_record_reward_claim(amount: u64) {
    BRIDGE_REWARD_CLAIMS_TOTAL.inc();
    if amount > 0 {
        BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL.inc_by(amount);
    }
}

#[cfg(not(feature = "telemetry"))]
fn telemetry_record_reward_claim(_: u64) {}

#[cfg(feature = "telemetry")]
fn telemetry_record_dispute(kind: &'static str, outcome: &'static str) {
    if let Ok(handle) =
        BRIDGE_DISPUTE_OUTCOMES_TOTAL.ensure_handle_for_label_values(&[kind, outcome])
    {
        handle.inc();
    }
}

#[cfg(not(feature = "telemetry"))]
fn telemetry_record_dispute(_: &'static str, _: &'static str) {}

fn duty_kind_label(kind: &DutyKind) -> Option<&'static str> {
    match kind {
        DutyKind::Withdrawal { .. } => Some("withdrawal"),
        DutyKind::Settlement { .. } => Some("settlement"),
        DutyKind::Deposit => None,
    }
}

#[derive(Debug)]
pub enum BridgeError {
    UnknownChannel(String),
    Storage(String),
    InvalidProof,
    Replay,
    DuplicateWithdrawal,
    WithdrawalMissing,
    AlreadyChallenged,
    ChallengeWindowOpen,
    UnauthorizedRelease,
    InsufficientBond {
        relayer: String,
        required: u64,
        available: u64,
    },
    RewardClaimRejected(String),
    RewardClaimAmountZero,
    RewardInsufficientPending {
        relayer: String,
        available: u64,
        requested: u64,
    },
    SettlementProofRequired {
        asset: String,
        commitment: [u8; 32],
    },
    SettlementProofDuplicate,
    SettlementProofChainMismatch {
        expected: Option<String>,
        found: String,
    },
    SettlementProofNotTracked {
        asset: String,
    },
    SettlementProofHashMismatch {
        expected: [u8; 32],
        found: [u8; 32],
    },
    SettlementProofHeightReplay {
        chain: String,
        previous: u64,
        submitted: u64,
    },
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::UnknownChannel(name) => write!(f, "bridge channel not found: {name}"),
            BridgeError::Storage(reason) => write!(f, "bridge storage error: {reason}"),
            BridgeError::InvalidProof => write!(f, "bridge proof rejected"),
            BridgeError::Replay => write!(f, "proof already processed"),
            BridgeError::DuplicateWithdrawal => write!(f, "withdrawal already pending"),
            BridgeError::WithdrawalMissing => write!(f, "withdrawal not found"),
            BridgeError::AlreadyChallenged => write!(f, "withdrawal already challenged"),
            BridgeError::ChallengeWindowOpen => write!(f, "challenge window still open"),
            BridgeError::UnauthorizedRelease => write!(f, "release not authorized"),
            BridgeError::InsufficientBond {
                relayer,
                required,
                available,
            } => write!(
                f,
                "relayer {relayer} bond {available} below required {required}"
            ),
            BridgeError::RewardClaimRejected(reason) => {
                write!(f, "reward claim authorization rejected: {reason}")
            }
            BridgeError::RewardClaimAmountZero => {
                write!(f, "reward claim amount must be greater than zero")
            }
            BridgeError::RewardInsufficientPending {
                relayer,
                available,
                requested,
            } => write!(
                f,
                "relayer {relayer} pending rewards {available} below requested {requested}"
            ),
            BridgeError::SettlementProofRequired { asset, commitment } => {
                write!(
                    f,
                    "settlement proof required for {asset} commitment {}",
                    crypto_suite::hex::encode(commitment)
                )
            }
            BridgeError::SettlementProofDuplicate => {
                write!(f, "settlement proof already submitted")
            }
            BridgeError::SettlementProofChainMismatch { expected, found } => {
                if let Some(expected_chain) = expected {
                    write!(
                        f,
                        "settlement proof chain {found} does not match required {expected_chain}"
                    )
                } else {
                    write!(f, "settlement proof chain {found} not permitted")
                }
            }
            BridgeError::SettlementProofNotTracked { asset } => {
                write!(f, "no settlement tracking entry for asset {asset}")
            }
            BridgeError::SettlementProofHashMismatch { expected, found } => {
                write!(
                    f,
                    "settlement proof hash {} does not match expected {}",
                    crypto_suite::hex::encode(found),
                    crypto_suite::hex::encode(expected)
                )
            }
            BridgeError::SettlementProofHeightReplay {
                chain,
                previous,
                submitted,
            } => {
                write!(
                    f,
                    "settlement proof height {submitted} on {chain} not above watermark {previous}"
                )
            }
        }
    }
}

impl std::error::Error for BridgeError {}

static GLOBAL_INCENTIVES: Lazy<RwLock<BridgeIncentiveParameters>> =
    Lazy::new(|| RwLock::new(BridgeIncentiveParameters::default()));

pub fn set_global_incentives(params: BridgeIncentiveParameters) {
    *GLOBAL_INCENTIVES.write().expect("bridge incentives lock") = params;
}

pub fn global_incentives() -> BridgeIncentiveParameters {
    GLOBAL_INCENTIVES
        .read()
        .expect("bridge incentives lock")
        .clone()
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub asset: String,
    pub confirm_depth: u64,
    pub fee_per_byte: u64,
    pub challenge_period_secs: u64,
    pub relayer_quorum: usize,
    pub headers_dir: String,
    pub requires_settlement_proof: bool,
    pub settlement_chain: Option<String>,
}

impl ChannelConfig {
    fn to_bridge_config(&self) -> BridgeConfig {
        BridgeConfig {
            confirm_depth: self.confirm_depth,
            fee_per_byte: self.fee_per_byte,
            headers_dir: self.headers_dir.clone(),
            challenge_period_secs: self.challenge_period_secs,
            relayer_quorum: self.relayer_quorum,
        }
    }

    pub fn for_asset(asset: &str) -> Self {
        Self {
            asset: asset.to_string(),
            confirm_depth: 6,
            fee_per_byte: 0,
            challenge_period_secs: 30,
            relayer_quorum: 2,
            headers_dir: format!("state/bridge_headers/{asset}"),
            requires_settlement_proof: false,
            settlement_chain: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct BridgeSnapshot {
    locked: HashMap<String, u64>,
    verified_headers: HashSet<[u8; 32]>,
    pending_withdrawals: HashMap<[u8; 32], PendingWithdrawal>,
}

#[derive(Debug, Clone, Default)]
struct DutyStore {
    next_id: u64,
    records: HashMap<u64, DutyRecord>,
    order: VecDeque<u64>,
    pending: HashMap<[u8; 32], Vec<u64>>,
}

impl DutyStore {
    fn assign(&mut self, mut record: DutyRecord) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        record.id = id;
        if let DutyKind::Withdrawal { commitment } = record.kind {
            self.pending.entry(commitment).or_default().push(id);
        }
        self.records.insert(id, record);
        self.order.push_back(id);
        self.enforce_retention();
        id
    }

    fn enforce_retention(&mut self) {
        while self.order.len() > DUTY_RETENTION {
            if let Some(id) = self.order.pop_front() {
                if let Some(record) = self.records.remove(&id) {
                    if let DutyKind::Withdrawal { commitment } = record.kind {
                        if let Some(ids) = self.pending.get_mut(&commitment) {
                            ids.retain(|candidate| *candidate != id);
                            if ids.is_empty() {
                                self.pending.remove(&commitment);
                            }
                        }
                    }
                }
            }
        }
    }

    fn update_status(&mut self, id: u64, status: DutyStatus) -> Option<DutyRecord> {
        let record = self.records.get_mut(&id)?;
        record.status = status;
        let clone = record.clone();
        if let DutyKind::Withdrawal { commitment } = clone.kind {
            if !clone.is_pending() {
                if let Some(ids) = self.pending.get_mut(&commitment) {
                    ids.retain(|candidate| *candidate != id);
                    if ids.is_empty() {
                        self.pending.remove(&commitment);
                    }
                }
            }
        }
        Some(clone)
    }

    fn duties_for_commitment(&self, commitment: &[u8; 32]) -> Vec<u64> {
        self.pending.get(commitment).cloned().unwrap_or_default()
    }

    fn records(&self) -> Vec<DutyRecord> {
        self.order
            .iter()
            .filter_map(|id| self.records.get(id))
            .cloned()
            .collect()
    }

    fn pending_count_for_relayer(&self, relayer: &str) -> usize {
        self.records
            .values()
            .filter(|record| record.relayer == relayer && record.is_pending())
            .count()
    }

    fn get(&self, id: u64) -> Option<&DutyRecord> {
        self.records.get(&id)
    }
}

#[derive(Debug, Clone)]
pub struct DepositReceipt {
    pub asset: String,
    pub nonce: u64,
    pub user: String,
    pub amount: u64,
    pub relayer: String,
    pub header_hash: [u8; 32],
    pub relayer_commitment: [u8; 32],
    pub proof_fingerprint: [u8; 32],
    pub bundle_relayers: Vec<String>,
    pub recorded_at: u64,
}

#[derive(Debug, Clone)]
pub struct PendingWithdrawalInfo {
    pub asset: String,
    pub commitment: [u8; 32],
    pub user: String,
    pub amount: u64,
    pub relayers: Vec<String>,
    pub initiated_at: u64,
    pub deadline: u64,
    pub challenged: bool,
    pub requires_settlement_proof: bool,
    pub settlement_chain: Option<String>,
    pub settlement_submitted_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RelayerInfo {
    pub id: String,
    pub stake: u64,
    pub slashes: u64,
    pub bond: u64,
    pub duties_assigned: u64,
    pub duties_completed: u64,
    pub duties_failed: u64,
    pub rewards_earned: u64,
    pub rewards_pending: u64,
    pub rewards_claimed: u64,
    pub penalties_applied: u64,
    pub pending_duties: usize,
}

#[derive(Debug, Clone)]
pub struct RelayerQuorumInfo {
    pub asset: String,
    pub quorum: u64,
    pub relayers: Vec<RelayerInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct IncentiveSummaryEntry {
    pub asset: String,
    pub pending_duties: usize,
    pub claimable_rewards: u64,
    pub receipt_count: usize,
    pub active_relayers: usize,
}

#[derive(Debug, Clone)]
pub struct ChallengeRecord {
    pub asset: String,
    pub commitment: [u8; 32],
    pub challenger: String,
    pub challenged_at: u64,
}

#[derive(Debug, Clone)]
pub struct SlashRecord {
    pub relayer: String,
    pub asset: String,
    pub slashes: u64,
    pub remaining_bond: u64,
    pub occurred_at: u64,
}

#[derive(Debug, Clone)]
pub struct RewardAccrualRecord {
    pub id: u64,
    pub relayer: String,
    pub asset: String,
    pub user: String,
    pub amount: u64,
    pub duty_id: u64,
    pub duty_kind: String,
    pub commitment: Option<[u8; 32]>,
    pub settlement_chain: Option<String>,
    pub proof_hash: Option<[u8; 32]>,
    pub bundle_relayers: Vec<String>,
    pub recorded_at: u64,
}

#[derive(Debug, Clone)]
pub struct RewardClaimRecord {
    pub id: u64,
    pub relayer: String,
    pub amount: u64,
    pub approval_key: String,
    pub claimed_at: u64,
    pub pending_before: u64,
    pub pending_after: u64,
}

#[derive(Debug, Clone)]
pub struct SettlementRecord {
    pub asset: String,
    pub commitment: [u8; 32],
    pub relayer: String,
    pub settlement_chain: Option<String>,
    pub proof_hash: [u8; 32],
    pub settlement_height: u64,
    pub submitted_at: u64,
}

#[derive(Debug, Clone, Default)]
struct SettlementState {
    required_chain: Option<String>,
    duty_ids: Vec<u64>,
    proof: Option<SettlementRecord>,
}

#[derive(Debug, Clone)]
pub struct DutyOutcomeSnapshot {
    pub relayer: String,
    pub status: String,
    pub reward: u64,
    pub penalty: u64,
    pub completed_at: Option<u64>,
    pub failed_at: Option<u64>,
    pub reason: Option<String>,
    pub duty_id: u64,
}

#[derive(Debug, Clone)]
pub struct DisputeAuditRecord {
    pub asset: String,
    pub commitment: [u8; 32],
    pub user: String,
    pub amount: u64,
    pub initiated_at: u64,
    pub deadline: u64,
    pub challenged: bool,
    pub challenger: Option<String>,
    pub challenged_at: Option<u64>,
    pub settlement_required: bool,
    pub settlement_chain: Option<String>,
    pub settlement_submitted_at: Option<u64>,
    pub relayer_outcomes: Vec<DutyOutcomeSnapshot>,
    pub expired: bool,
}

#[derive(Debug, Clone, Default)]
struct DisputeHistory {
    records: VecDeque<DisputeAuditRecord>,
}

impl DisputeHistory {
    fn upsert(&mut self, record: DisputeAuditRecord) {
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|entry| entry.commitment == record.commitment)
        {
            *existing = record;
        } else {
            self.records.push_back(record);
            while self.records.len() > DISPUTE_HISTORY_RETENTION {
                self.records.pop_front();
            }
        }
    }

    #[allow(dead_code)]
    fn remove(&mut self, commitment: &[u8; 32]) {
        self.records
            .retain(|record| record.commitment != *commitment);
    }

    fn iter(&self) -> impl Iterator<Item = &DisputeAuditRecord> {
        self.records.iter()
    }
}

#[derive(Debug, Clone)]
struct ChannelState {
    config: ChannelConfig,
    bridge: BridgeSnapshot,
    relayers: RelayerSet,
    receipts: VecDeque<DepositReceipt>,
    challenges: Vec<ChallengeRecord>,
    seen_fingerprints: HashSet<[u8; 32]>,
    next_nonce: u64,
}

impl ChannelState {
    fn new(config: ChannelConfig) -> Self {
        Self {
            bridge: BridgeSnapshot::default(),
            relayers: RelayerSet::default(),
            receipts: VecDeque::new(),
            challenges: Vec::new(),
            seen_fingerprints: HashSet::new(),
            next_nonce: 0,
            config,
        }
    }

    fn runtime_bridge(&self) -> ExternalBridge {
        let mut runtime = ExternalBridge::new(self.config.to_bridge_config());
        runtime.locked = self.bridge.locked.clone();
        runtime.verified_headers = self.bridge.verified_headers.clone();
        runtime.pending_withdrawals = self.bridge.pending_withdrawals.clone();
        runtime
    }

    fn update_from_runtime(&mut self, runtime: ExternalBridge) {
        self.bridge.locked = runtime.locked;
        self.bridge.verified_headers = runtime.verified_headers;
        self.bridge.pending_withdrawals = runtime.pending_withdrawals;
    }

    fn record_receipt(&mut self, receipt: DepositReceipt) {
        self.receipts.push_back(receipt);
        while self.receipts.len() > RECEIPT_RETENTION {
            self.receipts.pop_front();
        }
    }

    fn record_challenge(&mut self, record: ChallengeRecord) {
        self.challenges.push(record);
        if self.challenges.len() > CHALLENGE_RETENTION {
            let drop = self.challenges.len() - CHALLENGE_RETENTION;
            self.challenges.drain(0..drop);
        }
    }
}

#[derive(Debug, Clone, Default)]
struct BridgeState {
    channels: HashMap<String, ChannelState>,
    relayer_bonds: HashMap<String, u64>,
    accounting: HashMap<String, RelayerAccounting>,
    slash_log: Vec<SlashRecord>,
    duties: DutyStore,
    incentives: BridgeIncentiveParameters,
    token_bridge: TokenBridge,
    pending_rewards: HashMap<String, u64>,
    reward_accruals: VecDeque<RewardAccrualRecord>,
    reward_claims: VecDeque<RewardClaimRecord>,
    next_claim_id: u64,
    next_accrual_id: u64,
    settlement_log: VecDeque<SettlementRecord>,
    pending_settlements: HashMap<[u8; 32], SettlementState>,
    settlement_fingerprints: HashSet<[u8; 32]>,
    dispute_history: DisputeHistory,
    settlement_height_watermarks: HashMap<String, u64>,
}

mod state_codec {
    use super::*;
    use foundation_serialization::json::Map;

    fn missing(field: &'static str) -> CodecError {
        CodecError::MissingField(field)
    }

    fn invalid_type(field: &'static str, expected: &'static str) -> CodecError {
        CodecError::InvalidType { field, expected }
    }

    #[allow(dead_code)]
    fn invalid_value(field: &'static str, reason: impl Into<String>) -> CodecError {
        CodecError::InvalidValue {
            field,
            reason: reason.into(),
        }
    }

    fn get<'a>(object: &'a Map, field: &'static str) -> Result<&'a Value, CodecError> {
        object.get(field).ok_or_else(|| missing(field))
    }

    fn require_object<'a>(value: &'a Value, field: &'static str) -> Result<&'a Map, CodecError> {
        value
            .as_object()
            .ok_or_else(|| invalid_type(field, "an object"))
    }

    fn require_array<'a>(value: &'a Value, field: &'static str) -> Result<&'a [Value], CodecError> {
        value
            .as_array()
            .map(|values| values.as_slice())
            .ok_or_else(|| invalid_type(field, "an array"))
    }

    fn require_string<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, CodecError> {
        value
            .as_str()
            .ok_or_else(|| invalid_type(field, "a string"))
    }

    fn require_u64(value: &Value, field: &'static str) -> Result<u64, CodecError> {
        value
            .as_u64()
            .ok_or_else(|| invalid_type(field, "an integer"))
    }

    #[allow(dead_code)]
    fn require_bool(value: &Value, field: &'static str) -> Result<bool, CodecError> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            _ => Err(invalid_type(field, "a boolean")),
        }
    }

    fn encode_accounting_record(record: &RelayerAccounting) -> Value {
        let mut map = Map::new();
        map.insert("bond".into(), Value::Number(Number::from(record.bond)));
        map.insert(
            "rewards_earned".into(),
            Value::Number(Number::from(record.rewards_earned)),
        );
        map.insert(
            "rewards_pending".into(),
            Value::Number(Number::from(record.rewards_pending)),
        );
        map.insert(
            "rewards_claimed".into(),
            Value::Number(Number::from(record.rewards_claimed)),
        );
        map.insert(
            "penalties_applied".into(),
            Value::Number(Number::from(record.penalties_applied)),
        );
        map.insert(
            "duties_assigned".into(),
            Value::Number(Number::from(record.duties_assigned)),
        );
        map.insert(
            "duties_completed".into(),
            Value::Number(Number::from(record.duties_completed)),
        );
        map.insert(
            "duties_failed".into(),
            Value::Number(Number::from(record.duties_failed)),
        );
        Value::Object(map)
    }

    fn decode_accounting_record(value: &Value) -> Result<RelayerAccounting, CodecError> {
        let obj = require_object(value, "relayer_accounting_record")?;
        Ok(RelayerAccounting {
            bond: require_u64(get(obj, "bond")?, "bond")?,
            rewards_earned: require_u64(get(obj, "rewards_earned")?, "rewards_earned")?,
            rewards_pending: require_u64(get(obj, "rewards_pending")?, "rewards_pending")?,
            rewards_claimed: require_u64(get(obj, "rewards_claimed")?, "rewards_claimed")?,
            penalties_applied: require_u64(get(obj, "penalties_applied")?, "penalties_applied")?,
            duties_assigned: require_u64(get(obj, "duties_assigned")?, "duties_assigned")?,
            duties_completed: require_u64(get(obj, "duties_completed")?, "duties_completed")?,
            duties_failed: require_u64(get(obj, "duties_failed")?, "duties_failed")?,
        })
    }

    fn encode_accounting(accounting: &HashMap<String, RelayerAccounting>) -> Value {
        let mut map = Map::new();
        for (relayer, record) in accounting {
            map.insert(relayer.clone(), encode_accounting_record(record));
        }
        Value::Object(map)
    }

    fn decode_accounting(value: &Value) -> Result<HashMap<String, RelayerAccounting>, CodecError> {
        let obj = require_object(value, "relayer_accounting")?;
        let mut accounting = HashMap::new();
        for (relayer, entry) in obj.iter() {
            let record = decode_accounting_record(entry)
                .map_err(|_| invalid_type("relayer_accounting", "a relayer accounting record"))?;
            accounting.insert(relayer.clone(), record);
        }
        Ok(accounting)
    }

    fn encode_pending_rewards(pending: &HashMap<String, u64>) -> Value {
        let mut map = Map::new();
        for (asset, amount) in pending {
            map.insert(asset.clone(), Value::Number(Number::from(*amount)));
        }
        Value::Object(map)
    }

    fn decode_pending_rewards(value: &Value) -> Result<HashMap<String, u64>, CodecError> {
        let obj = require_object(value, "pending_rewards")?;
        let mut pending = HashMap::new();
        for (asset, entry) in obj.iter() {
            pending.insert(asset.clone(), require_u64(entry, "pending_reward")?);
        }
        Ok(pending)
    }

    fn encode_incentives(params: &BridgeIncentiveParameters) -> Value {
        let mut map = Map::new();
        map.insert(
            "min_bond".into(),
            Value::Number(Number::from(params.min_bond)),
        );
        map.insert(
            "duty_reward".into(),
            Value::Number(Number::from(params.duty_reward)),
        );
        map.insert(
            "failure_slash".into(),
            Value::Number(Number::from(params.failure_slash)),
        );
        map.insert(
            "challenge_slash".into(),
            Value::Number(Number::from(params.challenge_slash)),
        );
        map.insert(
            "duty_window_secs".into(),
            Value::Number(Number::from(params.duty_window_secs)),
        );
        Value::Object(map)
    }

    fn decode_incentives(value: &Value) -> Result<BridgeIncentiveParameters, CodecError> {
        let obj = require_object(value, "bridge_incentives")?;
        Ok(BridgeIncentiveParameters {
            min_bond: require_u64(get(obj, "min_bond")?, "min_bond")?,
            duty_reward: require_u64(get(obj, "duty_reward")?, "duty_reward")?,
            failure_slash: require_u64(get(obj, "failure_slash")?, "failure_slash")?,
            challenge_slash: require_u64(get(obj, "challenge_slash")?, "challenge_slash")?,
            duty_window_secs: require_u64(get(obj, "duty_window_secs")?, "duty_window_secs")?,
        })
    }

    fn encode_duty_kind(kind: &DutyKind) -> Value {
        let mut map = Map::new();
        match kind {
            DutyKind::Deposit => {
                map.insert("kind".into(), Value::String("deposit".into()));
            }
            DutyKind::Withdrawal { commitment } => {
                map.insert("kind".into(), Value::String("withdrawal".into()));
                map.insert(
                    "commitment".into(),
                    Value::String(crypto_suite::hex::encode(commitment)),
                );
            }
            DutyKind::Settlement {
                commitment,
                settlement_chain,
                proof_hash,
            } => {
                map.insert("kind".into(), Value::String("settlement".into()));
                map.insert(
                    "commitment".into(),
                    Value::String(crypto_suite::hex::encode(commitment)),
                );
                map.insert(
                    "settlement_chain".into(),
                    Value::String(settlement_chain.clone()),
                );
                map.insert(
                    "proof_hash".into(),
                    Value::String(crypto_suite::hex::encode(proof_hash)),
                );
            }
        }
        Value::Object(map)
    }

    fn decode_duty_kind(value: &Value) -> Result<DutyKind, CodecError> {
        let obj = require_object(value, "duty_kind")?;
        let kind = require_string(get(obj, "kind")?, "kind")?;
        match kind {
            "deposit" => Ok(DutyKind::Deposit),
            "withdrawal" => {
                let commitment_hex = require_string(get(obj, "commitment")?, "commitment")?;
                let commitment =
                    crypto_suite::hex::decode_array::<32>(commitment_hex).map_err(|source| {
                        CodecError::Hex {
                            field: "duty_kind_commitment",
                            source,
                        }
                    })?;
                Ok(DutyKind::Withdrawal { commitment })
            }
            "settlement" => {
                let commitment_hex = require_string(get(obj, "commitment")?, "commitment")?;
                let commitment =
                    crypto_suite::hex::decode_array::<32>(commitment_hex).map_err(|source| {
                        CodecError::Hex {
                            field: "duty_kind_commitment",
                            source,
                        }
                    })?;
                let settlement_chain =
                    require_string(get(obj, "settlement_chain")?, "settlement_chain")?;
                let proof_hash_hex = require_string(get(obj, "proof_hash")?, "proof_hash")?;
                let proof_hash =
                    crypto_suite::hex::decode_array::<32>(proof_hash_hex).map_err(|source| {
                        CodecError::Hex {
                            field: "duty_kind_proof_hash",
                            source,
                        }
                    })?;
                Ok(DutyKind::Settlement {
                    commitment,
                    settlement_chain: settlement_chain.to_string(),
                    proof_hash,
                })
            }
            other => Err(invalid_value("duty_kind", format!("unknown kind: {other}"))),
        }
    }

    fn encode_duty_status(status: &DutyStatus) -> Value {
        let mut map = Map::new();
        match status {
            DutyStatus::Pending => {
                map.insert("status".into(), Value::String("pending".into()));
            }
            DutyStatus::Completed {
                reward,
                completed_at,
            } => {
                map.insert("status".into(), Value::String("completed".into()));
                map.insert("reward".into(), Value::Number(Number::from(*reward)));
                map.insert(
                    "completed_at".into(),
                    Value::Number(Number::from(*completed_at)),
                );
            }
            DutyStatus::Failed {
                penalty,
                failed_at,
                reason,
            } => {
                map.insert("status".into(), Value::String("failed".into()));
                map.insert("penalty".into(), Value::Number(Number::from(*penalty)));
                map.insert("failed_at".into(), Value::Number(Number::from(*failed_at)));
                map.insert("reason".into(), Value::String(reason.as_str().to_string()));
            }
        }
        Value::Object(map)
    }

    fn decode_duty_status(value: &Value) -> Result<DutyStatus, CodecError> {
        let obj = require_object(value, "duty_status")?;
        let status = require_string(get(obj, "status")?, "status")?;
        match status {
            "pending" => Ok(DutyStatus::Pending),
            "completed" => {
                let reward = require_u64(get(obj, "reward")?, "reward")?;
                let completed_at = require_u64(get(obj, "completed_at")?, "completed_at")?;
                Ok(DutyStatus::Completed {
                    reward,
                    completed_at,
                })
            }
            "failed" => {
                let penalty = require_u64(get(obj, "penalty")?, "penalty")?;
                let failed_at = require_u64(get(obj, "failed_at")?, "failed_at")?;
                let reason_value = require_string(get(obj, "reason")?, "reason")?;
                let reason = match reason_value {
                    "invalid_proof" => DutyFailureReason::InvalidProof,
                    "bundle_mismatch" => DutyFailureReason::BundleMismatch,
                    "challenge_accepted" => DutyFailureReason::ChallengeAccepted,
                    "expired" => DutyFailureReason::Expired,
                    "insufficient_bond" => DutyFailureReason::InsufficientBond,
                    other => {
                        return Err(invalid_value(
                            "duty_status_reason",
                            format!("unknown duty failure reason: {other}"),
                        ))
                    }
                };
                Ok(DutyStatus::Failed {
                    penalty,
                    failed_at,
                    reason,
                })
            }
            other => Err(invalid_value(
                "duty_status",
                format!("unknown status: {other}"),
            )),
        }
    }

    fn encode_duty_record(record: &DutyRecord) -> Value {
        let mut map = Map::new();
        map.insert("id".into(), Value::Number(Number::from(record.id)));
        map.insert("relayer".into(), Value::String(record.relayer.clone()));
        map.insert("asset".into(), Value::String(record.asset.clone()));
        map.insert("user".into(), Value::String(record.user.clone()));
        map.insert("amount".into(), Value::Number(Number::from(record.amount)));
        map.insert(
            "assigned_at".into(),
            Value::Number(Number::from(record.assigned_at)),
        );
        map.insert(
            "deadline".into(),
            Value::Number(Number::from(record.deadline)),
        );
        map.insert(
            "bundle_relayers".into(),
            Value::Array(
                record
                    .bundle_relayers
                    .iter()
                    .map(|relayer| Value::String(relayer.clone()))
                    .collect(),
            ),
        );
        map.insert("kind".into(), encode_duty_kind(&record.kind));
        map.insert("status".into(), encode_duty_status(&record.status));
        Value::Object(map)
    }

    fn decode_duty_record(value: &Value) -> Result<DutyRecord, CodecError> {
        let obj = require_object(value, "duty_record")?;
        let id = require_u64(get(obj, "id")?, "id")?;
        let relayer = require_string(get(obj, "relayer")?, "relayer")?.to_string();
        let asset = require_string(get(obj, "asset")?, "asset")?.to_string();
        let user = require_string(get(obj, "user")?, "user")?.to_string();
        let amount = require_u64(get(obj, "amount")?, "amount")?;
        let assigned_at = require_u64(get(obj, "assigned_at")?, "assigned_at")?;
        let deadline = require_u64(get(obj, "deadline")?, "deadline")?;
        let bundle_relayers = if let Some(value) = obj.get("bundle_relayers") {
            let arr = require_array(value, "bundle_relayers")?;
            let mut relayers = Vec::with_capacity(arr.len());
            for entry in arr {
                relayers.push(require_string(entry, "bundle_relayer")?.to_string());
            }
            relayers
        } else {
            Vec::new()
        };
        let kind = decode_duty_kind(get(obj, "kind")?)?;
        let status = decode_duty_status(get(obj, "status")?)?;
        Ok(DutyRecord {
            id,
            relayer,
            asset,
            user,
            amount,
            assigned_at,
            deadline,
            bundle_relayers,
            kind,
            status,
        })
    }

    fn encode_duties(store: &DutyStore) -> Value {
        let mut map = Map::new();
        map.insert(
            "next_id".to_string(),
            Value::Number(Number::from(store.next_id)),
        );
        let order = store
            .order
            .iter()
            .map(|id| Value::Number(Number::from(*id)))
            .collect();
        map.insert("order".to_string(), Value::Array(order));
        let records: Vec<Value> = store
            .records
            .values()
            .map(|record| encode_duty_record(record))
            .collect();
        map.insert("records".to_string(), Value::Array(records));
        let mut pending = Vec::new();
        for (commitment, ids) in &store.pending {
            let mut entry = Map::new();
            entry.insert(
                "commitment".to_string(),
                Value::String(crypto_suite::hex::encode(commitment)),
            );
            entry.insert(
                "ids".to_string(),
                Value::Array(
                    ids.iter()
                        .map(|id| Value::Number(Number::from(*id)))
                        .collect(),
                ),
            );
            pending.push(Value::Object(entry));
        }
        map.insert("pending".to_string(), Value::Array(pending));
        Value::Object(map)
    }

    fn decode_duties(value: &Value) -> Result<DutyStore, CodecError> {
        let obj = require_object(value, "duty_store")?;
        let mut store = DutyStore::default();
        if let Some(next) = obj.get("next_id").and_then(Value::as_u64) {
            store.next_id = next;
        }
        if let Some(order_value) = obj.get("order") {
            let arr = require_array(order_value, "duty_order")?;
            let mut order = VecDeque::with_capacity(arr.len());
            for entry in arr {
                order.push_back(require_u64(entry, "duty_id")?);
            }
            store.order = order;
        }
        if let Some(records_value) = obj.get("records") {
            let arr = require_array(records_value, "duty_records")?;
            for entry in arr {
                let record = decode_duty_record(entry)
                    .map_err(|_| invalid_type("duty_records", "a duty record entry"))?;
                store.records.insert(record.id, record);
            }
        }
        if let Some(pending_value) = obj.get("pending") {
            let arr = require_array(pending_value, "duty_pending")?;
            for entry in arr {
                let pending_obj = require_object(entry, "duty_pending_entry")?;
                let commitment_hex = require_string(get(pending_obj, "commitment")?, "commitment")?;
                let commitment =
                    crypto_suite::hex::decode_array::<32>(commitment_hex).map_err(|source| {
                        CodecError::Hex {
                            field: "duty_pending",
                            source,
                        }
                    })?;
                let ids_val = get(pending_obj, "ids")?;
                let ids_arr = require_array(ids_val, "pending_ids")?;
                let mut ids = Vec::with_capacity(ids_arr.len());
                for entry in ids_arr {
                    ids.push(require_u64(entry, "pending_id")?);
                }
                store.pending.insert(commitment, ids);
            }
        }
        if store.order.is_empty() {
            let mut sorted: Vec<_> = store
                .records
                .values()
                .map(|record| (record.assigned_at, record.id))
                .collect();
            sorted.sort_by_key(|(assigned, _)| *assigned);
            store.order = sorted.into_iter().map(|(_, id)| id).collect();
        }
        if store.next_id == 0 {
            store.next_id = store
                .records
                .keys()
                .max()
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
        }
        if store.pending.is_empty() {
            for record in store.records.values() {
                if let DutyKind::Withdrawal { commitment } = record.kind {
                    if record.is_pending() {
                        store.pending.entry(commitment).or_default().push(record.id);
                    }
                }
            }
        }
        store.enforce_retention();
        Ok(store)
    }

    fn encode_reward_claims(claims: &VecDeque<RewardClaimRecord>, next_id: u64) -> Value {
        let mut map = Map::new();
        map.insert("next_id".into(), Value::Number(Number::from(next_id)));
        let entries: Vec<Value> = claims
            .iter()
            .map(|record| {
                let mut entry = Map::new();
                entry.insert("id".into(), Value::Number(Number::from(record.id)));
                entry.insert("relayer".into(), Value::String(record.relayer.clone()));
                entry.insert("amount".into(), Value::Number(Number::from(record.amount)));
                entry.insert(
                    "approval_key".into(),
                    Value::String(record.approval_key.clone()),
                );
                entry.insert(
                    "claimed_at".into(),
                    Value::Number(Number::from(record.claimed_at)),
                );
                entry.insert(
                    "pending_before".into(),
                    Value::Number(Number::from(record.pending_before)),
                );
                entry.insert(
                    "pending_after".into(),
                    Value::Number(Number::from(record.pending_after)),
                );
                Value::Object(entry)
            })
            .collect();
        map.insert("claims".into(), Value::Array(entries));
        Value::Object(map)
    }

    fn decode_reward_claims(
        value: &Value,
    ) -> Result<(VecDeque<RewardClaimRecord>, u64), CodecError> {
        let obj = require_object(value, "reward_claims")?;
        let mut claims = VecDeque::new();
        let mut max_id = 0;
        if let Some(entries) = obj.get("claims") {
            for entry in require_array(entries, "reward_claim_entries")? {
                let claim_obj = require_object(entry, "reward_claim_entry")?;
                let id = require_u64(get(claim_obj, "id")?, "id")?;
                let relayer = require_string(get(claim_obj, "relayer")?, "relayer")?.to_string();
                let amount = require_u64(get(claim_obj, "amount")?, "amount")?;
                let approval_key =
                    require_string(get(claim_obj, "approval_key")?, "approval_key")?.to_string();
                let claimed_at = require_u64(get(claim_obj, "claimed_at")?, "claimed_at")?;
                let pending_before =
                    require_u64(get(claim_obj, "pending_before")?, "pending_before")?;
                let pending_after = require_u64(get(claim_obj, "pending_after")?, "pending_after")?;
                claims.push_back(RewardClaimRecord {
                    id,
                    relayer,
                    amount,
                    approval_key,
                    claimed_at,
                    pending_before,
                    pending_after,
                });
                max_id = max_id.max(id);
            }
        }
        let next_id = obj
            .get("next_id")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| max_id.saturating_add(1));
        Ok((claims, next_id))
    }

    fn encode_reward_accruals(accruals: &VecDeque<RewardAccrualRecord>, next_id: u64) -> Value {
        let mut map = Map::new();
        map.insert("next_id".into(), Value::Number(Number::from(next_id)));
        let entries: Vec<Value> = accruals
            .iter()
            .map(|record| {
                let mut entry = Map::new();
                entry.insert("id".into(), Value::Number(Number::from(record.id)));
                entry.insert("relayer".into(), Value::String(record.relayer.clone()));
                entry.insert("asset".into(), Value::String(record.asset.clone()));
                entry.insert("user".into(), Value::String(record.user.clone()));
                entry.insert("amount".into(), Value::Number(Number::from(record.amount)));
                entry.insert(
                    "duty_id".into(),
                    Value::Number(Number::from(record.duty_id)),
                );
                entry.insert("duty_kind".into(), Value::String(record.duty_kind.clone()));
                if let Some(commitment) = record.commitment {
                    entry.insert(
                        "commitment".into(),
                        Value::String(crypto_suite::hex::encode(commitment)),
                    );
                }
                if let Some(chain) = &record.settlement_chain {
                    entry.insert("settlement_chain".into(), Value::String(chain.clone()));
                }
                if let Some(hash) = record.proof_hash {
                    entry.insert(
                        "proof_hash".into(),
                        Value::String(crypto_suite::hex::encode(hash)),
                    );
                }
                entry.insert(
                    "bundle_relayers".into(),
                    Value::Array(
                        record
                            .bundle_relayers
                            .iter()
                            .map(|relayer| Value::String(relayer.clone()))
                            .collect(),
                    ),
                );
                entry.insert(
                    "recorded_at".into(),
                    Value::Number(Number::from(record.recorded_at)),
                );
                Value::Object(entry)
            })
            .collect();
        map.insert("accruals".into(), Value::Array(entries));
        Value::Object(map)
    }

    fn decode_reward_accruals(
        value: &Value,
    ) -> Result<(VecDeque<RewardAccrualRecord>, u64), CodecError> {
        let obj = require_object(value, "reward_accruals")?;
        let mut accruals = VecDeque::new();
        let mut max_id = 0;
        if let Some(entries) = obj.get("accruals") {
            for entry in require_array(entries, "reward_accrual_entries")? {
                let accrual_obj = require_object(entry, "reward_accrual_entry")?;
                let id = require_u64(get(accrual_obj, "id")?, "id")?;
                let relayer = require_string(get(accrual_obj, "relayer")?, "relayer")?.to_string();
                let asset = require_string(get(accrual_obj, "asset")?, "asset")?.to_string();
                let user = require_string(get(accrual_obj, "user")?, "user")?.to_string();
                let amount = require_u64(get(accrual_obj, "amount")?, "amount")?;
                let duty_id = require_u64(get(accrual_obj, "duty_id")?, "duty_id")?;
                let duty_kind =
                    require_string(get(accrual_obj, "duty_kind")?, "duty_kind")?.to_string();
                let commitment = if let Some(value) = accrual_obj.get("commitment") {
                    Some(
                        crypto_suite::hex::decode_array::<32>(require_string(value, "commitment")?)
                            .map_err(|source| CodecError::Hex {
                                field: "reward_accrual_commitment",
                                source,
                            })?,
                    )
                } else {
                    None
                };
                let settlement_chain = accrual_obj
                    .get("settlement_chain")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                let proof_hash = if let Some(value) = accrual_obj.get("proof_hash") {
                    Some(
                        crypto_suite::hex::decode_array::<32>(require_string(value, "proof_hash")?)
                            .map_err(|source| CodecError::Hex {
                                field: "reward_accrual_proof_hash",
                                source,
                            })?,
                    )
                } else {
                    None
                };
                let bundle_relayers = if let Some(raw_relayers) = accrual_obj.get("bundle_relayers")
                {
                    let arr = require_array(raw_relayers, "bundle_relayers")?;
                    let relayers: Result<Vec<String>, CodecError> = arr
                        .iter()
                        .map(|value| require_string(value, "bundle_relayer").map(|s| s.to_string()))
                        .collect();
                    relayers?
                } else {
                    Vec::new()
                };
                let recorded_at = require_u64(get(accrual_obj, "recorded_at")?, "recorded_at")?;
                accruals.push_back(RewardAccrualRecord {
                    id,
                    relayer,
                    asset,
                    user,
                    amount,
                    duty_id,
                    duty_kind,
                    commitment,
                    settlement_chain,
                    proof_hash,
                    bundle_relayers,
                    recorded_at,
                });
                max_id = max_id.max(id);
            }
        }
        let next_id = obj
            .get("next_id")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| max_id.saturating_add(1));
        Ok((accruals, next_id))
    }

    fn encode_settlement_record(record: &SettlementRecord) -> Value {
        let mut map = Map::new();
        map.insert("asset".into(), Value::String(record.asset.clone()));
        map.insert(
            "commitment".into(),
            Value::String(crypto_suite::hex::encode(&record.commitment)),
        );
        map.insert("relayer".into(), Value::String(record.relayer.clone()));
        if let Some(chain) = &record.settlement_chain {
            map.insert("settlement_chain".into(), Value::String(chain.clone()));
        }
        map.insert(
            "proof_hash".into(),
            Value::String(crypto_suite::hex::encode(&record.proof_hash)),
        );
        map.insert(
            "settlement_height".into(),
            Value::Number(Number::from(record.settlement_height)),
        );
        map.insert(
            "submitted_at".into(),
            Value::Number(Number::from(record.submitted_at)),
        );
        Value::Object(map)
    }

    fn decode_settlement_record(value: &Value) -> Result<SettlementRecord, CodecError> {
        let obj = require_object(value, "settlement_record")?;
        Ok(SettlementRecord {
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            commitment: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "commitment")?,
                "commitment",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "settlement_commitment",
                source,
            })?,
            relayer: require_string(get(obj, "relayer")?, "relayer")?.to_string(),
            settlement_chain: obj
                .get("settlement_chain")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            proof_hash: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "proof_hash")?,
                "proof_hash",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "settlement_proof_hash",
                source,
            })?,
            settlement_height: require_u64(get(obj, "settlement_height")?, "settlement_height")?,
            submitted_at: require_u64(get(obj, "submitted_at")?, "submitted_at")?,
        })
    }

    fn encode_settlements(log: &VecDeque<SettlementRecord>) -> Value {
        let entries: Vec<Value> = log.iter().map(encode_settlement_record).collect();
        Value::Array(entries)
    }

    fn decode_settlements(value: &Value) -> Result<VecDeque<SettlementRecord>, CodecError> {
        let arr = require_array(value, "settlement_records")?;
        let mut records = VecDeque::new();
        for entry in arr {
            records.push_back(decode_settlement_record(entry)?);
        }
        Ok(records)
    }

    fn encode_pending_settlements(map: &HashMap<[u8; 32], SettlementState>) -> Value {
        let mut entries = Vec::new();
        for (commitment, state) in map {
            let mut obj = Map::new();
            obj.insert(
                "commitment".into(),
                Value::String(crypto_suite::hex::encode(commitment)),
            );
            if let Some(chain) = &state.required_chain {
                obj.insert("required_chain".into(), Value::String(chain.clone()));
            }
            obj.insert(
                "duty_ids".into(),
                Value::Array(
                    state
                        .duty_ids
                        .iter()
                        .map(|id| Value::Number(Number::from(*id)))
                        .collect(),
                ),
            );
            if let Some(record) = &state.proof {
                obj.insert("proof".into(), encode_settlement_record(record));
            }
            entries.push(Value::Object(obj));
        }
        Value::Array(entries)
    }

    fn decode_pending_settlements(
        value: &Value,
    ) -> Result<HashMap<[u8; 32], SettlementState>, CodecError> {
        let arr = require_array(value, "pending_settlements")?;
        let mut map = HashMap::new();
        for entry in arr {
            let obj = require_object(entry, "pending_settlement_entry")?;
            let commitment_hex = require_string(get(obj, "commitment")?, "commitment")?;
            let commitment =
                crypto_suite::hex::decode_array::<32>(commitment_hex).map_err(|source| {
                    CodecError::Hex {
                        field: "pending_settlement_commitment",
                        source,
                    }
                })?;
            let duty_ids_value = get(obj, "duty_ids")?;
            let duty_ids_array = require_array(duty_ids_value, "settlement_duty_ids")?;
            let mut duty_ids = Vec::with_capacity(duty_ids_array.len());
            for entry in duty_ids_array {
                duty_ids.push(require_u64(entry, "settlement_duty_id")?);
            }
            let proof = if let Some(proof_value) = obj.get("proof") {
                Some(decode_settlement_record(proof_value)?)
            } else {
                None
            };
            let state = SettlementState {
                required_chain: obj
                    .get("required_chain")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string()),
                duty_ids,
                proof,
            };
            map.insert(commitment, state);
        }
        Ok(map)
    }

    fn encode_duty_outcome(outcome: &DutyOutcomeSnapshot) -> Value {
        let mut map = Map::new();
        map.insert("relayer".into(), Value::String(outcome.relayer.clone()));
        map.insert("status".into(), Value::String(outcome.status.clone()));
        map.insert("reward".into(), Value::Number(Number::from(outcome.reward)));
        map.insert(
            "penalty".into(),
            Value::Number(Number::from(outcome.penalty)),
        );
        if let Some(ts) = outcome.completed_at {
            map.insert("completed_at".into(), Value::Number(Number::from(ts)));
        }
        if let Some(ts) = outcome.failed_at {
            map.insert("failed_at".into(), Value::Number(Number::from(ts)));
        }
        if let Some(reason) = &outcome.reason {
            map.insert("reason".into(), Value::String(reason.clone()));
        }
        map.insert(
            "duty_id".into(),
            Value::Number(Number::from(outcome.duty_id)),
        );
        Value::Object(map)
    }

    fn decode_duty_outcome(value: &Value) -> Result<DutyOutcomeSnapshot, CodecError> {
        let obj = require_object(value, "duty_outcome")?;
        Ok(DutyOutcomeSnapshot {
            relayer: require_string(get(obj, "relayer")?, "relayer")?.to_string(),
            status: require_string(get(obj, "status")?, "status")?.to_string(),
            reward: require_u64(get(obj, "reward")?, "reward")?,
            penalty: require_u64(get(obj, "penalty")?, "penalty")?,
            completed_at: obj.get("completed_at").and_then(Value::as_u64),
            failed_at: obj.get("failed_at").and_then(Value::as_u64),
            reason: obj
                .get("reason")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            duty_id: require_u64(get(obj, "duty_id")?, "duty_id")?,
        })
    }

    fn encode_dispute_record(record: &DisputeAuditRecord) -> Value {
        let mut map = Map::new();
        map.insert("asset".into(), Value::String(record.asset.clone()));
        map.insert(
            "commitment".into(),
            Value::String(crypto_suite::hex::encode(&record.commitment)),
        );
        map.insert("user".into(), Value::String(record.user.clone()));
        map.insert("amount".into(), Value::Number(Number::from(record.amount)));
        map.insert(
            "initiated_at".into(),
            Value::Number(Number::from(record.initiated_at)),
        );
        map.insert(
            "deadline".into(),
            Value::Number(Number::from(record.deadline)),
        );
        map.insert("challenged".into(), Value::Bool(record.challenged));
        if let Some(challenger) = &record.challenger {
            map.insert("challenger".into(), Value::String(challenger.clone()));
        }
        if let Some(at) = record.challenged_at {
            map.insert("challenged_at".into(), Value::Number(Number::from(at)));
        }
        map.insert(
            "settlement_required".into(),
            Value::Bool(record.settlement_required),
        );
        if let Some(chain) = &record.settlement_chain {
            map.insert("settlement_chain".into(), Value::String(chain.clone()));
        }
        if let Some(at) = record.settlement_submitted_at {
            map.insert(
                "settlement_submitted_at".into(),
                Value::Number(Number::from(at)),
            );
        }
        map.insert(
            "relayer_outcomes".into(),
            Value::Array(
                record
                    .relayer_outcomes
                    .iter()
                    .map(encode_duty_outcome)
                    .collect(),
            ),
        );
        map.insert("expired".into(), Value::Bool(record.expired));
        Value::Object(map)
    }

    fn decode_dispute_record(value: &Value) -> Result<DisputeAuditRecord, CodecError> {
        let obj = require_object(value, "dispute_record")?;
        let outcomes = if let Some(entries) = obj.get("relayer_outcomes") {
            let arr = require_array(entries, "dispute_relayer_outcomes")?;
            let mut out = Vec::with_capacity(arr.len());
            for entry in arr {
                out.push(decode_duty_outcome(entry)?);
            }
            out
        } else {
            Vec::new()
        };
        Ok(DisputeAuditRecord {
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            commitment: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "commitment")?,
                "commitment",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "dispute_commitment",
                source,
            })?,
            user: require_string(get(obj, "user")?, "user")?.to_string(),
            amount: require_u64(get(obj, "amount")?, "amount")?,
            initiated_at: require_u64(get(obj, "initiated_at")?, "initiated_at")?,
            deadline: require_u64(get(obj, "deadline")?, "deadline")?,
            challenged: obj
                .get("challenged")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            challenger: obj
                .get("challenger")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            challenged_at: obj.get("challenged_at").and_then(Value::as_u64),
            settlement_required: obj
                .get("settlement_required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            settlement_chain: obj
                .get("settlement_chain")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            settlement_submitted_at: obj.get("settlement_submitted_at").and_then(Value::as_u64),
            relayer_outcomes: outcomes,
            expired: obj.get("expired").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    fn encode_dispute_history(history: &DisputeHistory) -> Value {
        Value::Array(history.records.iter().map(encode_dispute_record).collect())
    }

    fn decode_dispute_history(value: &Value) -> Result<DisputeHistory, CodecError> {
        let arr = require_array(value, "dispute_history")?;
        let mut records = VecDeque::new();
        for entry in arr {
            records.push_back(decode_dispute_record(entry)?);
        }
        Ok(DisputeHistory { records })
    }

    fn encode_config(cfg: &ChannelConfig) -> Value {
        let mut map = Map::new();
        map.insert("asset".to_string(), Value::String(cfg.asset.clone()));
        map.insert(
            "confirm_depth".to_string(),
            Value::Number(Number::from(cfg.confirm_depth)),
        );
        map.insert(
            "fee_per_byte".to_string(),
            Value::Number(Number::from(cfg.fee_per_byte)),
        );
        map.insert(
            "challenge_period_secs".to_string(),
            Value::Number(Number::from(cfg.challenge_period_secs)),
        );
        map.insert(
            "relayer_quorum".to_string(),
            Value::Number(Number::from(cfg.relayer_quorum as u64)),
        );
        map.insert(
            "headers_dir".to_string(),
            Value::String(cfg.headers_dir.clone()),
        );
        map.insert(
            "requires_settlement_proof".to_string(),
            Value::Bool(cfg.requires_settlement_proof),
        );
        if let Some(chain) = &cfg.settlement_chain {
            map.insert("settlement_chain".to_string(), Value::String(chain.clone()));
        }
        Value::Object(map)
    }

    fn decode_config(value: &Value) -> Result<ChannelConfig, CodecError> {
        let obj = require_object(value, "channel_config")?;
        Ok(ChannelConfig {
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            confirm_depth: require_u64(get(obj, "confirm_depth")?, "confirm_depth")?,
            fee_per_byte: require_u64(get(obj, "fee_per_byte")?, "fee_per_byte")?,
            challenge_period_secs: require_u64(
                get(obj, "challenge_period_secs")?,
                "challenge_period_secs",
            )?,
            relayer_quorum: require_u64(get(obj, "relayer_quorum")?, "relayer_quorum")? as usize,
            headers_dir: require_string(get(obj, "headers_dir")?, "headers_dir")?.to_string(),
            requires_settlement_proof: obj
                .get("requires_settlement_proof")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            settlement_chain: obj
                .get("settlement_chain")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
        })
    }

    fn encode_snapshot(snapshot: &BridgeSnapshot) -> Value {
        let mut locked_map = Map::new();
        for (user, amount) in &snapshot.locked {
            locked_map.insert(user.clone(), Value::from(*amount));
        }
        let mut pending_map = Map::new();
        for (commitment, pending) in &snapshot.pending_withdrawals {
            pending_map.insert(crypto_suite::hex::encode(commitment), pending.to_value());
        }
        let verified = snapshot
            .verified_headers
            .iter()
            .map(|h| Value::String(crypto_suite::hex::encode(h)))
            .collect();
        let mut map = Map::new();
        map.insert("locked".to_string(), Value::Object(locked_map));
        map.insert("verified_headers".to_string(), Value::Array(verified));
        map.insert(
            "pending_withdrawals".to_string(),
            Value::Object(pending_map),
        );
        Value::Object(map)
    }

    fn decode_snapshot(value: &Value) -> Result<BridgeSnapshot, CodecError> {
        let obj = require_object(value, "bridge_snapshot")?;
        let locked_obj = require_object(get(obj, "locked")?, "locked")?;
        let mut locked = HashMap::new();
        for (user, amount) in locked_obj.iter() {
            locked.insert(user.clone(), require_u64(amount, "locked amount")?);
        }
        let verified_values = require_array(get(obj, "verified_headers")?, "verified_headers")?;
        let mut verified = HashSet::new();
        for entry in verified_values {
            let hex_str = require_string(entry, "verified_headers")?;
            let hash = crypto_suite::hex::decode_array::<32>(hex_str).map_err(|source| {
                CodecError::Hex {
                    field: "verified_headers",
                    source,
                }
            })?;
            verified.insert(hash);
        }
        let pending_obj = require_object(get(obj, "pending_withdrawals")?, "pending_withdrawals")?;
        let mut pending = HashMap::new();
        for (commitment_hex, value) in pending_obj.iter() {
            let commitment =
                crypto_suite::hex::decode_array::<32>(commitment_hex).map_err(|source| {
                    CodecError::Hex {
                        field: "pending_withdrawals",
                        source,
                    }
                })?;
            let withdrawal = PendingWithdrawal::from_value(value)?;
            pending.insert(commitment, withdrawal);
        }
        Ok(BridgeSnapshot {
            locked,
            verified_headers: verified,
            pending_withdrawals: pending,
        })
    }

    fn encode_receipt(receipt: &DepositReceipt) -> Value {
        let bundle_relayers = receipt
            .bundle_relayers
            .iter()
            .cloned()
            .map(Value::String)
            .collect();
        let mut map = Map::new();
        map.insert("asset".to_string(), Value::String(receipt.asset.clone()));
        map.insert(
            "nonce".to_string(),
            Value::Number(Number::from(receipt.nonce)),
        );
        map.insert("user".to_string(), Value::String(receipt.user.clone()));
        map.insert(
            "amount".to_string(),
            Value::Number(Number::from(receipt.amount)),
        );
        map.insert(
            "relayer".to_string(),
            Value::String(receipt.relayer.clone()),
        );
        map.insert(
            "header_hash".to_string(),
            Value::String(crypto_suite::hex::encode(&receipt.header_hash)),
        );
        map.insert(
            "relayer_commitment".to_string(),
            Value::String(crypto_suite::hex::encode(&receipt.relayer_commitment)),
        );
        map.insert(
            "proof_fingerprint".to_string(),
            Value::String(crypto_suite::hex::encode(&receipt.proof_fingerprint)),
        );
        map.insert("bundle_relayers".to_string(), Value::Array(bundle_relayers));
        map.insert(
            "recorded_at".to_string(),
            Value::Number(Number::from(receipt.recorded_at)),
        );
        Value::Object(map)
    }

    fn decode_receipt(value: &Value) -> Result<DepositReceipt, CodecError> {
        let obj = require_object(value, "deposit_receipt")?;
        let bundle_relayers_values =
            require_array(get(obj, "bundle_relayers")?, "bundle_relayers")?;
        let mut bundle_relayers = Vec::with_capacity(bundle_relayers_values.len());
        for entry in bundle_relayers_values {
            bundle_relayers.push(require_string(entry, "bundle_relayers")?.to_string());
        }
        Ok(DepositReceipt {
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            nonce: require_u64(get(obj, "nonce")?, "nonce")?,
            user: require_string(get(obj, "user")?, "user")?.to_string(),
            amount: require_u64(get(obj, "amount")?, "amount")?,
            relayer: require_string(get(obj, "relayer")?, "relayer")?.to_string(),
            header_hash: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "header_hash")?,
                "header_hash",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "header_hash",
                source,
            })?,
            relayer_commitment: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "relayer_commitment")?,
                "relayer_commitment",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "relayer_commitment",
                source,
            })?,
            proof_fingerprint: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "proof_fingerprint")?,
                "proof_fingerprint",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "proof_fingerprint",
                source,
            })?,
            bundle_relayers,
            recorded_at: require_u64(get(obj, "recorded_at")?, "recorded_at")?,
        })
    }

    fn encode_challenge(record: &ChallengeRecord) -> Value {
        let mut map = Map::new();
        map.insert("asset".to_string(), Value::String(record.asset.clone()));
        map.insert(
            "commitment".to_string(),
            Value::String(crypto_suite::hex::encode(&record.commitment)),
        );
        map.insert(
            "challenger".to_string(),
            Value::String(record.challenger.clone()),
        );
        map.insert(
            "challenged_at".to_string(),
            Value::Number(Number::from(record.challenged_at)),
        );
        Value::Object(map)
    }

    fn decode_challenge(value: &Value) -> Result<ChallengeRecord, CodecError> {
        let obj = require_object(value, "challenge_record")?;
        Ok(ChallengeRecord {
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            commitment: crypto_suite::hex::decode_array::<32>(require_string(
                get(obj, "commitment")?,
                "commitment",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "commitment",
                source,
            })?,
            challenger: require_string(get(obj, "challenger")?, "challenger")?.to_string(),
            challenged_at: require_u64(get(obj, "challenged_at")?, "challenged_at")?,
        })
    }

    fn encode_slash(record: &SlashRecord) -> Value {
        let mut map = Map::new();
        map.insert("relayer".to_string(), Value::String(record.relayer.clone()));
        map.insert("asset".to_string(), Value::String(record.asset.clone()));
        map.insert(
            "slashes".to_string(),
            Value::Number(Number::from(record.slashes)),
        );
        map.insert(
            "remaining_bond".to_string(),
            Value::Number(Number::from(record.remaining_bond)),
        );
        map.insert(
            "occurred_at".to_string(),
            Value::Number(Number::from(record.occurred_at)),
        );
        Value::Object(map)
    }

    fn decode_slash(value: &Value) -> Result<SlashRecord, CodecError> {
        let obj = require_object(value, "slash_record")?;
        Ok(SlashRecord {
            relayer: require_string(get(obj, "relayer")?, "relayer")?.to_string(),
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            slashes: require_u64(get(obj, "slashes")?, "slashes")?,
            remaining_bond: require_u64(get(obj, "remaining_bond")?, "remaining_bond")?,
            occurred_at: require_u64(get(obj, "occurred_at")?, "occurred_at")?,
        })
    }

    fn encode_channel(channel: &ChannelState) -> Value {
        let receipts = channel.receipts.iter().map(encode_receipt).collect();
        let challenges = channel.challenges.iter().map(encode_challenge).collect();
        let fingerprints = channel
            .seen_fingerprints
            .iter()
            .map(|fp| Value::String(crypto_suite::hex::encode(fp)))
            .collect();
        let mut map = Map::new();
        map.insert("config".to_string(), encode_config(&channel.config));
        map.insert("bridge".to_string(), encode_snapshot(&channel.bridge));
        map.insert("relayers".to_string(), channel.relayers.to_value());
        map.insert("receipts".to_string(), Value::Array(receipts));
        map.insert("challenges".to_string(), Value::Array(challenges));
        map.insert("seen_fingerprints".to_string(), Value::Array(fingerprints));
        map.insert(
            "next_nonce".to_string(),
            Value::Number(Number::from(channel.next_nonce)),
        );
        Value::Object(map)
    }

    fn decode_channel(value: &Value) -> Result<ChannelState, CodecError> {
        let obj = require_object(value, "channel_state")?;
        let mut state = ChannelState::new(decode_config(get(obj, "config")?)?);
        state.bridge = decode_snapshot(get(obj, "bridge")?)?;
        state.relayers = RelayerSet::from_value(get(obj, "relayers")?)?;
        let receipt_values = require_array(get(obj, "receipts")?, "receipts")?;
        let mut receipts = VecDeque::new();
        for entry in receipt_values {
            receipts.push_back(decode_receipt(entry)?);
        }
        state.receipts = receipts;
        let challenge_values = require_array(get(obj, "challenges")?, "challenges")?;
        let mut challenges = Vec::new();
        for entry in challenge_values {
            challenges.push(decode_challenge(entry)?);
        }
        state.challenges = challenges;
        let fingerprint_values =
            require_array(get(obj, "seen_fingerprints")?, "seen_fingerprints")?;
        let mut seen = HashSet::new();
        for entry in fingerprint_values {
            let hex_str = require_string(entry, "seen_fingerprints")?;
            let fp = crypto_suite::hex::decode_array::<32>(hex_str).map_err(|source| {
                CodecError::Hex {
                    field: "seen_fingerprints",
                    source,
                }
            })?;
            seen.insert(fp);
        }
        state.seen_fingerprints = seen;
        state.next_nonce = require_u64(get(obj, "next_nonce")?, "next_nonce")?;
        Ok(state)
    }

    pub(super) fn encode(state: &BridgeState) -> Value {
        let mut channels = Map::new();
        for (asset, channel) in &state.channels {
            channels.insert(asset.clone(), encode_channel(channel));
        }
        let mut bonds = Map::new();
        for (relayer, amount) in &state.relayer_bonds {
            bonds.insert(relayer.clone(), Value::from(*amount));
        }
        let slash_log = Value::Array(state.slash_log.iter().map(encode_slash).collect());
        let mut map = Map::new();
        map.insert("channels".to_string(), Value::Object(channels));
        map.insert("relayer_bonds".to_string(), Value::Object(bonds));
        map.insert(
            "relayer_accounting".to_string(),
            encode_accounting(&state.accounting),
        );
        map.insert("slash_log".to_string(), slash_log);
        map.insert("duties".to_string(), encode_duties(&state.duties));
        map.insert(
            "incentives".to_string(),
            encode_incentives(&state.incentives),
        );
        map.insert("token_bridge".to_string(), state.token_bridge.to_value());
        map.insert(
            "reward_claims".to_string(),
            encode_reward_claims(&state.reward_claims, state.next_claim_id),
        );
        map.insert(
            "reward_accruals".to_string(),
            encode_reward_accruals(&state.reward_accruals, state.next_accrual_id),
        );
        map.insert(
            "pending_rewards".to_string(),
            encode_pending_rewards(&state.pending_rewards),
        );
        map.insert(
            "settlement_log".to_string(),
            encode_settlements(&state.settlement_log),
        );
        map.insert(
            "pending_settlements".to_string(),
            encode_pending_settlements(&state.pending_settlements),
        );
        map.insert(
            "settlement_fingerprints".to_string(),
            Value::Array(
                state
                    .settlement_fingerprints
                    .iter()
                    .map(|fp| Value::String(crypto_suite::hex::encode(fp)))
                    .collect(),
            ),
        );
        map.insert(
            "dispute_history".to_string(),
            encode_dispute_history(&state.dispute_history),
        );
        let mut watermarks = Map::new();
        for (key, height) in &state.settlement_height_watermarks {
            watermarks.insert(key.clone(), Value::Number(Number::from(*height)));
        }
        map.insert(
            "settlement_height_watermarks".to_string(),
            Value::Object(watermarks),
        );
        Value::Object(map)
    }

    pub(super) fn decode(value: &Value) -> Result<BridgeState, CodecError> {
        let obj = require_object(value, "bridge_state")?;
        let channel_obj = require_object(get(obj, "channels")?, "channels")?;
        let mut channels = HashMap::new();
        for (asset, entry) in channel_obj.iter() {
            channels.insert(asset.clone(), decode_channel(entry)?);
        }
        let bonds_obj = require_object(get(obj, "relayer_bonds")?, "relayer_bonds")?;
        let mut relayer_bonds = HashMap::new();
        for (relayer, value) in bonds_obj.iter() {
            relayer_bonds.insert(relayer.clone(), require_u64(value, "relayer_bond")?);
        }
        let accounting = if let Some(accounting_value) = obj.get("relayer_accounting") {
            decode_accounting(accounting_value)?
        } else {
            HashMap::new()
        };
        let slash_values = require_array(get(obj, "slash_log")?, "slash_log")?;
        let mut slash_log = Vec::new();
        for entry in slash_values {
            slash_log.push(decode_slash(entry)?);
        }
        let duties = if let Some(duty_value) = obj.get("duties") {
            decode_duties(duty_value)?
        } else {
            DutyStore::default()
        };
        let incentives = if let Some(value) = obj.get("incentives") {
            decode_incentives(value).unwrap_or_else(|_| BridgeIncentiveParameters::default())
        } else {
            BridgeIncentiveParameters::default()
        };
        let token_bridge = TokenBridge::from_value(get(obj, "token_bridge")?)?;
        let (reward_claims, next_claim_id) = if let Some(value) = obj.get("reward_claims") {
            decode_reward_claims(value)?
        } else {
            (VecDeque::new(), 0)
        };
        let (reward_accruals, next_accrual_id) = if let Some(value) = obj.get("reward_accruals") {
            decode_reward_accruals(value)?
        } else {
            (VecDeque::new(), 0)
        };
        let pending_rewards = if let Some(value) = obj.get("pending_rewards") {
            decode_pending_rewards(value)?
        } else {
            HashMap::new()
        };
        let settlement_log = if let Some(value) = obj.get("settlement_log") {
            decode_settlements(value)?
        } else {
            VecDeque::new()
        };
        let pending_settlements = if let Some(value) = obj.get("pending_settlements") {
            decode_pending_settlements(value)?
        } else {
            HashMap::new()
        };
        let settlement_fingerprints = if let Some(value) = obj.get("settlement_fingerprints") {
            let arr = require_array(value, "settlement_fingerprints")?;
            let mut set = HashSet::new();
            for entry in arr {
                let fp_hex = require_string(entry, "settlement_fingerprint")?;
                let fp = crypto_suite::hex::decode_array::<32>(fp_hex).map_err(|source| {
                    CodecError::Hex {
                        field: "settlement_fingerprint",
                        source,
                    }
                })?;
                set.insert(fp);
            }
            set
        } else {
            HashSet::new()
        };
        let dispute_history = if let Some(value) = obj.get("dispute_history") {
            decode_dispute_history(value)?
        } else {
            DisputeHistory::default()
        };
        let settlement_height_watermarks =
            if let Some(value) = obj.get("settlement_height_watermarks") {
                let map = require_object(value, "settlement_height_watermarks")?;
                let mut out = HashMap::new();
                for (key, val) in map {
                    out.insert(key.clone(), require_u64(val, "settlement_height")?);
                }
                out
            } else {
                HashMap::new()
            };
        Ok(BridgeState {
            channels,
            relayer_bonds,
            accounting,
            slash_log,
            duties,
            incentives,
            token_bridge,
            pending_rewards,
            reward_accruals,
            reward_claims,
            next_claim_id,
            next_accrual_id,
            settlement_log,
            pending_settlements,
            settlement_fingerprints,
            dispute_history,
            settlement_height_watermarks,
        })
    }
}

pub struct Bridge {
    db: SimpleDb,
    sled: SledDb,
    state: BridgeState,
}

impl Default for Bridge {
    fn default() -> Self {
        let db = SimpleDb::default();
        Self::with_db(db)
    }
}

impl Bridge {
    pub fn open(path: &str) -> Self {
        let db = SimpleDb::open_named(names::BRIDGE, path);
        let sled_path = format!("{path}_sled");
        let sled = SledConfig::new()
            .path(&sled_path)
            .open()
            .unwrap_or_else(|e| panic!("open bridge sled store at {sled_path}: {e}"));
        Self::with_storage(db, sled)
    }

    pub fn with_db(db: SimpleDb) -> Self {
        let sled = if let Ok(path) = std::env::var("TB_BRIDGE_SLED_PATH") {
            SledConfig::new()
                .path(&path)
                .open()
                .unwrap_or_else(|e| panic!("open bridge sled store at {path}: {e}"))
        } else {
            SledConfig::new()
                .temporary(true)
                .open()
                .expect("open temporary bridge sled store")
        };
        Self::with_storage(db, sled)
    }

    fn with_storage(db: SimpleDb, sled: SledDb) -> Self {
        let mut state = Self::load_state(&db, &sled);
        Self::normalize_state(&mut state);
        set_global_incentives(state.incentives.clone());
        Self { db, sled, state }
    }

    fn load_state(db: &SimpleDb, sled: &SledDb) -> BridgeState {
        if let Ok(Some(bytes)) = sled.get(STATE_KEY) {
            if let Ok(value) = json::value_from_slice(bytes.as_ref()) {
                if let Ok(state) = state_codec::decode(&value) {
                    return state;
                }
            }
        }
        db.get(STATE_KEY)
            .and_then(|bytes| json::value_from_slice(&bytes).ok())
            .and_then(|value| state_codec::decode(&value).ok())
            .unwrap_or_default()
    }

    fn normalize_state(state: &mut BridgeState) {
        for (relayer, bond) in state.relayer_bonds.clone() {
            let entry = state
                .accounting
                .entry(relayer)
                .or_insert_with(RelayerAccounting::default);
            entry.bond = bond;
        }
        for channel in state.channels.values() {
            for relayer_id in channel.relayers.iter().map(|(id, _)| id.clone()) {
                state
                    .accounting
                    .entry(relayer_id)
                    .or_insert_with(RelayerAccounting::default);
            }
        }
        if state.incentives.min_bond == 0 {
            state.incentives = BridgeIncentiveParameters::default();
        }
        if state.next_claim_id == 0 {
            let max_id = state
                .reward_claims
                .iter()
                .map(|record| record.id)
                .max()
                .unwrap_or(0);
            state.next_claim_id = max_id.saturating_add(1);
        }
        if state.next_accrual_id == 0 {
            let max_id = state
                .reward_accruals
                .iter()
                .map(|record| record.id)
                .max()
                .unwrap_or(0);
            state.next_accrual_id = max_id.saturating_add(1);
        }
        if state.settlement_fingerprints.is_empty() {
            for record in &state.settlement_log {
                state
                    .settlement_fingerprints
                    .insert(Bridge::settlement_record_fingerprint(record));
            }
        }
        state.settlement_height_watermarks.clear();
        for record in &state.settlement_log {
            if let Some(chain) = &record.settlement_chain {
                let key = Self::settlement_watermark_key(&record.asset, chain, &record.commitment);
                let entry = state.settlement_height_watermarks.entry(key).or_insert(0);
                *entry = (*entry).max(record.settlement_height);
            }
        }
        for (_asset, channel) in &state.channels {
            if channel.config.requires_settlement_proof {
                for (commitment, _) in &channel.bridge.pending_withdrawals {
                    state
                        .pending_settlements
                        .entry(*commitment)
                        .or_insert_with(|| SettlementState {
                            required_chain: channel.config.settlement_chain.clone(),
                            duty_ids: Vec::new(),
                            proof: None,
                        });
                }
            }
        }
        for (_commitment, pending) in &state.pending_settlements {
            if let Some(record) = &pending.proof {
                if let Some(chain) = &record.settlement_chain {
                    let key =
                        Self::settlement_watermark_key(&record.asset, chain, &record.commitment);
                    let entry = state.settlement_height_watermarks.entry(key).or_insert(0);
                    *entry = (*entry).max(record.settlement_height);
                }
            }
        }
        state.pending_settlements.retain(|commitment, _| {
            state
                .channels
                .values()
                .any(|channel| channel.bridge.pending_withdrawals.contains_key(commitment))
        });
    }

    fn persist(&mut self) -> Result<(), BridgeError> {
        let value = state_codec::encode(&self.state);
        let rendered = json::to_string_value_pretty(&value);
        let bytes = rendered.as_bytes();
        self.db
            .put(STATE_KEY.as_bytes(), bytes)
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
        self.sled
            .insert(STATE_KEY, bytes)
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
        self.sled
            .flush()
            .map_err(|e| BridgeError::Storage(e.to_string()))
    }

    fn ensure_channel(&mut self, asset: &str) -> &mut ChannelState {
        self.state
            .channels
            .entry(asset.to_string())
            .or_insert_with(|| ChannelState::new(ChannelConfig::for_asset(asset)))
    }

    pub fn channel_config(&self, asset: &str) -> Option<ChannelConfig> {
        self.state
            .channels
            .get(asset)
            .map(|channel| channel.config.clone())
    }

    pub fn set_channel_config(
        &mut self,
        asset: &str,
        mut config: ChannelConfig,
    ) -> Result<(), BridgeError> {
        config.asset = asset.to_string();
        if let Err(err) = fs::create_dir_all(Path::new(&config.headers_dir)) {
            return Err(BridgeError::Storage(err.to_string()));
        }
        if let Some(channel) = self.state.channels.get_mut(asset) {
            channel.config = config;
        } else {
            self.state
                .channels
                .insert(asset.to_string(), ChannelState::new(config));
        }
        self.persist()
    }

    fn fingerprint(header: &PowHeader, proof: &Proof) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(&header_hash(&Self::as_light_header(header)));
        hasher.update(&proof.leaf);
        for limb in &proof.path {
            hasher.update(limb);
        }
        *hasher.finalize().as_bytes()
    }

    fn settlement_fingerprint(asset: &str, proof: &ExternalSettlementProof) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(asset.as_bytes());
        hasher.update(&proof.commitment);
        hasher.update(proof.settlement_chain.as_bytes());
        hasher.update(&proof.proof_hash);
        hasher.update(&proof.settlement_height.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    fn settlement_watermark_key(asset: &str, chain: &str, commitment: &[u8; 32]) -> String {
        format!("{asset}:{chain}:{}", crypto_suite::hex::encode(commitment))
    }

    fn settlement_record_fingerprint(record: &SettlementRecord) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(record.asset.as_bytes());
        hasher.update(&record.commitment);
        if let Some(chain) = &record.settlement_chain {
            hasher.update(chain.as_bytes());
        }
        hasher.update(&record.proof_hash);
        hasher.update(&record.settlement_height.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    fn as_light_header(header: &PowHeader) -> LightHeader {
        LightHeader {
            chain_id: header.chain_id.clone(),
            height: header.height,
            merkle_root: header.merkle_root,
            signature: header.signature,
        }
    }

    fn apply_slash(&mut self, relayer: &str, asset: &str, delta: u64) {
        if delta == 0 {
            return;
        }
        let bond = self
            .state
            .relayer_bonds
            .entry(relayer.to_string())
            .or_insert(0);
        let new_bond = bond.saturating_sub(delta);
        *bond = new_bond;
        {
            let accounting = self.accounting_mut(relayer);
            accounting.debit_bond(delta);
            if delta > 0 {
                accounting.apply_penalty(delta);
            }
        }
        let removed = self.remove_pending_reward(asset, delta);
        if removed > 0 {
            crate::telemetry::adjust_bridge_rewards_pending(-(removed as i64));
        }
        let record = SlashRecord {
            relayer: relayer.to_string(),
            asset: asset.to_string(),
            slashes: delta,
            remaining_bond: new_bond,
            occurred_at: now_secs(),
        };
        self.state.slash_log.push(record);
        if self.state.slash_log.len() > SLASH_RETENTION {
            let drop = self.state.slash_log.len() - SLASH_RETENTION;
            self.state.slash_log.drain(0..drop);
        }
    }

    fn sync_relayer_diffs(
        &mut self,
        asset: &str,
        before: HashMap<String, bridges::relayer::Relayer>,
        after: HashMap<String, bridges::relayer::Relayer>,
    ) {
        for (id, new_state) in after {
            let prev_stake = before.get(&id).map(|r| r.stake).unwrap_or(0);
            if new_state.stake < prev_stake {
                self.apply_slash(&id, asset, prev_stake - new_state.stake);
            }
        }
    }

    fn add_pending_reward(&mut self, asset: &str, amount: u64) {
        if amount == 0 {
            return;
        }
        let entry = self
            .state
            .pending_rewards
            .entry(asset.to_string())
            .or_insert(0);
        *entry = entry.saturating_add(amount);
    }

    fn remove_pending_reward(&mut self, asset: &str, amount: u64) -> u64 {
        if amount == 0 {
            return 0;
        }
        if let Some(entry) = self.state.pending_rewards.get_mut(asset) {
            let removed = (*entry).min(amount);
            *entry -= removed;
            if *entry == 0 {
                self.state.pending_rewards.remove(asset);
            }
            removed
        } else {
            0
        }
    }

    fn consume_pending_rewards(&mut self, mut amount: u64) -> u64 {
        if amount == 0 {
            return 0;
        }
        let mut assets: Vec<String> = self.state.pending_rewards.keys().cloned().collect();
        assets.sort();
        let mut removed_total = 0;
        for asset in assets {
            if amount == 0 {
                break;
            }
            let removed = self.remove_pending_reward(&asset, amount);
            amount -= removed;
            removed_total += removed;
        }
        removed_total
    }

    fn pending_rewards_for_asset(&self, asset: &str) -> u64 {
        self.state
            .pending_rewards
            .get(asset)
            .copied()
            .unwrap_or_default()
    }

    fn refresh_incentives(&mut self) {
        let global = global_incentives();
        if self.state.incentives != global {
            self.state.incentives = global;
        }
    }

    fn incentives(&self) -> &BridgeIncentiveParameters {
        &self.state.incentives
    }

    fn accounting_mut(&mut self, relayer: &str) -> &mut RelayerAccounting {
        let bond = self
            .state
            .relayer_bonds
            .get(relayer)
            .copied()
            .unwrap_or_default();
        let entry = self
            .state
            .accounting
            .entry(relayer.to_string())
            .or_insert_with(RelayerAccounting::default);
        entry.bond = bond;
        entry
    }

    fn accounting_snapshot(&self, relayer: &str) -> RelayerAccounting {
        self.state
            .accounting
            .get(relayer)
            .cloned()
            .unwrap_or_default()
    }

    fn push_reward_claim(&mut self, record: RewardClaimRecord) {
        self.state.reward_claims.push_back(record);
        while self.state.reward_claims.len() > REWARD_CLAIM_RETENTION {
            self.state.reward_claims.pop_front();
        }
    }

    fn record_reward_accrual(&mut self, duty: &DutyRecord, amount: u64) {
        if amount == 0 {
            return;
        }
        let mut commitment = None;
        let mut settlement_chain = None;
        let mut proof_hash = None;
        let duty_kind = match &duty.kind {
            DutyKind::Deposit => "deposit".to_string(),
            DutyKind::Withdrawal { commitment: value } => {
                commitment = Some(*value);
                "withdrawal".to_string()
            }
            DutyKind::Settlement {
                commitment: value,
                settlement_chain: chain,
                proof_hash: hash,
            } => {
                commitment = Some(*value);
                settlement_chain = Some(chain.clone());
                proof_hash = Some(*hash);
                "settlement".to_string()
            }
        };
        let record = RewardAccrualRecord {
            id: self.state.next_accrual_id,
            relayer: duty.relayer.clone(),
            asset: duty.asset.clone(),
            user: duty.user.clone(),
            amount,
            duty_id: duty.id,
            duty_kind,
            commitment,
            settlement_chain,
            proof_hash,
            bundle_relayers: duty.bundle_relayers.clone(),
            recorded_at: now_secs(),
        };
        self.state.next_accrual_id = self.state.next_accrual_id.saturating_add(1);
        self.add_pending_reward(&record.asset, amount);
        crate::telemetry::record_bridge_reward_accrual(&record.asset, amount);
        self.state.reward_accruals.push_back(record);
        while self.state.reward_accruals.len() > REWARD_ACCRUAL_RETENTION {
            self.state.reward_accruals.pop_front();
        }
    }

    fn refresh_dispute_history(&mut self, asset: Option<&str>) {
        let records = self.live_dispute_records(asset);
        for record in records.into_values() {
            self.state.dispute_history.upsert(record);
        }
    }

    fn record_settlement(&mut self, record: SettlementRecord) {
        self.state.settlement_log.push_back(record);
        while self.state.settlement_log.len() > SETTLEMENT_RETENTION {
            self.state.settlement_log.pop_front();
        }
    }

    fn relayer_entries(
        &self,
        asset_filter: Option<&str>,
        relayer_filter: Option<&str>,
    ) -> Vec<(String, RelayerInfo)> {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for (asset, channel) in &self.state.channels {
            if asset_filter.is_some() && asset_filter != Some(asset.as_str()) {
                continue;
            }
            for (id, relayer_state) in channel.relayers.iter() {
                if let Some(filter) = relayer_filter {
                    if filter != id {
                        continue;
                    }
                }
                let accounting = self.accounting_snapshot(id);
                let pending = self.state.duties.pending_count_for_relayer(id);
                entries.push((
                    asset.clone(),
                    RelayerInfo {
                        id: id.clone(),
                        stake: relayer_state.stake,
                        slashes: relayer_state.slashes,
                        bond: accounting.bond,
                        duties_assigned: accounting.duties_assigned,
                        duties_completed: accounting.duties_completed,
                        duties_failed: accounting.duties_failed,
                        rewards_earned: accounting.rewards_earned,
                        rewards_pending: accounting.rewards_pending,
                        rewards_claimed: accounting.rewards_claimed,
                        penalties_applied: accounting.penalties_applied,
                        pending_duties: pending,
                    },
                ));
                seen.insert(id.clone());
            }
        }
        for (relayer_id, accounting) in &self.state.accounting {
            if seen.contains(relayer_id) {
                continue;
            }
            if let Some(filter) = relayer_filter {
                if filter != relayer_id {
                    continue;
                }
            }
            entries.push((
                "*".to_string(),
                RelayerInfo {
                    id: relayer_id.clone(),
                    stake: 0,
                    slashes: 0,
                    bond: accounting.bond,
                    duties_assigned: accounting.duties_assigned,
                    duties_completed: accounting.duties_completed,
                    duties_failed: accounting.duties_failed,
                    rewards_earned: accounting.rewards_earned,
                    rewards_pending: accounting.rewards_pending,
                    rewards_claimed: accounting.rewards_claimed,
                    penalties_applied: accounting.penalties_applied,
                    pending_duties: 0,
                },
            ));
        }
        entries.sort_by(|(asset_a, info_a), (asset_b, info_b)| {
            asset_a.cmp(asset_b).then_with(|| info_a.id.cmp(&info_b.id))
        });
        entries
    }

    fn ensure_min_bond(&mut self, relayer: &str) -> Result<(), BridgeError> {
        let required = self.incentives().min_bond;
        let available = self
            .state
            .relayer_bonds
            .get(relayer)
            .copied()
            .unwrap_or_default();
        if available < required {
            return Err(BridgeError::InsufficientBond {
                relayer: relayer.to_string(),
                required,
                available,
            });
        }
        Ok(())
    }

    fn assign_duty(
        &mut self,
        asset: &str,
        relayer: &str,
        user: &str,
        amount: u64,
        kind: DutyKind,
        bundle_relayers: Vec<String>,
    ) -> u64 {
        let now = now_secs();
        let deadline = now + self.incentives().duty_window_secs;
        let record = DutyRecord {
            id: 0,
            relayer: relayer.to_string(),
            asset: asset.to_string(),
            user: user.to_string(),
            amount,
            assigned_at: now,
            deadline,
            bundle_relayers,
            kind,
            status: DutyStatus::Pending,
        };
        let id = self.state.duties.assign(record);
        crate::telemetry::increment_bridge_pending_duties();
        self.accounting_mut(relayer).assign_duty();
        id
    }

    fn record_duty_success(&mut self, duty_id: u64, reward: u64) {
        let completed_at = now_secs();
        if let Some(record) = self.state.duties.update_status(
            duty_id,
            DutyStatus::Completed {
                reward,
                completed_at,
            },
        ) {
            if let Some(kind_label) = duty_kind_label(&record.kind) {
                telemetry_record_dispute(kind_label, "success");
            }
            let relayer_id = record.relayer.clone();
            crate::telemetry::decrement_bridge_pending_duties();
            let accounting = self.accounting_mut(&relayer_id);
            accounting.complete_duty();
            if reward > 0 {
                accounting.accrue_reward(reward);
                self.record_reward_accrual(&record, reward);
            }
            let channel_asset = record.asset;
            if let Some(channel) = self.state.channels.get_mut(&channel_asset) {
                channel.relayers.mark_duty_completion(&relayer_id);
            }
        }
    }

    fn record_duty_failure(&mut self, duty_id: u64, penalty: u64, reason: DutyFailureReason) {
        let failed_at = now_secs();
        let reason_label = reason.as_str();
        let status_reason = reason.clone();
        if let Some(record) = self.state.duties.update_status(
            duty_id,
            DutyStatus::Failed {
                penalty,
                failed_at,
                reason: status_reason,
            },
        ) {
            if let Some(kind_label) = duty_kind_label(&record.kind) {
                telemetry_record_dispute(kind_label, reason_label);
            }
            let relayer_id = record.relayer.clone();
            crate::telemetry::decrement_bridge_pending_duties();
            let accounting = self.accounting_mut(&relayer_id);
            accounting.fail_duty();
            if let Some(channel) = self.state.channels.get_mut(&record.asset) {
                channel.relayers.mark_duty_failure(&relayer_id);
            }
        }
    }

    fn pending_duty_ids(&self, commitment: &[u8; 32]) -> Vec<u64> {
        self.state.duties.duties_for_commitment(commitment)
    }

    pub fn bond_relayer(&mut self, relayer: &str, amount: u64) -> Result<(), BridgeError> {
        let entry = self
            .state
            .relayer_bonds
            .entry(relayer.to_string())
            .or_insert(0);
        *entry = entry.saturating_add(amount);
        self.accounting_mut(relayer).credit_bond(amount);
        self.persist()
    }

    pub fn claim_rewards(
        &mut self,
        relayer: &str,
        amount: u64,
        approval_key: &str,
    ) -> Result<RewardClaimRecord, BridgeError> {
        self.refresh_incentives();
        if amount == 0 {
            return Err(BridgeError::RewardClaimAmountZero);
        }
        let pending_before = self.accounting_snapshot(relayer).rewards_pending;
        if pending_before < amount {
            return Err(BridgeError::RewardInsufficientPending {
                relayer: relayer.to_string(),
                available: pending_before,
                requested: amount,
            });
        }
        let approval = governance::ensure_reward_claim_authorized(approval_key, relayer, amount)
            .map_err(BridgeError::RewardClaimRejected)?;
        let claimed_at = now_secs();
        let accounting = self.accounting_mut(relayer);
        accounting.mark_claimed(amount);
        let pending_after = accounting.rewards_pending;
        let claimed_amount = pending_before.saturating_sub(pending_after);
        let drained = self.consume_pending_rewards(claimed_amount);
        if drained > 0 {
            crate::telemetry::adjust_bridge_rewards_pending(-(drained as i64));
        }
        let record = RewardClaimRecord {
            id: self.state.next_claim_id,
            relayer: relayer.to_string(),
            amount: claimed_amount,
            approval_key: approval.key,
            claimed_at,
            pending_before,
            pending_after,
        };
        self.state.next_claim_id = self.state.next_claim_id.saturating_add(1);
        self.push_reward_claim(record.clone());
        telemetry_record_reward_claim(claimed_amount);
        self.persist()?;
        Ok(record)
    }

    pub fn deposit(
        &mut self,
        asset: &str,
        relayer: &str,
        user: &str,
        amount: u64,
        header: &PowHeader,
        proof: &Proof,
        bundle: &RelayerBundle,
    ) -> Result<DepositReceipt, BridgeError> {
        self.refresh_incentives();
        self.ensure_min_bond(relayer)?;
        let fingerprint = Self::fingerprint(header, proof);
        {
            let channel = self.ensure_channel(asset);
            if !channel.seen_fingerprints.insert(fingerprint) {
                return Err(BridgeError::Replay);
            }
            channel.relayers.stake(relayer, 0);
        }
        let duty_id = self.assign_duty(
            asset,
            relayer,
            user,
            amount,
            DutyKind::Deposit,
            bundle.relayer_ids(),
        );

        let (mut runtime, mut relayers) = {
            let channel = self.ensure_channel(asset);
            (channel.runtime_bridge(), channel.relayers.clone())
        };
        let before = relayers.snapshot();
        let ok = runtime.deposit_with_relayer(
            &mut relayers,
            relayer,
            user,
            amount,
            header,
            proof,
            bundle,
        );
        let after = relayers.snapshot();
        self.sync_relayer_diffs(asset, before, after);
        if !ok {
            {
                let channel = self.ensure_channel(asset);
                channel.relayers = relayers;
                channel.seen_fingerprints.remove(&fingerprint);
            }
            let penalty = self.incentives().failure_slash;
            if penalty > 0 {
                self.apply_slash(relayer, asset, penalty);
            }
            self.record_duty_failure(duty_id, penalty, DutyFailureReason::InvalidProof);
            self.persist()?;
            return Err(BridgeError::InvalidProof);
        }

        let receipt = {
            let channel = self.ensure_channel(asset);
            channel.relayers = relayers;
            channel.update_from_runtime(runtime);
            let receipt = DepositReceipt {
                asset: asset.to_string(),
                nonce: channel.next_nonce,
                user: user.to_string(),
                amount,
                relayer: relayer.to_string(),
                header_hash: header_hash(&Self::as_light_header(header)),
                relayer_commitment: bundle.aggregate_commitment(user, amount),
                proof_fingerprint: fingerprint,
                bundle_relayers: bundle.relayer_ids(),
                recorded_at: now_secs(),
            };
            channel.next_nonce += 1;
            channel.record_receipt(receipt.clone());
            receipt
        };

        let reward = self.incentives().duty_reward;
        self.record_duty_success(duty_id, reward);
        self.state.token_bridge.lock(asset, amount);
        self.persist()?;
        Ok(receipt)
    }

    fn ensure_release_authorized(
        &self,
        asset: &str,
        commitment: &[u8; 32],
    ) -> Result<(), BridgeError> {
        let hash = crypto_suite::hex::encode(commitment);
        let payload = format!("bridge:{asset}:{hash}");
        governance::ensure_release_authorized(&payload)
            .map_err(|_| BridgeError::UnauthorizedRelease)
    }

    pub fn request_withdrawal(
        &mut self,
        asset: &str,
        relayer: &str,
        user: &str,
        amount: u64,
        bundle: &RelayerBundle,
    ) -> Result<[u8; 32], BridgeError> {
        self.refresh_incentives();
        let commitment = bundle.aggregate_commitment(user, amount);
        {
            let channel = self
                .state
                .channels
                .get_mut(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            if channel.bridge.pending_withdrawals.contains_key(&commitment) {
                return Err(BridgeError::DuplicateWithdrawal);
            }
        }
        self.ensure_release_authorized(asset, &commitment)?;
        let signer_list = bundle.relayer_ids();
        let mut unique_signers = HashSet::new();
        for signer in &signer_list {
            if unique_signers.insert(signer.clone()) {
                self.ensure_min_bond(signer)?;
            }
        }
        if unique_signers.insert(relayer.to_string()) {
            self.ensure_min_bond(relayer)?;
        }
        {
            let channel = self
                .state
                .channels
                .get_mut(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            channel.relayers.stake(relayer, 0);
            for signer in &signer_list {
                channel.relayers.stake(signer, 0);
            }
        }

        let (mut runtime, mut relayers) = {
            let channel = self
                .state
                .channels
                .get(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            (channel.runtime_bridge(), channel.relayers.clone())
        };
        let primary_duty = self.assign_duty(
            asset,
            relayer,
            user,
            amount,
            DutyKind::Withdrawal { commitment },
            signer_list.clone(),
        );
        let before = relayers.snapshot();
        let ok = runtime.unlock_with_relayer(&mut relayers, relayer, user, amount, bundle);
        let after = relayers.snapshot();
        self.sync_relayer_diffs(asset, before, after);
        if !ok {
            {
                let channel = self
                    .state
                    .channels
                    .get_mut(asset)
                    .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
                channel.relayers = relayers;
            }
            let penalty = self.incentives().failure_slash;
            if penalty > 0 {
                self.apply_slash(relayer, asset, penalty);
            }
            self.record_duty_failure(primary_duty, penalty, DutyFailureReason::InvalidProof);
            self.persist()?;
            return Err(BridgeError::InvalidProof);
        }
        {
            let channel = self
                .state
                .channels
                .get_mut(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            channel.relayers = relayers;
            channel.update_from_runtime(runtime);
            for signer in unique_signers.iter() {
                if signer != relayer {
                    channel.relayers.mark_duty_assignment(signer);
                }
            }
        }
        for signer in unique_signers.iter() {
            if signer != relayer {
                self.assign_duty(
                    asset,
                    signer,
                    user,
                    amount,
                    DutyKind::Withdrawal { commitment },
                    signer_list.clone(),
                );
            }
        }
        self.state.token_bridge.unlock(asset, amount);
        self.state.token_bridge.mint(asset, amount);
        self.refresh_dispute_history(Some(asset));
        self.persist()?;
        Ok(commitment)
    }

    pub fn submit_settlement_proof(
        &mut self,
        asset: &str,
        relayer: &str,
        proof: ExternalSettlementProof,
    ) -> Result<SettlementRecord, BridgeError> {
        self.refresh_incentives();
        if let Err(err) = self.ensure_min_bond(relayer) {
            telemetry_record_settlement_failure("insufficient_bond");
            return Err(err);
        }
        let (bundle_relayers, user, amount, required_chain) = match self.state.channels.get(asset) {
            Some(channel) => {
                if !channel.config.requires_settlement_proof {
                    telemetry_record_settlement_failure("not_tracked");
                    return Err(BridgeError::SettlementProofNotTracked {
                        asset: asset.to_string(),
                    });
                }
                let pending = match channel.bridge.pending_withdrawals.get(&proof.commitment) {
                    Some(pending) => pending,
                    None => {
                        telemetry_record_settlement_failure("withdrawal_missing");
                        return Err(BridgeError::WithdrawalMissing);
                    }
                };
                (
                    pending.relayers.clone(),
                    pending.user.clone(),
                    pending.amount,
                    channel.config.settlement_chain.clone(),
                )
            }
            None => {
                telemetry_record_settlement_failure("unknown_channel");
                return Err(BridgeError::UnknownChannel(asset.to_string()));
            }
        };
        if let Some(expected) = required_chain.as_ref() {
            if expected.as_str() != proof.settlement_chain {
                telemetry_record_settlement_failure("chain_mismatch");
                return Err(BridgeError::SettlementProofChainMismatch {
                    expected: Some(expected.clone()),
                    found: proof.settlement_chain.clone(),
                });
            }
        }
        let expected_hash = settlement_proof_digest(
            asset,
            &proof.commitment,
            &proof.settlement_chain,
            proof.settlement_height,
            &user,
            amount,
            &bundle_relayers,
        );
        if proof.proof_hash != expected_hash {
            telemetry_record_settlement_failure("hash_mismatch");
            return Err(BridgeError::SettlementProofHashMismatch {
                expected: expected_hash,
                found: proof.proof_hash,
            });
        }
        let watermark_key =
            Self::settlement_watermark_key(asset, &proof.settlement_chain, &proof.commitment);
        if let Some(previous) = self.state.settlement_height_watermarks.get(&watermark_key) {
            if proof.settlement_height <= *previous {
                telemetry_record_settlement_failure("height_replay");
                return Err(BridgeError::SettlementProofHeightReplay {
                    chain: proof.settlement_chain.clone(),
                    previous: *previous,
                    submitted: proof.settlement_height,
                });
            }
        }
        let fingerprint = Self::settlement_fingerprint(asset, &proof);
        if !self.state.settlement_fingerprints.insert(fingerprint) {
            telemetry_record_settlement_failure("duplicate");
            return Err(BridgeError::SettlementProofDuplicate);
        }
        {
            let entry = self
                .state
                .pending_settlements
                .entry(proof.commitment)
                .or_insert_with(|| SettlementState {
                    required_chain: required_chain.clone(),
                    duty_ids: Vec::new(),
                    proof: None,
                });
            if entry.proof.is_some() {
                telemetry_record_settlement_failure("duplicate");
                return Err(BridgeError::SettlementProofDuplicate);
            }
        }
        let duty_id = self.assign_duty(
            asset,
            relayer,
            &user,
            amount,
            DutyKind::Settlement {
                commitment: proof.commitment,
                settlement_chain: proof.settlement_chain.clone(),
                proof_hash: proof.proof_hash,
            },
            bundle_relayers.clone(),
        );
        let record = SettlementRecord {
            asset: asset.to_string(),
            commitment: proof.commitment,
            relayer: relayer.to_string(),
            settlement_chain: required_chain
                .clone()
                .or_else(|| Some(proof.settlement_chain.clone())),
            proof_hash: proof.proof_hash,
            settlement_height: proof.settlement_height,
            submitted_at: now_secs(),
        };
        if let Some(entry) = self.state.pending_settlements.get_mut(&proof.commitment) {
            entry.duty_ids.push(duty_id);
            entry.proof = Some(record.clone());
        }
        self.state
            .settlement_height_watermarks
            .insert(watermark_key, proof.settlement_height);
        self.record_settlement(record.clone());
        self.record_duty_success(duty_id, self.incentives().duty_reward);
        self.state.token_bridge.burn(asset, amount);
        telemetry_record_settlement_success();
        self.refresh_dispute_history(Some(asset));
        self.persist()?;
        Ok(record)
    }

    pub fn challenge_withdrawal(
        &mut self,
        asset: &str,
        commitment: [u8; 32],
        challenger: &str,
    ) -> Result<ChallengeRecord, BridgeError> {
        {
            let channel = self
                .state
                .channels
                .get(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            match channel.bridge.pending_withdrawals.get(&commitment) {
                Some(pending) if pending.challenged => {
                    return Err(BridgeError::AlreadyChallenged);
                }
                Some(_) => {}
                None => return Err(BridgeError::WithdrawalMissing),
            }
        }

        let (mut runtime, mut relayers) = {
            let channel = self
                .state
                .channels
                .get(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            (channel.runtime_bridge(), channel.relayers.clone())
        };
        let before = relayers.snapshot();
        if !runtime.challenge_withdrawal(&mut relayers, commitment) {
            return Err(BridgeError::AlreadyChallenged);
        }
        let after = relayers.snapshot();
        self.sync_relayer_diffs(asset, before, after);
        let record = {
            let channel = self
                .state
                .channels
                .get_mut(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            channel.relayers = relayers;
            channel.update_from_runtime(runtime);
            let record = ChallengeRecord {
                asset: asset.to_string(),
                commitment,
                challenger: challenger.to_string(),
                challenged_at: now_secs(),
            };
            channel.record_challenge(record.clone());
            record
        };
        let duty_ids = self.pending_duty_ids(&commitment);
        let penalty = self.incentives().challenge_slash;
        for duty_id in duty_ids {
            if let Some(record) = self.state.duties.get(duty_id).cloned() {
                if penalty > 0 {
                    self.apply_slash(&record.relayer, asset, penalty);
                }
            }
            self.record_duty_failure(duty_id, penalty, DutyFailureReason::ChallengeAccepted);
        }
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_CHALLENGES_TOTAL.inc();
        }
        self.refresh_dispute_history(Some(asset));
        self.persist()?;
        Ok(record)
    }

    pub fn finalize_withdrawal(
        &mut self,
        asset: &str,
        commitment: [u8; 32],
    ) -> Result<(), BridgeError> {
        let channel = self
            .state
            .channels
            .get_mut(asset)
            .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
        let requires_settlement_proof = channel.config.requires_settlement_proof;
        let pending = match channel.bridge.pending_withdrawals.get(&commitment) {
            Some(pending) => pending,
            None => return Err(BridgeError::WithdrawalMissing),
        };
        if pending.challenged {
            return Err(BridgeError::AlreadyChallenged);
        }
        let deadline = pending.initiated_at + channel.config.challenge_period_secs;
        if now_secs() < deadline {
            return Err(BridgeError::ChallengeWindowOpen);
        }
        if requires_settlement_proof {
            match self.state.pending_settlements.get(&commitment) {
                Some(state) if state.proof.is_some() => {}
                Some(_) | None => {
                    return Err(BridgeError::SettlementProofRequired {
                        asset: asset.to_string(),
                        commitment,
                    });
                }
            }
        }
        let pending_amount = pending.amount;
        let mut runtime = channel.runtime_bridge();
        if !runtime.finalize_withdrawal(commitment) {
            return Err(BridgeError::ChallengeWindowOpen);
        }
        channel.update_from_runtime(runtime);
        let reward = self.incentives().duty_reward;
        let duty_ids = self.pending_duty_ids(&commitment);
        for duty_id in duty_ids {
            self.record_duty_success(duty_id, reward);
        }
        self.state.pending_settlements.remove(&commitment);
        if !requires_settlement_proof {
            self.state.token_bridge.burn(asset, pending_amount);
        }
        self.refresh_dispute_history(Some(asset));
        self.persist()
    }

    pub fn locked_balance(&self, asset: &str, user: &str) -> Option<u64> {
        self.state
            .channels
            .get(asset)
            .and_then(|c| c.bridge.locked.get(user).copied())
    }

    pub fn pending_withdrawals(&self, asset: Option<&str>) -> Vec<PendingWithdrawalInfo> {
        let mut out = Vec::new();
        for (chan_asset, channel) in &self.state.channels {
            if asset.is_some() && asset != Some(chan_asset.as_str()) {
                continue;
            }
            for (commitment, pending) in &channel.bridge.pending_withdrawals {
                let deadline = pending.initiated_at + channel.config.challenge_period_secs;
                let settlement_state = self.state.pending_settlements.get(commitment);
                let settlement_submitted_at = settlement_state
                    .and_then(|state| state.proof.as_ref().map(|record| record.submitted_at));
                let settlement_chain = settlement_state
                    .and_then(|state| state.required_chain.clone())
                    .or_else(|| {
                        if channel.config.requires_settlement_proof {
                            channel.config.settlement_chain.clone()
                        } else {
                            None
                        }
                    });
                out.push((
                    pending.initiated_at,
                    PendingWithdrawalInfo {
                        asset: chan_asset.clone(),
                        commitment: *commitment,
                        user: pending.user.clone(),
                        amount: pending.amount,
                        relayers: pending.relayers.clone(),
                        initiated_at: pending.initiated_at,
                        deadline,
                        challenged: pending.challenged,
                        requires_settlement_proof: channel.config.requires_settlement_proof,
                        settlement_chain,
                        settlement_submitted_at,
                    },
                ));
            }
        }
        out.sort_by_key(|(initiated, _)| *initiated);
        out.into_iter().map(|(_, value)| value).collect()
    }

    pub fn relayer_accounting(
        &self,
        relayer: Option<&str>,
        asset: Option<&str>,
    ) -> Vec<(String, RelayerInfo)> {
        self.relayer_entries(asset, relayer)
    }

    pub fn reward_accruals(
        &self,
        relayer: Option<&str>,
        asset: Option<&str>,
        cursor: Option<u64>,
        limit: usize,
    ) -> (Vec<RewardAccrualRecord>, Option<u64>) {
        let accruals: Vec<_> = self
            .state
            .reward_accruals
            .iter()
            .cloned()
            .filter(|record| {
                relayer
                    .map(|target| target == record.relayer.as_str())
                    .unwrap_or(true)
            })
            .filter(|record| {
                asset
                    .map(|target| target == record.asset.as_str())
                    .unwrap_or(true)
            })
            .collect();
        paginate(accruals, cursor, limit)
    }

    pub fn incentive_summary(&self) -> Vec<IncentiveSummaryEntry> {
        let mut assets: HashSet<String> = self.state.channels.keys().cloned().collect();
        assets.extend(self.state.pending_rewards.keys().cloned());
        for record in &self.state.reward_accruals {
            assets.insert(record.asset.clone());
        }
        let mut asset_list: Vec<String> = assets.into_iter().collect();
        asset_list.sort();
        asset_list
            .into_iter()
            .map(|asset| {
                let pending_duties = self
                    .state
                    .duties
                    .records()
                    .iter()
                    .filter(|record| record.asset == asset && record.is_pending())
                    .count();
                let claimable_rewards = self.pending_rewards_for_asset(&asset);
                let receipt_count = self
                    .state
                    .reward_accruals
                    .iter()
                    .filter(|record| record.asset == asset)
                    .count();
                let active_relayers = self
                    .state
                    .channels
                    .get(&asset)
                    .map(|channel| channel.relayers.iter().count())
                    .unwrap_or(0);
                IncentiveSummaryEntry {
                    asset,
                    pending_duties,
                    claimable_rewards,
                    receipt_count,
                    active_relayers,
                }
            })
            .collect()
    }

    pub fn reward_claims(
        &self,
        relayer: Option<&str>,
        cursor: Option<u64>,
        limit: usize,
    ) -> (Vec<RewardClaimRecord>, Option<u64>) {
        let claims: Vec<_> = self
            .state
            .reward_claims
            .iter()
            .cloned()
            .filter(|record| {
                relayer
                    .map(|target| target == record.relayer.as_str())
                    .unwrap_or(true)
            })
            .collect();
        paginate(claims, cursor, limit)
    }

    pub fn settlement_records(
        &self,
        asset: Option<&str>,
        cursor: Option<u64>,
        limit: usize,
    ) -> (Vec<SettlementRecord>, Option<u64>) {
        let settlements: Vec<_> = self
            .state
            .settlement_log
            .iter()
            .cloned()
            .filter(|record| {
                asset
                    .map(|target| target == record.asset.as_str())
                    .unwrap_or(true)
            })
            .collect();
        paginate(settlements, cursor, limit)
    }

    pub fn supported_assets(&self) -> Vec<String> {
        let mut assets: Vec<String> = self.state.channels.keys().cloned().collect();
        assets.sort();
        assets
    }

    pub fn asset_snapshots(&self) -> Vec<AssetSnapshot> {
        self.state.token_bridge.asset_snapshots()
    }

    fn live_dispute_records(&self, asset: Option<&str>) -> HashMap<[u8; 32], DisputeAuditRecord> {
        let mut builders: HashMap<[u8; 32], DisputeAuditRecord> = HashMap::new();
        let now = now_secs();
        for (chan_asset, channel) in &self.state.channels {
            if asset.is_some() && asset != Some(chan_asset.as_str()) {
                continue;
            }
            for (commitment, pending) in &channel.bridge.pending_withdrawals {
                let deadline = pending.initiated_at + channel.config.challenge_period_secs;
                let entry = builders
                    .entry(*commitment)
                    .or_insert_with(|| DisputeAuditRecord {
                        asset: chan_asset.clone(),
                        commitment: *commitment,
                        user: pending.user.clone(),
                        amount: pending.amount,
                        initiated_at: pending.initiated_at,
                        deadline,
                        challenged: pending.challenged,
                        challenger: None,
                        challenged_at: None,
                        settlement_required: channel.config.requires_settlement_proof,
                        settlement_chain: channel.config.settlement_chain.clone(),
                        settlement_submitted_at: None,
                        relayer_outcomes: Vec::new(),
                        expired: now > deadline,
                    });
                entry.asset = chan_asset.clone();
                entry.user = pending.user.clone();
                entry.amount = pending.amount;
                entry.initiated_at = pending.initiated_at;
                entry.deadline = deadline;
                entry.challenged = pending.challenged;
                entry.settlement_required = channel.config.requires_settlement_proof;
                entry.settlement_chain = channel.config.settlement_chain.clone();
                entry.expired = now > deadline && !pending.challenged;
            }
            for challenge in &channel.challenges {
                if asset.is_some() && asset != Some(chan_asset.as_str()) {
                    continue;
                }
                let entry =
                    builders
                        .entry(challenge.commitment)
                        .or_insert_with(|| DisputeAuditRecord {
                            asset: chan_asset.clone(),
                            commitment: challenge.commitment,
                            user: String::new(),
                            amount: 0,
                            initiated_at: challenge.challenged_at,
                            deadline: challenge.challenged_at,
                            challenged: true,
                            challenger: Some(challenge.challenger.clone()),
                            challenged_at: Some(challenge.challenged_at),
                            settlement_required: channel.config.requires_settlement_proof,
                            settlement_chain: channel.config.settlement_chain.clone(),
                            settlement_submitted_at: None,
                            relayer_outcomes: Vec::new(),
                            expired: false,
                        });
                entry.asset = chan_asset.clone();
                entry.challenged = true;
                entry.challenger = Some(challenge.challenger.clone());
                entry.challenged_at = Some(challenge.challenged_at);
            }
        }

        for duty in self.state.duties.records() {
            if let Some(commitment) = duty.commitment() {
                if asset.is_some() && asset != Some(duty.asset.as_str()) {
                    continue;
                }
                let entry = builders
                    .entry(commitment)
                    .or_insert_with(|| DisputeAuditRecord {
                        asset: duty.asset.clone(),
                        commitment,
                        user: duty.user.clone(),
                        amount: duty.amount,
                        initiated_at: duty.assigned_at,
                        deadline: duty.deadline,
                        challenged: false,
                        challenger: None,
                        challenged_at: None,
                        settlement_required: false,
                        settlement_chain: None,
                        settlement_submitted_at: None,
                        relayer_outcomes: Vec::new(),
                        expired: false,
                    });
                entry.asset = duty.asset.clone();
                entry.user = duty.user.clone();
                entry.amount = duty.amount;
                entry.initiated_at = entry.initiated_at.min(duty.assigned_at);
                entry.deadline = entry.deadline.max(duty.deadline);
                entry.expired |= matches!(duty.status, DutyStatus::Pending) && now > duty.deadline;
                let snapshot = match &duty.status {
                    DutyStatus::Pending => DutyOutcomeSnapshot {
                        relayer: duty.relayer.clone(),
                        status: "pending".into(),
                        reward: 0,
                        penalty: 0,
                        completed_at: None,
                        failed_at: None,
                        reason: None,
                        duty_id: duty.id,
                    },
                    DutyStatus::Completed {
                        reward,
                        completed_at,
                    } => DutyOutcomeSnapshot {
                        relayer: duty.relayer.clone(),
                        status: "completed".into(),
                        reward: *reward,
                        penalty: 0,
                        completed_at: Some(*completed_at),
                        failed_at: None,
                        reason: None,
                        duty_id: duty.id,
                    },
                    DutyStatus::Failed {
                        penalty,
                        failed_at,
                        reason,
                    } => DutyOutcomeSnapshot {
                        relayer: duty.relayer.clone(),
                        status: "failed".into(),
                        reward: 0,
                        penalty: *penalty,
                        completed_at: None,
                        failed_at: Some(*failed_at),
                        reason: Some(reason.as_str().to_string()),
                        duty_id: duty.id,
                    },
                };
                entry.relayer_outcomes.push(snapshot);
            }
        }

        for (commitment, state) in &self.state.pending_settlements {
            if let Some(proof) = &state.proof {
                if asset.is_some() && asset != Some(proof.asset.as_str()) {
                    continue;
                }
                let entry = builders
                    .entry(*commitment)
                    .or_insert_with(|| DisputeAuditRecord {
                        asset: proof.asset.clone(),
                        commitment: *commitment,
                        user: String::new(),
                        amount: 0,
                        initiated_at: proof.submitted_at,
                        deadline: proof.submitted_at,
                        challenged: false,
                        challenger: None,
                        challenged_at: None,
                        settlement_required: true,
                        settlement_chain: proof.settlement_chain.clone(),
                        settlement_submitted_at: Some(proof.submitted_at),
                        relayer_outcomes: Vec::new(),
                        expired: false,
                    });
                entry.asset = proof.asset.clone();
                entry.settlement_required = true;
                entry.settlement_chain = proof.settlement_chain.clone();
                entry.settlement_submitted_at = Some(proof.submitted_at);
            }
        }

        for record in &self.state.settlement_log {
            if asset.is_some() && asset != Some(record.asset.as_str()) {
                continue;
            }
            let entry = builders
                .entry(record.commitment)
                .or_insert_with(|| DisputeAuditRecord {
                    asset: record.asset.clone(),
                    commitment: record.commitment,
                    user: String::new(),
                    amount: 0,
                    initiated_at: record.submitted_at,
                    deadline: record.submitted_at,
                    challenged: false,
                    challenger: None,
                    challenged_at: None,
                    settlement_required: true,
                    settlement_chain: record.settlement_chain.clone(),
                    settlement_submitted_at: Some(record.submitted_at),
                    relayer_outcomes: Vec::new(),
                    expired: false,
                });
            entry.asset = record.asset.clone();
            entry.settlement_required = true;
            entry.settlement_chain = record.settlement_chain.clone();
            entry.settlement_submitted_at = Some(record.submitted_at);
        }

        builders
    }

    pub fn dispute_audit(
        &self,
        asset: Option<&str>,
        cursor: Option<u64>,
        limit: usize,
    ) -> (Vec<DisputeAuditRecord>, Option<u64>) {
        let mut builders = self.live_dispute_records(asset);

        for record in self.state.dispute_history.iter() {
            if asset.is_some() && asset != Some(record.asset.as_str()) {
                continue;
            }
            builders
                .entry(record.commitment)
                .or_insert_with(|| record.clone());
        }

        let mut records: Vec<DisputeAuditRecord> = builders.into_values().collect();
        records
            .sort_by_key(|record| (record.asset.clone(), record.initiated_at, record.commitment));
        paginate(records, cursor, limit)
    }

    pub fn duty_log(
        &self,
        relayer: Option<&str>,
        asset: Option<&str>,
        limit: usize,
    ) -> Vec<DutyRecord> {
        let mut records: Vec<DutyRecord> = self.state.duties.records();
        if let Some(relayer_id) = relayer {
            records.retain(|record| record.relayer == relayer_id);
        }
        if let Some(asset_id) = asset {
            records.retain(|record| record.asset == asset_id);
        }
        records.sort_by_key(|record| record.id);
        if limit > 0 && records.len() > limit {
            let drop = records.len() - limit;
            records.drain(0..drop);
        }
        records.reverse();
        records
    }

    pub fn relayer_quorum(&self, asset: &str) -> Option<RelayerQuorumInfo> {
        let channel = self.state.channels.get(asset)?;
        let relayers: Vec<RelayerInfo> = self
            .relayer_entries(Some(asset), None)
            .into_iter()
            .map(|(_, info)| info)
            .collect();
        Some(RelayerQuorumInfo {
            asset: asset.to_string(),
            quorum: channel.config.relayer_quorum as u64,
            relayers,
        })
    }

    pub fn deposit_history(
        &self,
        asset: &str,
        cursor: Option<u64>,
        limit: usize,
    ) -> Vec<DepositReceipt> {
        if let Some(channel) = self.state.channels.get(asset) {
            let mut receipts: Vec<_> = channel.receipts.iter().cloned().collect();
            receipts.sort_by_key(|r| r.nonce);
            if let Some(start) = cursor {
                receipts.retain(|r| r.nonce >= start);
            }
            receipts.into_iter().take(limit).collect()
        } else {
            Vec::new()
        }
    }

    pub fn challenges(&self, asset: Option<&str>) -> Vec<ChallengeRecord> {
        let mut out = Vec::new();
        for (chan_asset, channel) in &self.state.channels {
            if asset.is_some() && asset != Some(chan_asset.as_str()) {
                continue;
            }
            out.extend(channel.challenges.iter().cloned());
        }
        out.sort_by_key(|c| c.challenged_at);
        out
    }

    pub fn slash_log(&self) -> &[SlashRecord] {
        &self.state.slash_log
    }

    pub fn relayer_status(
        &self,
        relayer: &str,
        asset: Option<&str>,
    ) -> Option<(String, RelayerInfo)> {
        self.relayer_entries(asset, Some(relayer))
            .into_iter()
            .next()
    }
}

#[cfg(any(test, feature = "integration-tests"))]
impl Bridge {
    pub fn force_enqueue_withdrawal_for_router(
        &mut self,
        asset: &str,
        user: &str,
        amount: u64,
        initiated_at: u64,
    ) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(asset.as_bytes());
        hasher.update(user.as_bytes());
        hasher.update(&amount.to_le_bytes());
        hasher.update(&initiated_at.to_le_bytes());
        let commitment = *hasher.finalize().as_bytes();
        let channel = self.ensure_channel(asset);
        channel.bridge.pending_withdrawals.insert(
            commitment,
            PendingWithdrawal {
                user: user.to_string(),
                amount,
                relayers: vec!["router".into()],
                initiated_at,
                challenged: false,
            },
        );
        self.refresh_dispute_history(Some(asset));
        commitment
    }
}

fn paginate<T>(items: Vec<T>, cursor: Option<u64>, limit: usize) -> (Vec<T>, Option<u64>) {
    if items.is_empty() {
        return (Vec::new(), None);
    }
    let start = cursor
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    if start >= items.len() {
        return (Vec::new(), None);
    }
    let effective_limit = limit.max(1);
    let end = start.saturating_add(effective_limit).min(items.len());
    let next_cursor = (end < items.len()).then_some(end as u64);
    let page = items.into_iter().skip(start).take(end - start).collect();
    (page, next_cursor)
}
