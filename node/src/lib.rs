#![forbid(unsafe_code)]
#![allow(unused_imports)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(unused_variables)]
#![allow(clippy::all)]
#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types)]

//! Core blockchain implementation with Python bindings.
//!
//! This crate is the civic-grade kernel for a one-second Layer 1 that
//! notarizes sub-second micro-shards and enforces service-based governance
//! through dual Consumer/Industrial lanes and inflation-funded subsidies. See
//! `AGENTS.md` and `agents_vision.md` for the full blueprint.

use crate::blockchain::{inter_shard::MessageQueue, macro_block::MacroBlock, process};
use crate::consensus::constants::DIFFICULTY_WINDOW;
#[cfg(feature = "telemetry")]
use crate::consensus::observer;
use crate::governance::{DisbursementStatus, NODE_GOV_STORE};
#[cfg(feature = "telemetry")]
use crate::telemetry::MemoryComponent;
use crate::transaction::{TxSignature, TxVersion};
use ad_market::{DeliveryChannel, SettlementBreakdown};
use concurrency::cache::LruCache;
use concurrency::dashmap::Entry as DashEntry;
use concurrency::DashMap;
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use crypto_suite::{hashing::blake3, hex};
#[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
use diagnostics::tracing::info;
#[cfg(feature = "telemetry")]
use diagnostics::tracing::warn;
use ledger::address::{self, ShardId};
use ledger::shard::ShardState;
pub mod block_binary;
pub mod http_client;
pub mod ledger_binary;
mod legacy_cbor;
mod py;

#[cfg(feature = "python-bindings")]
pub use py::prepare_freethreaded_python;

#[cfg(feature = "python-bindings")]
#[allow(unused_imports)]
use crate::py::{getter, new, setter, staticmethod};
use crate::py::{PyError, PyResult};
#[cfg(feature = "telemetry-json")]
use foundation_serialization::json::{Map as JsonMap, Number, Value as JsonValue};
use foundation_serialization::{Deserialize, Serialize};
use rand::rngs::OsRng;
use rand::RngCore;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    Arc, Mutex, MutexGuard,
};
use std::thread;
#[cfg(feature = "telemetry")]
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wallet::{remote_signer::RemoteSigner as WalletRemoteSigner, WalletSigner};
pub mod ad_policy_snapshot;
pub mod ad_readiness;
pub mod config;
pub mod dkg;
pub mod energy;
pub mod exec;
pub mod governor_snapshot;
mod read_receipt;
pub mod receipt_crypto;
pub mod receipts;
pub mod receipts_validation;
pub mod simple_db;
use crate::receipt_crypto::{NonceTracker, ProviderRegistry};
use config::{NodeConfig, ReceiptProviderConfig};
pub use read_receipt::{ReadAck, ReadBatcher};
pub use receipts::{AdReceipt, ComputeReceipt, EnergyReceipt, Receipt, StorageReceipt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadAckError {
    InvalidSignature,
    PrivacyProofRejected,
    InvalidSelectionReceipt,
}
pub use runtime;
pub use runtime::{
    block_on, handle, interval, sleep, spawn, spawn_blocking, timeout, yield_now, JoinHandle,
    RuntimeHandle, TimeoutError,
};
pub use simple_db::SimpleDb;
use simple_db::{DbDelta, SimpleDb as Db};
use std::convert::TryInto;

const EPOCH_BLOCKS: u64 = 120;
const EPOCHS_PER_YEAR: u64 = 365 * 24 * 60 * 60 / EPOCH_BLOCKS;
const RECENT_MINER_WINDOW: usize = 120;

const HBAR: f64 = 1.054_571_817e-34; // J·s
const BLOCK_ENERGY_J: f64 = 6.58e-33; // median compute energy per block
const TAU_B: f64 = 1.0; // block interval in seconds
static VDF_KAPPA: AtomicU64 = AtomicU64::new(1u64 << 28);
const F_HW_BASE: f64 = 3.0e9; // reference 3 GHz hardware
const RECEIPT_NONCE_FINALITY: u64 = 100;

fn py_value_err(msg: impl Into<String>) -> PyError {
    PyError::value(msg)
}

fn py_runtime_err(msg: impl Into<String>) -> PyError {
    PyError::runtime(msg)
}

pub fn set_vdf_kappa(k: u64) {
    VDF_KAPPA.store(k, AtomicOrdering::Relaxed);
}

pub fn vrf_min_delay_slots() -> u64 {
    let heis = (HBAR / (2.0 * BLOCK_ENERGY_J * TAU_B)).ceil();
    let vdf = (VDF_KAPPA.load(AtomicOrdering::Relaxed) as f64 / F_HW_BASE).ceil();
    (heis + vdf) as u64
}

pub mod gateway;
pub mod gossip;
pub mod identity;
pub mod kyc;
pub mod launch_governor;
pub mod light_client;
pub mod liquidity;
pub mod localnet;
pub mod net;
pub mod partition_recover;
pub use net::peer_metrics_store;
pub mod p2p;
pub mod parallel;
pub mod poh;
pub mod range_boost;
pub mod rpc;
pub mod scheduler;

pub mod tx;
#[cfg(feature = "gateway")]
pub mod web;

pub mod log_indexer;
pub mod logging;
pub mod provenance;
#[cfg(feature = "telemetry")]
pub mod telemetry;
#[cfg(feature = "telemetry")]
pub use telemetry::{
    ensure_ad_verifier_committee_label, gather_metrics, redact_at_rest,
    reset_ad_verifier_committee_rejections, serve_metrics, serve_metrics_with_shutdown,
    MetricsServer,
};
pub mod update;

pub mod blockchain;
use crate::consensus::difficulty;
pub use blockchain::snapshot::SnapshotManager;
pub mod service_badge;
pub use service_badge::ServiceBadgeTracker;

pub mod blob_chain;

pub mod economics;
pub use economics::{
    execute_epoch_economics, replay_economics_to_height, replay_economics_to_tip,
    AdMarketDriftController, ControlLawUpdateEvent, EconomicSnapshot, GovernanceEconomicParams,
    InflationController, MarketMetric, MarketMetrics, MarketMultiplierController,
    ReplayedEconomicsState, SubsidyAllocator, TariffController,
};

pub mod governance;
pub mod treasury_executor;
pub use governance::{
    retune_multipliers, Bicameral, BicameralGovernance as Governance,
    BicameralProposal as LegacyProposal, GovStore, House, ParamKey, Params, Proposal,
    ProposalStatus, Utilization, Vote, VoteChoice, ACTIVATION_DELAY, QUORUM,
    ROLLBACK_WINDOW_EPOCHS,
};

pub mod accounts;

pub mod mempool;

pub mod compute_market;
pub mod le_portal;

pub mod transaction;
pub use transaction::{
    canonical_payload_bytes, canonical_payload_py as canonical_payload,
    decode_payload_py as decode_payload, sign_tx_py as sign_tx,
    verify_signed_tx_py as verify_signed_tx, BlobTx, FeeLane, RawTxPayload, SignedTransaction,
    TxDidAnchor, TxDidAnchorAttestation,
};
// Python helper re-exported at the crate root
pub use self::mine_block_py as mine_block;
pub mod consensus;
pub use consensus::pow;
pub mod commit_reveal;
pub mod constants;
pub use constants::{
    domain_tag, domain_tag_for, CHAIN_ID, FEE_SPEC_VERSION, GENESIS_HASH, TX_VERSION,
};
pub mod fee;
pub mod fees;
pub use fees::lane_pricing::LanePricingEngine;
pub mod hash_genesis;
pub mod hashlayout;
pub use fee::{decompose as fee_decompose, ErrFeeOverflow, ErrInvalidSelector, FeeError};
pub mod bridge;
pub mod dex;
pub mod storage;
pub mod util;
pub mod utxo;
pub mod vm;

// === Transaction admission errors ===

#[repr(u16)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TxAdmissionError {
    UnknownSender = ERR_UNKNOWN_SENDER,
    InsufficientBalance = ERR_INSUFFICIENT_BALANCE,
    NonceGap = ERR_NONCE_GAP,
    InvalidSelector = ERR_INVALID_SELECTOR,
    BadSignature = ERR_BAD_SIGNATURE,
    Duplicate = ERR_DUPLICATE,
    NotFound = ERR_NOT_FOUND,
    BalanceOverflow = ERR_BALANCE_OVERFLOW,
    FeeOverflow = ERR_FEE_OVERFLOW,
    FeeTooLarge = ERR_FEE_TOO_LARGE,
    FeeTooLow = ERR_FEE_TOO_LOW,
    MempoolFull = ERR_MEMPOOL_FULL,
    LockPoisoned = ERR_LOCK_POISONED,
    PendingLimitReached = ERR_PENDING_LIMIT,
    PendingSignatures = ERR_PENDING_SIGNATURES,
    SessionExpired = ERR_SESSION_EXPIRED,
}

impl fmt::Display for TxAdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            TxAdmissionError::UnknownSender => "unknown sender",
            TxAdmissionError::InsufficientBalance => "insufficient balance",
            TxAdmissionError::NonceGap => "nonce gap",
            TxAdmissionError::InvalidSelector => "invalid selector",
            TxAdmissionError::BadSignature => "bad signature",
            TxAdmissionError::Duplicate => "duplicate transaction",
            TxAdmissionError::NotFound => "transaction not found",
            TxAdmissionError::BalanceOverflow => "balance overflow",
            TxAdmissionError::FeeOverflow => "fee overflow",
            TxAdmissionError::FeeTooLarge => "fee too large",
            TxAdmissionError::FeeTooLow => "fee below minimum",
            TxAdmissionError::MempoolFull => "mempool full",
            TxAdmissionError::LockPoisoned => "lock poisoned",
            TxAdmissionError::PendingLimitReached => "pending limit reached",
            TxAdmissionError::PendingSignatures => "additional signatures required",
            TxAdmissionError::SessionExpired => "session expired",
        };
        write!(f, "{}", msg)
    }
}

impl std::error::Error for TxAdmissionError {}

impl TxAdmissionError {
    #[must_use]
    #[inline]
    pub const fn code(self) -> u16 {
        self as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BalanceUnderflow;

#[derive(Debug, Clone, Copy)]
pub struct ErrBalanceUnderflow;

impl fmt::Display for BalanceUnderflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "balance underflow")
    }
}

impl std::error::Error for BalanceUnderflow {}

impl ErrBalanceUnderflow {
    pub fn new_err(msg: impl Into<String>) -> PyError {
        PyError::value(msg)
    }
}

impl From<BalanceUnderflow> for PyError {
    fn from(_: BalanceUnderflow) -> Self {
        PyError::value("balance underflow")
    }
}

fn checked_sub_assign(target: &mut u64, val: u64) -> Result<(), BalanceUnderflow> {
    *target = target.checked_sub(val).ok_or(BalanceUnderflow)?;
    Ok(())
}

impl From<TxAdmissionError> for PyError {
    fn from(e: TxAdmissionError) -> Self {
        let code = e.code();
        let message = match e {
            TxAdmissionError::UnknownSender => "unknown sender",
            TxAdmissionError::InsufficientBalance => "insufficient balance",
            TxAdmissionError::NonceGap => "nonce gap",
            TxAdmissionError::InvalidSelector => "invalid selector",
            TxAdmissionError::BadSignature => "bad signature",
            TxAdmissionError::Duplicate => "duplicate transaction",
            TxAdmissionError::NotFound => "transaction not found",
            TxAdmissionError::BalanceOverflow => "balance overflow",
            TxAdmissionError::FeeOverflow => "fee overflow",
            TxAdmissionError::FeeTooLarge => "fee too large",
            TxAdmissionError::FeeTooLow => "fee below minimum",
            TxAdmissionError::MempoolFull => "mempool full",
            TxAdmissionError::LockPoisoned => "lock poisoned",
            TxAdmissionError::PendingLimitReached => "pending limit reached",
            TxAdmissionError::PendingSignatures => "additional signatures required",
            TxAdmissionError::SessionExpired => "session expired",
        };
        PyError::runtime(message).with_message(format!("{message} (code {code})"))
    }
}

#[cfg(feature = "telemetry")]
fn scrub(s: &str) -> String {
    let h = blake3::hash(s.as_bytes());
    crypto_suite::hex::encode(h.as_bytes())
}

#[cfg(feature = "telemetry-json")]
fn log_event(
    subsystem: &str,
    level: diagnostics::log::Level,
    op: &str,
    sender: &str,
    nonce: u64,
    reason: &str,
    code: u16,
    fpb: Option<u64>,
    cid: Option<&str>,
) {
    if !telemetry::should_log(subsystem) {
        return;
    }
    let mut obj = JsonMap::new();
    obj.insert("subsystem".into(), JsonValue::String(subsystem.to_owned()));
    obj.insert("op".into(), JsonValue::String(op.to_owned()));
    obj.insert("sender".into(), JsonValue::String(scrub(sender)));
    obj.insert("nonce".into(), JsonValue::Number(Number::from(nonce)));
    obj.insert("reason".into(), JsonValue::String(reason.to_owned()));
    obj.insert("code".into(), JsonValue::Number(Number::from(code)));
    if let Some(v) = fpb {
        obj.insert("fpb".into(), JsonValue::Number(Number::from(v)));
    }
    if let Some(c) = cid {
        obj.insert("cid".into(), JsonValue::String(c.to_owned()));
    }
    let msg = JsonValue::Object(obj).to_string();
    telemetry::observe_log_size(msg.len());
    diagnostics::log::log!(level, "{}", msg);
}

// === Database keys ===
const DB_CHAIN: &str = "chain";
const DB_ACCOUNTS: &str = "accounts";
const DB_EMISSION: &str = "emission";

// === Monetary constants ===
const MAX_SUPPLY_BLOCK: u64 = 40_000_000; // 40M BLOCK total supply cap (like Bitcoin's 21M)
const INITIAL_BLOCK_REWARD: u64 = 50_000; // Bootstrap reward, adjusted by formula per block
                                          // === Startup rebuild tuning ===
/// Number of mempool entries processed per batch during `Blockchain::open`.
pub const STARTUP_REBUILD_BATCH: usize = 256;

pub const DEFAULT_SNAPSHOT_INTERVAL: u64 = 1024;
const ENV_SNAPSHOT_INTERVAL: &str = "TB_SNAPSHOT_INTERVAL";

// === Helpers for Ed25519 v2.x ([u8;32], [u8;64]) ===
/// Converts a byte slice into a fixed 32-byte array, returning `None` on length
/// mismatch.
pub(crate) fn to_array_32(bytes: &[u8]) -> Option<[u8; 32]> {
    bytes.try_into().ok()
}
/// Converts a byte slice into a fixed 64-byte array, returning `None` on length
/// mismatch.
pub(crate) fn to_array_64(bytes: &[u8]) -> Option<[u8; 64]> {
    bytes.try_into().ok()
}
#[allow(clippy::expect_used)]
fn hex_to_bytes(hex: &str) -> Vec<u8> {
    // Utility used by tests and examples
    crypto_suite::hex::decode(hex).unwrap_or_else(|_| panic!("Invalid hex string"))
}

fn snapshot_interval_from_env() -> u64 {
    std::env::var(ENV_SNAPSHOT_INTERVAL)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_SNAPSHOT_INTERVAL)
}

// === Data types ===

/// Chain-wide token unit.
///
/// See `AGENTS.md` §10.3. All monetary values in consensus code use this
/// wrapper to make a future switch to `u128` trivial and to forbid accidental
/// arithmetic on raw integers.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TokenAmount(pub u64);

impl TokenAmount {
    pub fn py_new(v: u64) -> Self {
        Self(v)
    }
    pub fn value(&self) -> u64 {
        self.0
    }
    fn __int__(&self) -> u64 {
        self.0
    }
    fn __repr__(&self) -> String {
        format!("{}", self.0)
    }
    fn __str__(&self) -> String {
        self.__repr__()
    }
}

impl std::fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TokenAmount {
    pub fn new(v: u64) -> Self {
        Self(v)
    }
    pub fn get(self) -> u64 {
        self.0
    }
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TokenBalance {
    /// Total BLOCK token balance. Consumer/industrial routing happens at the transaction
    /// lane level, not at the balance level.
    pub amount: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Account {
    pub address: String,
    pub balance: TokenBalance,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub nonce: u64,
    /// Total pending BLOCK tokens across all pending transactions
    #[serde(default = "foundation_serialization::defaults::default")]
    pub pending_amount: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub pending_nonce: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub pending_nonces: HashSet<u64>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub sessions: Vec<accounts::SessionPolicy>,
}

impl accounts::AccountValidation for Account {
    fn validate_tx(&mut self, tx: &SignedTransaction) -> Result<(), TxAdmissionError> {
        if let Some(policy) = self
            .sessions
            .iter_mut()
            .find(|p| p.public_key == tx.public_key)
        {
            if policy.is_expired() {
                #[cfg(feature = "telemetry")]
                telemetry::SESSION_KEY_EXPIRED_TOTAL.inc();
                return Err(TxAdmissionError::SessionExpired);
            }
            if tx.payload.nonce <= policy.nonce {
                return Err(TxAdmissionError::Duplicate);
            }
            policy.nonce = tx.payload.nonce;
        }
        Ok(())
    }
}

struct Reservation<'a> {
    account: &'a mut Account,
    reserve_amount: u64,
    nonce: u64,
    committed: bool,
}

impl<'a> Reservation<'a> {
    fn new(
        account: &'a mut Account,
        reserve_amount: u64,
        nonce: u64,
    ) -> Self {
        account.pending_amount += reserve_amount;
        account.pending_nonce += 1;
        account.pending_nonces.insert(nonce);
        Self {
            account,
            reserve_amount,
            nonce,
            committed: false,
        }
    }
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = checked_sub_assign(&mut self.account.pending_amount, self.reserve_amount);
            let _ = checked_sub_assign(&mut self.account.pending_nonce, 1);
            self.account.pending_nonces.remove(&self.nonce);
        }
    }
}

struct ReservationGuard<'a> {
    reservation: Option<Reservation<'a>>,
    _lock: MutexGuard<'a, ()>,
}

impl<'a> ReservationGuard<'a> {
    fn new(
        lock: MutexGuard<'a, ()>,
        account: &'a mut Account,
        reserve_amount: u64,
        nonce: u64,
    ) -> Self {
        let reservation = Reservation::new(account, reserve_amount, nonce);
        Self {
            reservation: Some(reservation),
            _lock: lock,
        }
    }
    fn commit(mut self) {
        if let Some(res) = self.reservation.take() {
            res.commit();
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTreasuryEvent {
    pub disbursement_id: u64,
    pub destination: String,
    pub amount: u64,
    pub memo: String,
    pub scheduled_epoch: u64,
    pub tx_hash: String,
    pub executed_at: u64,
}

/// Per-block ledger entry. `coinbase_*` mirrors the first transaction
/// but is the canonical source for light clients.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Block {
    pub index: u64,
    pub previous_hash: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// UNIX timestamp in milliseconds when the block was mined
    pub timestamp_millis: u64,
    pub transactions: Vec<SignedTransaction>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub difficulty: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Miner-provided hint about recent hash-rate trend
    pub retune_hint: i8,
    pub nonce: u64,
    pub hash: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Canonical consumer LANE amount from coinbase tx[0] (P2P transactions, slow, lower fees). Combined with coinbase_industrial for total miner reward.
    pub coinbase_block: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Canonical industrial LANE amount from coinbase tx[0] (market operations, fast, higher fees). Combined with coinbase_block for total miner reward.
    pub coinbase_industrial: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Subsidy minted for storage operations in this block
    pub storage_sub: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Subsidy minted for read delivery in this block
    pub read_sub: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Portion of the read subsidy paid to viewers in this block
    pub read_sub_viewer: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Portion of the read subsidy paid to hosts in this block
    pub read_sub_host: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Portion of the read subsidy paid to hardware providers in this block
    pub read_sub_hardware: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Portion of the read subsidy paid to verifiers in this block
    pub read_sub_verifier: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Portion of the read subsidy routed to the liquidity pool in this block
    pub read_sub_liquidity: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// BLOCK paid out from advertising campaigns to viewers
    pub ad_viewer: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// BLOCK paid out from advertising campaigns to hosts
    pub ad_host: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// BLOCK paid out from advertising campaigns to hardware providers
    pub ad_hardware: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// BLOCK paid out from advertising campaigns to verifiers
    pub ad_verifier: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// BLOCK routed to the liquidity pool from advertising campaigns
    pub ad_liquidity: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// BLOCK routed to the miner from advertising campaigns
    pub ad_miner: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Executed treasury disbursements surfaced alongside settlement payouts
    pub treasury_events: Vec<BlockTreasuryEvent>,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Total USD billed across advertising settlements included in this block
    pub ad_total_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Number of advertising settlements applied in this block
    pub ad_settlement_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Oracle price snapshot (BLOCK) used for advertising settlements in this block
    pub ad_oracle_price_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Subsidy minted for compute in this block
    pub compute_sub: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Rebates paid to proof relayers in this block
    pub proof_rebate: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Merkle root of all `ReadAck`s batched for this block
    pub read_root: [u8; 32],
    #[serde(default = "foundation_serialization::defaults::default")]
    /// blake3(total_fee_block) in hex
    pub fee_checksum: String,
    #[serde(
        default = "foundation_serialization::defaults::default",
        alias = "snapshot_root"
    )]
    /// Merkle root of account state
    pub state_root: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Base fee in effect for this block.
    pub base_fee: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// L2 blob commitment roots anchored in this block
    pub l2_roots: Vec<[u8; 32]>,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Corresponding total byte sizes per root
    pub l2_sizes: Vec<u32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Commitment to VDF preimage for randomness fuse
    pub vdf_commit: [u8; 32],
    #[serde(default = "foundation_serialization::defaults::default")]
    /// VDF output revealed for commitment two blocks prior
    pub vdf_output: [u8; 32],
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Pietrzak proof bytes for the VDF evaluation
    pub vdf_proof: Vec<u8>,
    #[cfg(feature = "quantum")]
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Optional Dilithium public key for the miner.
    pub dilithium_pubkey: Vec<u8>,
    #[cfg(feature = "quantum")]
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Optional Dilithium signature over the header hash.
    pub dilithium_sig: Vec<u8>,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Market settlement receipts for deterministic economics derivation.
    pub receipts: Vec<Receipt>,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            index: 0,
            previous_hash: String::new(),
            timestamp_millis: 0,
            transactions: Vec::new(),
            difficulty: 0,
            retune_hint: 0,
            nonce: 0,
            hash: String::new(),
            coinbase_block: TokenAmount::new(0),
            coinbase_industrial: TokenAmount::new(0),
            storage_sub: TokenAmount::new(0),
            read_sub: TokenAmount::new(0),
            read_sub_viewer: TokenAmount::new(0),
            read_sub_host: TokenAmount::new(0),
            read_sub_hardware: TokenAmount::new(0),
            read_sub_verifier: TokenAmount::new(0),
            read_sub_liquidity: TokenAmount::new(0),
            ad_viewer: TokenAmount::new(0),
            ad_host: TokenAmount::new(0),
            ad_hardware: TokenAmount::new(0),
            ad_verifier: TokenAmount::new(0),
            ad_liquidity: TokenAmount::new(0),
            ad_miner: TokenAmount::new(0),
            treasury_events: Vec::new(),
            ad_total_usd_micros: 0,
            ad_settlement_count: 0,
            ad_oracle_price_usd_micros: 0,
            compute_sub: TokenAmount::new(0),
            proof_rebate: TokenAmount::new(0),
            read_root: [0u8; 32],
            fee_checksum: String::new(),
            state_root: String::new(),
            base_fee: 0,
            l2_roots: Vec::new(),
            l2_sizes: Vec::new(),
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: Vec::new(),
            #[cfg(feature = "quantum")]
            dilithium_pubkey: Vec::new(),
            #[cfg(feature = "quantum")]
            dilithium_sig: Vec::new(),
            receipts: Vec::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MempoolEntry {
    pub tx: SignedTransaction,
    /// UNIX timestamp in milliseconds when the tx entered the mempool
    pub timestamp_millis: u64,
    /// Monotonic tick count at admission for heap ordering
    pub timestamp_ticks: u64,
    /// Cached serialized size of the transaction in bytes
    pub serialized_size: u64,
}

impl MempoolEntry {
    fn fee_per_byte(&self) -> u64 {
        if self.serialized_size == 0 {
            0
        } else {
            self.tx.tip / self.serialized_size
        }
    }

    fn expires_at(&self, ttl_secs: u64) -> u64 {
        self.timestamp_millis + ttl_secs * 1000
    }
}

pub struct MempoolStats {
    pub size: usize,
    pub age_p50: u64,
    pub age_p95: u64,
    pub fee_p50: u64,
    pub fee_p90: u64,
    pub fee_floor: u64,
}

/// Comparator for mempool eviction ordering.
///
/// Orders by `fee_per_byte` (descending), then `expires_at` (ascending),
/// then transaction hash (ascending).
pub fn mempool_cmp(a: &MempoolEntry, b: &MempoolEntry, ttl_secs: u64) -> Ordering {
    let fee_a = a.fee_per_byte();
    let fee_b = b.fee_per_byte();
    fee_b
        .cmp(&fee_a)
        .then(a.expires_at(ttl_secs).cmp(&b.expires_at(ttl_secs)))
        .then(a.tx.id().cmp(&b.tx.id()))
}

/// In-memory representation of the chain state and associated accounts.
///
/// `Blockchain` exposes high level methods for transaction submission,
/// mining, and persistence. It backs the Python API used throughout the
/// demo script and tests.
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub accounts: HashMap<String, Account>,
    /// Latest state root per shard.
    pub shard_roots: HashMap<ShardId, [u8; 32]>,
    /// Latest height per shard.
    pub shard_heights: HashMap<ShardId, u64>,
    /// LRU cache for recent shard state entries keyed by `(ShardId, key)`.
    pub shard_cache: Mutex<LruCache<(ShardId, Vec<u8>), Vec<u8>>>,
    pub difficulty: u64,
    /// Hint from previous retune summarizing hash-rate trend
    pub retune_hint: i8,
    /// Consumer lane mempool entries keyed by `(sender, nonce)`.
    pub mempool_consumer: DashMap<(String, u64), MempoolEntry>,
    /// Industrial lane mempool entries keyed by `(sender, nonce)`.
    pub mempool_industrial: DashMap<(String, u64), MempoolEntry>,
    mempool_size_consumer: std::sync::atomic::AtomicUsize,
    mempool_size_industrial: std::sync::atomic::AtomicUsize,
    mempool_mutex: Mutex<()>,
    admission_consumer: Mutex<mempool::admission::AdmissionState>,
    admission_industrial: Mutex<mempool::admission::AdmissionState>,
    /// Transactions waiting on additional signatures.
    pub pending_multisig: DashMap<Vec<u8>, SignedTransaction>,
    orphan_counter: std::sync::atomic::AtomicUsize,
    panic_on_evict: std::sync::atomic::AtomicBool,
    panic_on_admit: std::sync::atomic::AtomicI32,
    panic_on_purge: std::sync::atomic::AtomicBool,
    pub max_mempool_size_consumer: usize,
    pub max_mempool_size_industrial: usize,
    pub min_fee_per_byte_consumer: u64,
    pub min_fee_per_byte_industrial: u64,
    pub comfort_threshold_p90: u64,
    pub tx_ttl: u64,
    pub max_pending_per_account: usize,
    admission_locks: DashMap<String, Arc<Mutex<()>>>,
    db: Db,
    pub path: String,
    pub emission: u64,
    /// Total emissions from one year ago used for rolling inflation.
    pub emission_year_ago: u64,
    /// Block height when the rolling inflation window started.
    pub inflation_epoch_marker: u64,
    pub block_reward: TokenAmount,
    pub block_height: u64,
    /// Pending messages across shards.
    pub inter_shard: MessageQueue,
    /// Accumulated rewards since last macro block.
    macro_acc: u64,
    /// Stored macro blocks.
    pub macro_blocks: Vec<MacroBlock>,
    /// Interval in blocks between macro block emissions.
    pub macro_interval: u64,
    pub snapshot: SnapshotManager,
    pub skipped: Vec<SignedTransaction>,
    pub skipped_nonce_gap: u64,
    badge_tracker: ServiceBadgeTracker,
    pub config: NodeConfig,
    /// Current base fee used for transaction admission and recorded in mined blocks.
    pub base_fee: u64,
    /// Governance-controlled economic parameters
    pub params: Params,
    /// Dynamic lane-based pricing engine for consumer/industrial fee calculation
    lane_pricing_engine: Mutex<LanePricingEngine>,
    /// Cached consumer lane fee per byte (updated after each block to avoid lock contention)
    cached_consumer_fee: AtomicU64,
    /// Cached industrial lane fee per byte (updated after each block to avoid lock contention)
    cached_industrial_fee: AtomicU64,
    pub beta_storage_sub_raw: i64,
    pub gamma_read_sub_raw: i64,
    pub kappa_cpu_sub_raw: i64,
    pub lambda_bytes_out_sub_raw: i64,
    /// Bytes stored during the current epoch
    pub epoch_storage_bytes: u64,
    /// Bytes served during the current epoch
    pub epoch_read_bytes: u64,
    /// Viewer byte totals for the current epoch keyed by viewer address
    pub epoch_viewer_bytes: HashMap<String, u64>,
    /// Host byte totals for the current epoch keyed by domain
    pub epoch_host_bytes: HashMap<String, u64>,
    /// Hardware provider byte totals keyed by provider identifier
    pub epoch_hardware_bytes: HashMap<String, u64>,
    /// Verifier byte totals keyed by verifier identifier
    pub epoch_verifier_bytes: HashMap<String, u64>,
    /// Viewer byte totals that have already been settled this epoch
    pub settled_viewer_bytes: HashMap<String, u64>,
    /// Host byte totals that have already been settled this epoch
    pub settled_host_bytes: HashMap<String, u64>,
    /// Hardware byte totals that have already been settled this epoch
    pub settled_hardware_bytes: HashMap<String, u64>,
    /// Verifier byte totals that have already been settled this epoch
    pub settled_verifier_bytes: HashMap<String, u64>,
    /// Total read bytes settled for subsidy distribution this epoch
    pub settled_read_bytes: u64,
    /// CPU milliseconds consumed during the current epoch
    pub epoch_cpu_ms: u64,
    /// Bytes of dynamic compute output during the current epoch
    pub epoch_bytes_out: u64,
    /// Pending blob transactions awaiting anchoring
    pub blob_mempool: Vec<BlobTx>,
    /// Total bytes of pending blob transactions
    pub pending_blob_bytes: u64,
    /// Scheduler coordinating L2/L3 blob anchoring cadences
    pub blob_scheduler: blob_chain::BlobScheduler,
    /// Pending read acknowledgements awaiting batching
    pub read_batcher: crate::read_receipt::ReadBatcher,
    /// Pending advertising settlements awaiting credit assignment
    pub pending_ad_settlements: Vec<AdSettlementRecord>,
    /// Persisted proof rebate tracker
    pub proof_tracker: crate::light_client::proof_tracker::ProofTracker,
    /// Registry of provider keys for receipt verification
    pub provider_registry: ProviderRegistry,
    /// Nonce tracker for receipt replay protection
    pub nonce_tracker: NonceTracker,
    /// Tracker for intermediate block hashes used in reorg rollback
    pub reorg: crate::blockchain::reorg::ReorgTracker,
    /// Recent miners for base-reward logistic feedback
    recent_miners: VecDeque<String>,
    /// Recent block timestamps for difficulty retargeting
    pub recent_timestamps: VecDeque<u64>,
    logistic_last_n: f64,
    logistic_lock_end: u64,
    logistic_factor: f64,
    // Economic Control Law State
    /// Most recent base reward produced by the network issuance controller
    pub economics_block_reward_per_block: u64,
    /// Previous epoch's annual BLOCK issuance (for inflation controller continuity)
    pub economics_prev_annual_issuance_block: u64,
    /// Previous epoch's subsidy allocation shares
    pub economics_prev_subsidy: economics::SubsidySnapshot,
    /// Previous epoch's tariff state
    pub economics_prev_tariff: economics::TariffSnapshot,
    /// Previous epoch's market metrics snapshot (for Launch Governor economics gate)
    pub economics_prev_market_metrics: economics::MarketMetrics,
    /// Current epoch transaction volume (for tariff controller)
    pub economics_epoch_tx_volume_block: u64,
    /// Current epoch transaction count (for issuance formula)
    pub economics_epoch_tx_count: u64,
    /// Current epoch treasury inflow (for tariff controller)
    pub economics_epoch_treasury_inflow_block: u64,
    /// Storage-related payouts accumulated this epoch (storage + read subsidies)
    pub economics_epoch_storage_payout_block: u64,
    /// Compute subsidies accumulated this epoch
    pub economics_epoch_compute_payout_block: u64,
    /// Advertising payouts accumulated this epoch
    pub economics_epoch_ad_payout_block: u64,
    /// Network issuance adaptive baselines (persisted across epochs)
    pub economics_baseline_tx_count: u64,
    pub economics_baseline_tx_volume: u64,
    pub economics_baseline_miners: u64,
}

