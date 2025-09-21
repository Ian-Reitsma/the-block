#![forbid(unsafe_code)]

#[cfg(feature = "telemetry")]
use crate::telemetry::BRIDGE_CHALLENGES_TOTAL;
use crate::{governance, SimpleDb};
use blake3::Hasher;
use bridges::relayer::RelayerSet;
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header as LightHeader, Proof},
    Bridge as ExternalBridge, BridgeConfig, PendingWithdrawal, RelayerBundle, TokenBridge,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("bridge channel not found: {0}")]
    UnknownChannel(String),
    #[error("bridge storage error: {0}")]
    Storage(String),
    #[error("bridge proof rejected")]
    InvalidProof,
    #[error("proof already processed")]
    Replay,
    #[error("withdrawal already pending")]
    DuplicateWithdrawal,
    #[error("withdrawal not found")]
    WithdrawalMissing,
    #[error("withdrawal already challenged")]
    AlreadyChallenged,
    #[error("challenge window still open")]
    ChallengeWindowOpen,
    #[error("release not authorized")]
    UnauthorizedRelease,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BridgeSnapshot {
    locked: HashMap<String, u64>,
    verified_headers: HashSet<[u8; 32]>,
    pending_withdrawals: HashMap<[u8; 32], PendingWithdrawal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeRecord {
    pub asset: String,
    pub commitment: [u8; 32],
    pub challenger: String,
    pub challenged_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashRecord {
    pub relayer: String,
    pub asset: String,
    pub slashes: u64,
    pub remaining_bond: u64,
    pub occurred_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BridgeState {
    channels: HashMap<String, ChannelState>,
    relayer_bonds: HashMap<String, u64>,
    slash_log: Vec<SlashRecord>,
    token_bridge: TokenBridge,
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
        let db = SimpleDb::open(path);
        Self::with_db(db)
    }

    pub fn with_db(db: SimpleDb) -> Self {
        let state = db
            .get(STATE_KEY)
            .and_then(|bytes| bincode::deserialize(&bytes).ok())
            .unwrap_or_default();
        Self { db, state }
    }

    fn persist(&mut self) -> Result<(), BridgeError> {
        let bytes =
            bincode::serialize(&self.state).map_err(|e| BridgeError::Storage(e.to_string()))?;
        self.db
            .put(STATE_KEY.as_bytes(), &bytes)
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

    pub fn pending_withdrawals(&self, asset: Option<&str>) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for (chan_asset, channel) in &self.state.channels {
            if asset.is_some() && asset != Some(chan_asset.as_str()) {
                continue;
            }
            for (commitment, pending) in &channel.bridge.pending_withdrawals {
                let deadline = pending.initiated_at + channel.config.challenge_period_secs;
                out.push(serde_json::json!({
                    "asset": chan_asset,
                    "commitment": hex::encode(commitment),
                    "user": pending.user,
                    "amount": pending.amount,
                    "relayers": pending.relayers,
                    "initiated_at": pending.initiated_at,
                    "deadline": deadline,
                    "challenged": pending.challenged,
                }));
            }
        }
        out.sort_by_key(|value| value["initiated_at"].as_u64().unwrap_or_default());
        out
    }

    pub fn relayer_quorum(&self, asset: &str) -> Option<serde_json::Value> {
        let channel = self.state.channels.get(asset)?;
        let mut relayers: Vec<serde_json::Value> = channel
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
                serde_json::json!({
                    "id": id,
                    "stake": rel.stake,
                    "slashes": rel.slashes,
                    "bond": bond,
                })
            })
            .collect();
        relayers.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Some(serde_json::json!({
            "asset": asset,
            "quorum": channel.config.relayer_quorum,
            "relayers": relayers,
        }))
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
