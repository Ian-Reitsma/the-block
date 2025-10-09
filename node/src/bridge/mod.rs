#![forbid(unsafe_code)]

#[cfg(feature = "telemetry")]
use crate::telemetry::BRIDGE_CHALLENGES_TOTAL;
use crate::{governance, simple_db::names, SimpleDb};
use bridges::codec::Error as CodecError;
use bridges::relayer::RelayerSet;
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header as LightHeader, Proof},
    Bridge as ExternalBridge, BridgeConfig, PendingWithdrawal, RelayerBundle, TokenBridge,
};
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{
    hex,
    json::{self, Map, Value},
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_KEY: &str = "bridge/state";
const RECEIPT_RETENTION: usize = 512;
const CHALLENGE_RETENTION: usize = 256;
const SLASH_RETENTION: usize = 512;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
        }
    }
}

impl std::error::Error for BridgeError {}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub asset: String,
    pub confirm_depth: u64,
    pub fee_per_byte: u64,
    pub challenge_period_secs: u64,
    pub relayer_quorum: usize,
    pub headers_dir: String,
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

    fn for_asset(asset: &str) -> Self {
        Self {
            asset: asset.to_string(),
            confirm_depth: 6,
            fee_per_byte: 0,
            challenge_period_secs: 30,
            relayer_quorum: 2,
            headers_dir: format!("state/bridge_headers/{asset}"),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct BridgeSnapshot {
    locked: HashMap<String, u64>,
    verified_headers: HashSet<[u8; 32]>,
    pending_withdrawals: HashMap<[u8; 32], PendingWithdrawal>,
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
    slash_log: Vec<SlashRecord>,
    token_bridge: TokenBridge,
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

    fn require_bool(value: &Value, field: &'static str) -> Result<bool, CodecError> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            _ => Err(invalid_type(field, "a boolean")),
        }
    }

    fn encode_config(cfg: &ChannelConfig) -> Value {
        foundation_serialization::json!({
            "asset": cfg.asset.clone(),
            "confirm_depth": cfg.confirm_depth,
            "fee_per_byte": cfg.fee_per_byte,
            "challenge_period_secs": cfg.challenge_period_secs,
            "relayer_quorum": cfg.relayer_quorum,
            "headers_dir": cfg.headers_dir.clone(),
        })
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
        })
    }

    fn encode_snapshot(snapshot: &BridgeSnapshot) -> Value {
        let mut locked_map = Map::new();
        for (user, amount) in &snapshot.locked {
            locked_map.insert(user.clone(), Value::from(*amount));
        }
        let mut pending_map = Map::new();
        for (commitment, pending) in &snapshot.pending_withdrawals {
            pending_map.insert(hex::encode(commitment), pending.to_value());
        }
        let verified = snapshot
            .verified_headers
            .iter()
            .map(|h| Value::String(hex::encode(h)))
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
            let hash = hex::decode_array::<32>(hex_str).map_err(|source| CodecError::Hex {
                field: "verified_headers",
                source,
            })?;
            verified.insert(hash);
        }
        let pending_obj = require_object(get(obj, "pending_withdrawals")?, "pending_withdrawals")?;
        let mut pending = HashMap::new();
        for (commitment_hex, value) in pending_obj.iter() {
            let commitment =
                hex::decode_array::<32>(commitment_hex).map_err(|source| CodecError::Hex {
                    field: "pending_withdrawals",
                    source,
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
        foundation_serialization::json!({
            "asset": receipt.asset.clone(),
            "nonce": receipt.nonce,
            "user": receipt.user.clone(),
            "amount": receipt.amount,
            "relayer": receipt.relayer.clone(),
            "header_hash": hex::encode(&receipt.header_hash),
            "relayer_commitment": hex::encode(&receipt.relayer_commitment),
            "proof_fingerprint": hex::encode(&receipt.proof_fingerprint),
            "bundle_relayers": receipt.bundle_relayers.clone(),
            "recorded_at": receipt.recorded_at,
        })
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
            header_hash: hex::decode_array::<32>(require_string(
                get(obj, "header_hash")?,
                "header_hash",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "header_hash",
                source,
            })?,
            relayer_commitment: hex::decode_array::<32>(require_string(
                get(obj, "relayer_commitment")?,
                "relayer_commitment",
            )?)
            .map_err(|source| CodecError::Hex {
                field: "relayer_commitment",
                source,
            })?,
            proof_fingerprint: hex::decode_array::<32>(require_string(
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
        foundation_serialization::json!({
            "asset": record.asset.clone(),
            "commitment": hex::encode(&record.commitment),
            "challenger": record.challenger.clone(),
            "challenged_at": record.challenged_at,
        })
    }

    fn decode_challenge(value: &Value) -> Result<ChallengeRecord, CodecError> {
        let obj = require_object(value, "challenge_record")?;
        Ok(ChallengeRecord {
            asset: require_string(get(obj, "asset")?, "asset")?.to_string(),
            commitment: hex::decode_array::<32>(require_string(
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
        foundation_serialization::json!({
            "relayer": record.relayer.clone(),
            "asset": record.asset.clone(),
            "slashes": record.slashes,
            "remaining_bond": record.remaining_bond,
            "occurred_at": record.occurred_at,
        })
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
            .map(|fp| Value::String(hex::encode(fp)))
            .collect();
        foundation_serialization::json!({
            "config": encode_config(&channel.config),
            "bridge": encode_snapshot(&channel.bridge),
            "relayers": channel.relayers.to_value(),
            "receipts": Value::Array(receipts),
            "challenges": Value::Array(challenges),
            "seen_fingerprints": Value::Array(fingerprints),
            "next_nonce": channel.next_nonce,
        })
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
            let fp = hex::decode_array::<32>(hex_str).map_err(|source| CodecError::Hex {
                field: "seen_fingerprints",
                source,
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
        map.insert("slash_log".to_string(), slash_log);
        map.insert("token_bridge".to_string(), state.token_bridge.to_value());
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
        let slash_values = require_array(get(obj, "slash_log")?, "slash_log")?;
        let mut slash_log = Vec::new();
        for entry in slash_values {
            slash_log.push(decode_slash(entry)?);
        }
        let token_bridge = TokenBridge::from_value(get(obj, "token_bridge")?)?;
        Ok(BridgeState {
            channels,
            relayer_bonds,
            slash_log,
            token_bridge,
        })
    }
}

pub struct Bridge {
    db: SimpleDb,
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
        Self::with_db(db)
    }

    pub fn with_db(db: SimpleDb) -> Self {
        let state = db
            .get(STATE_KEY)
            .and_then(|bytes| {
                json::value_from_slice(&bytes)
                    .ok()
                    .and_then(|value| state_codec::decode(&value).ok())
            })
            .unwrap_or_default();
        Self { db, state }
    }

    fn persist(&mut self) -> Result<(), BridgeError> {
        let value = state_codec::encode(&self.state);
        let rendered = json::to_string_value_pretty(&value);
        self.db
            .put(STATE_KEY.as_bytes(), rendered.as_bytes())
            .map_err(|e| BridgeError::Storage(e.to_string()))
    }

    fn ensure_channel(&mut self, asset: &str) -> &mut ChannelState {
        self.state
            .channels
            .entry(asset.to_string())
            .or_insert_with(|| ChannelState::new(ChannelConfig::for_asset(asset)))
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
            let prev_slashes = before.get(&id).map(|r| r.slashes).unwrap_or(0);
            if new_state.slashes > prev_slashes {
                self.apply_slash(&id, asset, new_state.slashes - prev_slashes);
            }
        }
    }

    pub fn bond_relayer(&mut self, relayer: &str, amount: u64) -> Result<(), BridgeError> {
        self.state
            .relayer_bonds
            .entry(relayer.to_string())
            .and_modify(|bond| *bond = bond.saturating_add(amount))
            .or_insert(amount);
        self.persist()
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
        let fingerprint = Self::fingerprint(header, proof);
        {
            let channel = self.ensure_channel(asset);
            if !channel.seen_fingerprints.insert(fingerprint) {
                return Err(BridgeError::Replay);
            }
            channel.relayers.stake(relayer, 0);
        }

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

        self.state.token_bridge.lock(asset, amount);
        self.persist()?;
        Ok(receipt)
    }

    fn ensure_release_authorized(
        &self,
        asset: &str,
        commitment: &[u8; 32],
    ) -> Result<(), BridgeError> {
        let hash = hex::encode(commitment);
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
        {
            let channel = self
                .state
                .channels
                .get_mut(asset)
                .ok_or_else(|| BridgeError::UnknownChannel(asset.to_string()))?;
            channel.relayers.stake(relayer, 0);
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
        }
        self.persist()?;
        Ok(commitment)
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
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_CHALLENGES_TOTAL.inc();
        }
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
        if let Some(pending) = channel.bridge.pending_withdrawals.get(&commitment) {
            if pending.challenged {
                return Err(BridgeError::AlreadyChallenged);
            }
            let deadline = pending.initiated_at + channel.config.challenge_period_secs;
            if now_secs() < deadline {
                return Err(BridgeError::ChallengeWindowOpen);
            }
        } else {
            return Err(BridgeError::WithdrawalMissing);
        }
        let mut runtime = channel.runtime_bridge();
        if !runtime.finalize_withdrawal(commitment) {
            return Err(BridgeError::ChallengeWindowOpen);
        }
        channel.update_from_runtime(runtime);
        self.persist()
    }

    pub fn locked_balance(&self, asset: &str, user: &str) -> Option<u64> {
        self.state
            .channels
            .get(asset)
            .and_then(|c| c.bridge.locked.get(user).copied())
    }

    pub fn pending_withdrawals(&self, asset: Option<&str>) -> Vec<Value> {
        let mut out = Vec::new();
        for (chan_asset, channel) in &self.state.channels {
            if asset.is_some() && asset != Some(chan_asset.as_str()) {
                continue;
            }
            for (commitment, pending) in &channel.bridge.pending_withdrawals {
                let deadline = pending.initiated_at + channel.config.challenge_period_secs;
                let mut map = Map::new();
                map.insert("asset".to_string(), Value::String(chan_asset.clone()));
                map.insert(
                    "commitment".to_string(),
                    Value::String(hex::encode(commitment)),
                );
                map.insert("user".to_string(), Value::String(pending.user.clone()));
                map.insert("amount".to_string(), Value::from(pending.amount));
                map.insert(
                    "relayers".to_string(),
                    Value::Array(
                        pending
                            .relayers
                            .iter()
                            .map(|r| Value::String(r.clone()))
                            .collect(),
                    ),
                );
                map.insert(
                    "initiated_at".to_string(),
                    Value::from(pending.initiated_at),
                );
                map.insert("deadline".to_string(), Value::from(deadline));
                map.insert("challenged".to_string(), Value::Bool(pending.challenged));
                out.push((pending.initiated_at, Value::Object(map)));
            }
        }
        out.sort_by_key(|(initiated, _)| *initiated);
        out.into_iter().map(|(_, value)| value).collect()
    }

    pub fn relayer_quorum(&self, asset: &str) -> Option<Value> {
        let channel = self.state.channels.get(asset)?;
        let mut relayers: Vec<(String, Value)> = channel
            .relayers
            .snapshot()
            .into_iter()
            .map(|(id, rel)| {
                let bond = self
                    .state
                    .relayer_bonds
                    .get(&id)
                    .copied()
                    .unwrap_or_default();
                let mut map = Map::new();
                map.insert("id".to_string(), Value::String(id.clone()));
                map.insert("stake".to_string(), Value::from(rel.stake));
                map.insert("slashes".to_string(), Value::from(rel.slashes));
                map.insert("bond".to_string(), Value::from(bond));
                (id, Value::Object(map))
            })
            .collect();
        relayers.sort_by(|a, b| a.0.cmp(&b.0));
        let relayer_values = relayers.into_iter().map(|(_, value)| value).collect();
        let mut map = Map::new();
        map.insert("asset".to_string(), Value::String(asset.to_string()));
        map.insert(
            "quorum".to_string(),
            Value::from(channel.config.relayer_quorum as u64),
        );
        map.insert("relayers".to_string(), Value::Array(relayer_values));
        Some(Value::Object(map))
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
    ) -> Option<(String, u64, u64, u64)> {
        for (chan_asset, channel) in &self.state.channels {
            if asset.is_some() && asset != Some(chan_asset.as_str()) {
                continue;
            }
            for (id, status) in channel.relayers.iter() {
                if id == relayer {
                    let bond = self
                        .state
                        .relayer_bonds
                        .get(relayer)
                        .copied()
                        .unwrap_or_default();
                    return Some((chan_asset.clone(), status.stake, status.slashes, bond));
                }
            }
        }
        None
    }
}