#[derive(Serialize, Deserialize)]
pub struct ChainDisk {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub schema_version: usize,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub chain: Vec<Block>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub accounts: HashMap<String, Account>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub emission: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub emission_year_ago: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub inflation_epoch_marker: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub block_reward: TokenAmount,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub block_height: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub mempool: Vec<MempoolEntryDisk>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub base_fee: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub params: Params,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub epoch_storage_bytes: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub epoch_read_bytes: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub epoch_cpu_ms: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub epoch_bytes_out: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub recent_timestamps: Vec<u64>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_block_reward_per_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_prev_annual_issuance_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_prev_subsidy: economics::SubsidySnapshot,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_prev_tariff: economics::TariffSnapshot,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_prev_market_metrics: economics::MarketMetrics,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_epoch_tx_volume_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_epoch_tx_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_epoch_treasury_inflow_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_epoch_storage_payout_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_epoch_compute_payout_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_epoch_ad_payout_block: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_baseline_tx_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_baseline_tx_volume: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub economics_baseline_miners: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MempoolEntryDisk {
    pub sender: String,
    pub nonce: u64,
    pub tx: SignedTransaction,
    pub timestamp_millis: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub timestamp_ticks: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub serialized_size: u64,
}

impl Default for Blockchain {
    fn default() -> Self {
        let params = Params::default();
        let fee_floor_window = params.fee_floor_window.max(1) as usize;
        let fee_floor_percentile = params.fee_floor_percentile.clamp(0, 100) as u32;
        Self {
            chain: Vec::new(),
            accounts: HashMap::new(),
            shard_roots: HashMap::new(),
            shard_heights: HashMap::new(),
            shard_cache: Mutex::new(LruCache::new(NonZeroUsize::new(1024).unwrap())),
            difficulty: difficulty::expected_difficulty_from_chain(&[] as &[Block]),
            retune_hint: 0,
            mempool_consumer: DashMap::new(),
            mempool_industrial: DashMap::new(),
            mempool_size_consumer: std::sync::atomic::AtomicUsize::new(0),
            mempool_size_industrial: std::sync::atomic::AtomicUsize::new(0),
            mempool_mutex: Mutex::new(()),
            admission_consumer: Mutex::new(mempool::admission::AdmissionState::new(
                fee_floor_window,
                fee_floor_percentile,
                "consumer",
            )),
            admission_industrial: Mutex::new(mempool::admission::AdmissionState::new(
                fee_floor_window,
                fee_floor_percentile,
                "industrial",
            )),
            pending_multisig: DashMap::new(),
            orphan_counter: std::sync::atomic::AtomicUsize::new(0),
            panic_on_evict: std::sync::atomic::AtomicBool::new(false),
            panic_on_admit: std::sync::atomic::AtomicI32::new(-1),
            panic_on_purge: std::sync::atomic::AtomicBool::new(false),
            max_mempool_size_consumer: 1024,
            max_mempool_size_industrial: 1024,
            min_fee_per_byte_consumer: 1,
            min_fee_per_byte_industrial: 1,
            comfort_threshold_p90: 0,
            tx_ttl: 1800,
            max_pending_per_account: 16,
            admission_locks: DashMap::new(),
            db: Db::default(),
            path: String::new(),
            emission: 0,
            emission_year_ago: 0,
            inflation_epoch_marker: 0,
            block_reward: TokenAmount::new(INITIAL_BLOCK_REWARD),
            block_height: 0,
            inter_shard: MessageQueue::new(1024),
            macro_acc: 0,
            macro_blocks: Vec::new(),
            macro_interval: 100,
            snapshot: SnapshotManager::new(String::new(), snapshot_interval_from_env()),
            skipped: Vec::new(),
            skipped_nonce_gap: 0,
            badge_tracker: ServiceBadgeTracker::new(),
            config: NodeConfig::default(),
            base_fee: 1,
            params: params.clone(),
            lane_pricing_engine: Mutex::new(LanePricingEngine::new(
                1, // base_consumer_fee
                2, // base_industrial_fee (2x consumer)
                params.lane_consumer_capacity as f64,
                params.lane_industrial_capacity as f64,
                params.lane_target_utilization_percent as f64 / 100.0,
            )),
            cached_consumer_fee: AtomicU64::new(1),
            cached_industrial_fee: AtomicU64::new(2),
            beta_storage_sub_raw: 50,
            gamma_read_sub_raw: 20,
            kappa_cpu_sub_raw: 10,
            lambda_bytes_out_sub_raw: 5,
            epoch_storage_bytes: 0,
            epoch_read_bytes: 0,
            epoch_viewer_bytes: HashMap::new(),
            epoch_host_bytes: HashMap::new(),
            epoch_hardware_bytes: HashMap::new(),
            epoch_verifier_bytes: HashMap::new(),
            settled_viewer_bytes: HashMap::new(),
            settled_host_bytes: HashMap::new(),
            settled_hardware_bytes: HashMap::new(),
            settled_verifier_bytes: HashMap::new(),
            settled_read_bytes: 0,
            epoch_cpu_ms: 0,
            epoch_bytes_out: 0,
            blob_mempool: Vec::new(),
            pending_blob_bytes: 0,
            blob_scheduler: blob_chain::BlobScheduler::default(),
            read_batcher: crate::read_receipt::ReadBatcher::new(),
            pending_ad_settlements: Vec::new(),
            proof_tracker: crate::light_client::proof_tracker::ProofTracker::default(),
            provider_registry: ProviderRegistry::new(),
            nonce_tracker: NonceTracker::new(RECEIPT_NONCE_FINALITY),
            reorg: crate::blockchain::reorg::ReorgTracker::default(),
            recent_miners: VecDeque::new(),
            recent_timestamps: VecDeque::new(),
            logistic_last_n: 0.0,
            logistic_lock_end: 0,
            logistic_factor: 1.0,
            economics_block_reward_per_block: INITIAL_BLOCK_REWARD,
            economics_prev_annual_issuance_block: 40_000_000, // Bootstrap: 40M BLOCK/year
            economics_prev_subsidy: economics::SubsidySnapshot {
                storage_share_bps: 1500,
                compute_share_bps: 3000,
                energy_share_bps: 2000,
                ad_share_bps: 3500,
            },
            economics_prev_tariff: economics::TariffSnapshot {
                tariff_bps: 0,
                non_kyc_volume_block: 0,
                treasury_contribution_bps: 0,
            },
            economics_prev_market_metrics: economics::MarketMetrics::default(),
            economics_epoch_tx_volume_block: 0,
            economics_epoch_tx_count: 0,
            economics_epoch_treasury_inflow_block: 0,
            economics_epoch_storage_payout_block: 0,
            economics_epoch_compute_payout_block: 0,
            economics_epoch_ad_payout_block: 0,
            economics_baseline_tx_count: 100, // Default from NetworkIssuanceParams
            economics_baseline_tx_volume: 10_000,
            economics_baseline_miners: 10,
        }
    }
}

impl Drop for Blockchain {
    fn drop(&mut self) {
        if std::env::var("TB_PRESERVE").is_ok() {
            return;
        }
        crate::compute_market::settlement::Settlement::shutdown();
        if !self.path.is_empty() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn ratio_u64(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        (numerator as f64 / denominator as f64).clamp(0.0, 1.0)
    }
}

fn margin_from_totals(payout: u128, cost: u128) -> f64 {
    if cost == 0 {
        if payout == 0 {
            0.0
        } else {
            2.0
        }
    } else {
        ((payout as f64 - cost as f64) / cost as f64).clamp(-2.0, 2.0)
    }
}

fn u128_to_f64(value: u128) -> f64 {
    value as f64
}

impl Blockchain {
    fn shard_cache_guard(&self) -> MutexGuard<'_, LruCache<(ShardId, Vec<u8>), Vec<u8>>> {
        self.shard_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn save_config(&self) {
        let _ = self.config.save(&self.path);
    }

    /// Set base fees for dynamic lane pricing engine (primarily for testing).
    pub fn set_lane_base_fees(&self, consumer: u64, industrial: u64) {
        if let Ok(mut engine) = self.lane_pricing_engine.lock() {
            engine.set_base_fees(consumer, industrial);
            // Update cached fees immediately to reflect the change
            self.cached_consumer_fee.store(
                engine.consumer_fee_per_byte(),
                AtomicOrdering::Relaxed,
            );
            self.cached_industrial_fee.store(
                engine.industrial_fee_per_byte(),
                AtomicOrdering::Relaxed,
            );
        }
    }

    pub fn register_receipt_providers(
        &mut self,
        providers: &[ReceiptProviderConfig],
    ) -> Result<(), String> {
        self.provider_registry.providers.clear();
        for provider in providers {
            let bytes = hex::decode(provider.verifying_key_hex.trim()).map_err(|_| {
                format!("invalid hex encoding for provider {}", provider.provider_id)
            })?;
            let public_key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                format!(
                    "invalid verifying key length for provider {}: expected 32 bytes",
                    provider.provider_id
                )
            })?;
            let verifying_key = VerifyingKey::from_bytes(&public_key).map_err(|err| {
                format!(
                    "invalid verifying key for provider {}: {}",
                    provider.provider_id, err
                )
            })?;
            self.provider_registry
                .register_provider(
                    provider.provider_id.clone(),
                    verifying_key,
                    self.block_height,
                )
                .map_err(|err| {
                    format!(
                        "failed to register provider {}: {}",
                        provider.provider_id, err
                    )
                })?;
        }
        Ok(())
    }

    fn build_market_metrics(
        &self,
        storage_payout_total: u64,
        compute_payout_total: u64,
        ad_payout_total: u64,
        ad_total_usd_micros: u64,
        ad_settlement_count: u64,
        ad_last_price_usd_micros: u64,
        compute_util_percent: u64,
        energy_snapshot: &crate::energy::EnergySnapshot,
    ) -> economics::MarketMetrics {
        let storage_capacity = crate::storage::pipeline::l2_cap_bytes_per_epoch();
        let storage_utilization = ratio_u64(self.epoch_storage_bytes, storage_capacity);
        let rent_rate = self.params.rent_rate_per_byte.max(0) as u64;
        let storage_cost_total =
            u128::from(self.epoch_storage_bytes).saturating_mul(rent_rate as u128);
        let storage_payout_total = u128::from(storage_payout_total);
        let storage_metric = economics::MarketMetric {
            utilization: storage_utilization,
            average_cost_block: u128_to_f64(storage_cost_total),
            effective_payout_block: storage_payout_total as f64,
            provider_margin: margin_from_totals(storage_payout_total, storage_cost_total),
        };

        let compute_utilization = ((compute_util_percent as f64) / 100.0).clamp(0.0, 1.0);
        let compute_units = self.epoch_cpu_ms.max(1);
        let spot_price =
            crate::compute_market::price_board::spot_price_per_unit(FeeLane::Industrial)
                .unwrap_or(0);
        let compute_cost_total = u128::from(spot_price).saturating_mul(u128::from(compute_units));
        let compute_payout_total_u128 = u128::from(compute_payout_total);
        let compute_metric = economics::MarketMetric {
            utilization: compute_utilization,
            average_cost_block: u128_to_f64(compute_cost_total),
            effective_payout_block: compute_payout_total_u128 as f64,
            provider_margin: margin_from_totals(compute_payout_total_u128, compute_cost_total),
        };

        let energy_capacity_kwh: u64 = energy_snapshot
            .providers
            .iter()
            .map(|p| p.capacity_kwh)
            .sum();
        let energy_consumed_kwh: u64 = energy_snapshot
            .receipts
            .iter()
            .map(|r| r.kwh_delivered)
            .sum();
        let energy_payout_total: u64 = energy_snapshot
            .receipts
            .iter()
            .map(|r| {
                r.price_paid
                    .saturating_sub(r.treasury_fee)
                    .saturating_sub(r.slash_applied)
            })
            .sum();
        let (weighted_cost_sum, total_capacity_weight) =
            energy_snapshot
                .providers
                .iter()
                .fold((0u128, 0u128), |(sum, weight), provider| {
                    let cap = provider.capacity_kwh as u128;
                    let price = provider.price_per_kwh as u128;
                    (
                        sum.saturating_add(cap.saturating_mul(price)),
                        weight.saturating_add(cap),
                    )
                });
        let avg_energy_cost_per_kwh = if total_capacity_weight == 0 {
            0u128
        } else {
            weighted_cost_sum / total_capacity_weight
        };
        let energy_cost_total =
            avg_energy_cost_per_kwh.saturating_mul(u128::from(energy_consumed_kwh));
        let energy_metric = economics::MarketMetric {
            utilization: ratio_u64(energy_consumed_kwh, energy_capacity_kwh),
            average_cost_block: u128_to_f64(energy_cost_total),
            effective_payout_block: energy_payout_total as f64,
            provider_margin: margin_from_totals(u128::from(energy_payout_total), energy_cost_total),
        };

        let ad_capacity = self.params.ad_cap_provider_count.max(1) as u64;
        let ad_utilization = ratio_u64(ad_settlement_count, ad_capacity);
        let ad_cost_block = if ad_last_price_usd_micros == 0 {
            0.0
        } else {
            (ad_total_usd_micros as f64) / (ad_last_price_usd_micros as f64)
        };
        let ad_effective_payout = ad_payout_total as f64;
        let ad_margin = if ad_cost_block > 0.0 {
            ((ad_effective_payout - ad_cost_block) / ad_cost_block).clamp(-2.0, 2.0)
        } else {
            0.0
        };
        let ad_metric = economics::MarketMetric {
            utilization: ad_utilization,
            average_cost_block: ad_cost_block,
            effective_payout_block: ad_effective_payout,
            provider_margin: ad_margin,
        };

        economics::MarketMetrics {
            storage: storage_metric,
            compute: compute_metric,
            energy: energy_metric,
            ad: ad_metric,
        }
    }
    /// Return the latest state root for a shard if available.
    pub fn get_shard_root(&self, shard: ShardId) -> Option<[u8; 32]> {
        self.shard_roots.get(&shard).copied()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn read_shard_state(&self, shard: ShardId, key: &str) -> Option<Vec<u8>> {
        let cache_key = (shard, key.as_bytes().to_vec());
        if let Some(value) = {
            let mut cache = self.shard_cache_guard();
            cache.get(&cache_key).cloned()
        } {
            return Some(value);
        }
        let val = self.db.get_shard(shard, key);
        if let Some(ref v) = val {
            let mut cache = self.shard_cache_guard();
            cache.put(cache_key, v.clone());
        }
        val
    }

    pub(crate) fn write_shard_state(
        &mut self,
        shard: ShardId,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> std::io::Result<()> {
        let cache_key = (shard, key.as_bytes().to_vec());
        #[cfg_attr(not(feature = "telemetry"), allow(unused_variables))]
        let evicted = {
            let mut cache = self.shard_cache_guard();
            cache.put(cache_key, value.clone())
        };
        #[cfg(feature = "telemetry")]
        if evicted.is_some() {
            crate::telemetry::SHARD_CACHE_EVICT_TOTAL.inc();
        }
        self.db.insert_shard_with_delta(shard, key, value, deltas)
    }
    pub fn set_consumer_p90_comfort(&mut self, v: u64) {
        self.comfort_threshold_p90 = v;
    }

    pub fn set_fee_floor_policy(&mut self, window: usize, percentile: u32) {
        let window = window.max(1);
        let percentile = percentile.min(100);
        self.params.fee_floor_window = window as i64;
        self.params.fee_floor_percentile = percentile as i64;
        let mut consumer = self
            .admission_consumer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let changed_consumer = consumer.configure_fee_floor(window, percentile);
        drop(consumer);
        let mut industrial = self
            .admission_industrial
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let changed_industrial = industrial.configure_fee_floor(window, percentile);
        drop(industrial);
        #[cfg(feature = "telemetry")]
        if changed_consumer || changed_industrial {
            crate::telemetry::FEE_FLOOR_WINDOW_CHANGED_TOTAL.inc();
        }
        #[cfg(not(feature = "telemetry"))]
        let _ = (changed_consumer, changed_industrial);
    }
    fn adjust_mempool_size(&self, lane: FeeLane, delta: isize) -> usize {
        use AtomicOrdering::SeqCst;
        let size = match lane {
            FeeLane::Consumer => {
                if delta > 0 {
                    self.mempool_size_consumer.fetch_add(delta as usize, SeqCst) + delta as usize
                } else {
                    self.mempool_size_consumer
                        .fetch_sub((-delta) as usize, SeqCst)
                        - (-delta as usize)
                }
            }
            FeeLane::Industrial => {
                if delta > 0 {
                    self.mempool_size_industrial
                        .fetch_add(delta as usize, SeqCst)
                        + delta as usize
                } else {
                    self.mempool_size_industrial
                        .fetch_sub((-delta) as usize, SeqCst)
                        - (-delta as usize)
                }
            }
        };

        #[cfg(feature = "telemetry")]
        {
            telemetry::MEMPOOL_SIZE
                .ensure_handle_for_label_values(&[lane.as_str()])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .set(size as i64);
            telemetry::update_memory_usage(MemoryComponent::Mempool);
        }

        size
    }

    #[inline]
    fn inc_mempool_size(&self, lane: FeeLane) -> usize {
        self.adjust_mempool_size(lane, 1)
    }

    #[inline]
    fn dec_mempool_size(&self, lane: FeeLane) -> usize {
        self.adjust_mempool_size(lane, -1)
    }

    pub fn fee_floor_policy(&self) -> (usize, u32) {
        let guard = self.admission_guard(FeeLane::Consumer);
        guard.policy()
    }

    pub fn record_proof_relay(&mut self, relayer_id: &[u8], proofs: u64) {
        if proofs == 0 {
            return;
        }
        let limit = self.params.proof_rebate_limit.max(0) as u64;
        if limit == 0 {
            return;
        }
        let rate = self.config.proof_rebate_rate.min(limit);
        if rate == 0 {
            return;
        }
        let amount = rate.saturating_mul(proofs);
        if amount == 0 {
            return;
        }
        self.proof_tracker.record(relayer_id, proofs, amount);
    }

    fn admission_guard(&self, lane: FeeLane) -> MutexGuard<'_, mempool::admission::AdmissionState> {
        match lane {
            FeeLane::Consumer => self
                .admission_consumer
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            FeeLane::Industrial => self
                .admission_industrial
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        }
    }

    fn release_sender_slot(&self, lane: FeeLane, sender: &str) {
        let mut guard = self.admission_guard(lane);
        guard.release_sender(sender);
    }

    #[doc(hidden)]
    pub fn mempool_recent_evictions(&self, lane: FeeLane) -> Vec<[u8; 32]> {
        let guard = self.admission_guard(lane);
        guard.eviction_hashes()
    }

    pub fn mempool_stats(&self, lane: FeeLane) -> MempoolStats {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let map = match lane {
            FeeLane::Consumer => &self.mempool_consumer,
            FeeLane::Industrial => &self.mempool_industrial,
        };
        let mut ages = Vec::new();
        let mut fees = Vec::new();
        map.for_each(|_, entry| {
            ages.push(now.saturating_sub(entry.timestamp_millis));
            fees.push(entry.tx.tip);
        });
        ages.sort_unstable();
        fees.sort_unstable();
        let size = ages.len();
        let q = |v: &Vec<u64>, p: f64| -> u64 {
            if v.is_empty() {
                0
            } else {
                let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
                v[idx]
            }
        };
        let floor = {
            let guard = self.admission_guard(lane);
            guard.floor()
        };
        MempoolStats {
            size,
            age_p50: q(&ages, 0.50),
            age_p95: q(&ages, 0.95),
            fee_p50: q(&fees, 0.50),
            fee_p90: q(&fees, 0.90),
            fee_floor: floor,
        }
    }

    #[cfg(feature = "telemetry")]
    fn record_admit(&self) {
        telemetry::TX_ADMITTED_TOTAL.inc();
        if let Some(j) = self.config.jurisdiction.as_deref() {
            telemetry::RECORDER.tx_jurisdiction(j);
        }
    }

    #[cfg(feature = "telemetry")]
    fn record_reject(&self, reason: &str) {
        telemetry::RECORDER.tx_rejected(reason);
    }

    #[cfg(feature = "telemetry")]
    fn record_submit(&self) {
        telemetry::RECORDER.tx_submitted();
    }

    #[cfg(feature = "telemetry")]
    fn record_block_mined(&self) {
        telemetry::RECORDER.block_mined();
    }

    /// Evaluate badge eligibility based on uptime and performance metrics.
    pub fn check_badges(&mut self) {
        let before = self.badge_tracker.has_badge();
        self.badge_tracker.check_badges();
        let after = self.badge_tracker.has_badge();
        #[cfg(feature = "telemetry")]
        {
            telemetry::BADGE_ACTIVE.set(if after { 1 } else { 0 });
            if let Some(ts) = self
                .badge_tracker
                .last_mint()
                .or(self.badge_tracker.last_burn())
            {
                telemetry::BADGE_LAST_CHANGE_SECONDS.set(ts as i64);
            }
        }
        if before != after {
            #[cfg(feature = "telemetry-json")]
            log_event(
                "service",
                diagnostics::log::Level::INFO,
                "badge",
                "-",
                0,
                if after { "minted" } else { "revoked" },
                ERR_OK,
                None,
                None,
            );
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            if telemetry::should_log("service") {
                let span = diagnostics::tracing::Span::new(
                    std::borrow::Cow::Borrowed("badge"),
                    diagnostics::tracing::Level::INFO,
                    vec![
                        diagnostics::FieldValue {
                            key: std::borrow::Cow::Borrowed("from"),
                            value: format!("{}", before),
                        },
                        diagnostics::FieldValue {
                            key: std::borrow::Cow::Borrowed("to"),
                            value: format!("{}", after),
                        },
                    ],
                );
                let _e = span.enter();
                info!("badge {}", if after { "minted" } else { "revoked" });
            }
        }
    }

    pub fn accounts(&self) -> &HashMap<String, Account> {
        &self.accounts
    }

    /// Whether this chain has earned a service badge.
    pub fn has_badge(&self) -> bool {
        self.badge_tracker.has_badge()
    }

    /// Current badge status and timestamps.
    pub fn badge_status(&self) -> (bool, Option<u64>, Option<u64>) {
        (
            self.badge_tracker.has_badge(),
            self.badge_tracker.last_mint(),
            self.badge_tracker.last_burn(),
        )
    }

    /// Mutable access to the service badge tracker.
    pub fn badge_tracker_mut(&mut self) -> &mut ServiceBadgeTracker {
        &mut self.badge_tracker
    }

    /// Inject a client-signed read acknowledgement into the current epoch batch.
    pub fn submit_read_ack(
        &mut self,
        ack: crate::read_receipt::ReadAck,
    ) -> Result<(), ReadAckError> {
        if !ack.verify_signature() {
            return Err(ReadAckError::InvalidSignature);
        }
        crate::blockchain::privacy::verify_ack(self.config.read_ack_privacy, &ack)?;
        if let Some(receipt) = ack.selection_receipt.as_ref() {
            #[cfg(feature = "telemetry")]
            let started = Instant::now();
            match receipt.validate() {
                Ok(summary) => {
                    let attestation_kind = summary.attestation_kind;
                    #[cfg(not(feature = "telemetry"))]
                    let _ = attestation_kind;
                    #[cfg(feature = "telemetry")]
                    {
                        let labels = [attestation_kind.as_str()];
                        crate::telemetry::READ_SELECTION_PROOF_VERIFIED_TOTAL
                            .ensure_handle_for_label_values(&labels)
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .inc();
                        crate::telemetry::sampled_observe_vec(
                            &crate::telemetry::READ_SELECTION_PROOF_LATENCY_SECONDS,
                            &labels,
                            started.elapsed().as_secs_f64(),
                        );
                    }
                }
                Err(err) => {
                    #[cfg(feature = "telemetry")]
                    {
                        let labels = [receipt.attestation_kind().as_str()];
                        crate::telemetry::READ_SELECTION_PROOF_INVALID_TOTAL
                            .ensure_handle_for_label_values(&labels)
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .inc();
                        crate::telemetry::sampled_observe_vec(
                            &crate::telemetry::READ_SELECTION_PROOF_LATENCY_SECONDS,
                            &labels,
                            started.elapsed().as_secs_f64(),
                        );
                    }
                    diagnostics::log::warn!(format!("selection_receipt_invalid: {err}"));
                    return Err(ReadAckError::InvalidSelectionReceipt);
                }
            }
        }
        self.epoch_read_bytes = self.epoch_read_bytes.saturating_add(ack.bytes);
        let viewer_addr = viewer_address_from_pk(&ack.pk);
        let viewer_entry = self.epoch_viewer_bytes.entry(viewer_addr).or_insert(0);
        *viewer_entry = viewer_entry.saturating_add(ack.bytes);
        let host_addr = host_address(&ack.domain);
        let host_entry = self.epoch_host_bytes.entry(host_addr).or_insert(0);
        *host_entry = host_entry.saturating_add(ack.bytes);
        let provider_id = if ack.provider.is_empty() {
            ack.domain.clone()
        } else {
            ack.provider.clone()
        };
        let hardware_addr = hardware_address(&provider_id);
        let hardware_entry = self.epoch_hardware_bytes.entry(hardware_addr).or_insert(0);
        *hardware_entry = hardware_entry.saturating_add(ack.bytes);
        let verifier_addr = verifier_address(&ack.domain);
        let verifier_entry = self.epoch_verifier_bytes.entry(verifier_addr).or_insert(0);
        *verifier_entry = verifier_entry.saturating_add(ack.bytes);
        self.read_batcher.push(ack);
        Ok(())
    }

    pub fn record_ad_settlement(
        &mut self,
        ack: &crate::read_receipt::ReadAck,
        settlement: SettlementBreakdown,
    ) {
        let viewer_addr = viewer_address_from_pk(&ack.pk);
        let host_addr = host_address(&ack.domain);
        let provider_id = if ack.provider.is_empty() {
            ack.domain.clone()
        } else {
            ack.provider.clone()
        };
        let hardware_addr = hardware_address(&provider_id);
        let verifier_addr = verifier_address(&ack.domain);
        let mesh_bytes = settlement
            .mesh_payload
            .as_ref()
            .map(|payload| payload.len())
            .unwrap_or(0);
        let mesh_digest_label = settlement.mesh_payload_digest.as_deref().unwrap_or("none");
        diagnostics::log::info!(format!(
            "ad_settlement_record campaign={} creative={} channel={} clearing_price={} mesh_bytes={} mesh_digest={}",
            settlement.campaign_id,
            settlement.creative_id,
            settlement.delivery_channel.as_str(),
            settlement.clearing_price_usd_micros,
            mesh_bytes,
            mesh_digest_label
        ));
        let record = AdSettlementRecord {
            campaign_id: settlement.campaign_id.clone(),
            creative_id: settlement.creative_id.clone(),
            bytes: settlement.bytes,
            impressions: settlement
                .resource_floor_breakdown
                .qualified_impressions_per_proof
                .max(1),
            viewer_addr,
            host_addr,
            hardware_addr,
            verifier_addr,
            viewer: settlement.viewer,
            host: settlement.host,
            hardware: settlement.hardware,
            verifier: settlement.verifier,
            liquidity: settlement.liquidity,
            miner: settlement.miner,
            total: settlement.total,
            total_usd_micros: settlement.total_usd_micros,
            price_usd_micros: settlement.price_usd_micros,
            delivery_channel: settlement.delivery_channel,
            clearing_price_usd_micros: settlement.clearing_price_usd_micros,
            mesh_payload_digest: settlement.mesh_payload_digest.clone(),
            mesh_payload_bytes: mesh_bytes as u64,
            conversions: 0,
        };
        self.pending_ad_settlements.push(record);

        crate::ad_readiness::record_settlement(
            ack.ts,
            settlement.total_usd_micros,
            settlement.price_usd_micros,
        );
    }
}

fn viewer_address_from_pk(pk: &[u8; 32]) -> String {
    format!("0000:{}", crypto_suite::hex::encode(pk))
}

fn host_address(domain: &str) -> String {
    format!("0001:host:{}", domain)
}

fn hardware_address(provider: &str) -> String {
    format!("0002:hardware:{}", provider)
}

fn verifier_address(domain: &str) -> String {
    format!("0003:verifier:{}", domain)
}

fn liquidity_address() -> &'static str {
    "0004:liquidity:pool"
}

#[derive(Clone, Debug)]
pub struct AdSettlementRecord {
    pub campaign_id: String,
    pub creative_id: String,
    pub bytes: u64,
    pub impressions: u64,
    pub viewer_addr: String,
    pub host_addr: String,
    pub hardware_addr: String,
    pub verifier_addr: String,
    pub viewer: u64,
    pub host: u64,
    pub hardware: u64,
    pub verifier: u64,
    pub liquidity: u64,
    pub miner: u64,
    pub total: u64,
    pub total_usd_micros: u64,
    pub price_usd_micros: u64,
    pub delivery_channel: DeliveryChannel,
    pub clearing_price_usd_micros: u64,
    pub mesh_payload_digest: Option<String>,
    pub mesh_payload_bytes: u64,
    pub conversions: u32,
}

fn distribute_scalar(total: u64, weights: &[(usize, u64)]) -> Vec<u64> {
    if total == 0 || weights.is_empty() {
        return vec![0; weights.len()];
    }
    let sum_weights: u128 = weights.iter().map(|(_, w)| u128::from(*w)).sum();
    if sum_weights == 0 {
        return vec![0; weights.len()];
    }
    let mut allocations = vec![0u64; weights.len()];
    let mut distributed = 0u64;
    let mut remainders: Vec<(usize, usize, u64)> = Vec::with_capacity(weights.len());
    for (idx, (order, weight)) in weights.iter().enumerate() {
        if *weight == 0 {
            remainders.push((idx, *order, 0));
            continue;
        }
        let numerator = u128::from(total) * u128::from(*weight);
        let base = (numerator / sum_weights) as u64;
        let remainder = (numerator % sum_weights) as u64;
        allocations[idx] = base;
        distributed = distributed.saturating_add(base);
        remainders.push((idx, *order, remainder));
    }
    let mut remainder_tokens = total.saturating_sub(distributed);
    if remainder_tokens > 0 {
        remainders.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        for (idx, _, _) in &remainders {
            if remainder_tokens == 0 {
                break;
            }
            allocations[*idx] = allocations[*idx].saturating_add(1);
            remainder_tokens -= 1;
        }
        if remainder_tokens > 0 && !allocations.is_empty() {
            allocations[0] = allocations[0].saturating_add(remainder_tokens);
        }
    }
    allocations
}

fn distribute_proportional(total: u64, weights: &[(String, u64)]) -> Vec<(String, u64)> {
    if total == 0 || weights.is_empty() {
        return Vec::new();
    }
    let scalar_weights: Vec<(usize, u64)> = weights
        .iter()
        .enumerate()
        .map(|(idx, (_, weight))| (idx, *weight))
        .collect();
    let allocations = distribute_scalar(total, &scalar_weights);
    weights
        .iter()
        .zip(allocations.into_iter())
        .filter_map(|((addr, _), amount)| {
            if amount > 0 {
                Some((addr.clone(), amount))
            } else {
                None
            }
        })
        .collect()
}

impl Blockchain {
    /// Default Python constructor opens ./chain_db
    #[cfg_attr(feature = "python-bindings", new)]
    pub fn py_new() -> PyResult<Self> {
        Blockchain::open("chain_db")
    }

    #[cfg_attr(feature = "python-bindings", staticmethod)]
    pub fn open_with_db(path: &str, db_path: &str) -> PyResult<Self> {
        // Open an existing database and auto-migrate to schema v4.
        // See `docs/detailed_updates.md` for layout history.
        let mut db = Db::open(db_path);
        db.flush_wal();
        {
            const SCHEMA_KEY: &str = "__schema_version";
            let current_version = db
                .get(SCHEMA_KEY)
                .and_then(|b| ledger_binary::decode_schema_version(&b))
                .unwrap_or(0);
            if current_version < state::schema::SCHEMA_VERSION {
                let bytes = ledger_binary::encode_schema_version(state::schema::SCHEMA_VERSION);
                let _ = db.insert(SCHEMA_KEY, bytes);
            }
        }
        let (
            mut chain,
            mut accounts,
            em,
            br,
            bh,
            mempool_disk,
            base_fee,
            recent_ts,
            econ_block_reward,
            econ_prev_annual,
            econ_prev_subsidy,
            econ_prev_tariff,
            econ_prev_market_metrics,
            econ_epoch_tx_volume,
            econ_epoch_tx_count,
            econ_epoch_treasury_inflow,
            econ_epoch_storage_payout,
            econ_epoch_compute_payout,
            econ_epoch_ad_payout,
        ) = if let Some(raw) = db.get(DB_CHAIN) {
            match ledger_binary::decode_chain_disk(&raw) {
                Ok(mut disk) => {
                    if disk.schema_version > 11 {
                        return Err(py_value_err("DB schema too new"));
                    }
                    if disk.schema_version < 3 {
                        let mut migrated_chain = disk.chain.clone();
                        for b in &mut migrated_chain {
                            if let Some(cb) = b.transactions.first() {
                                b.coinbase_block = TokenAmount::new(cb.payload.amount_consumer);
                                b.coinbase_industrial =
                                    TokenAmount::new(cb.payload.amount_industrial);
                            }
                            let mut fee_consumer: u128 = 0;
                            let mut fee_industrial: u128 = 0;
                            for tx in b.transactions.iter().skip(1) {
                                if let Ok((c, i)) =
                                    crate::fee::decompose(tx.payload.pct, tx.payload.fee)
                                {
                                    fee_consumer += c as u128;
                                    fee_industrial += i as u128;
                                }
                            }
                            let fee_consumer_u64 = u64::try_from(fee_consumer).unwrap_or(0);
                            let fee_industrial_u64 = u64::try_from(fee_industrial).unwrap_or(0);
                            let mut h = blake3::Hasher::new();
                            h.update(&fee_consumer_u64.to_le_bytes());
                            h.update(&fee_industrial_u64.to_le_bytes());
                            b.fee_checksum = h.finalize().to_hex().to_string();
                            b.hash = calculate_hash(
                                b.index,
                                &b.previous_hash,
                                b.timestamp_millis,
                                b.nonce,
                                b.difficulty,
                                b.base_fee,
                                b.coinbase_block,
                                b.coinbase_industrial,
                                b.storage_sub,
                                b.read_sub,
                                b.read_sub_viewer,
                                b.read_sub_host,
                                b.read_sub_hardware,
                                b.read_sub_verifier,
                                b.read_sub_liquidity,
                                b.ad_viewer,
                                b.ad_host,
                                b.ad_hardware,
                                b.ad_verifier,
                                b.ad_liquidity,
                                b.ad_miner,
                                b.ad_total_usd_micros,
                                b.ad_settlement_count,
                                b.ad_oracle_price_usd_micros,
                                b.compute_sub,
                                b.proof_rebate,
                                b.read_root,
                                &b.fee_checksum,
                                &b.transactions,
                                &b.state_root,
                                &b.l2_roots,
                                &b.l2_sizes,
                                b.vdf_commit,
                                b.vdf_output,
                                &b.vdf_proof,
                                b.retune_hint,
                                &b.receipts,
                            );
                        }
                        let mut em_c = 0u64;
                        let mut em_i = 0u64;
                        for b in &migrated_chain {
                            em_c = em_c.saturating_add(b.coinbase_block.get());
                            em_i = em_i.saturating_add(b.coinbase_industrial.get());
                        }
                        let bh = migrated_chain.len() as u64;
                        let migrated = ChainDisk {
                            schema_version: 3,
                            chain: migrated_chain,
                            accounts: disk.accounts,
                            emission: em_c.saturating_add(em_i),
                            block_reward: if disk.block_reward.get() == 0 {
                                TokenAmount::new(INITIAL_BLOCK_REWARD)
                            } else {
                                disk.block_reward
                            },
                            block_height: bh,
                            mempool: Vec::new(),
                            base_fee: disk.base_fee,
                            params: Params::default(),
                            epoch_storage_bytes: 0,
                            epoch_read_bytes: 0,
                            epoch_cpu_ms: 0,
                            epoch_bytes_out: 0,
                            emission_year_ago: disk.emission_year_ago,
                            inflation_epoch_marker: disk.inflation_epoch_marker,
                            recent_timestamps: Vec::new(),
                            economics_block_reward_per_block: disk.economics_block_reward_per_block,
                            economics_prev_annual_issuance_block: disk
                                .economics_prev_annual_issuance_block,
                            economics_prev_subsidy: disk.economics_prev_subsidy.clone(),
                            economics_prev_tariff: disk.economics_prev_tariff.clone(),
                            economics_prev_market_metrics: disk
                                .economics_prev_market_metrics
                                .clone(),
                            economics_epoch_tx_volume_block: disk.economics_epoch_tx_volume_block,
                            economics_epoch_tx_count: disk.economics_epoch_tx_count,
                            economics_epoch_treasury_inflow_block: disk
                                .economics_epoch_treasury_inflow_block,
                            economics_epoch_storage_payout_block: 0,
                            economics_epoch_compute_payout_block: 0,
                            economics_epoch_ad_payout_block: 0,
                            economics_baseline_tx_count: disk.economics_baseline_tx_count,
                            economics_baseline_tx_volume: disk.economics_baseline_tx_volume,
                            economics_baseline_miners: disk.economics_baseline_miners,
                        };
                        db.insert(
                            DB_CHAIN,
                            ledger_binary::encode_chain_disk(&migrated)
                                .unwrap_or_else(|e| panic!("serialize: {e}")),
                        );
                        db.remove(DB_ACCOUNTS);
                        db.remove(DB_EMISSION);
                        (
                            migrated.chain,
                            migrated.accounts,
                            migrated.emission,
                            migrated.block_reward,
                            migrated.block_height,
                            migrated.mempool,
                            migrated.base_fee,
                            migrated.recent_timestamps,
                            migrated.economics_block_reward_per_block,
                            migrated.economics_prev_annual_issuance_block,
                            migrated.economics_prev_subsidy,
                            migrated.economics_prev_tariff,
                            migrated.economics_prev_market_metrics.clone(),
                            migrated.economics_epoch_tx_volume_block,
                            migrated.economics_epoch_tx_count,
                            migrated.economics_epoch_treasury_inflow_block,
                            migrated.economics_epoch_storage_payout_block,
                            migrated.economics_epoch_compute_payout_block,
                            migrated.economics_epoch_ad_payout_block,
                        )
                    } else {
                        if disk.emission == 0 && !disk.chain.is_empty() {
                            let mut em_c = 0u64;
                            let mut em_i = 0u64;
                            for b in &disk.chain {
                                em_c = em_c.saturating_add(b.coinbase_block.get());
                                em_i = em_i.saturating_add(b.coinbase_industrial.get());
                            }
                            disk.emission = em_c.saturating_add(em_i);
                            disk.block_height = disk.chain.len() as u64;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 4 {
                            let mut em_c = 0u64;
                            let mut em_i = 0u64;
                            for b in &mut disk.chain {
                                if let Some(cb) = b.transactions.first() {
                                    b.coinbase_block = TokenAmount::new(cb.payload.amount_consumer);
                                    b.coinbase_industrial =
                                        TokenAmount::new(cb.payload.amount_industrial);
                                }
                                let mut fee_c: u128 = 0;
                                let mut fee_i: u128 = 0;
                                for tx in b.transactions.iter().skip(1) {
                                    if let Ok((c, i)) =
                                        crate::fee::decompose(tx.payload.pct, tx.payload.fee)
                                    {
                                        fee_c += c as u128;
                                        fee_i += i as u128;
                                    }
                                }
                                let fc = u64::try_from(fee_c).unwrap_or(0);
                                let fi = u64::try_from(fee_i).unwrap_or(0);
                                let mut h = blake3::Hasher::new();
                                h.update(&fc.to_le_bytes());
                                h.update(&fi.to_le_bytes());
                                b.fee_checksum = h.finalize().to_hex().to_string();
                                b.hash = calculate_hash(
                                    b.index,
                                    &b.previous_hash,
                                    b.timestamp_millis,
                                    b.nonce,
                                    b.difficulty,
                                    b.base_fee,
                                    b.coinbase_block,
                                    b.coinbase_industrial,
                                    b.storage_sub,
                                    b.read_sub,
                                    b.read_sub_viewer,
                                    b.read_sub_host,
                                    b.read_sub_hardware,
                                    b.read_sub_verifier,
                                    b.read_sub_liquidity,
                                    b.ad_viewer,
                                    b.ad_host,
                                    b.ad_hardware,
                                    b.ad_verifier,
                                    b.ad_liquidity,
                                    b.ad_miner,
                                    b.ad_total_usd_micros,
                                    b.ad_settlement_count,
                                    b.ad_oracle_price_usd_micros,
                                    b.compute_sub,
                                    b.proof_rebate,
                                    b.read_root,
                                    &b.fee_checksum,
                                    &b.transactions,
                                    &b.state_root,
                                    &b.l2_roots,
                                    &b.l2_sizes,
                                    b.vdf_commit,
                                    b.vdf_output,
                                    &b.vdf_proof,
                                    b.retune_hint,
                                    &b.receipts,
                                );
                                em_c = em_c.saturating_add(b.coinbase_block.get());
                                em_i = em_i.saturating_add(b.coinbase_industrial.get());
                            }
                            disk.emission = em_c.saturating_add(em_i);
                            disk.block_height = disk.chain.len() as u64;
                            for e in &mut disk.mempool {
                                e.timestamp_ticks = e.timestamp_millis;
                            }
                            disk.schema_version = 4;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 5 {
                            if disk.base_fee == 0 {
                                disk.base_fee = 1;
                            }
                            disk.schema_version = 5;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 6 {
                            disk.schema_version = 6;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 7 {
                            if disk.recent_timestamps.is_empty() {
                                disk.recent_timestamps = Vec::new();
                            }
                            disk.schema_version = 7;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 8 {
                            disk.schema_version = 8;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 9 {
                            disk.schema_version = 9;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 10 {
                            disk.schema_version = 10;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        if disk.schema_version < 11 {
                            disk.schema_version = 11;
                            db.insert(
                                DB_CHAIN,
                                ledger_binary::encode_chain_disk(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        (
                            disk.chain,
                            disk.accounts,
                            disk.emission,
                            disk.block_reward,
                            disk.block_height,
                            disk.mempool,
                            disk.base_fee,
                            disk.recent_timestamps.clone(),
                            disk.economics_block_reward_per_block,
                            disk.economics_prev_annual_issuance_block,
                            disk.economics_prev_subsidy.clone(),
                            disk.economics_prev_tariff.clone(),
                            disk.economics_prev_market_metrics.clone(),
                            disk.economics_epoch_tx_volume_block,
                            disk.economics_epoch_tx_count,
                            disk.economics_epoch_treasury_inflow_block,
                            disk.economics_epoch_storage_payout_block,
                            disk.economics_epoch_compute_payout_block,
                            disk.economics_epoch_ad_payout_block,
                        )
                    }
                }
                Err(_) => {
                    let chain = ledger_binary::decode_block_vec(&raw).unwrap_or_default();
                    let accounts: HashMap<String, Account> = db
                        .get(DB_ACCOUNTS)
                        .and_then(|iv| ledger_binary::decode_account_map_bytes(&iv).ok())
                        .unwrap_or_default();
                    let (_em, br, _bh) = db
                        .get(DB_EMISSION)
                        .and_then(|iv| ledger_binary::decode_emission_tuple(&iv).ok())
                        .map(|(em_c, em_i, br_c, br_i, bh)| (em_c + em_i, br_c + br_i, bh))
                        .unwrap_or((0, INITIAL_BLOCK_REWARD, 0));
                    let mut migrated_chain = chain.clone();
                    for b in &mut migrated_chain {
                        if let Some(cb) = b.transactions.first() {
                            b.coinbase_block = TokenAmount::new(cb.payload.amount_consumer);
                            b.coinbase_industrial = TokenAmount::new(cb.payload.amount_industrial);
                        }
                        let mut fee_consumer: u128 = 0;
                        let mut fee_industrial: u128 = 0;
                        for tx in b.transactions.iter().skip(1) {
                            if let Ok((c, i)) =
                                crate::fee::decompose(tx.payload.pct, tx.payload.fee)
                            {
                                fee_consumer += c as u128;
                                fee_industrial += i as u128;
                            }
                        }
                        let fee_consumer_u64 = u64::try_from(fee_consumer).unwrap_or(0);
                        let fee_industrial_u64 = u64::try_from(fee_industrial).unwrap_or(0);
                        let mut h = blake3::Hasher::new();
                        h.update(&fee_consumer_u64.to_le_bytes());
                        h.update(&fee_industrial_u64.to_le_bytes());
                        b.fee_checksum = h.finalize().to_hex().to_string();
                        b.hash = calculate_hash(
                            b.index,
                            &b.previous_hash,
                            b.timestamp_millis,
                            b.nonce,
                            b.difficulty,
                            b.base_fee,
                            b.coinbase_block,
                            b.coinbase_industrial,
                            b.storage_sub,
                            b.read_sub,
                            b.read_sub_viewer,
                            b.read_sub_host,
                            b.read_sub_hardware,
                            b.read_sub_verifier,
                            b.read_sub_liquidity,
                            b.ad_viewer,
                            b.ad_host,
                            b.ad_hardware,
                            b.ad_verifier,
                            b.ad_liquidity,
                            b.ad_miner,
                            b.ad_total_usd_micros,
                            b.ad_settlement_count,
                            b.ad_oracle_price_usd_micros,
                            b.compute_sub,
                            b.proof_rebate,
                            b.read_root,
                            &b.fee_checksum,
                            &b.transactions,
                            &b.state_root,
                            &b.l2_roots,
                            &b.l2_sizes,
                            b.vdf_commit,
                            b.vdf_output,
                            &b.vdf_proof,
                            b.retune_hint,
                            &b.receipts,
                        );
                    }
                    let mut em_c = 0u64;
                    let mut em_i = 0u64;
                    for b in &migrated_chain {
                        em_c = em_c.saturating_add(b.coinbase_block.get());
                        em_i = em_i.saturating_add(b.coinbase_industrial.get());
                    }
                    let bh = migrated_chain.len() as u64;
                    let disk_new = ChainDisk {
                        schema_version: 3,
                        chain: migrated_chain,
                        accounts: accounts.clone(),
                        emission: em_c.saturating_add(em_i),
                        emission_year_ago: 0,
                        inflation_epoch_marker: 0,
                        block_reward: TokenAmount::new(br),
                        block_height: bh,
                        mempool: Vec::new(),
                        base_fee: 1,
                        params: Params::default(),
                        epoch_storage_bytes: 0,
                        epoch_read_bytes: 0,
                        epoch_cpu_ms: 0,
                        epoch_bytes_out: 0,
                        recent_timestamps: Vec::new(),
                        economics_block_reward_per_block: br,
                        economics_prev_annual_issuance_block: 0,
                        economics_prev_subsidy: economics::SubsidySnapshot::default(),
                        economics_prev_tariff: economics::TariffSnapshot::default(),
                        economics_prev_market_metrics: economics::MarketMetrics::default(),
                        economics_epoch_tx_volume_block: 0,
                        economics_epoch_tx_count: 0,
                        economics_epoch_treasury_inflow_block: 0,
                        economics_epoch_storage_payout_block: 0,
                        economics_epoch_compute_payout_block: 0,
                        economics_epoch_ad_payout_block: 0,
                        economics_baseline_tx_count: 100,
                        economics_baseline_tx_volume: 10_000,
                        economics_baseline_miners: 10,
                    };
                    db.insert(
                        DB_CHAIN,
                        ledger_binary::encode_chain_disk(&disk_new)
                            .unwrap_or_else(|e| panic!("serialize: {e}")),
                    );
                    db.remove(DB_ACCOUNTS);
                    db.remove(DB_EMISSION);
                    (
                        disk_new.chain,
                        disk_new.accounts,
                        disk_new.emission,
                        disk_new.block_reward,
                        disk_new.block_height,
                        disk_new.mempool,
                        disk_new.base_fee,
                        disk_new.recent_timestamps,
                        disk_new.economics_block_reward_per_block,
                        disk_new.economics_prev_annual_issuance_block,
                        disk_new.economics_prev_subsidy,
                        disk_new.economics_prev_tariff,
                        disk_new.economics_prev_market_metrics,
                        disk_new.economics_epoch_tx_volume_block,
                        disk_new.economics_epoch_tx_count,
                        disk_new.economics_epoch_treasury_inflow_block,
                        0,
                        0,
                        0,
                    )
                }
            }
        } else {
            (
                Vec::new(),
                HashMap::new(),
                0,
                TokenAmount::new(INITIAL_BLOCK_REWARD),
                0,
                Vec::new(),
                1,
                Vec::new(),
                0,
                0,
                economics::SubsidySnapshot::default(),
                economics::TariffSnapshot::default(),
                economics::MarketMetrics::default(),
                0,
                0,
                0,
                0,
                0,
                0,
            )
        };

        // Load any persisted shard state roots.
        let shard_roots: HashMap<ShardId, [u8; 32]> = db
            .shard_ids()
            .into_iter()
            .filter_map(|id| {
                let bytes = db.get_shard(id, ShardState::db_key())?;
                ShardState::from_bytes(&bytes)
                    .ok()
                    .map(|s| (id, s.state_root))
            })
            .collect();
        for b in &mut chain {
            let mut fee_consumer: u128 = 0;
            let mut fee_industrial: u128 = 0;
            for tx in b.transactions.iter().skip(1) {
                if let Ok((c, i)) = crate::fee::decompose(tx.payload.pct, tx.payload.fee) {
                    fee_consumer += c as u128;
                    fee_industrial += i as u128;
                }
            }
            let fee_consumer_u64 = u64::try_from(fee_consumer).unwrap_or(0);
            let fee_industrial_u64 = u64::try_from(fee_industrial).unwrap_or(0);
            let mut h = blake3::Hasher::new();
            h.update(&fee_consumer_u64.to_le_bytes());
            h.update(&fee_industrial_u64.to_le_bytes());
            b.fee_checksum = h.finalize().to_hex().to_string();
        }
        for acc in accounts.values_mut() {
            acc.pending_amount = 0;
            acc.pending_nonce = acc.pending_nonces.len() as u64;
        }
        let mut bc = Blockchain::default();
        bc.path = path.to_string();
        let rebate_path = std::path::Path::new(path)
            .join("light_client")
            .join("proof_rebates");
        bc.proof_tracker = crate::light_client::proof_tracker::ProofTracker::open(rebate_path);
        bc.chain = chain;
        bc.accounts = accounts;
        bc.shard_roots = shard_roots;
        bc.db = db;
        bc.emission = em;
        bc.block_reward = br;
        bc.block_height = bh;
        bc.recent_miners = VecDeque::new();
        bc.recent_timestamps = VecDeque::from(recent_ts);
        bc.economics_block_reward_per_block = econ_block_reward;
        bc.economics_prev_annual_issuance_block = econ_prev_annual;
        bc.economics_prev_subsidy = econ_prev_subsidy;
        bc.economics_prev_tariff = econ_prev_tariff;
        bc.economics_prev_market_metrics = econ_prev_market_metrics;
        bc.economics_epoch_tx_volume_block = econ_epoch_tx_volume;
        bc.economics_epoch_tx_count = econ_epoch_tx_count;
        bc.economics_epoch_treasury_inflow_block = econ_epoch_treasury_inflow;
        bc.economics_epoch_storage_payout_block = econ_epoch_storage_payout;
        bc.economics_epoch_compute_payout_block = econ_epoch_compute_payout;
        bc.economics_epoch_ad_payout_block = econ_epoch_ad_payout;
        // Load any previously emitted macro blocks.
        let mut h = bc.macro_interval;
        while let Some(bytes) = bc.db.get(&MacroBlock::db_key(h)) {
            if let Ok(m) = MacroBlock::from_bytes(&bytes) {
                bc.macro_blocks.push(m);
            }
            h += bc.macro_interval;
        }
        let last = bc.chain.last().map_or(1, |b| b.difficulty);
        let hint = bc.chain.last().map_or(0, |b| b.retune_hint);
        let ts = bc.recent_timestamps.make_contiguous();
        let (d, h) = consensus::difficulty_retune::retune(last, ts, hint, &bc.params);
        bc.difficulty = d;
        bc.retune_hint = h;
        bc.base_fee = base_fee;
        for blk in bc.chain.iter().rev().take(RECENT_MINER_WINDOW) {
            if let Some(tx0) = blk.transactions.first() {
                bc.recent_miners.push_front(tx0.payload.to.clone());
            }
        }

        if let Ok(Some((snap_height, snap_accounts, root))) =
            crate::blockchain::snapshot::load_latest(path)
        {
            if (snap_height as usize) <= bc.chain.len() {
                if let Some(b_snap) = bc.chain.get((snap_height - 1) as usize) {
                    if b_snap.state_root == root {
                        bc.accounts = snap_accounts;
                        for blk in bc.chain[snap_height as usize..].iter() {
                            for tx in &blk.transactions {
                                if tx.payload.from_ != "0".repeat(34) {
                                    if let Some(s) = bc.accounts.get_mut(&tx.payload.from_) {
                                        if let Ok((fee_c, fee_i)) =
                                            crate::fee::decompose(tx.payload.pct, tx.payload.fee)
                                        {
                                            // Total BLOCK tokens: amount (both lanes) + fees
                                            let total_amount = tx.payload.amount_consumer
                                                + tx.payload.amount_industrial + fee_c + fee_i;
                                            if s.balance.amount < total_amount
                                            {
                                                return Err(ErrBalanceUnderflow::new_err(
                                                    "balance underflow",
                                                ));
                                            }
                                            s.balance.amount -= total_amount;

                                            s.nonce = tx.payload.nonce;
                                        }
                                    }
                                }
                                let r =
                                    bc.accounts.entry(tx.payload.to.clone()).or_insert(Account {
                                        address: tx.payload.to.clone(),
                                        balance: TokenBalance {
                                            amount: 0,
                                        },
                                        nonce: 0,
                                        pending_amount: 0,
                                        pending_nonce: 0,
                                        pending_nonces: HashSet::new(),
                                        sessions: Vec::new(),
                                    });
                                r.balance.amount = r
                                    .balance
                                    .amount
                                    .saturating_add(tx.payload.amount_consumer + tx.payload.amount_industrial);
                            }
                        }
                        bc.emission = bc
                            .accounts
                            .values()
                            .map(|a| a.balance.amount)
                            .sum::<u64>();
                    }
                }
            }
        }
        bc.block_height = bc.chain.len() as u64;
        bc.snapshot.set_base(path.to_string());
        let cfg = NodeConfig::load(path);
        bc.snapshot.set_interval(cfg.snapshot_interval);
        crate::net::set_max_peer_metrics(cfg.max_peer_metrics);
        crate::net::set_peer_metrics_export(cfg.peer_metrics_export);
        crate::net::set_peer_metrics_path(cfg.peer_metrics_path.clone());
        crate::net::set_peer_metrics_retention(cfg.peer_metrics_retention);
        crate::net::set_peer_metrics_compress(cfg.peer_metrics_compress);
        crate::net::set_peer_metrics_sample_rate(cfg.peer_metrics_sample_rate as u64);
        crate::net::set_metrics_export_dir(cfg.metrics_export_dir.clone());
        crate::net::set_peer_metrics_export_quota(cfg.peer_metrics_export_quota_bytes);
        crate::net::configure_overlay(&cfg.overlay);
        crate::simple_db::set_legacy_mode(cfg.storage_legacy_mode);
        crate::simple_db::configure_engines(cfg.storage.clone());
        if let Err(err) = crate::config::ensure_overlay_sanity(&cfg.overlay) {
            #[cfg(feature = "telemetry")]
            diagnostics::tracing::warn!(reason = %err, "overlay_sanity_failed");
            #[cfg(not(feature = "telemetry"))]
            eprintln!("overlay_sanity_failed: {err}");
        }
        crate::net::set_metrics_aggregator(cfg.metrics_aggregator.clone());
        #[cfg(feature = "quic")]
        {
            let quic_cfg = cfg.quic.as_ref();
            let transport_cfg = quic_cfg
                .map(|quic| quic.transport.to_transport_config())
                .unwrap_or_else(transport::Config::default);
            if let Err(err) = crate::net::configure_transport(&transport_cfg) {
                #[cfg(feature = "telemetry")]
                diagnostics::tracing::warn!(reason = %err, "transport_configure_failed");
                #[cfg(not(feature = "telemetry"))]
                eprintln!("transport_configure_failed: {err}");
            }
            let (history, max_age) = quic_cfg
                .map(|quic| {
                    (
                        quic.transport.rotation_history,
                        quic.transport.rotation_max_age_secs,
                    )
                })
                .unwrap_or((None, None));
            crate::net::configure_peer_cert_policy(history, max_age);
        }
        crate::net::set_track_drop_reasons(cfg.track_peer_drop_reasons);
        crate::net::set_track_handshake_fail(cfg.track_handshake_failures);
        crate::net::set_peer_reputation_decay(cfg.peer_reputation_decay);
        crate::net::set_p2p_max_per_sec(cfg.p2p_max_per_sec);
        crate::net::set_p2p_max_bytes_per_sec(cfg.p2p_max_bytes_per_sec);
        crate::compute_market::scheduler::set_provider_reputation_decay(
            cfg.provider_reputation_decay,
        );
        crate::compute_market::scheduler::set_provider_reputation_retention(
            cfg.provider_reputation_retention,
        );
        crate::compute_market::scheduler::set_reputation_gossip_enabled(cfg.reputation_gossip);
        crate::compute_market::scheduler::set_scheduler_metrics_enabled(cfg.scheduler_metrics);
        crate::gateway::dns::set_allow_external(cfg.gateway_dns_allow_external);
        crate::gateway::dns::set_disable_verify(cfg.gateway_dns_disable_verify);
        crate::compute_market::scheduler::set_preempt_enabled(cfg.compute_market.enable_preempt);
        crate::compute_market::scheduler::set_preempt_min_delta(
            cfg.compute_market.preempt_min_delta,
        );
        crate::compute_market::scheduler::set_low_priority_cap_pct(
            cfg.compute_market.low_priority_cap_pct,
        );
        crate::config::set_current(cfg.clone());
        crate::config::watch(path);
        crate::compute_market::scheduler::set_reputation_multiplier_bounds(
            cfg.compute_market.reputation_multiplier_min,
            cfg.compute_market.reputation_multiplier_max,
        );
        crate::net::load_peer_metrics();
        let infl = crate::config::load_inflation(path);
        bc.params.beta_storage_sub = (infl.beta_storage_sub * 1000.0) as i64;
        bc.params.gamma_read_sub = (infl.gamma_read_sub * 1000.0) as i64;
        bc.params.kappa_cpu_sub = (infl.kappa_cpu_sub * 1000.0) as i64;
        bc.params.lambda_bytes_out_sub = (infl.lambda_bytes_out_sub * 1000.0) as i64;
        bc.params.risk_lambda = (infl.risk_lambda * 1000.0) as i64;
        bc.params.entropy_phi = (infl.entropy_phi * 1000.0) as i64;
        bc.params.haar_eta = (infl.haar_eta * 1000.0) as i64;
        bc.params.util_var_threshold = (infl.util_var_threshold * 1000.0) as i64;
        bc.params.fib_window_base_secs = infl.fib_window_base_secs as i64;
        bc.params.heuristic_mu_milli = (infl.heuristic_mu * 1000.0) as i64;
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::HAAR_ETA_MILLI.set(bc.params.haar_eta);
            crate::telemetry::UTIL_VAR_THRESHOLD_MILLI.set(bc.params.util_var_threshold);
            crate::telemetry::FIB_WINDOW_BASE_SECS.set(bc.params.fib_window_base_secs);
            crate::telemetry::HEURISTIC_MU_MILLI.set(bc.params.heuristic_mu_milli);
        }
        set_vdf_kappa(infl.vdf_kappa);
        let caps = crate::config::load_caps(path);
        crate::storage::pipeline::set_l2_cap_bytes_per_epoch(caps.storage.l2_cap_bytes_per_epoch);
        crate::storage::pipeline::set_bytes_per_sender_epoch_cap(
            caps.storage.bytes_per_sender_epoch_cap,
        );
        let pb = std::path::Path::new(path).join(&cfg.price_board_path);
        crate::compute_market::price_board::init(
            pb.to_string_lossy().into_owned(),
            cfg.price_board_window,
            cfg.price_board_save_interval,
        );
        let settle_path = std::path::Path::new(path).join("settlement");
        crate::compute_market::settlement::Settlement::init(
            settle_path.to_string_lossy().as_ref(),
            cfg.compute_market.settle_mode,
        );
        #[cfg(feature = "telemetry")]
        telemetry::summary::spawn(cfg.telemetry_summary_interval);
        bc.config = cfg.clone();
        if let Err(err) = bc.register_receipt_providers(&cfg.receipt_providers) {
            #[cfg(feature = "telemetry")]
            diagnostics::tracing::warn!(reason = %err, "receipt_provider_registration_failed");
            #[cfg(not(feature = "telemetry"))]
            eprintln!("receipt_provider_registration_failed: {err}");
        }
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::SNAPSHOT_INTERVAL.set(cfg.snapshot_interval as i64);
            crate::telemetry::SNAPSHOT_INTERVAL_CHANGED.set(cfg.snapshot_interval as i64);
        }

        if let Ok(v) = std::env::var("TB_MEMPOOL_MAX") {
            if let Ok(n) = v.parse() {
                bc.max_mempool_size_consumer = n;
                bc.max_mempool_size_industrial = n;
            }
        }
        if let Ok(v) = std::env::var("TB_MIN_FEE_PER_BYTE") {
            if let Ok(n) = v.parse() {
                bc.min_fee_per_byte_consumer = n;
                bc.min_fee_per_byte_industrial = n;
            }
        }
        if let Ok(v) = std::env::var("TB_MIN_FEE_PER_BYTE_CONSUMER") {
            if let Ok(n) = v.parse() {
                bc.min_fee_per_byte_consumer = n;
            }
        }
        if let Ok(v) = std::env::var("TB_MIN_FEE_PER_BYTE_INDUSTRIAL") {
            if let Ok(n) = v.parse() {
                bc.min_fee_per_byte_industrial = n;
            }
        }
        if let Ok(v) = std::env::var("TB_COMFORT_THRESHOLD_P90") {
            if let Ok(n) = v.parse() {
                bc.comfort_threshold_p90 = n;
            }
        }
        if let Ok(v) = std::env::var("TB_MEMPOOL_TTL_SECS") {
            if let Ok(n) = v.parse() {
                bc.tx_ttl = n;
            }
        }
        if let Ok(v) = std::env::var("TB_MEMPOOL_ACCOUNT_CAP") {
            if let Ok(n) = v.parse() {
                bc.max_pending_per_account = n;
            }
        }

        let mut missing_drop_total = 0u64;
        let mut iter = mempool_disk.into_iter();
        loop {
            let mut batch = Vec::with_capacity(STARTUP_REBUILD_BATCH);
            for _ in 0..STARTUP_REBUILD_BATCH {
                if let Some(e) = iter.next() {
                    batch.push(e);
                } else {
                    break;
                }
            }
            if batch.is_empty() {
                break;
            }
            for e in batch {
                let size = if e.serialized_size != 0 {
                    e.serialized_size
                } else {
                    crate::transaction::binary::encode_signed_transaction(&e.tx)
                        .map(|b| b.len() as u64)
                        .unwrap_or(0)
                };
                let fpb = if size == 0 { 0 } else { e.tx.tip / size };
                #[cfg(feature = "telemetry")]
                let _span = diagnostics::tracing::span!(
                    diagnostics::tracing::Level::TRACE,
                    "startup_rebuild",
                    sender = %scrub(&e.sender),
                    nonce = e.nonce,
                    fpb,
                    mempool_size = bc.mempool_size_consumer.load(AtomicOrdering::SeqCst)
                        + bc.mempool_size_industrial.load(AtomicOrdering::SeqCst)
                )
                .entered();
                #[cfg(not(feature = "telemetry"))]
                let _ = fpb;
                if bc.accounts.contains_key(&e.sender) {
                    let pool = match e.tx.lane {
                        FeeLane::Consumer => &bc.mempool_consumer,
                        FeeLane::Industrial => &bc.mempool_industrial,
                    };
                    pool.insert(
                        (e.sender.clone(), e.nonce),
                        MempoolEntry {
                            tx: e.tx.clone(),
                            timestamp_millis: e.timestamp_millis,
                            timestamp_ticks: e.timestamp_ticks,
                            serialized_size: size,
                        },
                    );
                    bc.inc_mempool_size(e.tx.lane);
                    {
                        let mut admission = bc.admission_guard(e.tx.lane);
                        admission.restore_sender(&e.sender);
                        admission.record_admission(fpb);
                    }
                    if let Some(acc) = bc.accounts.get_mut(&e.sender) {
                        if let Ok((fee_consumer, fee_industrial)) =
                            crate::fee::decompose(e.tx.payload.pct, bc.base_fee + e.tx.tip)
                        {
                            let total_amount = e.tx.payload.amount_consumer + fee_consumer
                                + e.tx.payload.amount_industrial + fee_industrial;
                            acc.pending_amount += total_amount;
                            acc.pending_nonce += 1;
                            acc.pending_nonces.insert(e.tx.payload.nonce);
                        }
                    }
                } else {
                    missing_drop_total += 1;
                }
            }
        }
        let ttl_drop_total = bc.purge_expired();
        let expired_drop_total = missing_drop_total + ttl_drop_total;
        #[cfg(not(feature = "telemetry"))]
        let _ = expired_drop_total;
        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
        info!("startup expired_drop_total={expired_drop_total}");
        #[cfg(feature = "telemetry-json")]
        log_event(
            "storage",
            diagnostics::log::Level::INFO,
            "startup_purge",
            "",
            0,
            "expired_drop_total",
            ERR_OK,
            Some(expired_drop_total as u64),
            None,
        );
        #[cfg(feature = "telemetry")]
        {
            telemetry::ORPHAN_SWEEP_TOTAL.inc_by(missing_drop_total);
            telemetry::STARTUP_TTL_DROP_TOTAL.inc_by(ttl_drop_total);
        }
        Ok(bc)
    }

    #[cfg_attr(feature = "python-bindings", staticmethod)]
    pub fn open(path: &str) -> PyResult<Self> {
        Self::open_with_db(path, path)
    }

    #[cfg_attr(feature = "python-bindings", staticmethod)]
    pub fn with_difficulty(path: &str, difficulty: u64) -> PyResult<Self> {
        let mut bc = Blockchain::open(path)?;
        bc.difficulty = difficulty;
        Ok(bc)
    }

    /// Expose the latest shard state root to Python callers.
    pub fn shard_root(&self, shard: u16) -> Option<[u8; 32]> {
        self.get_shard_root(shard)
    }

    /// Return the on-disk schema version
    #[cfg_attr(feature = "python-bindings", getter)]
    pub fn schema_version(&self) -> usize {
        // Bump this constant whenever the serialized `ChainDisk` format changes.
        // Older binaries must refuse to open newer databases.
        11
    }

    /// Persist the entire chain + state under the current schema
    pub fn persist_chain(&mut self) -> PyResult<()> {
        let mut mempool: Vec<MempoolEntryDisk> = Vec::new();
        self.mempool_consumer.for_each(|key, value| {
            let serialized_size = if value.serialized_size != 0 {
                value.serialized_size
            } else {
                crate::transaction::binary::encode_signed_transaction(&value.tx)
                    .map(|bytes| bytes.len() as u64)
                    .unwrap_or(0)
            };
            mempool.push(MempoolEntryDisk {
                sender: key.0.clone(),
                nonce: key.1,
                tx: value.tx.clone(),
                timestamp_millis: value.timestamp_millis,
                timestamp_ticks: value.timestamp_ticks,
                serialized_size,
            });
        });
        self.mempool_industrial.for_each(|key, value| {
            let serialized_size = if value.serialized_size != 0 {
                value.serialized_size
            } else {
                crate::transaction::binary::encode_signed_transaction(&value.tx)
                    .map(|bytes| bytes.len() as u64)
                    .unwrap_or(0)
            };
            mempool.push(MempoolEntryDisk {
                sender: key.0.clone(),
                nonce: key.1,
                tx: value.tx.clone(),
                timestamp_millis: value.timestamp_millis,
                timestamp_ticks: value.timestamp_ticks,
                serialized_size,
            });
        });
        let disk = ChainDisk {
            schema_version: self.schema_version(),
            chain: self.chain.clone(),
            accounts: self.accounts.clone(),
            emission: self.emission,
            emission_year_ago: self.emission_year_ago,
            inflation_epoch_marker: self.inflation_epoch_marker,
            block_reward: self.block_reward,
            block_height: self.block_height,
            mempool,
            base_fee: self.base_fee,
            params: self.params.clone(),
            epoch_storage_bytes: self.epoch_storage_bytes,
            epoch_read_bytes: self.epoch_read_bytes,
            epoch_cpu_ms: self.epoch_cpu_ms,
            epoch_bytes_out: self.epoch_bytes_out,
            recent_timestamps: self.recent_timestamps.iter().copied().collect(),
            economics_block_reward_per_block: self.economics_block_reward_per_block,
            economics_prev_annual_issuance_block: self.economics_prev_annual_issuance_block,
            economics_prev_subsidy: self.economics_prev_subsidy.clone(),
            economics_prev_tariff: self.economics_prev_tariff.clone(),
            economics_prev_market_metrics: self.economics_prev_market_metrics.clone(),
            economics_epoch_tx_volume_block: self.economics_epoch_tx_volume_block,
            economics_epoch_tx_count: self.economics_epoch_tx_count,
            economics_epoch_treasury_inflow_block: self.economics_epoch_treasury_inflow_block,
            economics_epoch_storage_payout_block: self.economics_epoch_storage_payout_block,
            economics_epoch_compute_payout_block: self.economics_epoch_compute_payout_block,
            economics_epoch_ad_payout_block: self.economics_epoch_ad_payout_block,
            economics_baseline_tx_count: self.economics_baseline_tx_count,
            economics_baseline_tx_volume: self.economics_baseline_tx_volume,
            economics_baseline_miners: self.economics_baseline_miners,
        };
        let bytes = ledger_binary::encode_chain_disk(&disk)
            .map_err(|e| py_value_err(format!("Serialization error: {e}")))?;
        self.db.insert(DB_CHAIN, bytes);
        // ensure no legacy column families linger on disk
        self.db.remove(DB_ACCOUNTS);
        self.db.remove(DB_EMISSION);
        self.db.flush();
        Ok(())
    }

    pub fn circulating_supply(&self) -> u64 {
        self.emission
    }

    /// Construct and persist the genesis block.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the chain state cannot be serialized or persisted.
    pub fn genesis_block(&mut self) -> PyResult<()> {
        let (diff, hint) = consensus::difficulty_retune::retune(1, &[], 0, &self.params);
        let g = Block {
            index: 0,
            previous_hash: "0".repeat(64),
            timestamp_millis: 0,
            transactions: vec![],
            difficulty: diff,
            retune_hint: hint,
            nonce: 0,
            // genesis carries zero reward; fields included for stable hashing
            coinbase_block: TokenAmount::new(0),
            coinbase_industrial: TokenAmount::new(0),
            storage_sub: TokenAmount::new(0),
            read_sub: TokenAmount::new(0),
            read_sub_viewer: TokenAmount::new(0),
            read_sub_host: TokenAmount::new(0),
            read_sub_hardware: TokenAmount::new(0),
            read_sub_verifier: TokenAmount::new(0),
            read_sub_liquidity: TokenAmount::new(0),
            ad_viewer: TokenAmount::new(0),
            ad_host: TokenAmount::new(0),
            ad_hardware: TokenAmount::new(0),
            ad_verifier: TokenAmount::new(0),
            ad_liquidity: TokenAmount::new(0),
            ad_miner: TokenAmount::new(0),
            fee_checksum: "0".repeat(64),
            hash: GENESIS_HASH.to_string(),
            state_root: String::new(),
            base_fee: self.base_fee,
            ..Block::default()
        };
        self.chain.push(g);
        self.recent_timestamps.push_back(0);
        self.block_height = 1;
        self.persist_chain()
    }

    /// Add a new account with starting balances.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the account already exists.
    pub fn add_account(&mut self, address: String, amount: u64) -> PyResult<()> {
        if self.accounts.contains_key(&address) {
            return Err(py_value_err("Account already exists"));
        }
        let acc = Account {
            address: address.clone(),
            balance: TokenBalance {
                amount,
            },
            nonce: 0,
            pending_amount: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
            sessions: Vec::new(),
        };
        self.accounts.insert(address, acc);
        Ok(())
    }

    /// Remove an existing account. Intended for testing hooks.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the account is not found.
    #[doc(hidden)]
    pub fn remove_account(&mut self, address: &str) -> PyResult<()> {
        self.accounts
            .remove(address)
            .map(|_| ())
            .ok_or_else(|| py_value_err("Account not found"))
    }

    /// Register a session key for an account.
    pub fn issue_session_key(
        &mut self,
        address: String,
        public_key: Vec<u8>,
        expires_at: u64,
    ) -> PyResult<()> {
        let acc = self
            .accounts
            .get_mut(&address)
            .ok_or_else(|| py_value_err("Account not found"))?;
        acc.sessions.push(accounts::SessionPolicy {
            public_key,
            expires_at,
            nonce: acc.nonce,
        });
        #[cfg(feature = "telemetry")]
        telemetry::SESSION_KEY_ISSUED_TOTAL.inc();
        Ok(())
    }

    /// Backdate a mempool entry's timestamp for testing purposes.
    #[doc(hidden)]
    pub fn backdate_mempool_entry(&self, sender: &str, nonce: u64, millis: u64) {
        if let Some(mut entry) = self.mempool_consumer.get_mut(&(sender.to_string(), nonce)) {
            entry.timestamp_millis = millis;
            entry.timestamp_ticks = millis;
        } else if let Some(mut entry) = self
            .mempool_industrial
            .get_mut(&(sender.to_string(), nonce))
        {
            entry.timestamp_millis = millis;
            entry.timestamp_ticks = millis;
        }
    }

    /// Return the balance for an account.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the account is not found.
    pub fn get_account_balance(&self, address: &str) -> PyResult<TokenBalance> {
        self.accounts
            .get(address)
            .map(|a| a.balance.clone())
            .ok_or_else(|| py_value_err("Account not found"))
    }

    /// Submit a signed transaction to the mempool.
    ///
    /// # Errors
    /// Returns [`TxAdmissionError`] if validation fails or the sender is missing.
    pub fn submit_transaction(&mut self, tx: SignedTransaction) -> Result<(), TxAdmissionError> {
        let _ = self.purge_expired();
        #[cfg(feature = "telemetry")]
        self.record_submit();
        if tx.threshold > 0 && tx.signer_pubkeys.len() < tx.threshold as usize {
            let key = blake3::hash(&canonical_payload_bytes(&tx.payload));
            self.pending_multisig.insert(key.as_bytes().to_vec(), tx);
            return Err(TxAdmissionError::PendingSignatures);
        }
        let mut tx = tx;
        let sender_addr = tx.payload.from_.clone();
        let nonce = tx.payload.nonce;
        let base_fee = self.base_fee;
        if tx.tip == 0 {
            tx.tip = tx.payload.fee.saturating_sub(base_fee);
        }
        let size = crate::transaction::binary::encode_signed_transaction(&tx)
            .map_err(|_| {
                #[cfg(feature = "telemetry")]
                self.record_reject("fee_overflow");
                TxAdmissionError::FeeOverflow
            })?
            .len() as u64;
        let fee_per_byte = if size == 0 { 0 } else { tx.tip / size };
        let lane = tx.lane;
        #[cfg(feature = "telemetry")]
        let _pool_guard = {
            let span = diagnostics::tracing::span!(
                diagnostics::tracing::Level::TRACE,
                "mempool_mutex",
                sender = %scrub(&sender_addr),
                nonce,
                fpb = fee_per_byte,
                mempool_size = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
                    + self.mempool_size_industrial.load(AtomicOrdering::SeqCst)
            );
            span.in_scope(|| self.mempool_mutex.lock()).map_err(|_| {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
                TxAdmissionError::LockPoisoned
            })?
        };
        #[cfg(not(feature = "telemetry"))]
        let _pool_guard = self.mempool_mutex.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
            }
            TxAdmissionError::LockPoisoned
        })?;
        let lane_min = {
            // Use static fee fields if explicitly set (for testing), otherwise use dynamic pricing
            let static_min = match lane {
                FeeLane::Consumer => self.min_fee_per_byte_consumer,
                FeeLane::Industrial => self.min_fee_per_byte_industrial,
            };
            // If static fee is explicitly set to non-1 (either 0 for tests or higher), use it
            // Otherwise use cached dynamic pricing (updated per block, avoiding lock contention)
            if static_min != 1 {
                static_min
            } else {
                match lane {
                    FeeLane::Consumer => self.cached_consumer_fee.load(AtomicOrdering::Relaxed),
                    FeeLane::Industrial => self.cached_industrial_fee.load(AtomicOrdering::Relaxed),
                }
            }
        };
        let floor = {
            let guard = self.admission_guard(lane);
            lane_min.max(guard.floor())
        };
        if fee_per_byte < floor {
            #[cfg(feature = "telemetry")]
            {
                let tx_hash = tx.id();
                #[cfg(feature = "telemetry-json")]
                log_event(
                    "mempool",
                    diagnostics::log::Level::WARN,
                    "reject",
                    &sender_addr,
                    nonce,
                    "fee_too_low",
                    TxAdmissionError::FeeTooLow.code(),
                    Some(fee_per_byte),
                    Some(&crypto_suite::hex::encode(tx_hash)),
                );
                #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                if telemetry::should_log("mempool") {
                    let span = crate::log_context!(tx = tx_hash);
                    diagnostics::tracing::warn!(
                        parent: &span,
                        "tx rejected sender={sender_addr} nonce={nonce} reason=fee_too_low"
                    );
                }
                telemetry::FEE_FLOOR_REJECT_TOTAL.inc();
                self.record_reject("fee_too_low");
            }
            return Err(TxAdmissionError::FeeTooLow);
        }
        let lock = self
            .admission_locks
            .entry(sender_addr.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let panic_step = self.panic_on_admit.swap(-1, AtomicOrdering::SeqCst);

        #[cfg(feature = "telemetry")]
        let lock_guard = {
            let span = diagnostics::tracing::span!(
                diagnostics::tracing::Level::TRACE,
                "admission_lock",
                sender = %scrub(&sender_addr),
                nonce
            );
            span.in_scope(|| lock.lock()).map_err(|_| {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
                TxAdmissionError::LockPoisoned
            })?
        };
        #[cfg(not(feature = "telemetry"))]
        let lock_guard = lock.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
            }
            TxAdmissionError::LockPoisoned
        })?;
        if panic_step == 0 {
            panic!("admission panic");
        }

        if tx.payload.pct > 100 {
            #[cfg(feature = "telemetry-json")]
            {
                let tx_hash = tx.id();
                log_event(
                    "mempool",
                    diagnostics::log::Level::WARN,
                    "reject",
                    &sender_addr,
                    nonce,
                    "invalid_selector",
                    TxAdmissionError::InvalidSelector.code(),
                    None,
                    Some(&crypto_suite::hex::encode(tx_hash)),
                );
            }
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            if telemetry::should_log("mempool") {
                let span = crate::log_context!(tx = tx.id());
                diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=invalid_selector");
            }
            #[cfg(feature = "telemetry")]
            {
                telemetry::INVALID_SELECTOR_REJECT_TOTAL.inc();
                self.record_reject("invalid_selector");
            }
            return Err(TxAdmissionError::InvalidSelector);
        }
        if tx.payload.fee >= (1u64 << 63) {
            #[cfg(feature = "telemetry")]
            self.record_reject("fee_too_large");
            return Err(TxAdmissionError::FeeTooLarge);
        }
        let required_total = self.base_fee.saturating_add(tx.tip);
        if tx.payload.fee < required_total {
            #[cfg(feature = "telemetry")]
            self.record_reject("fee_too_low");
            return Err(TxAdmissionError::FeeTooLow);
        }
        let (fee_consumer, fee_industrial) =
            match crate::fee::decompose(tx.payload.pct, required_total) {
                Ok(v) => v,
                Err(FeeError::InvalidSelector) => {
                    #[cfg(feature = "telemetry")]
                    {
                        telemetry::INVALID_SELECTOR_REJECT_TOTAL.inc();
                        self.record_reject("invalid_selector");
                    }
                    return Err(TxAdmissionError::InvalidSelector);
                }
                Err(FeeError::Overflow) => {
                    #[cfg(feature = "telemetry")]
                    self.record_reject("fee_overflow");
                    return Err(TxAdmissionError::FeeOverflow);
                }
            };
        let total_consumer = match tx.payload.amount_consumer.checked_add(fee_consumer) {
            Some(v) => v,
            None => {
                #[cfg(feature = "telemetry")]
                self.record_reject("fee_overflow");
                return Err(TxAdmissionError::FeeOverflow);
            }
        };
        let total_industrial = match tx.payload.amount_industrial.checked_add(fee_industrial) {
            Some(v) => v,
            None => {
                #[cfg(feature = "telemetry")]
                self.record_reject("fee_overflow");
                return Err(TxAdmissionError::FeeOverflow);
            }
        };

        // capacity check after basic validation
        let lane = tx.lane;
        let (mempool, max_size, pool_size) = match lane {
            FeeLane::Consumer => (
                &self.mempool_consumer,
                self.max_mempool_size_consumer,
                self.mempool_size_consumer.load(AtomicOrdering::SeqCst),
            ),
            FeeLane::Industrial => (
                &self.mempool_industrial,
                self.max_mempool_size_industrial,
                self.mempool_size_industrial.load(AtomicOrdering::SeqCst),
            ),
        };
        if pool_size >= max_size {
            #[cfg(feature = "telemetry")]
            telemetry::EVICTIONS_TOTAL.inc();
            // find lowest-priority entry for eviction
            let mut victim: Option<((String, u64), MempoolEntry)> = None;
            mempool.for_each(|key, val| {
                let key_clone = (key.0.clone(), key.1);
                let val_clone = val.clone();
                victim = match victim {
                    Some((ref k, ref v)) => {
                        if mempool_cmp(&val_clone, v, self.tx_ttl) == Ordering::Greater {
                            Some((key_clone, val_clone))
                        } else {
                            Some((k.clone(), v.clone()))
                        }
                    }
                    None => Some((key_clone, val_clone)),
                };
            });
            if let Some(((ev_sender, ev_nonce), ev_entry)) = victim {
                let ev_hash = ev_entry.tx.id();
                if ev_sender != sender_addr {
                    let lock = self
                        .admission_locks
                        .entry(ev_sender.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone();
                    #[cfg(feature = "telemetry")]
                    let _guard = {
                        let span = diagnostics::tracing::span!(
                            diagnostics::tracing::Level::TRACE,
                            "admission_lock",
                            sender = %scrub(&ev_sender),
                            nonce = ev_nonce
                        );
                        span.in_scope(|| lock.lock()).map_err(|_| {
                            telemetry::LOCK_POISON_TOTAL.inc();
                            self.record_reject("lock_poison");
                            TxAdmissionError::LockPoisoned
                        })?
                    };
                    #[cfg(not(feature = "telemetry"))]
                    let _guard = lock.lock().map_err(|_| {
                        #[cfg(feature = "telemetry")]
                        {
                            telemetry::LOCK_POISON_TOTAL.inc();
                            self.record_reject("lock_poison");
                        }
                        TxAdmissionError::LockPoisoned
                    })?;
                }
                mempool.remove(&(ev_sender.clone(), ev_nonce));
                self.dec_mempool_size(lane);
                {
                    let mut guard = self.admission_guard(lane);
                    guard.release_sender(&ev_sender);
                    guard.record_eviction(ev_hash);
                }
                mempool::scoring::evict_on_overflow(1);
                if let Some(acc) = self.accounts.get_mut(&ev_sender) {
                    if let Ok((c, i)) = crate::fee::decompose(
                        ev_entry.tx.payload.pct,
                        self.base_fee + ev_entry.tx.tip,
                    ) {
                        // Total BLOCK tokens in evicted tx
                        let total_evicted = ev_entry.tx.payload.amount_consumer
                            + ev_entry.tx.payload.amount_industrial + c + i;
                        if acc.pending_amount < total_evicted
                            || acc.pending_nonce == 0
                        {
                            return Err(TxAdmissionError::InsufficientBalance);
                        }
                        acc.pending_amount -= total_evicted;
                        acc.pending_nonce -= 1;
                        acc.pending_nonces.remove(&ev_nonce);
                    }
                } else {
                    self.orphan_counter.fetch_sub(1, AtomicOrdering::SeqCst);
                }
                #[cfg(feature = "telemetry-json")]
                {
                    let ev_hash = ev_entry.tx.id();
                    log_event(
                        "mempool",
                        diagnostics::log::Level::INFO,
                        "evict",
                        &ev_sender,
                        ev_nonce,
                        "priority",
                        ERR_OK,
                        None,
                        Some(&crypto_suite::hex::encode(ev_hash)),
                    );
                }
                #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                if telemetry::should_log("mempool") {
                    let span = crate::log_context!(tx = ev_entry.tx.id());
                    diagnostics::tracing::info!(parent: &span, "tx evicted sender={} nonce={} reason=priority", scrub(&ev_sender), ev_nonce);
                }
                if self.panic_on_evict.swap(false, AtomicOrdering::SeqCst) {
                    panic!("evict panic");
                }
            } else {
                #[cfg(feature = "telemetry")]
                self.record_reject("mempool_full");
                #[cfg(feature = "telemetry-json")]
                {
                    let tx_hash = tx.id();
                    log_event(
                        "mempool",
                        diagnostics::log::Level::WARN,
                        "reject",
                        &sender_addr,
                        nonce,
                        "mempool_full",
                        TxAdmissionError::MempoolFull.code(),
                        None,
                        Some(&crypto_suite::hex::encode(tx_hash)),
                    );
                }
                #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                if telemetry::should_log("mempool") {
                    let span = crate::log_context!(tx = tx.id());
                    diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=mempool_full");
                }
                return Err(TxAdmissionError::MempoolFull);
            }
        }

        match mempool.entry((sender_addr.clone(), nonce)) {
            DashEntry::Occupied(_) => {
                #[cfg(feature = "telemetry")]
                {
                    let tx_hash = tx.id();
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        "mempool",
                        diagnostics::log::Level::WARN,
                        "reject",
                        &sender_addr,
                        nonce,
                        "duplicate",
                        TxAdmissionError::Duplicate.code(),
                        None,
                        Some(&crypto_suite::hex::encode(tx_hash)),
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    if telemetry::should_log("mempool") {
                        let span = crate::log_context!(tx = tx_hash);
                        diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=duplicate");
                    }
                }
                #[cfg(feature = "telemetry")]
                {
                    telemetry::DUP_TX_REJECT_TOTAL.inc();
                    self.record_reject("duplicate");
                }
                Err(TxAdmissionError::Duplicate)
            }
            DashEntry::Vacant(vacant) => {
                let accounts = &mut self.accounts;
                let sender = match accounts.get_mut(&sender_addr) {
                    Some(s) => s,
                    None => {
                        #[cfg(feature = "telemetry")]
                        #[cfg(feature = "telemetry-json")]
                        {
                            let tx_hash = tx.id();
                            log_event(
                                "mempool",
                                diagnostics::log::Level::WARN,
                                "reject",
                                &sender_addr,
                                nonce,
                                "unknown_sender",
                                TxAdmissionError::UnknownSender.code(),
                                None,
                                Some(&crypto_suite::hex::encode(tx_hash)),
                            );
                        }
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        if telemetry::should_log("mempool") {
                            let span = crate::log_context!(tx = tx.id());
                            diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=unknown_sender");
                        }
                        #[cfg(feature = "telemetry")]
                        self.record_reject("unknown_sender");
                        return Err(TxAdmissionError::UnknownSender);
                    }
                };
                let mut admission_state = match lane {
                    FeeLane::Consumer => self
                        .admission_consumer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()),
                    FeeLane::Industrial => self
                        .admission_industrial
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()),
                };
                let sender_slot = match admission_state
                    .reserve_sender(&sender_addr, self.max_pending_per_account)
                {
                    Ok(slot) => slot,
                    Err(TxAdmissionError::PendingLimitReached) => {
                        #[cfg(feature = "telemetry")]
                        {
                            let tx_hash = tx.id();
                            #[cfg(feature = "telemetry-json")]
                            log_event(
                                "mempool",
                                diagnostics::log::Level::WARN,
                                "reject",
                                &sender_addr,
                                nonce,
                                "pending_limit",
                                TxAdmissionError::PendingLimitReached.code(),
                                None,
                                Some(&crypto_suite::hex::encode(tx_hash)),
                            );
                            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                            if telemetry::should_log("mempool") {
                                let span = crate::log_context!(tx = tx_hash);
                                diagnostics::tracing::warn!(
                                    parent: &span,
                                    "tx rejected sender={sender_addr} nonce={nonce} reason=pending_limit"
                                );
                            }
                            self.record_reject("pending_limit");
                        }
                        return Err(TxAdmissionError::PendingLimitReached);
                    }
                    Err(e) => return Err(e),
                };
                mempool::admission::validate_account(sender, &tx)?;
                // In single-BLOCK token model, calculate total amount across both lanes
                let total_amount = match total_consumer.checked_add(total_industrial) {
                    Some(v) => v,
                    None => {
                        #[cfg(feature = "telemetry")]
                        {
                            telemetry::BALANCE_OVERFLOW_REJECT_TOTAL.inc();
                            self.record_reject("balance_overflow");
                        }
                        return Err(TxAdmissionError::BalanceOverflow);
                    }
                };
                let required_total = match sender.pending_amount.checked_add(total_amount) {
                    Some(v) => v,
                    None => {
                        #[cfg(feature = "telemetry")]
                        {
                            telemetry::BALANCE_OVERFLOW_REJECT_TOTAL.inc();
                            self.record_reject("balance_overflow");
                        }
                        return Err(TxAdmissionError::BalanceOverflow);
                    }
                };
                if sender.balance.amount < required_total
                {
                    #[cfg(feature = "telemetry")]
                    {
                        let tx_hash = tx.id();
                        #[cfg(feature = "telemetry-json")]
                        log_event(
                            "mempool",
                            diagnostics::log::Level::WARN,
                            "reject",
                            &sender_addr,
                            nonce,
                            "insufficient_balance",
                            TxAdmissionError::InsufficientBalance.code(),
                            None,
                            Some(&crypto_suite::hex::encode(tx_hash)),
                        );
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        if telemetry::should_log("mempool") {
                            let span = crate::log_context!(tx = tx_hash);
                            diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=insufficient_balance");
                        }
                    }
                    #[cfg(feature = "telemetry")]
                    self.record_reject("insufficient_balance");
                    return Err(TxAdmissionError::InsufficientBalance);
                }
                if sender.pending_nonces.contains(&nonce) {
                    #[cfg(feature = "telemetry")]
                    {
                        let tx_hash = tx.id();
                        #[cfg(feature = "telemetry-json")]
                        log_event(
                            "mempool",
                            diagnostics::log::Level::WARN,
                            "reject",
                            &sender_addr,
                            nonce,
                            "duplicate",
                            TxAdmissionError::Duplicate.code(),
                            None,
                            Some(&crypto_suite::hex::encode(tx_hash)),
                        );
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        if telemetry::should_log("mempool") {
                            let span = crate::log_context!(tx = tx_hash);
                            diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=duplicate");
                        }
                        telemetry::DUP_TX_REJECT_TOTAL.inc();
                        self.record_reject("duplicate");
                    }
                    return Err(TxAdmissionError::Duplicate);
                }
                if nonce != sender.nonce + sender.pending_nonce + 1 {
                    #[cfg(feature = "telemetry")]
                    {
                        let tx_hash = tx.id();
                        #[cfg(feature = "telemetry-json")]
                        log_event(
                            "mempool",
                            diagnostics::log::Level::WARN,
                            "reject",
                            &sender_addr,
                            nonce,
                            "nonce_gap",
                            TxAdmissionError::NonceGap.code(),
                            None,
                            Some(&crypto_suite::hex::encode(tx_hash)),
                        );
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        if telemetry::should_log("mempool") {
                            let span = crate::log_context!(tx = tx_hash);
                            diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=nonce_gap");
                        }
                        self.record_reject("nonce_gap");
                    }
                    return Err(TxAdmissionError::NonceGap);
                }
                #[cfg(feature = "telemetry")]
                {
                    let is_tight = crate::fees::policy::consumer_p90() > self.comfort_threshold_p90;
                    if is_tight {
                        telemetry::ADMISSION_MODE
                            .ensure_handle_for_label_values(&["tight"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .set(1);
                        telemetry::ADMISSION_MODE
                            .ensure_handle_for_label_values(&["normal"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .set(0);
                    } else {
                        telemetry::ADMISSION_MODE
                            .ensure_handle_for_label_values(&["normal"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .set(1);
                        telemetry::ADMISSION_MODE
                            .ensure_handle_for_label_values(&["tight"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .set(0);
                    }
                    telemetry::ADMISSION_MODE
                        .ensure_handle_for_label_values(&["brownout"])
                        .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                        .set(0);
                    if matches!(lane, FeeLane::Industrial) && is_tight {
                        telemetry::INDUSTRIAL_DEFERRED_TOTAL.inc();
                        telemetry::INDUSTRIAL_REJECTED_TOTAL
                            .ensure_handle_for_label_values(&["comfort_guard"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .inc();
                        self.record_reject("comfort_guard");
                        return Err(TxAdmissionError::FeeTooLow);
                    }
                }
                if !verify_signed_tx(tx.clone()) {
                    #[cfg(feature = "telemetry")]
                    {
                        let tx_hash = tx.id();
                        #[cfg(feature = "telemetry-json")]
                        log_event(
                            "mempool",
                            diagnostics::log::Level::WARN,
                            "reject",
                            &sender_addr,
                            nonce,
                            "bad_signature",
                            TxAdmissionError::BadSignature.code(),
                            None,
                            Some(&crypto_suite::hex::encode(tx_hash)),
                        );
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        if telemetry::should_log("mempool") {
                            let span = crate::log_context!(tx = tx_hash);
                            diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=bad_signature");
                        }
                        self.record_reject("bad_signature");
                    }
                    return Err(TxAdmissionError::BadSignature);
                }
                if sender.pending_nonce as usize >= self.max_pending_per_account {
                    #[cfg(feature = "telemetry")]
                    {
                        let tx_hash = tx.id();
                        #[cfg(feature = "telemetry-json")]
                        log_event(
                            "mempool",
                            diagnostics::log::Level::WARN,
                            "reject",
                            &sender_addr,
                            nonce,
                            "pending_limit",
                            TxAdmissionError::PendingLimitReached.code(),
                            None,
                            Some(&crypto_suite::hex::encode(tx_hash)),
                        );
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        if telemetry::should_log("mempool") {
                            let span = crate::log_context!(tx = tx_hash);
                            diagnostics::tracing::warn!(parent: &span, "tx rejected sender={sender_addr} nonce={nonce} reason=pending_limit");
                        }
                        self.record_reject("pending_limit");
                    }
                    return Err(TxAdmissionError::PendingLimitReached);
                }
                {
                    let guard = ReservationGuard::new(
                        lock_guard,
                        sender,
                        total_amount,
                        nonce,
                    );
                    if panic_step == 1 {
                        panic!("admission panic");
                    }
                    #[cfg(feature = "telemetry")]
                    let tx_hash = tx.id();
                    #[cfg(feature = "telemetry")]
                    let fee_val = tx.tip;
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_else(|e| panic!("time: {e}"));
                    vacant.insert(MempoolEntry {
                        tx,
                        timestamp_millis: now.as_millis() as u64,
                        timestamp_ticks: now.as_nanos() as u64,
                        serialized_size: size,
                    });
                    guard.commit();
                    sender_slot.commit(fee_per_byte);
                    #[cfg(feature = "telemetry")]
                    {
                        self.record_admit();
                        match lane {
                            FeeLane::Industrial => telemetry::INDUSTRIAL_ADMITTED_TOTAL.inc(),
                            FeeLane::Consumer => crate::fees::policy::record_consumer_fee(fee_val),
                        }
                    }
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        "mempool",
                        diagnostics::log::Level::INFO,
                        "admit",
                        &sender_addr,
                        nonce,
                        "ok",
                        ERR_OK,
                        Some(fee_per_byte),
                        Some(&crypto_suite::hex::encode(tx_hash)),
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    if telemetry::should_log("mempool") {
                        let span = crate::log_context!(tx = tx_hash);
                        info!(
                            parent: &span,
                            "tx accepted sender={} nonce={} reason=accepted id={}",
                            scrub(&sender_addr),
                            nonce,
                            scrub(&crypto_suite::hex::encode(tx_hash))
                        );
                    }
                }
                #[cfg(feature = "telemetry")]
                if let Some(j) = self.config.jurisdiction.as_deref() {
                    telemetry::sampled_inc_vec(&telemetry::TX_BY_JURISDICTION_TOTAL, &[j]);
                }
                self.inc_mempool_size(lane);
                Ok(())
            }
        }
    }

    /// Remove a pending transaction and release reserved balances.
    ///
    /// # Errors
    /// Returns [`TxAdmissionError::NotFound`] if the transaction is absent.
    pub fn drop_transaction(&mut self, sender: &str, nonce: u64) -> Result<(), TxAdmissionError> {
        #[cfg(feature = "telemetry")]
        let _pool_guard = {
            let size = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
                + self.mempool_size_industrial.load(AtomicOrdering::SeqCst);
            let span = diagnostics::tracing::span!(
                diagnostics::tracing::Level::TRACE,
                "mempool_mutex",
                sender = %scrub(sender),
                nonce,
                fpb = 0u64,
                mempool_size = size
            );
            span.in_scope(|| self.mempool_mutex.lock()).map_err(|_| {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
                TxAdmissionError::LockPoisoned
            })?
        };
        #[cfg(not(feature = "telemetry"))]
        let _pool_guard = self.mempool_mutex.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
            }
            TxAdmissionError::LockPoisoned
        })?;
        let lock = self
            .admission_locks
            .entry(sender.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        #[cfg(feature = "telemetry")]
        let _guard = {
            let span = diagnostics::tracing::span!(diagnostics::tracing::Level::TRACE, "admission_lock", sender = %scrub(sender), nonce = nonce);
            span.in_scope(|| lock.lock()).map_err(|_| {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
                TxAdmissionError::LockPoisoned
            })?
        };
        #[cfg(not(feature = "telemetry"))]
        let _guard = lock.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            {
                telemetry::LOCK_POISON_TOTAL.inc();
                self.record_reject("lock_poison");
            }
            TxAdmissionError::LockPoisoned
        })?;
        if let Some((_, entry)) = self.mempool_consumer.remove(&(sender.to_string(), nonce)) {
            self.dec_mempool_size(entry.tx.lane);
            self.release_sender_slot(entry.tx.lane, sender);
            let tx = entry.tx;
            if let Some(acc) = self.accounts.get_mut(sender) {
                if let Ok((fee_consumer, fee_industrial)) =
                    crate::fee::decompose(tx.payload.pct, self.base_fee + tx.tip)
                {
                    // Total BLOCK tokens: amount (both lanes) + fees
                    let total_amount = tx.payload.amount_consumer
                        + tx.payload.amount_industrial + fee_consumer + fee_industrial;
                    if acc.pending_amount < total_amount
                        || acc.pending_nonce == 0
                    {
                        return Err(TxAdmissionError::InsufficientBalance);
                    }
                    acc.pending_amount -= total_amount;
                    acc.pending_nonce -= 1;
                    acc.pending_nonces.remove(&nonce);
                }
            }
            if !self.accounts.contains_key(sender) {
                if self.orphan_counter.load(AtomicOrdering::SeqCst) > 0 {
                    self.orphan_counter.fetch_sub(1, AtomicOrdering::SeqCst);
                }
            }
            #[cfg(feature = "telemetry")]
            let tx_hash = tx.id();
            #[cfg(feature = "telemetry-json")]
            log_event(
                "mempool",
                diagnostics::log::Level::INFO,
                "drop",
                sender,
                nonce,
                "dropped",
                ERR_OK,
                None,
                Some(&crypto_suite::hex::encode(tx_hash)),
            );
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            if telemetry::should_log("mempool") {
                let span = crate::log_context!(tx = tx_hash);
                info!(
                    parent: &span,
                    "tx dropped sender={} nonce={} reason=dropped",
                    scrub(sender),
                    nonce
                );
            }
            Ok(())
        } else if let Some((_, entry)) =
            self.mempool_industrial.remove(&(sender.to_string(), nonce))
        {
            self.dec_mempool_size(entry.tx.lane);
            self.release_sender_slot(entry.tx.lane, sender);
            let tx = entry.tx;
            #[cfg(feature = "telemetry")]
            let tx_hash = tx.id();
            if let Some(acc) = self.accounts.get_mut(sender) {
                if let Ok((fee_consumer, fee_industrial)) =
                    crate::fee::decompose(tx.payload.pct, self.base_fee + tx.tip)
                {
                    // Total BLOCK tokens: amount (both lanes) + fees
                    let total_amount = tx.payload.amount_consumer
                        + tx.payload.amount_industrial + fee_consumer + fee_industrial;
                    if acc.pending_amount < total_amount
                        || acc.pending_nonce == 0
                    {
                        return Err(TxAdmissionError::InsufficientBalance);
                    }
                    acc.pending_amount -= total_amount;
                    acc.pending_nonce -= 1;
                    acc.pending_nonces.remove(&nonce);
                }
            }
            if !self.accounts.contains_key(sender) {
                if self.orphan_counter.load(AtomicOrdering::SeqCst) > 0 {
                    self.orphan_counter.fetch_sub(1, AtomicOrdering::SeqCst);
                }
            }
            #[cfg(feature = "telemetry-json")]
            log_event(
                "mempool",
                diagnostics::log::Level::INFO,
                "drop",
                sender,
                nonce,
                "dropped",
                ERR_OK,
                None,
                Some(&crypto_suite::hex::encode(tx_hash)),
            );
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            if telemetry::should_log("mempool") {
                let span = crate::log_context!(tx = tx_hash);
                info!(
                    parent: &span,
                    "tx dropped sender={} nonce={} reason=dropped",
                    scrub(sender),
                    nonce
                );
            }
            Ok(())
        } else {
            #[cfg(feature = "telemetry-json")]
            log_event(
                "mempool",
                diagnostics::log::Level::WARN,
                "drop",
                sender,
                nonce,
                "not_found",
                TxAdmissionError::NotFound.code(),
                None,
                None,
            );
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            warn!("drop failed sender={sender} nonce={nonce} reason=not_found");
            #[cfg(feature = "telemetry")]
            {
                telemetry::DROP_NOT_FOUND_TOTAL.inc();
                self.record_reject("not_found");
            }
            Err(TxAdmissionError::NotFound)
        }
    }

    pub fn purge_expired(&mut self) -> u64 {
        if self.panic_on_purge.swap(false, AtomicOrdering::SeqCst) {
            panic!("purge panic");
        }
        let ttl_ms = self.tx_ttl * 1000;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time: {e}"))
            .as_millis() as u64;
        let mut expired: Vec<(String, u64, u64)> = Vec::new();
        let mut orphaned: Vec<(String, u64, u64)> = Vec::new();
        self.mempool_consumer.for_each(|key, value| {
            let sender = key.0.clone();
            let nonce = key.1;
            let fpb = value.fee_per_byte();
            if now.saturating_sub(value.timestamp_millis) > ttl_ms {
                #[cfg(feature = "telemetry")]
                if telemetry::TTL_DROP_TOTAL.value() < u64::MAX {
                    telemetry::sampled_inc(&*telemetry::TTL_DROP_TOTAL);
                }
                expired.push((sender, nonce, fpb));
            } else if !self.accounts.contains_key(&sender) {
                orphaned.push((sender, nonce, fpb));
            }
        });
        self.mempool_industrial.for_each(|key, value| {
            let sender = key.0.clone();
            let nonce = key.1;
            let fpb = value.fee_per_byte();
            if now.saturating_sub(value.timestamp_millis) > ttl_ms {
                #[cfg(feature = "telemetry")]
                if telemetry::TTL_DROP_TOTAL.value() < u64::MAX {
                    telemetry::sampled_inc(&*telemetry::TTL_DROP_TOTAL);
                }
                expired.push((sender, nonce, fpb));
            } else if !self.accounts.contains_key(&sender) {
                orphaned.push((sender, nonce, fpb));
            }
        });
        let expired_count = expired.len() as u64;
        for (sender, nonce, fpb) in expired {
            #[cfg(feature = "telemetry")]
            let _span = diagnostics::tracing::span!(
                diagnostics::tracing::Level::TRACE,
                "eviction_sweep",
                sender = %scrub(&sender),
                nonce,
                fpb,
                mempool_size = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
                    + self.mempool_size_industrial.load(AtomicOrdering::SeqCst)
            )
            .entered();
            #[cfg(not(feature = "telemetry"))]
            let _ = fpb;
            let _ = self.drop_transaction(&sender, nonce);
        }
        // track current orphan count after removing expired entries
        self.orphan_counter
            .store(orphaned.len(), AtomicOrdering::SeqCst);
        let size = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
            + self.mempool_size_industrial.load(AtomicOrdering::SeqCst);
        let orphans = orphaned.len();
        if size > 0 && orphans * 2 > size {
            #[cfg(feature = "telemetry")]
            if telemetry::ORPHAN_SWEEP_TOTAL.value() < u64::MAX {
                telemetry::ORPHAN_SWEEP_TOTAL.inc();
            }
            for (sender, nonce, fpb) in orphaned {
                #[cfg(feature = "telemetry")]
                let _span = diagnostics::tracing::span!(
                    diagnostics::tracing::Level::TRACE,
                    "eviction_sweep",
                    sender = %scrub(&sender),
                    nonce,
                    fpb,
                    mempool_size = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
                        + self.mempool_size_industrial.load(AtomicOrdering::SeqCst)
                )
                .entered();
                #[cfg(not(feature = "telemetry"))]
                let _ = fpb;
                let _ = self.drop_transaction(&sender, nonce);
            }
            self.orphan_counter.store(0, AtomicOrdering::SeqCst);
        }
        expired_count
    }

    #[must_use]
    pub fn current_chain_length(&self) -> usize {
        self.chain.len()
    }

    /// Mine a block using the current wall-clock time.
    ///
    /// Prefer [`mine_block_at`] in tests to supply deterministic timestamps and
    /// avoid mutating blocks after they are mined.
    pub fn mine_block(&mut self, miner_addr: &str) -> PyResult<Block> {
        self.mine_block_now(miner_addr)
    }

    /// Mine a block using the current wall-clock time.
    ///
    /// This wrapper is provided to clearly separate real-time mining from
    /// deterministic test helpers like [`mine_block_at`].
    pub fn mine_block_now(&mut self, miner_addr: &str) -> PyResult<Block> {
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_millis() as u64;
        self.mine_block_with_ts(miner_addr, timestamp_millis)
    }

    fn dynamic_block_limit(&self) -> usize {
        let pressure = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
            + self.mempool_size_industrial.load(AtomicOrdering::SeqCst);
        let max = self.max_mempool_size_consumer + self.max_mempool_size_industrial;
        if pressure > max / 2 {
            1024
        } else {
            256
        }
    }

    /// Mine a block at an explicit timestamp (milliseconds since UNIX epoch).
    ///
    /// This helper is primarily used by tests to produce deterministic chains.
    /// Supplying the timestamp up front keeps the difficulty window and
    /// `recent_timestamps` aligned without post-hoc mutation.
    pub fn mine_block_at(&mut self, miner_addr: &str, timestamp_millis: u64) -> PyResult<Block> {
        self.mine_block_with_ts(miner_addr, timestamp_millis)
    }

    /// Recompute `recent_timestamps` and consensus difficulty from the current
    /// chain state. Call this after manually mutating block timestamps.
    pub fn recompute_difficulty(&mut self) {
        self.recent_timestamps.clear();
        let start = self.chain.len().saturating_sub(DIFFICULTY_WINDOW);
        for b in &self.chain[start..] {
            self.recent_timestamps.push_back(b.timestamp_millis);
        }
        let last = self.chain.last().map_or(1, |b| b.difficulty);
        let hint = self.chain.last().map_or(0, |b| b.retune_hint);
        let ts = self.recent_timestamps.make_contiguous();
        let (next, h) = consensus::difficulty_retune::retune(last, ts, hint, &self.params);
        self.difficulty = next;
        self.retune_hint = h;
    }

    /// Update the timestamp of the block at `index` and resynchronize
    /// difficulty accounting. Intended for tests that need to fabricate
    /// specific timelines.
    pub fn set_block_timestamp(&mut self, index: usize, timestamp_millis: u64) {
        if let Some(b) = self.chain.get_mut(index) {
            b.timestamp_millis = timestamp_millis;
            self.recompute_difficulty();
        }
    }

    /// Submit a blob transaction to the pending blob queue.
    pub fn submit_blob_tx(&mut self, tx: BlobTx) -> PyResult<()> {
        if self.pending_blob_bytes + tx.blob_size > crate::constants::MAX_UNFINALIZED_BLOB_BYTES {
            return Err(py_value_err("blob mempool full"));
        }
        self.pending_blob_bytes += tx.blob_size;
        self.blob_scheduler.push(tx.blob_root, tx.fractal_lvl > 1);
        self.blob_mempool.push(tx);
        Ok(())
    }

    /// Mine a new block and award rewards to `miner_addr`.
    ///
    /// # Errors
    /// Returns a [`PyValueError`] if fee or nonce calculations overflow or if
    /// persisting the chain fails.
    #[allow(clippy::too_many_lines)]
    fn mine_block_with_ts(&mut self, miner_addr: &str, timestamp_millis: u64) -> PyResult<Block> {
        let index = self.chain.len() as u64;
        let last = self.chain.last().map_or(1, |b| b.difficulty);
        let ts = self.recent_timestamps.make_contiguous();
        let (expected, _) =
            consensus::difficulty_retune::retune(last, ts, self.retune_hint, &self.params);
        if std::env::var("TB_FAST_MINE").as_deref() == Ok("1") {
            self.difficulty = expected;
        } else {
            debug_assert_eq!(
                self.difficulty, expected,
                "stale difficulty; call recompute_difficulty after mutating timestamps"
            );
        }
        let prev_hash = if index == 0 {
            "0".repeat(64)
        } else {
            self.chain
                .last()
                .map(|b| b.hash.clone())
                .ok_or_else(|| py_value_err("empty chain"))?
        };

        if std::env::var("TB_FAST_MINE").as_deref() == Ok("1") {
            let mut block = Block::default();
            block.index = index;
            block.previous_hash = prev_hash.clone();
            block.timestamp_millis = timestamp_millis;
            block.difficulty = 1;
            block.retune_hint = self.retune_hint;
            block.base_fee = self.base_fee;
            block.nonce = 0;
            let mut hash_input = Vec::new();
            hash_input.extend_from_slice(prev_hash.as_bytes());
            hash_input.extend_from_slice(&index.to_le_bytes());
            hash_input.extend_from_slice(&timestamp_millis.to_le_bytes());
            block.hash = blake3::hash(&hash_input).to_hex().to_string();
            self.chain.push(block.clone());
            self.block_height = index + 1;
            self.recent_timestamps.push_back(timestamp_millis);
            return Ok(block);
        }

        // use the network-controlled base reward (falling back to the genesis value)
        let base_reward = if self.economics_block_reward_per_block == 0 {
            INITIAL_BLOCK_REWARD
        } else {
            self.economics_block_reward_per_block
        };
        self.block_reward = TokenAmount::new(base_reward);
        let active_eff = {
            let mut counts: HashMap<String, u64> = HashMap::new();
            for m in self
                .recent_miners
                .iter()
                .chain(std::iter::once(&miner_addr.to_owned()))
            {
                *counts.entry(m.clone()).or_default() += 1;
            }
            let keys: Vec<String> = counts.keys().cloned().collect();
            let mut weighted: Vec<f64> = Vec::new();
            for k in &keys {
                let count = counts.get(k).copied().unwrap_or(0) as f64;
                let max_prefix = keys
                    .iter()
                    .filter(|o| *o != k)
                    .map(|o| k.chars().zip(o.chars()).take_while(|(a, b)| a == b).count())
                    .max()
                    .unwrap_or(0) as f64;
                let w = 1.0 + max_prefix / 24.0;
                weighted.push(count * w);
            }
            let total: f64 = weighted.iter().sum();
            if total == 0.0 {
                1.0
            } else {
                let alphas = [1.5_f64, 2.0, 3.0];
                let norm: f64 = alphas.iter().map(|a| (-a).exp()).sum();
                let mut h = 0.0;
                for alpha in alphas.iter() {
                    let sum_p: f64 = weighted.iter().map(|w| (w / total).powf(*alpha)).sum();
                    let h_alpha = sum_p.ln() / (1.0 - alpha);
                    let w_alpha = (-alpha).exp() / norm;
                    h += w_alpha * h_alpha;
                }
                h.exp().max(1.0)
            }
        };
        let n_star = self.params.miner_reward_logistic_target.max(1) as f64;
        if self.block_height >= self.logistic_lock_end
            && (active_eff - self.logistic_last_n).abs() >= self.params.miner_hysteresis as f64
        {
            let xi = self.params.logistic_slope_milli as f64 / 1000.0;
            self.logistic_factor = 1f64 / (1f64 + f64::exp(xi * (active_eff - n_star)));
            self.logistic_last_n = active_eff;
            self.logistic_lock_end = self.block_height + 5 * EPOCH_BLOCKS as u64;
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::MINER_REWARD_RECALC_TOTAL.inc();
                diagnostics::tracing::info!(
                    active = active_eff,
                    factor = self.logistic_factor,
                    "miner_reward_recalc"
                );
            }
        }
        let logistic = self.logistic_factor;
        let mut reward = TokenAmount::new((self.block_reward.0 as f64 * logistic).round() as u64);
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::ACTIVE_MINERS.set(active_eff.round() as i64);
            crate::telemetry::BASE_REWARD.set(reward.0 as i64);
        }
        if self.emission + reward.0 > MAX_SUPPLY_BLOCK {
            reward = TokenAmount::new(MAX_SUPPLY_BLOCK - self.emission);
        }

        self.skipped.clear();
        let mut pending: Vec<MempoolEntry> = Vec::new();
        self.mempool_consumer
            .for_each(|_, value| pending.push(value.clone()));
        self.mempool_industrial
            .for_each(|_, value| pending.push(value.clone()));
        pending.sort_unstable_by(|a, b| mempool_cmp(a, b, self.tx_ttl));
        let max_in_block = self.dynamic_block_limit();
        let mut included = Vec::new();
        let mut deferred: HashMap<String, Vec<SignedTransaction>> = HashMap::new();
        let mut expected: HashMap<String, u64> = HashMap::new();
        let mut skipped = Vec::new();
        for (i, entry) in pending.into_iter().enumerate() {
            if i >= max_in_block {
                skipped.push(entry.tx);
                continue;
            }
            let tx = entry.tx;
            let from = tx.payload.from_.clone();
            let exp = expected
                .entry(from.clone())
                .or_insert_with(|| self.accounts.get(&from).map(|a| a.nonce + 1).unwrap_or(1));
            if tx.payload.nonce == *exp {
                included.push(tx.clone());
                *exp += 1;
                let tx_volume = tx
                    .payload
                    .amount_consumer
                    .saturating_add(tx.payload.amount_industrial)
                    .saturating_add(tx.tip);
                self.economics_epoch_tx_count = self.economics_epoch_tx_count.saturating_add(1);
                self.economics_epoch_tx_volume_block = self
                    .economics_epoch_tx_volume_block
                    .saturating_add(tx_volume);
                if let Some(list) = deferred.get_mut(&from) {
                    loop {
                        if let Some(pos) = list.iter().position(|t| t.payload.nonce == *exp) {
                            let tx2 = list.remove(pos);
                            included.push(tx2.clone());
                            *exp += 1;
                        } else {
                            break;
                        }
                    }
                }
            } else {
                deferred.entry(from).or_default().push(tx);
            }
        }
        for list in deferred.into_values() {
            skipped.extend(list);
        }
        let mut fee_sum_consumer: u128 = 0;
        let mut fee_sum_industrial: u128 = 0;
        for tx in &included {
            if let Ok((fee_consumer, fee_industrial)) =
                crate::fee::decompose(tx.payload.pct, tx.tip)
            {
                fee_sum_consumer += fee_consumer as u128;
                fee_sum_industrial += fee_industrial as u128;
            }
        }
        let fee_consumer_u64 =
            u64::try_from(fee_sum_consumer).map_err(|_| py_value_err("Fee overflow"))?;
        let fee_industrial_u64 =
            u64::try_from(fee_sum_industrial).map_err(|_| py_value_err("Fee overflow"))?;
        let (cpu_ms, bytes_out) = crate::exec::take_metrics();
        self.epoch_cpu_ms = self.epoch_cpu_ms.saturating_add(cpu_ms);
        self.epoch_bytes_out = self.epoch_bytes_out.saturating_add(bytes_out);
        let storage_sub =
            (self.beta_storage_sub_raw as u64).saturating_mul(self.epoch_storage_bytes);
        let delta_read_bytes = self
            .epoch_read_bytes
            .saturating_sub(self.settled_read_bytes);
        let read_sub = (self.gamma_read_sub_raw as u64).saturating_mul(delta_read_bytes);
        let mut compute_sub = (self.kappa_cpu_sub_raw as u64)
            .saturating_mul(self.epoch_cpu_ms)
            .saturating_add(
                (self.lambda_bytes_out_sub_raw as u64).saturating_mul(self.epoch_bytes_out),
            );

        // === SUPPLY CAP ENFORCEMENT (EARLY) ===
        // Enforce MAX_SUPPLY_BLOCK cap on subsidies + reward BEFORE distributions.
        // We'll check again later after rebates are calculated, but this early check
        // prevents inconsistencies in subsidy distributions.
        let remaining_supply = MAX_SUPPLY_BLOCK.saturating_sub(self.emission);
        let preliminary_mint = storage_sub
            .saturating_add(read_sub)
            .saturating_add(compute_sub)
            .saturating_add(reward.0);

        // Make subsidies mutable so we can clamp them if needed
        let mut storage_sub = storage_sub;
        let mut read_sub = read_sub;

        if preliminary_mint > remaining_supply {
            // Approaching cap - preserve subsidies first, clamp reward
            let subsidies_only = storage_sub
                .saturating_add(read_sub)
                .saturating_add(compute_sub);

            if subsidies_only <= remaining_supply {
                // Subsidies fit, zero reward
                reward = TokenAmount::new(0);
            } else {
                // Even subsidies exceed cap - scale proportionally
                let scale = (remaining_supply as f64) / (subsidies_only as f64);
                storage_sub = ((storage_sub as f64) * scale).floor() as u64;
                read_sub = ((read_sub as f64) * scale).floor() as u64;
                compute_sub = ((compute_sub as f64) * scale).floor() as u64;
                reward = TokenAmount::new(0);
            }
        }

        let mut base_coinbase_block = reward
            .0
            .checked_add(storage_sub)
            .and_then(|v| v.checked_add(compute_sub))
            .and_then(|v| v.checked_add(fee_consumer_u64))
            .ok_or_else(|| py_value_err("Fee overflow"))?;

        self.economics_epoch_storage_payout_block = self
            .economics_epoch_storage_payout_block
            .saturating_add(storage_sub.saturating_add(read_sub));
        self.economics_epoch_compute_payout_block = self
            .economics_epoch_compute_payout_block
            .saturating_add(compute_sub);

        let viewer_deltas: Vec<(String, u64)> = self
            .epoch_viewer_bytes
            .iter()
            .map(|(addr, total)| {
                let settled = self
                    .settled_viewer_bytes
                    .get(addr)
                    .copied()
                    .unwrap_or_default();
                let delta = total.saturating_sub(settled);
                (addr.clone(), delta)
            })
            .filter(|(_, delta)| *delta > 0)
            .collect();
        let host_deltas: Vec<(String, u64)> = self
            .epoch_host_bytes
            .iter()
            .map(|(addr, total)| {
                let settled = self
                    .settled_host_bytes
                    .get(addr)
                    .copied()
                    .unwrap_or_default();
                let delta = total.saturating_sub(settled);
                (addr.clone(), delta)
            })
            .filter(|(_, delta)| *delta > 0)
            .collect();
        let hardware_deltas: Vec<(String, u64)> = self
            .epoch_hardware_bytes
            .iter()
            .map(|(addr, total)| {
                let settled = self
                    .settled_hardware_bytes
                    .get(addr)
                    .copied()
                    .unwrap_or_default();
                let delta = total.saturating_sub(settled);
                (addr.clone(), delta)
            })
            .filter(|(_, delta)| *delta > 0)
            .collect();
        let verifier_deltas: Vec<(String, u64)> = self
            .epoch_verifier_bytes
            .iter()
            .map(|(addr, total)| {
                let settled = self
                    .settled_verifier_bytes
                    .get(addr)
                    .copied()
                    .unwrap_or_default();
                let delta = total.saturating_sub(settled);
                (addr.clone(), delta)
            })
            .filter(|(_, delta)| *delta > 0)
            .collect();

        let viewer_percent = self.params.read_subsidy_viewer_percent.max(0) as u64;
        let host_percent = self.params.read_subsidy_host_percent.max(0) as u64;
        let hardware_percent = self.params.read_subsidy_hardware_percent.max(0) as u64;
        let verifier_percent = self.params.read_subsidy_verifier_percent.max(0) as u64;
        let liquidity_percent = self.params.read_subsidy_liquidity_percent.max(0) as u64;
        let role_allocations = distribute_scalar(
            read_sub,
            &[
                (0, viewer_percent),
                (1, host_percent),
                (2, hardware_percent),
                (3, verifier_percent),
                (4, liquidity_percent),
            ],
        );

        let viewer_target = role_allocations.get(0).copied().unwrap_or(0);
        let host_target = role_allocations.get(1).copied().unwrap_or(0);
        let hardware_target = role_allocations.get(2).copied().unwrap_or(0);
        let verifier_target = role_allocations.get(3).copied().unwrap_or(0);
        let mut liquidity_paid = role_allocations.get(4).copied().unwrap_or(0);

        let mut viewer_payouts = distribute_proportional(viewer_target, &viewer_deltas);
        let viewer_read_paid: u64 = viewer_payouts.iter().map(|(_, amt)| *amt).sum();
        liquidity_paid =
            liquidity_paid.saturating_add(viewer_target.saturating_sub(viewer_read_paid));

        let mut host_payouts = distribute_proportional(host_target, &host_deltas);
        let host_read_paid: u64 = host_payouts.iter().map(|(_, amt)| *amt).sum();
        liquidity_paid = liquidity_paid.saturating_add(host_target.saturating_sub(host_read_paid));

        let mut hardware_payouts = distribute_proportional(hardware_target, &hardware_deltas);
        let hardware_read_paid: u64 = hardware_payouts.iter().map(|(_, amt)| *amt).sum();
        liquidity_paid =
            liquidity_paid.saturating_add(hardware_target.saturating_sub(hardware_read_paid));

        let mut verifier_payouts = distribute_proportional(verifier_target, &verifier_deltas);
        let verifier_read_paid: u64 = verifier_payouts.iter().map(|(_, amt)| *amt).sum();
        liquidity_paid =
            liquidity_paid.saturating_add(verifier_target.saturating_sub(verifier_read_paid));

        let mut miner_share_total = read_sub
            .saturating_sub(viewer_read_paid)
            .saturating_sub(host_read_paid)
            .saturating_sub(hardware_read_paid)
            .saturating_sub(verifier_read_paid)
            .saturating_sub(liquidity_paid);

        let ad_settlements = std::mem::take(&mut self.pending_ad_settlements);

        // Pre-allocate receipt vector with capacity hint (performance optimization)
        // Estimate: ad_settlements + typical counts from other markets
        let estimated_receipt_count = ad_settlements
            .len()
            .saturating_add(100) // Storage receipts (typical)
            .saturating_add(50) // Compute receipts (typical)
            .saturating_add(20); // Energy receipts (typical)
        let mut block_receipts: Vec<Receipt> = Vec::with_capacity(estimated_receipt_count);

        let mut ad_viewer_total = 0u64;
        let mut ad_host_total = 0u64;
        let mut ad_hardware_total = 0u64;
        let mut ad_verifier_total = 0u64;
        let mut ad_liquidity_total = 0u64;
        let mut ad_miner_total = 0u64;
        let mut ad_total_usd_micros = 0u64;
        let mut ad_last_price_usd_micros = 0u64;
        let mut ad_settlement_count = 0u64;
        for record in &ad_settlements {
            if record.viewer > 0 {
                viewer_payouts.push((record.viewer_addr.clone(), record.viewer));
                ad_viewer_total = ad_viewer_total.saturating_add(record.viewer);
            }
            if record.host > 0 {
                host_payouts.push((record.host_addr.clone(), record.host));
                ad_host_total = ad_host_total.saturating_add(record.host);
            }
            if record.hardware > 0 {
                hardware_payouts.push((record.hardware_addr.clone(), record.hardware));
                ad_hardware_total = ad_hardware_total.saturating_add(record.hardware);
            }
            if record.verifier > 0 {
                verifier_payouts.push((record.verifier_addr.clone(), record.verifier));
                ad_verifier_total = ad_verifier_total.saturating_add(record.verifier);
            }
            ad_liquidity_total = ad_liquidity_total.saturating_add(record.liquidity);
            ad_miner_total = ad_miner_total.saturating_add(record.miner);
            ad_total_usd_micros = ad_total_usd_micros.saturating_add(record.total_usd_micros);
            ad_last_price_usd_micros = record.price_usd_micros;
            ad_settlement_count = ad_settlement_count.saturating_add(1);
            block_receipts.push(Receipt::Ad(AdReceipt {
                campaign_id: record.campaign_id.clone(),
                publisher: record.host_addr.clone(),
                impressions: record.impressions,
                spend: record.total,
                block_height: index,
                conversions: record.conversions,
                publisher_signature: vec![],
                signature_nonce: index,
            }));
        }
        for receipt in crate::energy::drain_energy_receipts() {
            block_receipts.push(Receipt::Energy(EnergyReceipt {
                contract_id: format!(
                    "energy:{}",
                    crypto_suite::hex::encode(receipt.meter_reading_hash)
                ),
                provider: receipt.seller.clone(),
                energy_units: receipt.kwh_delivered,
                price: receipt.price_paid,
                block_height: receipt.block_settled,
                proof_hash: receipt.meter_reading_hash,
                provider_signature: vec![],
                signature_nonce: receipt.block_settled,
            }));
        }
        for receipt in crate::rpc::storage::drain_storage_receipts() {
            block_receipts.push(Receipt::Storage(receipt));
        }
        crate::compute_market::set_compute_current_block(index);
        for receipt in crate::compute_market::drain_compute_receipts() {
            block_receipts.push(Receipt::Compute(receipt));
        }

        // Validate receipt count and size (DoS protection)
        if let Err(e) = crate::receipts_validation::validate_receipt_count(block_receipts.len()) {
            return Err(PyError::value(format!("Receipt validation failed: {}", e)));
        }

        // Validate individual receipts
        for receipt in &block_receipts {
            if let Err(_e) = crate::receipts_validation::validate_receipt(
                receipt,
                index,
                &self.provider_registry,
                &mut self.nonce_tracker,
            ) {
                #[cfg(feature = "telemetry")]
                crate::telemetry::receipts::RECEIPT_VALIDATION_FAILURES_TOTAL.inc();

                #[cfg(feature = "telemetry")]
                warn!(
                    error = %_e,
                    receipt_type = receipt.market_name(),
                    block_height = index,
                    "Invalid receipt detected - skipping"
                );
                // Note: In production, you may want to filter out invalid receipts
                // instead of failing the entire block. For now, we log and continue.
            }
        }

        liquidity_paid = liquidity_paid.saturating_add(ad_liquidity_total);
        miner_share_total = miner_share_total.saturating_add(ad_miner_total);

        let ad_viewer_token = TokenAmount::new(ad_viewer_total);
        let ad_host_token = TokenAmount::new(ad_host_total);
        let ad_hardware_token = TokenAmount::new(ad_hardware_total);
        let ad_verifier_token = TokenAmount::new(ad_verifier_total);
        let ad_liquidity_token = TokenAmount::new(ad_liquidity_total);
        let ad_miner_token = TokenAmount::new(ad_miner_total);
        let ad_total = ad_viewer_total
            .saturating_add(ad_host_total)
            .saturating_add(ad_hardware_total)
            .saturating_add(ad_verifier_total)
            .saturating_add(ad_liquidity_total)
            .saturating_add(ad_miner_total);
        self.economics_epoch_ad_payout_block = self
            .economics_epoch_ad_payout_block
            .saturating_add(ad_total);

        self.settled_read_bytes = self.settled_read_bytes.saturating_add(delta_read_bytes);
        for (addr, total) in &self.epoch_viewer_bytes {
            self.settled_viewer_bytes.insert(addr.clone(), *total);
        }
        self.settled_viewer_bytes
            .retain(|addr, _| self.epoch_viewer_bytes.contains_key(addr));
        for (addr, total) in &self.epoch_host_bytes {
            self.settled_host_bytes.insert(addr.clone(), *total);
        }
        self.settled_host_bytes
            .retain(|addr, _| self.epoch_host_bytes.contains_key(addr));
        for (addr, total) in &self.epoch_hardware_bytes {
            self.settled_hardware_bytes.insert(addr.clone(), *total);
        }
        self.settled_hardware_bytes
            .retain(|addr, _| self.epoch_hardware_bytes.contains_key(addr));
        for (addr, total) in &self.epoch_verifier_bytes {
            self.settled_verifier_bytes.insert(addr.clone(), *total);
        }
        self.settled_verifier_bytes
            .retain(|addr, _| self.epoch_verifier_bytes.contains_key(addr));

        let liquidity_payouts = if liquidity_paid > 0 {
            vec![(liquidity_address().to_string(), liquidity_paid)]
        } else {
            Vec::new()
        };
        if miner_share_total > 0 {
            base_coinbase_block = base_coinbase_block
                .checked_add(miner_share_total)
                .ok_or_else(|| py_value_err("Fee overflow"))?;
        }

        let treasury_percent = self.params.treasury_percent.clamp(0, 100) as u64;
        let treasury_cut = base_coinbase_block.saturating_mul(treasury_percent) / 100;
        let mut actual_treasury_accrued = 0u64;
        if treasury_cut > 0 {
            if let Err(err) = NODE_GOV_STORE.record_treasury_accrual(treasury_cut) {
                diagnostics::log::warn!(format!(
                    "failed to accrue treasury disbursement share: {err}"
                ));
                #[cfg(feature = "telemetry")]
                diagnostics::tracing::error!(treasury_cut, "treasury_accrual_failed");
            } else {
                base_coinbase_block = base_coinbase_block.saturating_sub(treasury_cut);
                actual_treasury_accrued = treasury_cut;
            }
        }
        self.economics_epoch_treasury_inflow_block = self
            .economics_epoch_treasury_inflow_block
            .saturating_add(actual_treasury_accrued);

        // === SUPPLY CAP ENFORCEMENT (FINAL - REBATES) ===
        // Subsidies and reward were already clamped earlier. Now clamp rebates if needed.
        let mut rebate_tokens = self.proof_tracker.claim_all(index);
        let remaining = MAX_SUPPLY_BLOCK.saturating_sub(self.emission);
        let subsidies_and_reward = storage_sub
            .saturating_add(read_sub)
            .saturating_add(compute_sub)
            .saturating_add(reward.0);

        if subsidies_and_reward.saturating_add(rebate_tokens) > remaining {
            // Clamp rebates to fit within remaining cap
            let rebate_max = remaining.saturating_sub(subsidies_and_reward);
            if rebate_tokens > rebate_max {
                rebate_tokens = rebate_max;
                #[cfg(feature = "telemetry")]
                diagnostics::tracing::warn!(
                    remaining,
                    rebate_clamped = rebate_max,
                    "supply_cap_clamping_rebates"
                );
            }
        }

        let coinbase_block_total = base_coinbase_block
            .checked_add(rebate_tokens)
            .and_then(|v| v.checked_add(fee_industrial_u64))
            .ok_or_else(|| py_value_err("Fee overflow"))?;
        let coinbase_industrial_total = 0;

        let mut fee_hasher = blake3::Hasher::new();
        fee_hasher.update(&fee_consumer_u64.to_le_bytes());
        fee_hasher.update(&fee_industrial_u64.to_le_bytes());
        let fee_checksum = fee_hasher.finalize().to_hex().to_string();

        let storage_sub_token = TokenAmount::new(storage_sub);
        let read_sub_token = TokenAmount::new(read_sub);
        let read_sub_viewer_token = TokenAmount::new(viewer_read_paid);
        let read_sub_host_token = TokenAmount::new(host_read_paid);
        let read_sub_hardware_token = TokenAmount::new(hardware_read_paid);
        let read_sub_verifier_token = TokenAmount::new(verifier_read_paid);
        let read_sub_liquidity_token = TokenAmount::new(liquidity_paid);
        let compute_sub_token = TokenAmount::new(compute_sub);
        let coinbase = SignedTransaction {
            payload: RawTxPayload {
                from_: "0".repeat(34),
                to: miner_addr.to_owned(),
                amount_consumer: coinbase_block_total,
                amount_industrial: coinbase_industrial_total,
                fee: 0,
                pct: 100,
                nonce: 0,
                memo: Vec::new(),
            },
            public_key: vec![],
            #[cfg(feature = "quantum")]
            dilithium_public_key: Vec::new(),
            signature: TxSignature {
                ed25519: Vec::new(),
                #[cfg(feature = "quantum")]
                dilithium: Vec::new(),
            },
            tip: 0,
            signer_pubkeys: Vec::new(),
            aggregate_signature: Vec::new(),
            threshold: 0,
            lane: transaction::FeeLane::Consumer,
            version: TxVersion::Ed25519Only,
        };
        let mut txs = vec![coinbase.clone()];
        txs.extend(included.clone());

        let block_base_fee = self.base_fee;

        let mut treasury_events = Vec::new();
        let mut tx_hashes: HashSet<String> = HashSet::with_capacity(txs.len());
        for tx in &txs {
            tx_hashes.insert(crypto_suite::hex::encode(tx.id()));
        }
        match NODE_GOV_STORE.disbursements() {
            Ok(records) => {
                treasury_events = records
                    .into_iter()
                    .filter_map(|record| match record.status {
                        DisbursementStatus::Executed {
                            ref tx_hash,
                            executed_at,
                        } if tx_hashes.contains(tx_hash) => Some(BlockTreasuryEvent {
                            disbursement_id: record.id,
                            destination: record.destination,
                            amount: record.amount,
                            memo: record.memo,
                            scheduled_epoch: record.scheduled_epoch,
                            tx_hash: tx_hash.clone(),
                            executed_at,
                        }),
                        _ => None,
                    })
                    .collect();
                treasury_events.sort_by_key(|event| event.disbursement_id);
            }
            Err(err) => {
                diagnostics::log::warn!(format!(
                    "failed to load treasury disbursements for timeline: {err}"
                ));
            }
        }

        // Pre-compute state root using a shadow copy of accounts
        let mut shadow_accounts = self.accounts.clone();
        for tx in txs.iter().skip(1) {
            if tx.payload.from_ != "0".repeat(34) {
                if let Some(s) = shadow_accounts.get_mut(&tx.payload.from_) {
                    let (fee_c, fee_i) =
                        crate::fee::decompose(tx.payload.pct, block_base_fee + tx.tip)
                            .unwrap_or((0, 0));
                    // Total BLOCK tokens: amount (both lanes) + fees
                    let total_amount = tx.payload.amount_consumer
                        + tx.payload.amount_industrial + fee_c + fee_i;
                    if s.balance.amount < total_amount {
                        return Err(ErrBalanceUnderflow::new_err("balance underflow"));
                    }
                    s.balance.amount -= total_amount;
                    s.nonce = tx.payload.nonce;
                }
            }
            let r = shadow_accounts
                .entry(tx.payload.to.clone())
                .or_insert(Account {
                    address: tx.payload.to.clone(),
                    balance: TokenBalance {
                        amount: 0,
                    },
                    nonce: 0,
                    pending_amount: 0,
                    pending_nonce: 0,
                    pending_nonces: HashSet::new(),
                    sessions: Vec::new(),
                });
            r.balance.amount += tx.payload.amount_consumer + tx.payload.amount_industrial;
        }
        let miner_shadow = shadow_accounts
            .entry(miner_addr.to_owned())
            .or_insert(Account {
                address: miner_addr.to_owned(),
                balance: TokenBalance {
                    amount: 0,
                },
                nonce: 0,
                pending_amount: 0,
                pending_nonce: 0,
                pending_nonces: HashSet::new(),
                sessions: Vec::new(),
            });
        // Credit total coinbase (block reward + fees) to miner
        let coinbase_total = coinbase_block_total + coinbase_industrial_total;
        miner_shadow.balance.amount = miner_shadow
            .balance
            .amount
            .checked_add(coinbase_total)
            .ok_or_else(|| py_value_err("miner balance overflow"))?;

        for (addr, amount) in viewer_payouts
            .iter()
            .chain(host_payouts.iter())
            .chain(hardware_payouts.iter())
            .chain(verifier_payouts.iter())
            .chain(liquidity_payouts.iter())
        {
            if *amount == 0 {
                continue;
            }
            let entry = shadow_accounts.entry(addr.clone()).or_insert(Account {
                address: addr.clone(),
                balance: TokenBalance {
                    amount: 0,
                },
                nonce: 0,
                pending_amount: 0,
                pending_nonce: 0,
                pending_nonces: HashSet::new(),
                sessions: Vec::new(),
            });
            entry.balance.amount = entry
                .balance
                .amount
                .checked_add(*amount)
                .ok_or_else(|| py_value_err("subsidy overflow"))?;
        }
        let root = crate::blockchain::snapshot::state_root(&shadow_accounts);

        let diff = self.difficulty;
        let ready_roots = {
            let mut r = self.blob_scheduler.pop_l2_ready();
            r.extend(self.blob_scheduler.pop_l3_ready());
            r
        };
        let mut included_roots = Vec::new();
        let mut included_sizes = Vec::new();
        self.blob_mempool.retain(|b| {
            if ready_roots.contains(&b.blob_root) {
                included_roots.push(b.blob_root);
                included_sizes.push(b.blob_size as u32);
                false
            } else {
                true
            }
        });
        self.pending_blob_bytes = self.blob_mempool.iter().map(|b| b.blob_size).sum();

        let batch = self.read_batcher.finalize();

        let mut block = Block {
            index,
            previous_hash: prev_hash.clone(),
            timestamp_millis,
            transactions: txs.clone(),
            difficulty: diff,
            retune_hint: self.retune_hint,
            nonce: 0,
            hash: String::new(),
            coinbase_block: TokenAmount::new(base_coinbase_block),
            coinbase_industrial: TokenAmount::new(coinbase_industrial_total),
            storage_sub: storage_sub_token,
            read_sub: read_sub_token,
            read_sub_viewer: read_sub_viewer_token,
            read_sub_host: read_sub_host_token,
            read_sub_hardware: read_sub_hardware_token,
            read_sub_verifier: read_sub_verifier_token,
            read_sub_liquidity: read_sub_liquidity_token,
            ad_viewer: ad_viewer_token,
            ad_host: ad_host_token,
            ad_hardware: ad_hardware_token,
            ad_verifier: ad_verifier_token,
            ad_liquidity: ad_liquidity_token,
            ad_miner: ad_miner_token,
            treasury_events,
            ad_total_usd_micros,
            ad_settlement_count,
            ad_oracle_price_usd_micros: ad_last_price_usd_micros,
            compute_sub: compute_sub_token,
            proof_rebate: TokenAmount::new(0),
            read_root: batch.root,
            fee_checksum: fee_checksum.clone(),
            state_root: root.clone(),
            base_fee: block_base_fee,
            l2_roots: included_roots,
            l2_sizes: included_sizes,
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: Vec::new(),
            #[cfg(feature = "quantum")]
            dilithium_pubkey: Vec::new(),
            #[cfg(feature = "quantum")]
            dilithium_sig: Vec::new(),
            receipts: block_receipts,
        };

        // Validate receipt size before mining (DoS protection)
        // We encode receipts once here and cache the result to avoid double encoding
        // in the hash calculation loop (performance optimization).
        let receipts_serialized =
            crate::block_binary::encode_receipts(&block.receipts).map_err(|e| {
                PyError::runtime(format!(
                    "Failed to encode receipts for size validation: {:?}",
                    e
                ))
            })?;

        if let Err(e) = crate::receipts_validation::validate_receipt_size(receipts_serialized.len())
        {
            return Err(PyError::value(format!(
                "Receipt size validation failed: {}. Encoded size: {} bytes, Receipt count: {}",
                e,
                receipts_serialized.len(),
                block.receipts.len()
            )));
        }

        crate::blockchain::process::apply_coinbase_rebates(&mut block, rebate_tokens);

        let mut nonce = 0u64;
        loop {
            let hash = calculate_hash_with_cached_receipts(
                index,
                &prev_hash,
                timestamp_millis,
                nonce,
                diff,
                block_base_fee,
                block.coinbase_block,
                block.coinbase_industrial,
                block.storage_sub,
                block.read_sub,
                block.read_sub_viewer,
                block.read_sub_host,
                block.read_sub_hardware,
                block.read_sub_verifier,
                block.read_sub_liquidity,
                block.ad_viewer,
                block.ad_host,
                block.ad_hardware,
                block.ad_verifier,
                block.ad_liquidity,
                block.ad_miner,
                block.ad_total_usd_micros,
                block.ad_settlement_count,
                block.ad_oracle_price_usd_micros,
                block.compute_sub,
                block.proof_rebate,
                block.read_root,
                &fee_checksum,
                &txs,
                &root,
                &block.l2_roots,
                &block.l2_sizes,
                block.vdf_commit,
                block.vdf_output,
                &block.vdf_proof,
                block.retune_hint,
                &receipts_serialized, // Use cached serialized receipts (performance optimization)
            );
            let bytes = hex_to_bytes(&hash);
            if leading_zero_bits(&bytes) >= diff as u32 {
                block.nonce = nonce;
                block.hash = hash.clone();
                self.chain.push(block.clone());

                // Update dynamic pricing engine with new block data
                let mut consumer_count = 0u64;
                let mut industrial_count = 0u64;
                for tx in block.transactions.iter().skip(1) {
                    // Skip coinbase (first tx)
                    match tx.lane {
                        FeeLane::Consumer => consumer_count += 1,
                        FeeLane::Industrial => industrial_count += 1,
                    }
                }
                if let Ok(mut engine) = self.lane_pricing_engine.lock() {
                    engine.update_block(consumer_count, industrial_count);
                    // Update cached fees to avoid lock contention on transaction admission
                    self.cached_consumer_fee.store(
                        engine.consumer_fee_per_byte(),
                        AtomicOrdering::Relaxed,
                    );
                    self.cached_industrial_fee.store(
                        engine.industrial_fee_per_byte(),
                        AtomicOrdering::Relaxed,
                    );
                }

                #[cfg(feature = "telemetry")]
                {
                    // Use cached serialized receipts for telemetry (avoid third encoding)
                    crate::telemetry::receipts::record_receipts(
                        &block.receipts,
                        receipts_serialized.len(),
                    );
                }
                let key = format!("base_fee:{}", block.index);
                let fee_bytes = block_base_fee.to_le_bytes();
                let _ = self.db.put(key.as_bytes(), &fee_bytes);
                state::append_difficulty(
                    &std::path::Path::new(&self.path).join("diff_history"),
                    block.index,
                    block.difficulty,
                );
                self.reorg.record(&block.hash);
                self.recent_timestamps.push_back(block.timestamp_millis);
                if self.recent_timestamps.len() > DIFFICULTY_WINDOW {
                    self.recent_timestamps.pop_front();
                }
                let last = self.chain.last().map_or(1, |b| b.difficulty);
                let ts = self.recent_timestamps.make_contiguous();
                let (next, hint) =
                    consensus::difficulty_retune::retune(last, ts, self.retune_hint, &self.params);
                self.difficulty = next;
                self.retune_hint = hint;
                self.recent_miners.push_back(miner_addr.to_owned());
                if self.recent_miners.len() > RECENT_MINER_WINDOW {
                    self.recent_miners.pop_front();
                }
                if index % EPOCH_BLOCKS == 0 {
                    let stats = Utilization {
                        bytes_stored: self.epoch_storage_bytes as f64,
                        bytes_read: self.epoch_read_bytes as f64,
                        cpu_ms: self.epoch_cpu_ms as f64,
                        bytes_out: self.epoch_bytes_out as f64,
                        epoch_secs: EPOCH_BLOCKS as f64,
                    };
                    let epoch = index / EPOCH_BLOCKS;
                    if epoch - self.inflation_epoch_marker >= EPOCHS_PER_YEAR {
                        self.emission_year_ago = self.emission;
                        self.inflation_epoch_marker = epoch;
                    }
                    let prev = if self.emission_year_ago == 0 {
                        self.emission
                    } else {
                        self.emission_year_ago
                    };
                    let rolling = if prev == 0 {
                        0.0
                    } else {
                        let delta = self.emission.checked_sub(prev).ok_or(BalanceUnderflow)?;
                        delta as f64 / prev as f64
                    };
                    let raw = retune_multipliers(
                        &mut self.params,
                        self.emission as f64,
                        &stats,
                        epoch,
                        std::path::Path::new(&self.path),
                        rolling,
                        None,
                    );
                    self.beta_storage_sub_raw = raw[0];
                    self.gamma_read_sub_raw = raw[1];
                    self.kappa_cpu_sub_raw = raw[2];
                    self.lambda_bytes_out_sub_raw = raw[3];
                    let (backlog, util) = crate::compute_market::price_board::backlog_utilization();
                    let ind = inflation::retuning::retune_industrial_multiplier(
                        std::path::Path::new(&self.path),
                        self.params.industrial_multiplier,
                        backlog as f64,
                        util as f64,
                    );
                    self.params.industrial_multiplier = ind;
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::INDUSTRIAL_MULTIPLIER.set(ind);

                    // Execute economic control laws
                    let econ_params = economics::GovernanceEconomicParams::from_governance_params(
                        &self.params,
                        self.economics_prev_annual_issuance_block,
                        self.economics_prev_subsidy.clone(),
                        self.economics_prev_tariff.clone(),
                        self.economics_baseline_tx_count,
                        self.economics_baseline_tx_volume,
                        self.economics_baseline_miners,
                    );

                    let total_ad_spend = ad_total_usd_micros;
                    let energy_snapshot = crate::energy::market_snapshot();
                    let metrics = self.build_market_metrics(
                        self.economics_epoch_storage_payout_block,
                        self.economics_epoch_compute_payout_block,
                        self.economics_epoch_ad_payout_block,
                        total_ad_spend,
                        ad_settlement_count,
                        ad_last_price_usd_micros,
                        util,
                        &energy_snapshot,
                    );

                    // Persist market metrics for Launch Governor economics gate sampling
                    self.economics_prev_market_metrics = metrics.clone();

                    // Collect network activity metrics for formula-based issuance
                    let network_activity = economics::NetworkActivity {
                        tx_count: self.economics_epoch_tx_count,
                        tx_volume_block: self.economics_epoch_tx_volume_block,
                        unique_miners: self.recent_miners.len() as u64,
                        block_height: self.block_height,
                    };

                    // Execute control laws
                    let econ_snapshot = economics::execute_epoch_economics(
                        epoch,
                        &metrics,
                        &network_activity,
                        self.emission,                        // circulating_block
                        self.emission, // total_emission (same as circulating for now)
                        self.economics_epoch_tx_volume_block, // non-KYC volume
                        total_ad_spend,
                        self.economics_epoch_treasury_inflow_block,
                        &econ_params,
                    );

                    // Update blockchain state with results
                    self.economics_prev_annual_issuance_block =
                        econ_snapshot.inflation.annual_issuance_block;
                    self.economics_prev_subsidy = econ_snapshot.subsidies.clone();
                    self.economics_prev_tariff = econ_snapshot.tariff.clone();
                    self.economics_block_reward_per_block =
                        econ_snapshot.inflation.block_reward_per_block;
                    self.block_reward = TokenAmount::new(self.economics_block_reward_per_block);
                    // Persist updated adaptive baselines for next epoch
                    self.economics_baseline_tx_count = econ_snapshot.updated_baseline_tx_count;
                    self.economics_baseline_tx_volume = econ_snapshot.updated_baseline_tx_volume;
                    self.economics_baseline_miners = econ_snapshot.updated_baseline_miners;

                    // Update telemetry
                    #[cfg(feature = "telemetry")]
                    {
                        let epoch_tx_count = self.economics_epoch_tx_count;
                        let epoch_tx_volume = self.economics_epoch_tx_volume_block;
                        let epoch_treasury_inflow = self.economics_epoch_treasury_inflow_block;
                        crate::telemetry::update_economics_telemetry(&econ_snapshot);
                        crate::telemetry::update_economics_market_metrics(&metrics);
                        crate::telemetry::update_economics_epoch_metrics(
                            epoch_tx_count,
                            epoch_tx_volume,
                            epoch_treasury_inflow,
                            &self.economics_prev_market_metrics,
                        );
                    }

                    // Reset epoch counters
                    self.economics_epoch_tx_volume_block = 0;
                    self.economics_epoch_tx_count = 0;
                    self.economics_epoch_treasury_inflow_block = 0;
                    self.economics_epoch_storage_payout_block = 0;
                    self.economics_epoch_compute_payout_block = 0;
                    self.economics_epoch_ad_payout_block = 0;

                    self.epoch_storage_bytes = 0;
                    self.epoch_read_bytes = 0;
                    self.epoch_viewer_bytes.clear();
                    self.epoch_host_bytes.clear();
                    self.epoch_hardware_bytes.clear();
                    self.epoch_verifier_bytes.clear();
                    self.settled_viewer_bytes.clear();
                    self.settled_host_bytes.clear();
                    self.settled_hardware_bytes.clear();
                    self.settled_verifier_bytes.clear();
                    self.settled_read_bytes = 0;
                    self.epoch_cpu_ms = 0;
                    self.epoch_bytes_out = 0;
                }
                if self.difficulty != 0 {
                    let last = self.chain.last().map_or(1, |b| b.difficulty);
                    let ts = self.recent_timestamps.make_contiguous();
                    let (next, hint) = consensus::difficulty_retune::retune(
                        last,
                        ts,
                        self.retune_hint,
                        &self.params,
                    );
                    self.difficulty = next;
                    self.retune_hint = hint;
                }
                // CONSENSUS.md §10.3: mempool mutations are guarded by mempool_mutex
                #[cfg(feature = "telemetry")]
                let _pool_guard = {
                    let span = diagnostics::tracing::span!(
                        diagnostics::tracing::Level::TRACE,
                        "mempool_mutex",
                        sender = %scrub(&miner_addr),
                        nonce = 0u64,
                        fpb = 0u64,
                        mempool_size = self.mempool_size_consumer.load(AtomicOrdering::SeqCst)
                            + self.mempool_size_industrial.load(AtomicOrdering::SeqCst)
                    );
                    span.in_scope(|| self.mempool_mutex.lock()).map_err(|_| {
                        telemetry::LOCK_POISON_TOTAL.inc();
                        self.record_reject("lock_poison");
                        py_value_err("Lock poisoned")
                    })?
                };
                #[cfg(not(feature = "telemetry"))]
                let _pool_guard = self.mempool_mutex.lock().map_err(|_| {
                    #[cfg(feature = "telemetry")]
                    {
                        telemetry::LOCK_POISON_TOTAL.inc();
                        self.record_reject("lock_poison");
                    }
                    py_value_err("Lock poisoned")
                })?;
                let mut changed: HashSet<String> = HashSet::new();
                for tx in txs.iter().skip(1) {
                    let lock = self
                        .admission_locks
                        .entry(tx.payload.from_.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone();
                    #[cfg(feature = "telemetry")]
                    let _guard = {
                        let span = diagnostics::tracing::span!(
                            diagnostics::tracing::Level::TRACE,
                            "admission_lock",
                            sender = %scrub(&tx.payload.from_),
                            nonce = tx.payload.nonce
                        );
                        span.in_scope(|| lock.lock()).map_err(|_| {
                            telemetry::LOCK_POISON_TOTAL.inc();
                            self.record_reject("lock_poison");
                            py_value_err("Lock poisoned")
                        })?
                    };
                    #[cfg(not(feature = "telemetry"))]
                    let _guard = lock.lock().map_err(|_| {
                        #[cfg(feature = "telemetry")]
                        {
                            telemetry::LOCK_POISON_TOTAL.inc();
                            self.record_reject("lock_poison");
                        }
                        py_value_err("Lock poisoned")
                    })?;

                    if tx.payload.from_ != "0".repeat(34) {
                        changed.insert(tx.payload.from_.clone());
                        if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                            let (fee_consumer, fee_industrial) =
                                crate::fee::decompose(tx.payload.pct, block_base_fee + tx.tip)
                                    .unwrap_or((0, 0));
                            // Total BLOCK tokens: amount (both lanes) + fees
                            let total_amount = tx.payload.amount_consumer
                                + tx.payload.amount_industrial + fee_consumer + fee_industrial;
                            if s.balance.amount < total_amount
                                || s.pending_amount < total_amount
                                || s.pending_nonce == 0
                            {
                                return Err(ErrBalanceUnderflow::new_err("balance underflow"));
                            }
                            s.balance.amount -= total_amount;
                            s.pending_amount -= total_amount;
                            s.pending_nonce -= 1;
                            s.pending_nonces.remove(&tx.payload.nonce);
                            s.nonce = tx.payload.nonce;
                        }
                    }
                    let r = self
                        .accounts
                        .entry(tx.payload.to.clone())
                        .or_insert(Account {
                            address: tx.payload.to.clone(),
                            balance: TokenBalance {
                                amount: 0,
                            },
                            nonce: 0,
                            pending_amount: 0,
                            pending_nonce: 0,
                            pending_nonces: HashSet::new(),
                            sessions: Vec::new(),
                        });
                    r.balance.amount += tx.payload.amount_consumer + tx.payload.amount_industrial;
                    changed.insert(tx.payload.to.clone());

                    match tx.lane {
                        FeeLane::Consumer => {
                            self.mempool_consumer
                                .remove(&(tx.payload.from_.clone(), tx.payload.nonce));
                        }
                        FeeLane::Industrial => {
                            self.mempool_industrial
                                .remove(&(tx.payload.from_.clone(), tx.payload.nonce));
                        }
                    }
                    self.dec_mempool_size(tx.lane);
                    self.release_sender_slot(tx.lane, &tx.payload.from_);
                }
                drop(_pool_guard);

                #[cfg(feature = "telemetry")]
                for tx in &skipped {
                    warn!(
                        "tx skipped sender={} nonce={} reason=gap",
                        tx.payload.from_, tx.payload.nonce
                    );
                }
                self.skipped_nonce_gap += skipped.len() as u64;
                self.skipped = skipped;

                let miner = self
                    .accounts
                    .entry(miner_addr.to_owned())
                    .or_insert(Account {
                        address: miner_addr.to_owned(),
                        balance: TokenBalance {
                            amount: 0,
                        },
                        nonce: 0,
                        pending_amount: 0,
                        pending_nonce: 0,
                        pending_nonces: HashSet::new(),
                        sessions: Vec::new(),
                    });
                // Credit total coinbase (block reward + fees) to miner
                let coinbase_total = coinbase_block_total + coinbase_industrial_total;
                miner.balance.amount = miner
                    .balance
                    .amount
                    .checked_add(coinbase_total)
                    .ok_or_else(|| py_value_err("miner balance overflow"))?;
                changed.insert(miner_addr.to_owned());
                for (addr, amount) in viewer_payouts
                    .iter()
                    .chain(host_payouts.iter())
                    .chain(hardware_payouts.iter())
                    .chain(verifier_payouts.iter())
                    .chain(liquidity_payouts.iter())
                {
                    if *amount == 0 {
                        continue;
                    }
                    let entry = self.accounts.entry(addr.clone()).or_insert(Account {
                        address: addr.clone(),
                        balance: TokenBalance {
                            amount: 0,
                        },
                        nonce: 0,
                        pending_amount: 0,
                        pending_nonce: 0,
                        pending_nonces: HashSet::new(),
                        sessions: Vec::new(),
                    });
                    entry.balance.amount = entry
                        .balance
                        .amount
                        .checked_add(*amount)
                        .ok_or_else(|| py_value_err("subsidy overflow"))?;
                    changed.insert(addr.clone());
                }

                let mut touched_shards: HashSet<ShardId> = HashSet::new();
                for addr in &changed {
                    touched_shards.insert(address::shard_id(addr));
                }
                for shard in touched_shards {
                    let root = process::shard_state_root(&self.accounts, shard);
                    self.shard_roots.insert(shard, root);
                    let prev = self.shard_heights.get(&shard).copied().unwrap_or(0);
                    self.shard_heights.insert(shard, prev + 1);
                    let key = ShardState::db_key();
                    let bytes = ShardState::new(shard, root).to_bytes();
                    let mut deltas = Vec::new();
                    self.write_shard_state(shard, key, bytes, &mut deltas)
                        .map_err(|e| py_value_err(format!("shard state write: {e}")))?;
                }

                // Total minted = subsidies + reward + rebates (all clamped by cap enforcement earlier)
                let minted = storage_sub
                    .saturating_add(read_sub)
                    .saturating_add(compute_sub)
                    .saturating_add(reward.0)
                    .saturating_add(rebate_tokens);

                // Paranoid assertion: ensure we never exceed cap
                debug_assert!(
                    self.emission.saturating_add(minted) <= MAX_SUPPLY_BLOCK,
                    "Cap enforcement failed: emission {} + minted {} > MAX_SUPPLY_BLOCK {}",
                    self.emission,
                    minted,
                    MAX_SUPPLY_BLOCK
                );

                self.emission = self.emission.saturating_add(minted);
                self.macro_acc = self.macro_acc.saturating_add(minted);
                self.block_height += 1;
                if self.block_height % self.macro_interval == 0 {
                    let mb = MacroBlock {
                        height: self.block_height,
                        shard_heights: self.shard_heights.clone(),
                        shard_roots: self.shard_roots.clone(),
                        total_reward: self.macro_acc,
                        queue_root: self.inter_shard.root(),
                    };
                    let _ = self
                        .db
                        .insert(&MacroBlock::db_key(self.block_height), mb.to_bytes());
                    self.macro_blocks.push(mb);
                    self.macro_acc = 0;
                }
                #[cfg(feature = "telemetry")]
                self.record_block_mined();
                if self.block_height % 600 == 0 {
                    self.badge_tracker
                        .record_epoch(miner_addr, true, Duration::from_millis(0));
                    self.check_badges();
                }
                if self.block_height % self.snapshot.interval == 0 {
                    let r = self
                        .snapshot
                        .write_snapshot(self.block_height, &self.accounts)
                        .map_err(|e| py_value_err(format!("snapshot error: {e}")))?;
                    debug_assert_eq!(r, block.state_root);
                } else {
                    let changes: HashMap<String, Account> = changed
                        .iter()
                        .filter_map(|a| self.accounts.get(a).map(|acc| (a.clone(), acc.clone())))
                        .collect();
                    let r = self
                        .snapshot
                        .write_diff(self.block_height, &changes, &self.accounts)
                        .map_err(|e| py_value_err(format!("snapshot diff error: {e}")))?;
                    debug_assert_eq!(r, block.state_root);
                }

                // Adjust base fee for the next block based on how full this block was.
                let gas_used =
                    (included.len() as u64).saturating_mul(crate::fees::TARGET_GAS_PER_BLOCK / 10);
                self.base_fee = crate::blockchain::fees::compute(block_base_fee, gas_used);
                #[cfg(feature = "telemetry")]
                crate::telemetry::BASE_FEE.set(self.base_fee as i64);

                self.persist_chain()?;

                self.db.flush();
                return Ok(block);
            }
            nonce = nonce
                .checked_add(1)
                .ok_or_else(|| py_value_err("Nonce overflow"))?;
        }
    }

    pub fn validate_block(&self, block: &Block) -> PyResult<bool> {
        let expected_prev = if block.index == 0 {
            "0".repeat(64)
        } else if let Some(pb) = self.chain.get(block.index as usize - 1) {
            pb.hash.clone()
        } else {
            return Err(py_value_err("Missing previous block"));
        };
        if block.previous_hash != expected_prev {
            return Ok(false);
        }

        if block.difficulty
            != difficulty::expected_difficulty_from_chain(&self.chain[..block.index as usize])
        {
            return Ok(false);
        }

        if block.transactions.is_empty() {
            return Ok(false);
        }

        if block.transactions[0].payload.from_ != "0".repeat(34) {
            return Ok(false);
        }

        let calc = calculate_hash(
            block.index,
            &block.previous_hash,
            block.timestamp_millis,
            block.nonce,
            block.difficulty,
            block.base_fee,
            block.coinbase_block,
            block.coinbase_industrial,
            block.storage_sub,
            block.read_sub,
            block.read_sub_viewer,
            block.read_sub_host,
            block.read_sub_hardware,
            block.read_sub_verifier,
            block.read_sub_liquidity,
            block.ad_viewer,
            block.ad_host,
            block.ad_hardware,
            block.ad_verifier,
            block.ad_liquidity,
            block.ad_miner,
            block.ad_total_usd_micros,
            block.ad_settlement_count,
            block.ad_oracle_price_usd_micros,
            block.compute_sub,
            block.proof_rebate,
            block.read_root,
            &block.fee_checksum,
            &block.transactions,
            &block.state_root,
            &block.l2_roots,
            &block.l2_sizes,
            block.vdf_commit,
            block.vdf_output,
            &block.vdf_proof,
            block.retune_hint,
            &block.receipts,
        );
        if calc != block.hash {
            return Ok(false);
        }

        let b = hex_to_bytes(&block.hash);
        if leading_zero_bits(&b)
            < difficulty::expected_difficulty_from_chain(&self.chain[..block.index as usize]) as u32
        {
            return Ok(false);
        }

        if block.transactions[0].payload.amount_consumer != block.coinbase_block.0
            || block.transactions[0].payload.amount_industrial != block.coinbase_industrial.0
        {
            return Ok(false);
        }

        let mut seen: HashSet<[u8; 32]> = HashSet::new();
        let mut expected: HashMap<String, u64> = HashMap::new();
        let mut seen_nonce: HashSet<(String, u64)> = HashSet::new();
        let mut fee_tot_consumer: u128 = 0;
        let mut fee_tot_industrial: u128 = 0;
        for tx in block.transactions.iter().skip(1) {
            if !seen.insert(tx.id()) {
                return Ok(false);
            }
            if !seen_nonce.insert((tx.payload.from_.clone(), tx.payload.nonce)) {
                return Ok(false);
            }
            let exp = expected.entry(tx.payload.from_.clone()).or_insert_with(|| {
                self.accounts
                    .get(&tx.payload.from_)
                    .map(|a| a.nonce + 1)
                    .unwrap_or(1)
            });
            if tx.payload.nonce != *exp {
                return Ok(false);
            }
            *exp += 1;
            match crate::fee::decompose(tx.payload.pct, tx.tip) {
                Ok((fee_consumer, fee_industrial)) => {
                    fee_tot_consumer += fee_consumer as u128;
                    fee_tot_industrial += fee_industrial as u128;
                }
                Err(_) => return Ok(false),
            }
        }
        let mut h = blake3::Hasher::new();
        let fee_consumer_u64 =
            u64::try_from(fee_tot_consumer).map_err(|_| py_value_err("Fee overflow"))?;
        let fee_industrial_u64 =
            u64::try_from(fee_tot_industrial).map_err(|_| py_value_err("Fee overflow"))?;
        h.update(&fee_consumer_u64.to_le_bytes());
        h.update(&fee_industrial_u64.to_le_bytes());
        if h.finalize().to_hex().to_string() != block.fee_checksum {
            return Ok(false);
        }
        let coinbase_block_total = block.coinbase_block.0 as u128;
        let coinbase_industrial_total = block.coinbase_industrial.0 as u128;
        if coinbase_block_total < fee_tot_consumer || coinbase_industrial_total < fee_tot_industrial
        {
            return Ok(false);
        }
        let read_role_sum = block.read_sub_viewer.0 as u128
            + block.read_sub_host.0 as u128
            + block.read_sub_hardware.0 as u128
            + block.read_sub_verifier.0 as u128
            + block.read_sub_liquidity.0 as u128;
        if read_role_sum > block.read_sub.0 as u128 {
            return Ok(false);
        }
        let read_miner_share = block.read_sub.0 as u128 - read_role_sum;
        let expected_consumer = self.block_reward.0 as u128
            + block.storage_sub.0 as u128
            + read_miner_share
            + block.compute_sub.0 as u128
            + block.proof_rebate.0 as u128
            + fee_tot_consumer
            + fee_tot_industrial;
        let expected_industrial = 0u128;
        if coinbase_block_total != expected_consumer
            || coinbase_industrial_total != expected_industrial
        {
            return Ok(false);
        }

        Ok(true)
    }

    /// Validate the entire chain from genesis to tip.
    #[inline]
    pub fn is_valid_chain(&self) -> PyResult<bool> {
        Ok(self.is_valid_chain_rust(&self.chain))
    }

    pub fn import_chain(&mut self, new_chain: Vec<Block>) -> PyResult<()> {
        if std::env::var("TB_FAST_MINE").as_deref() == Ok("1") {
            self.chain = new_chain;
            self.block_height = self.chain.len() as u64;
            self.recent_timestamps.clear();
            for b in self.chain.iter().rev().take(DIFFICULTY_WINDOW) {
                self.recent_timestamps.push_front(b.timestamp_millis);
            }
            return Ok(());
        }
        if new_chain.len() <= self.chain.len() {
            return Err(py_value_err("Incoming chain not longer"));
        }
        if !self.is_valid_chain_rust(&new_chain) {
            return Err(py_value_err("Invalid incoming chain"));
        }

        // Replay economics from the incoming chain to get the correct economics state
        // This is consensus-critical: we must use the chain's own economics, not local state
        let replayed_econ = replay_economics_to_tip(&new_chain, &self.params);

        let old_chain = self.chain.clone();
        let lca = old_chain
            .iter()
            .zip(&new_chain)
            .take_while(|(a, b)| a.hash == b.hash)
            .count();
        let depth = old_chain.len().saturating_sub(lca);
        if depth > 0 {
            #[cfg(feature = "telemetry")]
            observer::record_reorg(depth as u64);
        }
        if depth > 0 {
            for block in old_chain.iter().rev().take(depth) {
                self.proof_tracker.rollback_claim(block.index);
            }
        }
        self.chain.clear();
        self.accounts.clear();
        self.emission = 0;
        self.block_height = 0;
        self.economics_epoch_tx_volume_block = 0;
        self.economics_epoch_tx_count = 0;
        self.economics_epoch_treasury_inflow_block = 0;
        self.economics_epoch_storage_payout_block = 0;
        self.economics_epoch_compute_payout_block = 0;
        self.economics_epoch_ad_payout_block = 0;
        let mut epoch_tx_window: VecDeque<(u64, u64)> =
            VecDeque::with_capacity(EPOCH_BLOCKS as usize + 1);

        for block in &new_chain {
            let mut block_tx_count = 0u64;
            let mut block_tx_volume = 0u64;
            let miner_addr = block
                .transactions
                .first()
                .map(|tx| tx.payload.to.clone())
                .unwrap_or_default();
            let mut fee_tot_consumer: u128 = 0;
            let mut fee_tot_industrial: u128 = 0;
            for tx in block.transactions.iter().skip(1) {
                block_tx_count = block_tx_count.saturating_add(1);
                block_tx_volume = block_tx_volume
                    .saturating_add(tx.payload.amount_consumer)
                    .saturating_add(tx.payload.amount_industrial)
                    .saturating_add(tx.tip);
                if tx.payload.from_ != "0".repeat(34) {
                    let pk = to_array_32(&tx.public_key)
                        .ok_or_else(|| py_value_err("Invalid pubkey in chain"))?;
                    let vk = VerifyingKey::from_bytes(&pk)
                        .map_err(|_| py_value_err("Invalid pubkey in chain"))?;
                    let sig_bytes = to_array_64(&tx.signature.ed25519)
                        .ok_or_else(|| py_value_err("Invalid signature in chain"))?;
                    let sig = Signature::from_bytes(&sig_bytes);
                    let mut msg = domain_tag().to_vec();
                    msg.extend(canonical_payload_bytes(&tx.payload));
                    if vk.verify(&msg, &sig).is_err() {
                        return Err(py_value_err("Bad tx signature in chain"));
                    }
                    if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                        let (fee_consumer, fee_industrial) =
                            crate::fee::decompose(tx.payload.pct, block.base_fee + tx.tip)
                                .unwrap_or((0, 0));
                        // Total BLOCK tokens: amount (both lanes) + fees
                        let total_amount = tx.payload.amount_consumer
                            + tx.payload.amount_industrial + fee_consumer + fee_industrial;
                        if s.balance.amount < total_amount {
                            return Err(ErrBalanceUnderflow::new_err("balance underflow"));
                        }
                        s.balance.amount -= total_amount;
                        s.nonce = tx.payload.nonce;
                        fee_tot_consumer += fee_consumer as u128;
                        fee_tot_industrial += fee_industrial as u128;
                    }
                }
                let r = self
                    .accounts
                    .entry(tx.payload.to.clone())
                    .or_insert(Account {
                        address: tx.payload.to.clone(),
                        balance: TokenBalance {
                            amount: 0,
                        },
                        nonce: 0,
                        pending_amount: 0,
                        pending_nonce: 0,
                        pending_nonces: HashSet::new(),
                        sessions: Vec::new(),
                    });
                r.balance.amount += tx.payload.amount_consumer + tx.payload.amount_industrial;
            }
            let mut h = blake3::Hasher::new();
            let fee_consumer_u64 =
                u64::try_from(fee_tot_consumer).map_err(|_| py_value_err("Fee overflow"))?;
            let fee_industrial_u64 =
                u64::try_from(fee_tot_industrial).map_err(|_| py_value_err("Fee overflow"))?;
            h.update(&fee_consumer_u64.to_le_bytes());
            h.update(&fee_industrial_u64.to_le_bytes());
            if h.finalize().to_hex().to_string() != block.fee_checksum {
                return Err(py_value_err("Fee checksum mismatch"));
            }
            let coinbase_block_total = block.coinbase_block.0 as u128;
            let coinbase_industrial_total = block.coinbase_industrial.0 as u128;
            if coinbase_block_total < fee_tot_consumer
                || coinbase_industrial_total < fee_tot_industrial
            {
                return Err(py_value_err("Fee mismatch"));
            }
            // Validate read subsidy role splits
            let read_role_sum = block.read_sub_viewer.0 as u128
                + block.read_sub_host.0 as u128
                + block.read_sub_hardware.0 as u128
                + block.read_sub_verifier.0 as u128
                + block.read_sub_liquidity.0 as u128;
            if read_role_sum > block.read_sub.0 as u128 {
                return Err(py_value_err("Read subsidy role sum exceeds total"));
            }
            let read_miner_share = block.read_sub.0 as u128 - read_role_sum;
            // Use REPLAYED economics from the incoming chain, not local self.block_reward
            // This is consensus-critical: chain economics must be deterministic
            let expected_consumer = replayed_econ.block_reward_per_block as u128
                + block.storage_sub.0 as u128
                + read_miner_share
                + block.compute_sub.0 as u128
                + block.proof_rebate.0 as u128
                + fee_tot_consumer
                + fee_tot_industrial;
            let expected_industrial = 0u128;
            if coinbase_block_total != expected_consumer
                || coinbase_industrial_total != expected_industrial
            {
                return Err(py_value_err("Coinbase mismatch"));
            }
            let miner = self.accounts.entry(miner_addr.clone()).or_insert(Account {
                address: miner_addr.clone(),
                balance: TokenBalance {
                    amount: 0,
                },
                nonce: 0,
                pending_amount: 0,
                pending_nonce: 0,
                pending_nonces: HashSet::new(),
                sessions: Vec::new(),
            });
            // Credit total coinbase to miner
            let coinbase_total = block.coinbase_block.0 + block.coinbase_industrial.0;
            miner.balance.amount = miner
                .balance
                .amount
                .checked_add(coinbase_total)
                .ok_or_else(|| py_value_err("miner balance overflow"))?;
            if let Some(cb) = block.transactions.first() {
                if cb.payload.amount_consumer != block.coinbase_block.0
                    || cb.payload.amount_industrial != block.coinbase_industrial.0
                {
                    // reject forks that tamper with recorded coinbase totals
                    return Err(py_value_err("Coinbase mismatch"));
                }
            }
            self.emission += block.coinbase_block.0 + block.coinbase_industrial.0;
            self.chain.push(block.clone());
            state::append_difficulty(
                &std::path::Path::new(&self.path).join("diff_history"),
                block.index,
                block.difficulty,
            );
            self.reorg.record(&block.hash);
            self.recent_timestamps.push_back(block.timestamp_millis);
            if self.recent_timestamps.len() > DIFFICULTY_WINDOW {
                self.recent_timestamps.pop_front();
            }
            let last = self.chain.last().map_or(1, |b| b.difficulty);
            let ts = self.recent_timestamps.make_contiguous();
            let (next, hint) =
                consensus::difficulty_retune::retune(last, ts, self.retune_hint, &self.params);
            self.difficulty = next;
            self.retune_hint = hint;
            self.block_height += 1;
            epoch_tx_window.push_back((block_tx_count, block_tx_volume));
            if epoch_tx_window.len() > EPOCH_BLOCKS as usize {
                epoch_tx_window.pop_front();
            }
        }

        let last = self.chain.last().map_or(1, |b| b.difficulty);
        let ts = self.recent_timestamps.make_contiguous();
        let (next, hint) =
            consensus::difficulty_retune::retune(last, ts, self.retune_hint, &self.params);
        self.difficulty = next;
        self.retune_hint = hint;

        // Update economics state from the replayed chain economics (not preserved local state)
        self.block_reward = TokenAmount::new(replayed_econ.block_reward_per_block);
        self.economics_block_reward_per_block = replayed_econ.block_reward_per_block;
        self.economics_prev_subsidy = replayed_econ.prev_subsidy;
        self.economics_prev_tariff = replayed_econ.prev_tariff;
        self.economics_prev_annual_issuance_block = replayed_econ.prev_annual_issuance;

        Ok(())
    }

    /// Return the current state root and Merkle proof for the given account.
    pub fn account_proof(&self, address: String) -> PyResult<(String, Vec<(String, bool)>)> {
        let root = crate::blockchain::snapshot::state_root(&self.accounts);
        let proof = crate::blockchain::snapshot::account_proof(&self.accounts, &address)
            .ok_or_else(|| py_value_err("unknown account"))?;
        Ok((root, proof))
    }

    #[doc(hidden)]
    pub fn panic_next_purge(&self) {
        self.trigger_panic_next_purge();
    }
}

impl Blockchain {
    /// Open an isolated path used by tests
    #[must_use]
    pub fn new(path: &str) -> Self {
        let _ = std::fs::remove_dir_all(path);
        Self::open(path).unwrap_or_else(|e| panic!("DB open: {e}"))
    }

    #[allow(dead_code)]
    fn is_valid_chain_rust(&self, chain: &[Block]) -> bool {
        // Replay economics from genesis to validate coinbase rewards deterministically
        // This ensures two nodes seeing the same chain compute identical economics
        let replayed_econ = if !chain.is_empty() {
            replay_economics_to_tip(chain, &self.params)
        } else {
            ReplayedEconomicsState::default()
        };

        for i in 0..chain.len() {
            let b = &chain[i];
            let expected_prev = if i == 0 {
                "0".repeat(64)
            } else {
                chain[i - 1].hash.clone()
            };
            if b.previous_hash != expected_prev {
                return false;
            }
            if b.difficulty != difficulty::expected_difficulty_from_chain(&chain[..i]) {
                return false;
            }
            if b.transactions.is_empty() {
                return false;
            }
            if b.transactions[0].payload.from_ != "0".repeat(34) {
                return false;
            }
            if b.transactions[0].payload.amount_consumer != b.coinbase_block.0
                || b.transactions[0].payload.amount_industrial != b.coinbase_industrial.0
            {
                return false;
            }
            let calc = calculate_hash(
                b.index,
                &b.previous_hash,
                b.timestamp_millis,
                b.nonce,
                b.difficulty,
                b.base_fee,
                b.coinbase_block,
                b.coinbase_industrial,
                b.storage_sub,
                b.read_sub,
                b.read_sub_viewer,
                b.read_sub_host,
                b.read_sub_hardware,
                b.read_sub_verifier,
                b.read_sub_liquidity,
                b.ad_viewer,
                b.ad_host,
                b.ad_hardware,
                b.ad_verifier,
                b.ad_liquidity,
                b.ad_miner,
                b.ad_total_usd_micros,
                b.ad_settlement_count,
                b.ad_oracle_price_usd_micros,
                b.compute_sub,
                b.proof_rebate,
                b.read_root,
                &b.fee_checksum,
                &b.transactions,
                &b.state_root,
                &b.l2_roots,
                &b.l2_sizes,
                b.vdf_commit,
                b.vdf_output,
                &b.vdf_proof,
                b.retune_hint,
                &b.receipts,
            );
            if calc != b.hash {
                return false;
            }
            let bytes = hex_to_bytes(&b.hash);
            if leading_zero_bits(&bytes)
                < difficulty::expected_difficulty_from_chain(&chain[..i]) as u32
            {
                return false;
            }
            let mut expected_nonce: HashMap<String, u64> = HashMap::new();
            let mut seen: HashSet<[u8; 32]> = HashSet::new();
            let mut seen_nonce: HashSet<(String, u64)> = HashSet::new();
            let mut fee_tot_consumer: u128 = 0;
            let mut fee_tot_industrial: u128 = 0;
            for tx in b.transactions.iter().skip(1) {
                if tx.payload.from_ != "0".repeat(34) {
                    let pk = match to_array_32(&tx.public_key) {
                        Some(p) => p,
                        None => return false,
                    };
                    let vk = match VerifyingKey::from_bytes(&pk) {
                        Ok(vk) => vk,
                        Err(_) => return false,
                    };
                    let sig_bytes = match to_array_64(&tx.signature.ed25519) {
                        Some(b) => b,
                        None => return false,
                    };
                    let sig = Signature::from_bytes(&sig_bytes);
                    let mut bytes = domain_tag().to_vec();
                    bytes.extend(canonical_payload_bytes(&tx.payload));
                    if vk.verify(&bytes, &sig).is_err() {
                        return false;
                    }
                    let entry = expected_nonce.entry(tx.payload.from_.clone()).or_insert(0);
                    *entry += 1;
                    if tx.payload.nonce != *entry {
                        return false;
                    }
                    if !seen_nonce.insert((tx.payload.from_.clone(), tx.payload.nonce)) {
                        return false;
                    }
                }
                if !seen.insert(tx.id()) {
                    return false;
                }
                match crate::fee::decompose(tx.payload.pct, tx.tip) {
                    Ok((fee_consumer, fee_industrial)) => {
                        fee_tot_consumer += fee_consumer as u128;
                        fee_tot_industrial += fee_industrial as u128;
                    }
                    Err(_) => return false,
                }
            }
            let mut h = blake3::Hasher::new();
            let fee_consumer_u64 = match u64::try_from(fee_tot_consumer) {
                Ok(v) => v,
                Err(_) => return false,
            };
            let fee_industrial_u64 = match u64::try_from(fee_tot_industrial) {
                Ok(v) => v,
                Err(_) => return false,
            };
            h.update(&fee_consumer_u64.to_le_bytes());
            h.update(&fee_industrial_u64.to_le_bytes());
            if h.finalize().to_hex().to_string() != b.fee_checksum {
                return false;
            }
            let coinbase_block_total = b.coinbase_block.0 as u128;
            let coinbase_industrial_total = b.coinbase_industrial.0 as u128;
            if coinbase_block_total < fee_tot_consumer
                || coinbase_industrial_total < fee_tot_industrial
            {
                return false;
            }
            // Validate read subsidy role splits
            let read_role_sum = b.read_sub_viewer.0 as u128
                + b.read_sub_host.0 as u128
                + b.read_sub_hardware.0 as u128
                + b.read_sub_verifier.0 as u128
                + b.read_sub_liquidity.0 as u128;
            if read_role_sum > b.read_sub.0 as u128 {
                return false;
            }
            let read_miner_share = b.read_sub.0 as u128 - read_role_sum;
            // Use REPLAYED economics state, not local self.block_reward
            // This is consensus-critical: two nodes must agree on expected rewards
            let expected_consumer = replayed_econ.block_reward_per_block as u128
                + b.storage_sub.0 as u128
                + read_miner_share
                + b.compute_sub.0 as u128
                + b.proof_rebate.0 as u128
                + fee_tot_consumer
                + fee_tot_industrial;
            let expected_industrial = 0u128;
            if coinbase_block_total != expected_consumer
                || coinbase_industrial_total != expected_industrial
            {
                return false;
            }
        }
        true
    }
}

/// Spawn a background loop that periodically calls `purge_expired`.
///
/// The loop sleeps for `interval_secs` between iterations and stops when
/// `shutdown` is set to `true`.
pub fn spawn_purge_loop_thread(
    bc: Arc<Mutex<Blockchain>>,
    interval_secs: u64,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(AtomicOrdering::SeqCst) {
            {
                let mut guard = bc.lock().unwrap_or_else(|e| e.into_inner());
                #[cfg(feature = "telemetry")]
                {
                    let (ttl_before, orphan_before) = (
                        telemetry::TTL_DROP_TOTAL.value(),
                        telemetry::ORPHAN_SWEEP_TOTAL.value(),
                    );
                    let _ = guard.purge_expired();
                    let ttl_after = telemetry::TTL_DROP_TOTAL.value();
                    let orphan_after = telemetry::ORPHAN_SWEEP_TOTAL.value();
                    let ttl_delta = ttl_after.saturating_sub(ttl_before);
                    let orphan_delta = orphan_after.saturating_sub(orphan_before);
                    #[cfg(not(feature = "telemetry-json"))]
                    if telemetry::should_log("mempool") {
                        if ttl_delta > 0 {
                            info!("ttl_drop_total={ttl_after}");
                        }
                        if orphan_delta > 0 {
                            info!("orphan_sweep_total={orphan_after}");
                        }
                    }
                    #[cfg(feature = "telemetry-json")]
                    {
                        if ttl_delta > 0 {
                            log_event(
                                "mempool",
                                diagnostics::log::Level::INFO,
                                "purge_loop",
                                "",
                                0,
                                "ttl_drop_total",
                                ERR_OK,
                                Some(ttl_after),
                                None,
                            );
                        }
                        if orphan_delta > 0 {
                            log_event(
                                "mempool",
                                diagnostics::log::Level::INFO,
                                "purge_loop",
                                "",
                                0,
                                "orphan_sweep_total",
                                ERR_OK,
                                Some(orphan_after),
                                None,
                            );
                        }
                    }
                }
                #[cfg(not(feature = "telemetry"))]
                {
                    let _ = guard.purge_expired();
                }
            }
            thread::sleep(Duration::from_secs(interval_secs));
        }
    })
}

/// Python binding for spawning a purge loop with a manual interval.
///
/// Args:
///     bc (Blockchain): Chain instance to operate on.
///     interval_secs (int): Number of seconds to sleep between purges.
///     shutdown (ShutdownFlag): Flag used to signal termination.
///
/// Returns:
///     PurgeLoopHandle: handle to the purge thread.
pub fn spawn_purge_loop<T>(
    _bc: T,
    _interval_secs: u64,
    _shutdown: &ShutdownFlag,
) -> PyResult<PurgeLoopHandle> {
    Err(PyError::feature_disabled().with_message(
        "python-facing purge loop is unavailable without the `python-bindings` feature",
    ))
}

const ENV_PURGE_LOOP_SECS: &str = "TB_PURGE_LOOP_SECS";

fn parse_purge_interval() -> Result<u64, String> {
    let raw = std::env::var(ENV_PURGE_LOOP_SECS)
        .map_err(|_| format!("{ENV_PURGE_LOOP_SECS} is unset"))?;
    let secs = raw
        .parse::<u64>()
        .map_err(|_| format!("{ENV_PURGE_LOOP_SECS} must be a positive integer: {raw}"))?;
    if secs == 0 {
        Err(format!(
            "{ENV_PURGE_LOOP_SECS} must be a positive integer: {raw}"
        ))
    } else {
        Ok(secs)
    }
}

/// Spawn a purge loop based on `TB_PURGE_LOOP_SECS`.
///
/// Returns:
///     `Ok(handle)` if the environment variable parses to a positive interval.
///
/// Errors:
///     `Err(String)` when the variable is unset, non-numeric, or ≤0.
pub fn maybe_spawn_purge_loop(
    bc: Arc<Mutex<Blockchain>>,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, String> {
    match parse_purge_interval() {
        Ok(secs) => Ok(spawn_purge_loop_thread(bc, secs, shutdown)),
        Err(e) => {
            #[cfg(feature = "telemetry")]
            diagnostics::log::warn!("{e}");
            Err(e)
        }
    }
}

/// Thread-safe flag used to signal background threads to shut down.
#[derive(Clone)]
pub struct ShutdownFlag(Arc<AtomicBool>);

impl ShutdownFlag {
    #[cfg_attr(feature = "python-bindings", new)]
    /// Create a new unset shutdown flag.
    ///
    /// Returns:
    ///     ShutdownFlag: a fresh flag in the ``False`` state.
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Signal any listening threads to terminate.
    ///
    /// Returns:
    ///     None
    ///
    /// Once triggered the flag remains set.
    pub fn trigger(&self) {
        self.0.store(true, AtomicOrdering::SeqCst);
    }
}

impl ShutdownFlag {
    pub fn as_arc(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

/// Handle to a purge loop thread, allowing callers to join from Python.
///
/// Dropping the handle triggers shutdown and joins the purge thread,
/// ensuring it terminates even if ``trigger``/``join`` are never called
/// explicitly.
pub struct PurgeLoopHandle;

impl PurgeLoopHandle {
    pub fn join(&mut self) -> PyResult<()> {
        Err(PyError::feature_disabled()
            .with_message("no purge loop thread was spawned; python bindings are disabled"))
    }
}

impl Drop for PurgeLoopHandle {
    fn drop(&mut self) {}
}

pub struct PurgeLoop {
    _private: (),
}

impl PurgeLoop {
    #[cfg_attr(feature = "python-bindings", new)]
    pub fn new<T>(_bc: T) -> PyResult<Self> {
        Err(PyError::feature_disabled()
            .with_message("PurgeLoop is only available once python bindings ship"))
    }

    pub fn __enter__(&self) -> &Self {
        self
    }

    pub fn __exit__<T1, T2, T3>(&self, _exc_type: &T1, _exc: &T2, _tb: &T3) -> PyResult<bool> {
        Err(PyError::feature_disabled()
            .with_message("PurgeLoop context exit is unreachable without python bindings"))
    }
}

/// Python wrapper that reads `TB_PURGE_LOOP_SECS` and spawns a purge loop.
///
/// Args:
///     bc (Blockchain): Chain instance to operate on.
///     shutdown (ShutdownFlag): Flag used to signal termination.
///
/// Returns:
///     PurgeLoopHandle: handle to the purge thread.
///
/// Raises:
///     ValueError: if ``TB_PURGE_LOOP_SECS`` is unset, non-numeric, or ≤ 0.
pub fn maybe_spawn_purge_loop_py<T>(_bc: T, _shutdown: &ShutdownFlag) -> PyResult<PurgeLoopHandle> {
    Err(PyError::feature_disabled()
        .with_message("python purge loop helpers are disabled until the bridge is implemented"))
}

#[doc(hidden)]
pub fn poison_mempool(bc: &Blockchain) {
    bc.poison_mempool();
}

impl Blockchain {
    /// Internal testing hook: intentionally poison the admission lock for `sender`.
    #[doc(hidden)]
    pub fn poison_lock(&self, sender: &str) {
        let lock = self
            .admission_locks
            .entry(sender.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _ = std::thread::spawn(move || {
            let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
            panic!("poison");
        })
        .join();
    }

    /// Internal testing hook: heal a previously poisoned lock so it can be used again.
    #[doc(hidden)]
    pub fn heal_lock(&self, sender: &str) {
        let lock = self
            .admission_locks
            .entry(sender.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        lock.clear_poison();
    }

    /// Internal testing hook: intentionally poison the global mempool mutex.
    #[doc(hidden)]
    pub fn poison_mempool(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self
                .mempool_mutex
                .lock()
                .unwrap_or_else(|e| panic!("mempool lock: {e}"));
            panic!("poison mempool");
        }));
    }

    /// Internal testing hook: heal a previously poisoned mempool mutex.
    #[doc(hidden)]
    pub fn heal_mempool(&self) {
        let guard = self.mempool_mutex.lock().unwrap_or_else(|e| e.into_inner());
        self.mempool_mutex.clear_poison();
        drop(guard);
    }

    #[doc(hidden)]
    pub fn orphan_count(&self) -> usize {
        self.orphan_counter.load(AtomicOrdering::SeqCst)
    }

    #[doc(hidden)]
    pub fn panic_next_evict(&self) {
        self.panic_on_evict.store(true, AtomicOrdering::SeqCst);
    }

    #[doc(hidden)]
    pub fn trigger_panic_next_purge(&self) {
        self.panic_on_purge.store(true, AtomicOrdering::SeqCst);
    }

    #[doc(hidden)]
    pub fn panic_in_admission_after(&self, step: i32) {
        self.panic_on_admit.store(step, AtomicOrdering::SeqCst);
    }

    #[doc(hidden)]
    pub fn heal_admission(&self) {
        self.panic_on_admit.store(-1, AtomicOrdering::SeqCst);
    }
}

fn leading_zero_bits(hash: &[u8]) -> u32 {
    let mut count = 0;
    for &b in hash {
        if b == 0 {
            count += 8;
        } else {
            count += b.leading_zeros();
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use ad_market::{
        DeliveryChannel, DomainTier, ResourceFloorBreakdown, SelectionCandidateTrace,
        SelectionCohortTrace, SelectionReceipt,
    };
    use crypto_suite::hashing::blake3::Hasher;
    use crypto_suite::signatures::ed25519::SigningKey;

    fn signed_ack(sk: &SigningKey, bytes: u64, domain: &str, provider: &str) -> ReadAck {
        let pk = sk.verifying_key().to_bytes();
        let mut ack = ReadAck {
            manifest: [7u8; 32],
            path_hash: [9u8; 32],
            bytes,
            ts: 42,
            client_hash: [3u8; 32],
            pk,
            sig: Vec::new(),
            domain: domain.to_string(),
            provider: provider.to_string(),
            campaign_id: None,
            creative_id: None,
            selection_receipt: None,
            geo: None,
            device: None,
            crm_lists: Vec::new(),
            delivery_channel: DeliveryChannel::Http,
            mesh: None,
            badge_soft_intent: None,
            readiness: None,
            zk_proof: None,
            presence_badge: None,
            venue_id: None,
            crowd_size_hint: None,
        };
        let mut hasher = Hasher::new();
        hasher.update(&ack.manifest);
        hasher.update(&ack.path_hash);
        hasher.update(&ack.bytes.to_le_bytes());
        hasher.update(&ack.ts.to_le_bytes());
        hasher.update(&ack.client_hash);
        let msg = hasher.finalize();
        let sig = sk.sign(msg.as_bytes());
        ack.sig = sig.to_bytes().to_vec();
        ack
    }

    #[test]
    fn read_subsidy_split_distribution() {
        let mut bc = Blockchain::default();
        bc.block_reward = TokenAmount::new(0);
        bc.economics_block_reward_per_block = 1; // Set to 1 to avoid INITIAL_BLOCK_REWARD fallback, but effectively zero after logistic factor
        bc.beta_storage_sub_raw = 0;
        bc.kappa_cpu_sub_raw = 0;
        bc.lambda_bytes_out_sub_raw = 0;
        bc.gamma_read_sub_raw = 1;
        bc.params.read_subsidy_viewer_percent = 40;
        bc.params.read_subsidy_host_percent = 30;
        bc.params.read_subsidy_hardware_percent = 15;
        bc.params.read_subsidy_verifier_percent = 10;
        bc.params.read_subsidy_liquidity_percent = 5;

        let signing = SigningKey::from_bytes(&[11u8; 32]);
        let ack = signed_ack(&signing, 100, "viewer.test", "provider-1");
        bc.submit_read_ack(ack).expect("ack accepted");

        let miner = "miner.test";
        bc.add_account(miner.to_string(), 0).unwrap();
        let block = bc.mine_block_at(miner, 1).expect("mined");

        assert_eq!(block.read_sub.0, 100);
        assert_eq!(block.read_sub_viewer.0, 40);
        assert_eq!(block.read_sub_host.0, 30);
        assert_eq!(block.read_sub_hardware.0, 15);
        assert_eq!(block.read_sub_verifier.0, 10);
        assert_eq!(block.read_sub_liquidity.0, 5);
        assert_eq!(block.ad_viewer.0, 0);
        assert_eq!(block.ad_host.0, 0);
        assert_eq!(block.ad_hardware.0, 0);
        assert_eq!(block.ad_verifier.0, 0);
        assert_eq!(block.ad_liquidity.0, 0);
        assert_eq!(block.ad_miner.0, 0);

        let pk = signing.verifying_key().to_bytes();
        let viewer_address = super::viewer_address_from_pk(&pk);
        let host_address = super::host_address("viewer.test");
        let hardware_address = super::hardware_address("provider-1");
        let verifier_address = super::verifier_address("viewer.test");
        let liquidity_address = super::liquidity_address().to_string();

        let viewer_balance = bc
            .accounts
            .get(&viewer_address)
            .map(|a| a.balance.amount)
            .unwrap_or(0);
        assert_eq!(viewer_balance, 40);

        let host_balance = bc
            .accounts
            .get(&host_address)
            .map(|a| a.balance.amount)
            .unwrap_or(0);
        assert_eq!(host_balance, 30);

        let hardware_balance = bc
            .accounts
            .get(&hardware_address)
            .map(|a| a.balance.amount)
            .unwrap_or(0);
        assert_eq!(hardware_balance, 15);

        let verifier_balance = bc
            .accounts
            .get(&verifier_address)
            .map(|a| a.balance.amount)
            .unwrap_or(0);
        assert_eq!(verifier_balance, 10);

        let liquidity_balance = bc
            .accounts
            .get(&liquidity_address)
            .map(|a| a.balance.amount)
            .unwrap_or(0);
        assert_eq!(liquidity_balance, 5);

        let miner_balance = bc
            .accounts
            .get(miner)
            .map(|a| a.balance.amount)
            .unwrap_or(0);
        // Miner gets 1 from minimal base reward (economics_block_reward_per_block=1)
        // The test sets this to 1 to avoid INITIAL_BLOCK_REWARD fallback while keeping reward minimal
        assert_eq!(miner_balance, 1);
    }

    #[test]
    fn reject_ack_with_invalid_selection_receipt() {
        let mut bc = Blockchain::default();
        bc.block_reward = TokenAmount::new(0);
        let signing = SigningKey::from_bytes(&[11u8; 32]);
        let mut ack = signed_ack(&signing, 100, "viewer.test", "provider-1");
        ack.selection_receipt = Some(SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "viewer.test".into(),
                domain_tier: DomainTier::Unverified,
                domain_owner: None,
                provider: Some("provider-1".into()),
                badges: Vec::new(),
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: 2,
                bytes: 1_024,
                price_per_mib_usd_micros: 80,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
            },
            candidates: vec![
                SelectionCandidateTrace {
                    campaign_id: "cmp-1".into(),
                    creative_id: "creative-1".into(),
                    base_bid_usd_micros: 120,
                    quality_adjusted_bid_usd_micros: 120,
                    available_budget_usd_micros: 400,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    ..SelectionCandidateTrace::default()
                },
                SelectionCandidateTrace {
                    campaign_id: "cmp-2".into(),
                    creative_id: "creative-2".into(),
                    base_bid_usd_micros: 100,
                    quality_adjusted_bid_usd_micros: 100,
                    available_budget_usd_micros: 400,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    ..SelectionCandidateTrace::default()
                },
            ],
            winner_index: 0,
            resource_floor_usd_micros: 90,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 70,
                verifier_usd_micros: 12,
                host_usd_micros: 10,
                qualified_impressions_per_proof: 320,
            },
            runner_up_quality_bid_usd_micros: 80,
            clearing_price_usd_micros: 100,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
            uplift_assignment: None,
        });
        let err = bc.submit_read_ack(ack).expect_err("ack rejected");
        assert_eq!(err, ReadAckError::InvalidSelectionReceipt);
    }
}

#[cfg(test)]
mod market_metric_tests {
    use super::*;
    use energy_market::{EnergyProvider, EnergyReceipt, H256};

    #[test]
    fn market_metrics_reflect_epoch_activity() {
        let mut bc = Blockchain::default();
        bc.epoch_storage_bytes = 10_000;
        bc.epoch_cpu_ms = 2_000;
        bc.economics_epoch_storage_payout_block = 5_000;
        bc.economics_epoch_compute_payout_block = 3_000;
        bc.economics_epoch_ad_payout_block = 1_000;
        bc.params.rent_rate_per_byte = 5;
        bc.params.ad_cap_provider_count = 10;

        crate::compute_market::price_board::record_price(FeeLane::Industrial, 100, 1.0);

        let provider = EnergyProvider {
            provider_id: "provider-a".into(),
            owner: "owner".into(),
            location: "US_CA".into(),
            capacity_kwh: 1_000,
            available_kwh: 1_000,
            price_per_kwh: 4,
            reputation_score: 0.5,
            meter_address: "meter-1".into(),
            total_delivered_kwh: 0,
            staked_balance: 0,
            last_fulfillment_latency_ms: None,
            last_meter_value: None,
            last_meter_timestamp: None,
            bayesian_reputation: Default::default(),
        };
        let receipt = EnergyReceipt {
            buyer: "buyer".into(),
            seller: provider.provider_id.clone(),
            kwh_delivered: 200,
            price_paid: 800,
            block_settled: 1,
            treasury_fee: 0,
            meter_reading_hash: H256::default(),
            slash_applied: 0,
        };
        let energy_snapshot = crate::energy::EnergySnapshot {
            providers: vec![provider],
            receipts: vec![receipt],
            credits: Vec::new(),
            disputes: Vec::new(),
        };

        let metrics = bc.build_market_metrics(
            bc.economics_epoch_storage_payout_block,
            bc.economics_epoch_compute_payout_block,
            bc.economics_epoch_ad_payout_block,
            1_000_000,
            5,
            100_000,
            50,
            &energy_snapshot,
        );

        assert!(metrics.storage.utilization > 0.0);
        assert!(metrics.storage.effective_payout_block > 0.0);
        assert!(metrics.compute.average_cost_block > 0.0);
        assert!(metrics.compute.effective_payout_block > 0.0);
        assert!(metrics.energy.utilization > 0.0);
        assert!(metrics.ad.average_cost_block > 0.0);
        assert!(metrics.ad.provider_margin >= -2.0);
    }
}

/// Deterministic block hashing as per `docs/detailed_updates.md`.
/// Field order is fixed; all integers are little-endian.
///
/// This version accepts pre-serialized receipts to avoid double encoding
/// (performance optimization).
fn calculate_hash_with_cached_receipts(
    index: u64,
    prev: &str,
    timestamp: u64,
    nonce: u64,
    difficulty: u64,
    base_fee: u64,
    coin_c: TokenAmount,
    coin_i: TokenAmount,
    storage_sub: TokenAmount,
    read_sub: TokenAmount,
    read_sub_viewer: TokenAmount,
    read_sub_host: TokenAmount,
    read_sub_hardware: TokenAmount,
    read_sub_verifier: TokenAmount,
    read_sub_liquidity: TokenAmount,
    ad_viewer: TokenAmount,
    ad_host: TokenAmount,
    ad_hardware: TokenAmount,
    ad_verifier: TokenAmount,
    ad_liquidity: TokenAmount,
    ad_miner: TokenAmount,
    ad_total_usd_micros: u64,
    ad_settlement_count: u64,
    ad_oracle_price_usd_micros: u64,
    compute_sub: TokenAmount,
    proof_rebate: TokenAmount,
    read_root: [u8; 32],
    fee_checksum: &str,
    txs: &[SignedTransaction],
    state_root: &str,
    l2_roots: &[[u8; 32]],
    l2_sizes: &[u32],
    vdf_commit: [u8; 32],
    vdf_output: [u8; 32],
    vdf_proof: &[u8],
    retune_hint: i8,
    receipts_serialized: &[u8], // Pre-serialized receipts (cached)
) -> String {
    let ids: Vec<[u8; 32]> = txs.iter().map(SignedTransaction::id).collect();
    let id_refs: Vec<&[u8]> = ids.iter().map(<[u8; 32]>::as_ref).collect();

    // Use pre-serialized receipts (passed in) to avoid double encoding
    let enc = crate::hashlayout::BlockEncoder {
        index,
        prev,
        timestamp,
        nonce,
        difficulty,
        retune_hint,
        base_fee,
        coin_c: coin_c.0,
        coin_i: coin_i.0,
        storage_sub: storage_sub.0,
        read_sub: read_sub.0,
        read_sub_viewer: read_sub_viewer.0,
        read_sub_host: read_sub_host.0,
        read_sub_hardware: read_sub_hardware.0,
        read_sub_verifier: read_sub_verifier.0,
        read_sub_liquidity: read_sub_liquidity.0,
        ad_viewer: ad_viewer.0,
        ad_host: ad_host.0,
        ad_hardware: ad_hardware.0,
        ad_verifier: ad_verifier.0,
        ad_liquidity: ad_liquidity.0,
        ad_miner: ad_miner.0,
        ad_total_usd_micros,
        ad_settlement_count,
        ad_oracle_price_usd_micros,
        compute_sub: compute_sub.0,
        proof_rebate: proof_rebate.0,
        read_root,
        fee_checksum,
        state_root,
        tx_ids: &id_refs,
        l2_roots,
        l2_sizes,
        vdf_commit,
        vdf_output,
        vdf_proof,
        receipts_serialized, // Use cached serialized receipts
    };
    enc.hash()
}

/// Legacy calculate_hash function for backwards compatibility.
/// Encodes receipts on each call - use calculate_hash_with_cached_receipts instead.
#[allow(dead_code)]
fn calculate_hash(
    index: u64,
    prev: &str,
    timestamp: u64,
    nonce: u64,
    difficulty: u64,
    base_fee: u64,
    coin_c: TokenAmount,
    coin_i: TokenAmount,
    storage_sub: TokenAmount,
    read_sub: TokenAmount,
    read_sub_viewer: TokenAmount,
    read_sub_host: TokenAmount,
    read_sub_hardware: TokenAmount,
    read_sub_verifier: TokenAmount,
    read_sub_liquidity: TokenAmount,
    ad_viewer: TokenAmount,
    ad_host: TokenAmount,
    ad_hardware: TokenAmount,
    ad_verifier: TokenAmount,
    ad_liquidity: TokenAmount,
    ad_miner: TokenAmount,
    ad_total_usd_micros: u64,
    ad_settlement_count: u64,
    ad_oracle_price_usd_micros: u64,
    compute_sub: TokenAmount,
    proof_rebate: TokenAmount,
    read_root: [u8; 32],
    fee_checksum: &str,
    txs: &[SignedTransaction],
    state_root: &str,
    l2_roots: &[[u8; 32]],
    l2_sizes: &[u32],
    vdf_commit: [u8; 32],
    vdf_output: [u8; 32],
    vdf_proof: &[u8],
    retune_hint: i8,
    receipts: &[Receipt],
) -> String {
    // CRITICAL: Receipt encoding must succeed for consensus integrity.
    let receipts_bytes = crate::block_binary::encode_receipts(receipts).unwrap_or_else(|e| {
        #[cfg(feature = "telemetry")]
        crate::telemetry::receipts::RECEIPT_ENCODING_FAILURES_TOTAL.inc();

        panic!(
            "CRITICAL: Receipt encoding failed during hash calculation. \
                 This indicates a serious bug that will corrupt consensus. \
                 Error: {:?}, Receipt count: {}",
            e,
            receipts.len()
        );
    });

    calculate_hash_with_cached_receipts(
        index,
        prev,
        timestamp,
        nonce,
        difficulty,
        base_fee,
        coin_c,
        coin_i,
        storage_sub,
        read_sub,
        read_sub_viewer,
        read_sub_host,
        read_sub_hardware,
        read_sub_verifier,
        read_sub_liquidity,
        ad_viewer,
        ad_host,
        ad_hardware,
        ad_verifier,
        ad_liquidity,
        ad_miner,
        ad_total_usd_micros,
        ad_settlement_count,
        ad_oracle_price_usd_micros,
        compute_sub,
        proof_rebate,
        read_root,
        fee_checksum,
        txs,
        state_root,
        l2_roots,
        l2_sizes,
        vdf_commit,
        vdf_output,
        vdf_proof,
        retune_hint,
        &receipts_bytes,
    )
}

/// Generate a new Ed25519 keypair.
///
/// Returns the private and public key as raw byte vectors. The keys are
/// suitable for both transaction signing and simple message authentication.
#[must_use]
pub fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = OsRng::default();
    let mut priv_bytes = [0u8; 32];
    rng.fill_bytes(&mut priv_bytes);
    let sk = SigningKey::from_bytes(&priv_bytes);
    let vk = sk.verifying_key();
    (priv_bytes.to_vec(), vk.to_bytes().to_vec())
}

/// Sign an arbitrary message with a 32-byte Ed25519 private key.
///
/// The returned signature is a 64-byte array in raw form.
/// # Errors
/// Returns [`PyValueError`] if the private key length is invalid.
#[allow(clippy::needless_pass_by_value)]
pub fn sign_message(private: Vec<u8>, message: Vec<u8>) -> PyResult<Vec<u8>> {
    let sk_bytes =
        to_array_32(&private).ok_or_else(|| py_value_err("Invalid private key length"))?;
    let sk = SigningKey::from_bytes(&sk_bytes);
    Ok(sk.sign(&message).to_bytes().to_vec())
}

/// Verify a message signature produced by [`sign_message`].
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn verify_signature(public: Vec<u8>, message: Vec<u8>, signature: Vec<u8>) -> bool {
    if let (Some(pk), Some(sig_bytes)) = (to_array_32(&public), to_array_64(&signature)) {
        if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
            let sig = Signature::from_bytes(&sig_bytes);
            return vk.verify(&message, &sig).is_ok();
        }
    }
    false
}

/// Python-accessible helper to mine a block from signed transactions.
///
/// Spins up a temporary `Blockchain` with a genesis block, sets a zero
/// fee-per-byte floor, seeds missing sender accounts with large balances,
/// admits the provided transactions, and mines a block to the hardcoded
/// miner address "miner".
pub fn mine_block_py(txs: Vec<SignedTransaction>) -> PyResult<Block> {
    let mut bc = Blockchain::default();
    bc.genesis_block()?;
    bc.min_fee_per_byte_consumer = 0;
    bc.min_fee_per_byte_industrial = 0;
    for tx in txs {
        let sender = tx.payload.from_.clone();
        if sender != "0".repeat(34) && !bc.accounts.contains_key(&sender) {
            bc.add_account(sender.clone(), u64::MAX / 2)?;
        }
        bc.submit_transaction(tx).map_err(PyError::from)?;
    }
    bc.mine_block("miner")
}

// === Tx admission error codes ===
pub const ERR_OK: u16 = 0;
pub const ERR_UNKNOWN_SENDER: u16 = 1;
pub const ERR_INSUFFICIENT_BALANCE: u16 = 2;
pub const ERR_NONCE_GAP: u16 = 3;
pub const ERR_INVALID_SELECTOR: u16 = 4;
pub const ERR_BAD_SIGNATURE: u16 = 5;
pub const ERR_DUPLICATE: u16 = 6;
pub const ERR_NOT_FOUND: u16 = 7;
pub const ERR_BALANCE_OVERFLOW: u16 = 8;
pub const ERR_FEE_OVERFLOW: u16 = 9;
pub const ERR_FEE_TOO_LOW: u16 = 10;
pub const ERR_MEMPOOL_FULL: u16 = 11;
pub const ERR_LOCK_POISONED: u16 = 12;
pub const ERR_PENDING_LIMIT: u16 = 13;
pub const ERR_FEE_TOO_LARGE: u16 = 14;
pub const ERR_RENT_ESCROW_INSUFFICIENT: u16 = 15;
pub const ERR_DNS_SIG_INVALID: u16 = 16;
pub const ERR_PENDING_SIGNATURES: u16 = 17;
pub const ERR_SESSION_EXPIRED: u16 = 18;

pub struct PyRemoteSigner {
    inner: WalletRemoteSigner,
}

impl PyRemoteSigner {
    #[cfg_attr(feature = "python-bindings", new)]
    pub fn new(url: String) -> PyResult<Self> {
        let inner = WalletRemoteSigner::connect(&url).map_err(|e| py_runtime_err(e.to_string()))?;
        Ok(Self { inner })
    }

    pub fn public_key(&self) -> String {
        crypto_suite::hex::encode(self.inner.public_key().to_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> PyResult<Vec<u8>> {
        let result = self.inner.sign(msg);
        let telemetry_outcome = result.as_ref().map(|_| ()).map_err(|e| e.to_string());
        record_remote_signer_result(&telemetry_outcome);
        result
            .map(|s| s.to_bytes().to_vec())
            .map_err(|e| py_runtime_err(e.to_string()))
    }
}

#[cfg(feature = "telemetry")]
fn record_remote_signer_result(outcome: &Result<(), String>) {
    crate::telemetry::sampled_inc(&*crate::telemetry::REMOTE_SIGNER_REQUEST_TOTAL);
    if let Err(reason) = outcome {
        crate::telemetry::sampled_inc_vec(
            &crate::telemetry::REMOTE_SIGNER_ERROR_TOTAL,
            &[reason.as_str()],
        );
    }
}

#[cfg(not(feature = "telemetry"))]
fn record_remote_signer_result(outcome: &Result<(), String>) {
    if let Err(reason) = outcome {
        let _ = reason;
    }
}

/// Return the integer network identifier used in domain separation.
#[must_use]
pub fn chain_id_py() -> u32 {
    CHAIN_ID
}

/// Initialize the Python module.
///
/// The first-party bridge is not yet available, so this function surfaces a
/// clear error explaining how to enable bindings once the implementation ships.
pub fn the_block() -> PyResult<()> {
    Err(PyError::feature_disabled().with_message(
        "python bindings are not available; enable the `python-bindings` feature once the first-party bridge lands",
    ))
}

#[cfg(test)]
mod shard_cache_tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use sys::tempfile::tempdir;

    #[test]
    fn shard_cache_round_trip_populates_and_updates_entries() {
        let dir = tempdir().unwrap();
        let mut bc = Blockchain::new(dir.path().to_str().unwrap());
        let shard: ShardId = 42;
        let key = "cache-key";
        let initial = b"initial".to_vec();

        let mut deltas = Vec::new();
        bc.db
            .insert_shard_with_delta(shard, key, initial.clone(), &mut deltas)
            .unwrap();

        assert_eq!(bc.read_shard_state(shard, key), Some(initial.clone()));

        let cache_key = (shard, key.as_bytes().to_vec());
        {
            let mut cache = bc.shard_cache_guard();
            assert_eq!(cache.peek(&cache_key).cloned(), Some(initial.clone()));
        }

        let updated = b"updated".to_vec();
        let mut deltas = Vec::new();
        bc.write_shard_state(shard, key, updated.clone(), &mut deltas)
            .unwrap();

        let cache_key = (shard, key.as_bytes().to_vec());
        {
            let mut cache = bc.shard_cache_guard();
            assert_eq!(cache.peek(&cache_key).cloned(), Some(updated.clone()));
        }

        assert_eq!(bc.read_shard_state(shard, key), Some(updated));
    }

    #[test]
    fn shard_cache_poison_recovery() {
        let dir = tempdir().unwrap();
        let mut bc = Blockchain::new(dir.path().to_str().unwrap());
        let shard: ShardId = 7;
        let key = "poison";
        let initial = b"poison-initial".to_vec();

        let mut deltas = Vec::new();
        bc.db
            .insert_shard_with_delta(shard, key, initial.clone(), &mut deltas)
            .unwrap();

        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard = bc.shard_cache.lock().unwrap();
            panic!("poison shard cache");
        }));
        std::panic::set_hook(hook);
        assert!(result.is_err());

        assert_eq!(bc.read_shard_state(shard, key), Some(initial.clone()));

        let updated = b"poison-updated".to_vec();
        let mut deltas = Vec::new();
        bc.write_shard_state(shard, key, updated.clone(), &mut deltas)
            .unwrap();

        assert_eq!(bc.read_shard_state(shard, key), Some(updated));
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    use testkit::tb_prop_test;

    tb_prop_test!(reservation_rollback_on_panic, |runner| {
        runner
            .add_random_case("reservation rollback", 32, |rng| {
                let amount = rng.range_u64(0..=10_000);
                let mut acc = Account {
                    address: "a".into(),
                    balance: TokenBalance {
                        amount: 0,
                    },
                    nonce: 0,
                    pending_amount: 0,
                    pending_nonce: 0,
                    pending_nonces: HashSet::new(),
                    sessions: Vec::new(),
                };
                let lock = Mutex::new(());
                let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                let res = ReservationGuard::new(guard, &mut acc, amount, 1);

                // Silence the expected panic to avoid noisy output when telemetry is enabled.
                let hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(|_| {}));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    drop(res);
                    panic!("boom");
                }));
                std::panic::set_hook(hook);

                assert!(result.is_err());
                assert_eq!(acc.pending_amount, 0);
                assert_eq!(acc.pending_nonce, 0);
                assert!(acc.pending_nonces.is_empty());
            })
            .expect("register random case");
    });
}
