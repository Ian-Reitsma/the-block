#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![allow(clippy::all)]

//! Core blockchain implementation with Python bindings.
//!
//! Exposes a minimal proof-of-work chain with dual-token economics. See
//! `AGENTS.md` for the high-level specification.

use dashmap::DashMap;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
#[cfg(feature = "telemetry")]
use log::{info, warn};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use pyo3::PyTypeInfo;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
#[cfg(feature = "telemetry-json")]
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
mod simple_db;
use simple_db::SimpleDb as Db;
use std::convert::TryInto;
use thiserror::Error;

#[cfg(feature = "telemetry")]
pub mod telemetry;
#[cfg(feature = "telemetry")]
pub use telemetry::gather as gather_metrics;

pub mod blockchain;
use blockchain::difficulty;

pub mod transaction;
pub use transaction::{
    canonical_payload_bytes, canonical_payload_py as canonical_payload, sign_tx_py as sign_tx,
    verify_signed_tx_py as verify_signed_tx, RawTxPayload, SignedTransaction,
};
use transaction::{canonical_payload_py, sign_tx_py, verify_signed_tx_py};
pub mod consensus;
pub mod constants;
pub use constants::{domain_tag, CHAIN_ID, FEE_SPEC_VERSION, GENESIS_HASH, TX_VERSION};
pub mod fee;
pub mod hash_genesis;
pub mod hashlayout;
pub use fee::{decompose as fee_decompose, ErrFeeOverflow, ErrInvalidSelector, FeeError};

// === Transaction admission errors ===

#[derive(Debug, Error, PartialEq)]
pub enum TxAdmissionError {
    #[error("unknown sender")]
    UnknownSender,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("bad nonce")]
    BadNonce,
    #[error("invalid selector")]
    InvalidSelector,
    #[error("bad signature")]
    BadSignature,
    #[error("duplicate transaction")]
    Duplicate,
    #[error("transaction not found")]
    NotFound,
    #[error("balance overflow")]
    BalanceOverflow,
    #[error("fee overflow")]
    FeeOverflow,
    #[error("fee below minimum")]
    FeeTooLow,
    #[error("mempool full")]
    MempoolFull,
    #[error("lock poisoned")]
    LockPoisoned,
    #[error("pending limit reached")]
    PendingLimitReached,
}

create_exception!(the_block, ErrUnknownSender, PyException);
create_exception!(the_block, ErrInsufficientBalance, PyException);
create_exception!(the_block, ErrBadNonce, PyException);
create_exception!(the_block, ErrBadSignature, PyException);
create_exception!(the_block, ErrDuplicateTx, PyException);
create_exception!(the_block, ErrTxNotFound, PyException);
create_exception!(the_block, ErrFeeTooLow, PyException);
create_exception!(the_block, ErrMempoolFull, PyException);
create_exception!(the_block, ErrLockPoisoned, PyException);
create_exception!(the_block, ErrPendingLimit, PyException);

impl From<TxAdmissionError> for PyErr {
    fn from(e: TxAdmissionError) -> Self {
        match e {
            TxAdmissionError::UnknownSender => ErrUnknownSender::new_err("unknown sender"),
            TxAdmissionError::InsufficientBalance => {
                ErrInsufficientBalance::new_err("insufficient balance")
            }
            TxAdmissionError::BadNonce => ErrBadNonce::new_err("bad nonce"),
            TxAdmissionError::InvalidSelector => ErrInvalidSelector::new_err("invalid selector"),
            TxAdmissionError::BadSignature => ErrBadSignature::new_err("bad signature"),
            TxAdmissionError::Duplicate => ErrDuplicateTx::new_err("duplicate transaction"),
            TxAdmissionError::NotFound => ErrTxNotFound::new_err("transaction not found"),
            TxAdmissionError::BalanceOverflow => PyValueError::new_err("balance overflow"),
            TxAdmissionError::FeeOverflow => ErrFeeOverflow::new_err("fee overflow"),
            TxAdmissionError::FeeTooLow => ErrFeeTooLow::new_err("fee below minimum"),
            TxAdmissionError::MempoolFull => ErrMempoolFull::new_err("mempool full"),
            TxAdmissionError::LockPoisoned => ErrLockPoisoned::new_err("lock poisoned"),
            TxAdmissionError::PendingLimitReached => {
                ErrPendingLimit::new_err("pending limit reached")
            }
        }
    }
}

#[cfg(feature = "telemetry-json")]
fn log_event(level: log::Level, op: &str, sender: &str, nonce: u64, code: &str, fpb: Option<u64>) {
    let mut obj = serde_json::Map::new();
    obj.insert("op".into(), json!(op));
    obj.insert("sender".into(), json!(sender));
    obj.insert("nonce".into(), json!(nonce));
    obj.insert("code".into(), json!(code));
    if let Some(v) = fpb {
        obj.insert("fpb".into(), json!(v));
    }
    let msg = serde_json::Value::Object(obj).to_string();
    log::log!(level, "{}", msg);
}

// === Database keys ===
const DB_CHAIN: &str = "chain";
const DB_ACCOUNTS: &str = "accounts";
const DB_EMISSION: &str = "emission";

// === Monetary constants ===
const MAX_SUPPLY_CONSUMER: u64 = 20_000_000_000_000;
const MAX_SUPPLY_INDUSTRIAL: u64 = 20_000_000_000_000;
const INITIAL_BLOCK_REWARD_CONSUMER: u64 = 60_000;
const INITIAL_BLOCK_REWARD_INDUSTRIAL: u64 = 30_000;
const DECAY_NUMERATOR: u64 = 99995; // ~0.005% per block
const DECAY_DENOMINATOR: u64 = 100_000;

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
    hex::decode(hex).expect("Invalid hex string")
}

// === Data types ===

/// Chain-wide token unit.
///
/// See `AGENTS.md` §10.3. All monetary values in consensus code use this
/// wrapper to make a future switch to `u128` trivial and to forbid accidental
/// arithmetic on raw integers.
#[pyclass]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TokenAmount(pub u64);

#[pymethods]
impl TokenAmount {
    #[new]
    pub fn py_new(v: u64) -> Self {
        Self(v)
    }
    #[getter]
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

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TokenBalance {
    #[pyo3(get, set)]
    pub consumer: u64,
    #[pyo3(get, set)]
    pub industrial: u64,
}

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct Pending {
    #[pyo3(get)]
    #[serde(default)]
    pub consumer: u64,
    #[pyo3(get)]
    #[serde(default)]
    pub industrial: u64,
    #[pyo3(get)]
    #[serde(default)]
    pub nonce: u64,
    #[pyo3(get)]
    #[serde(default)]
    pub nonces: HashSet<u64>,
}

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Account {
    #[pyo3(get)]
    pub address: String,
    #[pyo3(get)]
    pub balance: TokenBalance,
    #[pyo3(get, set)]
    #[serde(default)]
    pub nonce: u64,
    #[pyo3(get)]
    #[serde(default)]
    pub pending: Pending,
}

struct Reservation<'a> {
    pending: &'a mut Pending,
    reserve_consumer: u64,
    reserve_industrial: u64,
    nonce: u64,
    committed: bool,
}

impl<'a> Reservation<'a> {
    fn new(
        pending: &'a mut Pending,
        reserve_consumer: u64,
        reserve_industrial: u64,
        nonce: u64,
    ) -> Self {
        pending.consumer += reserve_consumer;
        pending.industrial += reserve_industrial;
        pending.nonce += 1;
        pending.nonces.insert(nonce);
        Self {
            pending,
            reserve_consumer,
            reserve_industrial,
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
            self.pending.consumer = self.pending.consumer.saturating_sub(self.reserve_consumer);
            self.pending.industrial = self
                .pending
                .industrial
                .saturating_sub(self.reserve_industrial);
            self.pending.nonce = self.pending.nonce.saturating_sub(1);
            self.pending.nonces.remove(&self.nonce);
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
        pending: &'a mut Pending,
        reserve_consumer: u64,
        reserve_industrial: u64,
        nonce: u64,
    ) -> Self {
        let reservation = Reservation::new(pending, reserve_consumer, reserve_industrial, nonce);
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

/// Per-block ledger entry. `coinbase_*` mirrors the first transaction
/// but is the canonical source for light clients.
#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Block {
    #[pyo3(get)]
    pub index: u64,
    #[pyo3(get)]
    pub previous_hash: String,
    #[pyo3(get)]
    pub transactions: Vec<SignedTransaction>,
    #[pyo3(get)]
    #[serde(default)]
    pub difficulty: u64,
    #[pyo3(get)]
    pub nonce: u64,
    #[pyo3(get)]
    pub hash: String,
    #[pyo3(get)]
    #[serde(default)]
    /// Canonical consumer reward recorded in the header. Must match tx[0].
    pub coinbase_consumer: TokenAmount,
    #[pyo3(get)]
    #[serde(default)]
    /// Canonical industrial reward recorded in the header. Must match tx[0].
    pub coinbase_industrial: TokenAmount,
    #[pyo3(get)]
    #[serde(default)]
    /// blake3(total_fee_ct || total_fee_it) in hex
    pub fee_checksum: String,
}

#[derive(Clone)]
pub struct MempoolEntry {
    pub tx: SignedTransaction,
    /// UNIX timestamp in milliseconds when the tx entered the mempool
    pub timestamp_millis: u64,
}

/// In-memory representation of the chain state and associated accounts.
///
/// `Blockchain` exposes high level methods for transaction submission,
/// mining, and persistence. It backs the Python API used throughout the
/// demo script and tests.
#[pyclass]
pub struct Blockchain {
    pub chain: Vec<Block>,
    #[pyo3(get)]
    pub accounts: HashMap<String, Account>,
    #[pyo3(get, set)]
    pub difficulty: u64,
    pub mempool: DashMap<(String, u64), MempoolEntry>,
    mempool_size: std::sync::atomic::AtomicUsize,
    mempool_mutex: Mutex<()>,
    orphan_counter: std::sync::atomic::AtomicUsize,
    #[pyo3(get, set)]
    pub max_mempool_size: usize,
    #[pyo3(get, set)]
    pub min_fee_per_byte: u64,
    #[pyo3(get, set)]
    pub tx_ttl: u64,
    #[pyo3(get, set)]
    pub max_pending_per_account: usize,
    admission_locks: DashMap<String, Arc<Mutex<()>>>,
    db: Db,
    #[pyo3(get)]
    pub path: String,
    #[pyo3(get, set)]
    pub emission_consumer: u64,
    #[pyo3(get, set)]
    pub emission_industrial: u64,
    #[pyo3(get, set)]
    pub block_reward_consumer: TokenAmount,
    #[pyo3(get, set)]
    pub block_reward_industrial: TokenAmount,
    #[pyo3(get, set)]
    pub block_height: u64,
    #[pyo3(get)]
    pub skipped: Vec<SignedTransaction>,
    #[pyo3(get)]
    pub skipped_nonce_gap: u64,
}

#[pyclass]
#[derive(Serialize, Deserialize)]
pub struct ChainDisk {
    #[serde(default)]
    pub schema_version: usize,
    #[serde(default)]
    pub chain: Vec<Block>,
    #[serde(default)]
    pub accounts: HashMap<String, Account>,
    #[serde(default)]
    pub emission_consumer: u64,
    #[serde(default)]
    pub emission_industrial: u64,
    #[serde(default)]
    pub block_reward_consumer: TokenAmount,
    #[serde(default)]
    pub block_reward_industrial: TokenAmount,
    #[serde(default)]
    pub block_height: u64,
    #[serde(default)]
    pub mempool: Vec<MempoolEntryDisk>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MempoolEntryDisk {
    pub sender: String,
    pub nonce: u64,
    pub tx: SignedTransaction,
    pub timestamp_millis: u64,
}

impl Default for Blockchain {
    fn default() -> Self {
        Self {
            chain: Vec::new(),
            accounts: HashMap::new(),
            difficulty: difficulty::expected_difficulty(0, 1),
            mempool: DashMap::new(),
            mempool_size: std::sync::atomic::AtomicUsize::new(0),
            mempool_mutex: Mutex::new(()),
            orphan_counter: std::sync::atomic::AtomicUsize::new(0),
            max_mempool_size: 1024,
            min_fee_per_byte: 1,
            tx_ttl: 1800,
            max_pending_per_account: 16,
            admission_locks: DashMap::new(),
            db: Db::default(),
            path: String::new(),
            emission_consumer: 0,
            emission_industrial: 0,
            block_reward_consumer: TokenAmount::new(INITIAL_BLOCK_REWARD_CONSUMER),
            block_reward_industrial: TokenAmount::new(INITIAL_BLOCK_REWARD_INDUSTRIAL),
            block_height: 0,
            skipped: Vec::new(),
            skipped_nonce_gap: 0,
        }
    }
}

impl Blockchain {
    #[cfg(feature = "telemetry")]
    fn inc_mempool_size(&self) -> usize {
        let size = self
            .mempool_size
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        telemetry::MEMPOOL_SIZE.set(size as i64);
        size
    }

    #[cfg(not(feature = "telemetry"))]
    fn inc_mempool_size(&self) -> usize {
        self.mempool_size
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    #[cfg(feature = "telemetry")]
    fn dec_mempool_size(&self) -> usize {
        let size = self
            .mempool_size
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
            - 1;
        telemetry::MEMPOOL_SIZE.set(size as i64);
        size
    }

    #[cfg(not(feature = "telemetry"))]
    fn dec_mempool_size(&self) -> usize {
        self.mempool_size
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
            - 1
    }

    #[cfg(feature = "telemetry")]
    fn record_admit(&self) {
        telemetry::TX_ADMITTED_TOTAL.inc();
    }

    #[cfg(feature = "telemetry")]
    fn record_reject(&self) {
        telemetry::TX_REJECTED_TOTAL.inc();
    }
}

#[pymethods]
impl Blockchain {
    /// Default Python constructor opens ./chain_db
    #[new]
    pub fn py_new() -> PyResult<Self> {
        Blockchain::open("chain_db")
    }

    #[staticmethod]
    pub fn open(path: &str) -> PyResult<Self> {
        // Open an existing database and auto-migrate to schema v4.
        // See `docs/detailed_updates.md` for layout history.
        let mut db = Db::open(path);
        let (mut chain, mut accounts, em_c, em_i, br_c, br_i, bh, mempool_disk) = if let Some(raw) =
            db.get(DB_CHAIN)
        {
            match bincode::deserialize::<ChainDisk>(&raw) {
                Ok(mut disk) => {
                    if disk.schema_version > 4 {
                        return Err(PyValueError::new_err("DB schema too new"));
                    }
                    if disk.schema_version < 3 {
                        let mut migrated_chain = disk.chain.clone();
                        for b in &mut migrated_chain {
                            if let Some(cb) = b.transactions.first() {
                                b.coinbase_consumer = TokenAmount::new(cb.payload.amount_consumer);
                                b.coinbase_industrial =
                                    TokenAmount::new(cb.payload.amount_industrial);
                            }
                            let mut fee_consumer: u128 = 0;
                            let mut fee_industrial: u128 = 0;
                            for tx in b.transactions.iter().skip(1) {
                                if let Ok((c, i)) =
                                    crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
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
                                b.nonce,
                                b.difficulty,
                                b.coinbase_consumer,
                                b.coinbase_industrial,
                                &b.fee_checksum,
                                &b.transactions,
                            );
                        }
                        let mut em_c = 0u64;
                        let mut em_i = 0u64;
                        for b in &migrated_chain {
                            em_c = em_c.saturating_add(b.coinbase_consumer.get());
                            em_i = em_i.saturating_add(b.coinbase_industrial.get());
                        }
                        let bh = migrated_chain.len() as u64;
                        let migrated = ChainDisk {
                            schema_version: 3,
                            chain: migrated_chain,
                            accounts: disk.accounts,
                            emission_consumer: em_c,
                            emission_industrial: em_i,
                            block_reward_consumer: if disk.block_reward_consumer.get() == 0 {
                                TokenAmount::new(INITIAL_BLOCK_REWARD_CONSUMER)
                            } else {
                                disk.block_reward_consumer
                            },
                            block_reward_industrial: if disk.block_reward_industrial.get() == 0 {
                                TokenAmount::new(INITIAL_BLOCK_REWARD_INDUSTRIAL)
                            } else {
                                disk.block_reward_industrial
                            },
                            block_height: bh,
                            mempool: Vec::new(),
                        };
                        db.insert(
                            DB_CHAIN,
                            bincode::serialize(&migrated)
                                .unwrap_or_else(|e| panic!("serialize: {e}")),
                        );
                        db.remove(DB_ACCOUNTS);
                        db.remove(DB_EMISSION);
                        (
                            migrated.chain,
                            migrated.accounts,
                            migrated.emission_consumer,
                            migrated.emission_industrial,
                            migrated.block_reward_consumer,
                            migrated.block_reward_industrial,
                            migrated.block_height,
                            migrated.mempool,
                        )
                    } else {
                        if disk.emission_consumer == 0
                            && disk.emission_industrial == 0
                            && !disk.chain.is_empty()
                        {
                            let mut em_c = 0u64;
                            let mut em_i = 0u64;
                            for b in &disk.chain {
                                em_c = em_c.saturating_add(b.coinbase_consumer.get());
                                em_i = em_i.saturating_add(b.coinbase_industrial.get());
                            }
                            disk.emission_consumer = em_c;
                            disk.emission_industrial = em_i;
                            disk.block_height = disk.chain.len() as u64;
                            db.insert(
                                DB_CHAIN,
                                bincode::serialize(&disk)
                                    .unwrap_or_else(|e| panic!("serialize: {e}")),
                            );
                        }
                        (
                            disk.chain,
                            disk.accounts,
                            disk.emission_consumer,
                            disk.emission_industrial,
                            disk.block_reward_consumer,
                            disk.block_reward_industrial,
                            disk.block_height,
                            disk.mempool,
                        )
                    }
                }
                Err(_) => {
                    let chain: Vec<Block> = bincode::deserialize(&raw).unwrap_or_default();
                    let accounts: HashMap<String, Account> = db
                        .get(DB_ACCOUNTS)
                        .and_then(|iv| bincode::deserialize(&iv).ok())
                        .unwrap_or_default();
                    let (br_c, br_i): (u64, u64) = db
                        .get(DB_EMISSION)
                        .and_then(|iv| bincode::deserialize::<(u64, u64, u64, u64, u64)>(&iv).ok())
                        .map(|(_em_c, _em_i, br_c, br_i, _bh)| (br_c, br_i))
                        .unwrap_or((
                            INITIAL_BLOCK_REWARD_CONSUMER,
                            INITIAL_BLOCK_REWARD_INDUSTRIAL,
                        ));
                    let mut migrated_chain = chain.clone();
                    for b in &mut migrated_chain {
                        if let Some(cb) = b.transactions.first() {
                            b.coinbase_consumer = TokenAmount::new(cb.payload.amount_consumer);
                            b.coinbase_industrial = TokenAmount::new(cb.payload.amount_industrial);
                        }
                        let mut fee_consumer: u128 = 0;
                        let mut fee_industrial: u128 = 0;
                        for tx in b.transactions.iter().skip(1) {
                            if let Ok((c, i)) =
                                crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
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
                            b.nonce,
                            b.difficulty,
                            b.coinbase_consumer,
                            b.coinbase_industrial,
                            &b.fee_checksum,
                            &b.transactions,
                        );
                    }
                    let mut em_c = 0u64;
                    let mut em_i = 0u64;
                    for b in &migrated_chain {
                        em_c = em_c.saturating_add(b.coinbase_consumer.get());
                        em_i = em_i.saturating_add(b.coinbase_industrial.get());
                    }
                    let bh = migrated_chain.len() as u64;
                    let disk_new = ChainDisk {
                        schema_version: 3,
                        chain: migrated_chain,
                        accounts: accounts.clone(),
                        emission_consumer: em_c,
                        emission_industrial: em_i,
                        block_reward_consumer: TokenAmount::new(br_c),
                        block_reward_industrial: TokenAmount::new(br_i),
                        block_height: bh,
                        mempool: Vec::new(),
                    };
                    db.insert(
                        DB_CHAIN,
                        bincode::serialize(&disk_new).unwrap_or_else(|e| panic!("serialize: {e}")),
                    );
                    db.remove(DB_ACCOUNTS);
                    db.remove(DB_EMISSION);
                    (
                        disk_new.chain,
                        disk_new.accounts,
                        disk_new.emission_consumer,
                        disk_new.emission_industrial,
                        disk_new.block_reward_consumer,
                        disk_new.block_reward_industrial,
                        disk_new.block_height,
                        disk_new.mempool,
                    )
                }
            }
        } else {
            (
                Vec::new(),
                HashMap::new(),
                0,
                0,
                TokenAmount::new(INITIAL_BLOCK_REWARD_CONSUMER),
                TokenAmount::new(INITIAL_BLOCK_REWARD_INDUSTRIAL),
                0,
                Vec::new(),
            )
        };
        for b in &mut chain {
            let mut fee_consumer: u128 = 0;
            let mut fee_industrial: u128 = 0;
            for tx in b.transactions.iter().skip(1) {
                if let Ok((c, i)) = crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee) {
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
            acc.pending.consumer = 0;
            acc.pending.industrial = 0;
            acc.pending.nonce = acc.pending.nonces.len() as u64;
        }
        let mut bc = Blockchain::default();
        bc.path = path.to_string();
        bc.chain = chain;
        bc.accounts = accounts;
        bc.db = db;
        bc.emission_consumer = em_c;
        bc.emission_industrial = em_i;
        bc.block_reward_consumer = br_c;
        bc.block_reward_industrial = br_i;
        bc.block_height = bh;
        for e in mempool_disk {
            bc.mempool.insert(
                (e.sender.clone(), e.nonce),
                MempoolEntry { tx: e.tx, timestamp_millis: e.timestamp_millis },
            );
            bc.inc_mempool_size();
        }
        for entry in bc.mempool.iter() {
            let tx = &entry.value().tx;
            if let Some(acc) = bc.accounts.get_mut(&tx.payload.from_) {
                if let Ok((fee_consumer, fee_industrial)) =
                    crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
                {
                    acc.pending.consumer += tx.payload.amount_consumer + fee_consumer;
                    acc.pending.industrial += tx.payload.amount_industrial + fee_industrial;
                    acc.pending.nonce += 1;
                    acc.pending.nonces.insert(tx.payload.nonce);
                }
            }
        }
        #[cfg(feature = "telemetry")]
        let _span = tracing::span!(tracing::Level::TRACE, "startup_rebuild").entered();
        bc.purge_expired();
        if let Ok(v) = std::env::var("TB_MEMPOOL_MAX") {
            if let Ok(n) = v.parse() {
                bc.max_mempool_size = n;
            }
        }
        if let Ok(v) = std::env::var("TB_MIN_FEE_PER_BYTE") {
            if let Ok(n) = v.parse() {
                bc.min_fee_per_byte = n;
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
        Ok(bc)
    }

    #[staticmethod]
    pub fn with_difficulty(path: &str, difficulty: u64) -> PyResult<Self> {
        let mut bc = Blockchain::open(path)?;
        bc.difficulty = difficulty;
        Ok(bc)
    }

    /// Return the on-disk schema version
    #[getter]
    pub fn schema_version(&self) -> usize {
        // Bump this constant whenever the serialized `ChainDisk` format changes.
        // Older binaries must refuse to open newer databases.
        4
    }

    /// Persist the entire chain + state under the current schema
    pub fn persist_chain(&mut self) -> PyResult<()> {
        let mempool: Vec<MempoolEntryDisk> = self
            .mempool
            .iter()
            .map(|e| MempoolEntryDisk {
                sender: e.key().0.clone(),
                nonce: e.key().1,
                tx: e.value().tx.clone(),
                timestamp_millis: e.value().timestamp_millis,
            })
            .collect();
        let disk = ChainDisk {
            schema_version: self.schema_version(),
            chain: self.chain.clone(),
            accounts: self.accounts.clone(),
            emission_consumer: self.emission_consumer,
            emission_industrial: self.emission_industrial,
            block_reward_consumer: self.block_reward_consumer,
            block_reward_industrial: self.block_reward_industrial,
            block_height: self.block_height,
            mempool,
        };
        let bytes = bincode::serialize(&disk)
            .map_err(|e| PyValueError::new_err(format!("Serialization error: {e}")))?;
        self.db.insert(DB_CHAIN, bytes);
        // ensure no legacy column families linger on disk
        self.db.remove(DB_ACCOUNTS);
        self.db.remove(DB_EMISSION);
        self.db.flush();
        Ok(())
    }

    pub fn circulating_supply(&self) -> (u64, u64) {
        (self.emission_consumer, self.emission_industrial)
    }

    /// Construct and persist the genesis block.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the chain state cannot be serialized or persisted.
    pub fn genesis_block(&mut self) -> PyResult<()> {
        let g = Block {
            index: 0,
            previous_hash: "0".repeat(64),
            transactions: vec![],
            difficulty: difficulty::expected_difficulty(0, self.difficulty),
            nonce: 0,
            // genesis carries zero reward; fields included for stable hashing
            coinbase_consumer: TokenAmount::new(0),
            coinbase_industrial: TokenAmount::new(0),
            fee_checksum: "0".repeat(64),
            hash: GENESIS_HASH.to_string(),
        };
        self.chain.push(g);
        self.block_height = 1;
        let bytes =
            bincode::serialize(&self.chain).map_err(|e| PyValueError::new_err(e.to_string()))?;
        self.db.insert(DB_CHAIN, bytes);
        self.db.flush();
        Ok(())
    }

    /// Add a new account with starting balances.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the account already exists.
    pub fn add_account(&mut self, address: String, consumer: u64, industrial: u64) -> PyResult<()> {
        if self.accounts.contains_key(&address) {
            return Err(PyValueError::new_err("Account already exists"));
        }
        let acc = Account {
            address: address.clone(),
            balance: TokenBalance {
                consumer,
                industrial,
            },
            nonce: 0,
            pending: Pending::default(),
        };
        self.accounts.insert(address, acc);
        Ok(())
    }

    /// Return the balance for an account.
    ///
    /// # Errors
    /// Returns [`PyValueError`] if the account is not found.
    pub fn get_account_balance(&self, address: &str) -> PyResult<TokenBalance> {
        self.accounts
            .get(address)
            .map(|a| a.balance.clone())
            .ok_or_else(|| PyValueError::new_err("Account not found"))
    }

    /// Submit a signed transaction to the mempool.
    ///
    /// # Errors
    /// Returns [`TxAdmissionError`] if validation fails or the sender is missing.
    pub fn submit_transaction(&mut self, tx: SignedTransaction) -> Result<(), TxAdmissionError> {
        self.purge_expired();
        let sender_addr = tx.payload.from_.clone();
        let nonce = tx.payload.nonce;
        let _pool_guard = self.mempool_mutex.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            telemetry::LOCK_POISON_TOTAL.inc();
            TxAdmissionError::LockPoisoned
        })?;
        let lock = self
            .admission_locks
            .entry(sender_addr.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        #[cfg(feature = "telemetry")]
        let lock_guard = {
            let span =
                tracing::span!(tracing::Level::TRACE, "admission_lock", sender = %sender_addr);
            span.in_scope(|| lock.lock()).map_err(|_| {
                telemetry::LOCK_POISON_TOTAL.inc();
                TxAdmissionError::LockPoisoned
            })?
        };
        #[cfg(not(feature = "telemetry"))]
        let lock_guard = lock.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            telemetry::LOCK_POISON_TOTAL.inc();
            TxAdmissionError::LockPoisoned
        })?;

        if tx.payload.fee_selector > 2 {
            #[cfg(feature = "telemetry-json")]
            log_event(
                log::Level::Warn,
                "reject",
                &sender_addr,
                nonce,
                "invalid_selector",
                None,
            );
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            warn!("tx rejected sender={sender_addr} nonce={nonce} reason=invalid_selector");
            return Err(TxAdmissionError::InvalidSelector);
        }
        let (fee_consumer, fee_industrial) =
            crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee).map_err(
                |e| match e {
                    FeeError::InvalidSelector => TxAdmissionError::InvalidSelector,
                    FeeError::Overflow => TxAdmissionError::FeeOverflow,
                },
            )?;
        let total_consumer = tx
            .payload
            .amount_consumer
            .checked_add(fee_consumer)
            .ok_or(TxAdmissionError::FeeOverflow)?;
        let total_industrial = tx
            .payload
            .amount_industrial
            .checked_add(fee_industrial)
            .ok_or(TxAdmissionError::FeeOverflow)?;

        // capacity check after basic validation
        if self.mempool_size.load(std::sync::atomic::Ordering::SeqCst) >= self.max_mempool_size {
            #[cfg(feature = "telemetry")]
            telemetry::EVICTIONS_TOTAL.inc();
            #[cfg(feature = "telemetry")]
            self.record_reject();
            #[cfg(feature = "telemetry-json")]
            log_event(
                log::Level::Warn,
                "reject",
                &sender_addr,
                nonce,
                "mempool_full",
                None,
            );
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            warn!("tx rejected sender={sender_addr} nonce={nonce} reason=mempool_full");
            return Err(TxAdmissionError::MempoolFull);
        }

        match self.mempool.entry((sender_addr.clone(), nonce)) {
            dashmap::mapref::entry::Entry::Occupied(_) => {
                #[cfg(feature = "telemetry")]
                #[cfg(feature = "telemetry-json")]
                log_event(
                    log::Level::Warn,
                    "reject",
                    &sender_addr,
                    nonce,
                    "duplicate",
                    None,
                );
                #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                warn!("tx rejected sender={sender_addr} nonce={nonce} reason=duplicate");
                #[cfg(feature = "telemetry")]
                {
                    telemetry::DUP_TX_REJECT_TOTAL.inc();
                    self.record_reject();
                }
                Err(TxAdmissionError::Duplicate)
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                let sender = match self.accounts.get_mut(&sender_addr) {
                    Some(s) => s,
                    None => {
                        #[cfg(feature = "telemetry")]
                        #[cfg(feature = "telemetry-json")]
                        log_event(
                            log::Level::Warn,
                            "reject",
                            &sender_addr,
                            nonce,
                            "unknown_sender",
                            None,
                        );
                        #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                        warn!(
                            "tx rejected sender={sender_addr} nonce={nonce} reason=unknown_sender"
                        );
                        #[cfg(feature = "telemetry")]
                        self.record_reject();
                        return Err(TxAdmissionError::UnknownSender);
                    }
                };
                let required_consumer = sender
                    .pending
                    .consumer
                    .checked_add(total_consumer)
                    .ok_or(TxAdmissionError::BalanceOverflow)?;
                let required_industrial = sender
                    .pending
                    .industrial
                    .checked_add(total_industrial)
                    .ok_or(TxAdmissionError::BalanceOverflow)?;
                if sender.balance.consumer < required_consumer
                    || sender.balance.industrial < required_industrial
                {
                    #[cfg(feature = "telemetry")]
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Warn,
                        "reject",
                        &sender_addr,
                        nonce,
                        "insufficient_balance",
                        None,
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    warn!("tx rejected sender={sender_addr} nonce={nonce} reason=insufficient_balance");
                    #[cfg(feature = "telemetry")]
                    self.record_reject();
                    return Err(TxAdmissionError::InsufficientBalance);
                }
                if sender.pending.nonces.contains(&nonce) {
                    #[cfg(feature = "telemetry")]
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Warn,
                        "reject",
                        &sender_addr,
                        nonce,
                        "duplicate",
                        None,
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    warn!("tx rejected sender={sender_addr} nonce={nonce} reason=duplicate");
                    #[cfg(feature = "telemetry")]
                    {
                        telemetry::DUP_TX_REJECT_TOTAL.inc();
                        self.record_reject();
                    }
                    return Err(TxAdmissionError::Duplicate);
                }
                if nonce != sender.nonce + sender.pending.nonce + 1 {
                    #[cfg(feature = "telemetry")]
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Warn,
                        "reject",
                        &sender_addr,
                        nonce,
                        "bad_nonce",
                        None,
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    warn!("tx rejected sender={sender_addr} nonce={nonce} reason=bad_nonce");
                    #[cfg(feature = "telemetry")]
                    self.record_reject();
                    return Err(TxAdmissionError::BadNonce);
                }
                // fee per byte check
                let size = bincode::serialize(&tx)
                    .map_err(|_| TxAdmissionError::FeeOverflow)?
                    .len() as u64;
                let fee_per_byte = if size == 0 { 0 } else { tx.payload.fee / size };
                if fee_per_byte < self.min_fee_per_byte {
                    #[cfg(feature = "telemetry")]
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Warn,
                        "reject",
                        &sender_addr,
                        nonce,
                        "fee_too_low",
                        Some(fee_per_byte),
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    warn!("tx rejected sender={sender_addr} nonce={nonce} reason=fee_too_low");
                    #[cfg(feature = "telemetry")]
                    {
                        telemetry::FEE_FLOOR_REJECT_TOTAL.inc();
                        self.record_reject();
                    }
                    return Err(TxAdmissionError::FeeTooLow);
                }
                if !verify_signed_tx(tx.clone()) {
                    #[cfg(feature = "telemetry")]
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Warn,
                        "reject",
                        &sender_addr,
                        nonce,
                        "bad_signature",
                        None,
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    warn!("tx rejected sender={sender_addr} nonce={nonce} reason=bad_signature");
                    #[cfg(feature = "telemetry")]
                    self.record_reject();
                    return Err(TxAdmissionError::BadSignature);
                }
                if sender.pending.nonce as usize >= self.max_pending_per_account {
                    #[cfg(feature = "telemetry")]
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Warn,
                        "reject",
                        &sender_addr,
                        nonce,
                        "pending_limit",
                        None,
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    warn!("tx rejected sender={sender_addr} nonce={nonce} reason=pending_limit");
                    #[cfg(feature = "telemetry")]
                    self.record_reject();
                    return Err(TxAdmissionError::PendingLimitReached);
                }
                {
                    let guard = ReservationGuard::new(
                        lock_guard,
                        &mut sender.pending,
                        total_consumer,
                        total_industrial,
                        nonce,
                    );
                    #[cfg(feature = "telemetry")]
                    let tx_id = tx.id();
                    vacant.insert(MempoolEntry {
                        tx,
                        timestamp_millis: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_else(|e| panic!("time: {e}"))
                            .as_millis() as u64,
                    });
                    guard.commit();
                    #[cfg(feature = "telemetry")]
                    self.record_admit();
                    #[cfg(feature = "telemetry-json")]
                    log_event(
                        log::Level::Info,
                        "admit",
                        &sender_addr,
                        nonce,
                        "ok",
                        Some(fee_per_byte),
                    );
                    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
                    info!(
                        "tx accepted sender={sender_addr} nonce={nonce} reason=accepted id={}",
                        hex::encode(tx_id)
                    );
                }
                self.inc_mempool_size();
                Ok(())
            }
        }
    }

    /// Remove a pending transaction and release reserved balances.
    ///
    /// # Errors
    /// Returns [`TxAdmissionError::NotFound`] if the transaction is absent.
    pub fn drop_transaction(&mut self, sender: &str, nonce: u64) -> Result<(), TxAdmissionError> {
        let _pool_guard = self.mempool_mutex.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            telemetry::LOCK_POISON_TOTAL.inc();
            TxAdmissionError::LockPoisoned
        })?;
        let lock = self
            .admission_locks
            .entry(sender.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        #[cfg(feature = "telemetry")]
        let _guard = {
            let span = tracing::span!(tracing::Level::TRACE, "admission_lock", sender = %sender, nonce = nonce);
            span.in_scope(|| lock.lock()).map_err(|_| {
                telemetry::LOCK_POISON_TOTAL.inc();
                TxAdmissionError::LockPoisoned
            })?
        };
        #[cfg(not(feature = "telemetry"))]
        let _guard = lock.lock().map_err(|_| {
            #[cfg(feature = "telemetry")]
            telemetry::LOCK_POISON_TOTAL.inc();
            TxAdmissionError::LockPoisoned
        })?;
        if let Some((_, entry)) = self.mempool.remove(&(sender.to_string(), nonce)) {
            self.dec_mempool_size();
            let tx = entry.tx;
            if let Some(acc) = self.accounts.get_mut(sender) {
                if let Ok((fee_consumer, fee_industrial)) =
                    crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
                {
                    let total_consumer = tx.payload.amount_consumer + fee_consumer;
                    let total_industrial = tx.payload.amount_industrial + fee_industrial;
                    acc.pending.consumer = acc.pending.consumer.saturating_sub(total_consumer);
                    acc.pending.industrial =
                        acc.pending.industrial.saturating_sub(total_industrial);
                    acc.pending.nonce = acc.pending.nonce.saturating_sub(1);
                    acc.pending.nonces.remove(&nonce);
                }
            }
            if !self.accounts.contains_key(sender) {
                self.orphan_counter
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
            #[cfg(feature = "telemetry-json")]
            log_event(log::Level::Info, "drop", sender, nonce, "dropped", None);
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            info!("tx dropped sender={sender} nonce={nonce} reason=dropped");
            Ok(())
        } else {
            #[cfg(feature = "telemetry-json")]
            log_event(log::Level::Warn, "drop", sender, nonce, "not_found", None);
            #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
            warn!("drop failed sender={sender} nonce={nonce} reason=not_found");
            Err(TxAdmissionError::NotFound)
        }
    }

    pub fn purge_expired(&mut self) {
        let ttl_ms = self.tx_ttl * 1000;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time: {e}"))
            .as_millis() as u64;
        let mut expired = Vec::new();
        let mut orphaned = Vec::new();
        #[cfg(feature = "telemetry")]
        let _span = tracing::span!(tracing::Level::TRACE, "eviction_sweep").entered();
        for entry in self.mempool.iter() {
            let sender = &entry.key().0;
            let nonce = entry.key().1;
            if now.saturating_sub(entry.value().timestamp_millis) > ttl_ms {
                #[cfg(feature = "telemetry")]
                telemetry::TTL_DROP_TOTAL.inc();
                expired.push((sender.clone(), nonce));
            } else if !self.accounts.contains_key(sender) {
                self.orphan_counter
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                orphaned.push((sender.clone(), nonce));
            }
        }
        for (sender, nonce) in expired {
            let _ = self.drop_transaction(&sender, nonce);
        }
        let size = self.mempool_size.load(std::sync::atomic::Ordering::SeqCst);
        let orphans = self.orphan_counter.load(std::sync::atomic::Ordering::SeqCst);
        if size > 0 && orphans * 2 > size {
            #[cfg(feature = "telemetry")]
            telemetry::ORPHAN_SWEEP_TOTAL.inc();
            for (sender, nonce) in orphaned {
                let _ = self.drop_transaction(&sender, nonce);
            }
            self.orphan_counter
                .store(0, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[must_use]
    pub fn current_chain_length(&self) -> usize {
        self.chain.len()
    }

    /// Mine a new block and credit rewards to `miner_addr`.
    ///
    /// # Errors
    /// Returns a [`PyValueError`] if fee or nonce calculations overflow or if
    /// persisting the chain fails.
    #[allow(clippy::too_many_lines)]
    pub fn mine_block(&mut self, miner_addr: &str) -> PyResult<Block> {
        let index = self.chain.len() as u64;
        let prev_hash = if index == 0 {
            "0".repeat(64)
        } else {
            self.chain
                .last()
                .map(|b| b.hash.clone())
                .ok_or_else(|| PyValueError::new_err("empty chain"))?
        };

        // apply decay first so reward reflects current height
        self.block_reward_consumer = TokenAmount::new(
            u64::try_from(
                (u128::from(self.block_reward_consumer.0) * u128::from(DECAY_NUMERATOR))
                    / u128::from(DECAY_DENOMINATOR),
            )
            .map_err(|_| PyValueError::new_err("reward overflow"))?,
        );
        self.block_reward_industrial = TokenAmount::new(
            u64::try_from(
                (u128::from(self.block_reward_industrial.0) * u128::from(DECAY_NUMERATOR))
                    / u128::from(DECAY_DENOMINATOR),
            )
            .map_err(|_| PyValueError::new_err("reward overflow"))?,
        );
        let mut reward_consumer = self.block_reward_consumer;
        let mut reward_industrial = self.block_reward_industrial;
        if self.emission_consumer + reward_consumer.0 > MAX_SUPPLY_CONSUMER {
            reward_consumer = TokenAmount::new(MAX_SUPPLY_CONSUMER - self.emission_consumer);
        }
        if self.emission_industrial + reward_industrial.0 > MAX_SUPPLY_INDUSTRIAL {
            reward_industrial = TokenAmount::new(MAX_SUPPLY_INDUSTRIAL - self.emission_industrial);
        }

        self.skipped.clear();
        let mut pending: Vec<SignedTransaction> =
            self.mempool.iter().map(|e| e.value().tx.clone()).collect();
        pending.sort_unstable_by(|a, b| {
            a.payload
                .from_
                .cmp(&b.payload.from_)
                .then(a.payload.nonce.cmp(&b.payload.nonce))
        });
        let mut included = Vec::new();
        let mut skipped = Vec::new();
        let mut expected: HashMap<String, u64> = HashMap::new();
        for tx in pending {
            let exp = expected.entry(tx.payload.from_.clone()).or_insert_with(|| {
                self.accounts
                    .get(&tx.payload.from_)
                    .map(|a| a.nonce + 1)
                    .unwrap_or(1)
            });
            if tx.payload.nonce == *exp {
                included.push(tx);
                *exp += 1;
            } else {
                skipped.push(tx);
            }
        }
        let mut fee_sum_consumer: u128 = 0;
        let mut fee_sum_industrial: u128 = 0;
        for tx in &included {
            if let Ok((fee_consumer, fee_industrial)) =
                crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
            {
                fee_sum_consumer += fee_consumer as u128;
                fee_sum_industrial += fee_industrial as u128;
            }
        }
        let fee_consumer_u64 =
            u64::try_from(fee_sum_consumer).map_err(|_| PyValueError::new_err("Fee overflow"))?;
        let fee_industrial_u64 =
            u64::try_from(fee_sum_industrial).map_err(|_| PyValueError::new_err("Fee overflow"))?;
        let coinbase_consumer = reward_consumer
            .0
            .checked_add(fee_consumer_u64)
            .ok_or_else(|| PyValueError::new_err("Fee overflow"))?;
        let coinbase_industrial = reward_industrial
            .0
            .checked_add(fee_industrial_u64)
            .ok_or_else(|| PyValueError::new_err("Fee overflow"))?;

        let mut fee_hasher = blake3::Hasher::new();
        fee_hasher.update(&fee_consumer_u64.to_le_bytes());
        fee_hasher.update(&fee_industrial_u64.to_le_bytes());
        let fee_checksum = fee_hasher.finalize().to_hex().to_string();

        let coinbase = SignedTransaction {
            payload: RawTxPayload {
                from_: "0".repeat(34),
                to: miner_addr.to_owned(),
                amount_consumer: coinbase_consumer,
                amount_industrial: coinbase_industrial,
                fee: 0,
                fee_selector: 0,
                nonce: 0,
                memo: Vec::new(),
            },
            public_key: vec![],
            signature: vec![],
        };
        let mut txs = vec![coinbase.clone()];
        txs.extend(included.clone());
        let diff = difficulty::expected_difficulty(index, self.difficulty);
        let mut block = Block {
            index,
            previous_hash: prev_hash.clone(),
            transactions: txs.clone(),
            difficulty: diff,
            nonce: 0,
            hash: String::new(),
            coinbase_consumer: TokenAmount::new(coinbase_consumer),
            coinbase_industrial: TokenAmount::new(coinbase_industrial),
            fee_checksum: fee_checksum.clone(),
        };

        let mut nonce = 0u64;
        loop {
            let hash = calculate_hash(
                index,
                &prev_hash,
                nonce,
                diff,
                TokenAmount::new(coinbase_consumer),
                TokenAmount::new(coinbase_industrial),
                &fee_checksum,
                &txs,
            );
            let bytes = hex_to_bytes(&hash);
            if leading_zero_bits(&bytes) >= diff as u32 {
                block.nonce = nonce;
                block.hash = hash.clone();
                self.chain.push(block.clone());
                // CONSENSUS.md §10.3: mempool mutations are guarded by mempool_mutex
                let _pool_guard = self.mempool_mutex.lock().map_err(|_| {
                    #[cfg(feature = "telemetry")]
                    telemetry::LOCK_POISON_TOTAL.inc();
                    PyValueError::new_err("Lock poisoned")
                })?;
                for tx in txs.iter().skip(1) {
                    let lock = self
                        .admission_locks
                        .entry(tx.payload.from_.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone();
                    #[cfg(feature = "telemetry")]
                    let _guard = {
                        let span = tracing::span!(
                            tracing::Level::TRACE,
                            "admission_lock",
                            sender = %tx.payload.from_,
                            nonce = tx.payload.nonce
                        );
                        span.in_scope(|| lock.lock()).map_err(|_| {
                            telemetry::LOCK_POISON_TOTAL.inc();
                            PyValueError::new_err("Lock poisoned")
                        })?
                    };
                    #[cfg(not(feature = "telemetry"))]
                    let _guard = lock.lock().map_err(|_| {
                        #[cfg(feature = "telemetry")]
                        telemetry::LOCK_POISON_TOTAL.inc();
                        PyValueError::new_err("Lock poisoned")
                    })?;

                    if tx.payload.from_ != "0".repeat(34) {
                        if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                            let (fee_consumer, fee_industrial) =
                                crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
                                    .unwrap_or((0, 0));
                            let total_consumer = tx.payload.amount_consumer + fee_consumer;
                            let total_industrial = tx.payload.amount_industrial + fee_industrial;
                            s.balance.consumer = s.balance.consumer.saturating_sub(total_consumer);
                            s.balance.industrial =
                                s.balance.industrial.saturating_sub(total_industrial);
                            s.pending.consumer = s.pending.consumer.saturating_sub(total_consumer);
                            s.pending.industrial =
                                s.pending.industrial.saturating_sub(total_industrial);
                            s.pending.nonce = s.pending.nonce.saturating_sub(1);
                            s.pending.nonces.remove(&tx.payload.nonce);
                            s.nonce = tx.payload.nonce;
                        }
                    }
                    let r = self
                        .accounts
                        .entry(tx.payload.to.clone())
                        .or_insert(Account {
                            address: tx.payload.to.clone(),
                            balance: TokenBalance {
                                consumer: 0,
                                industrial: 0,
                            },
                            nonce: 0,
                            pending: Pending::default(),
                        });
                    r.balance.consumer += tx.payload.amount_consumer;
                    r.balance.industrial += tx.payload.amount_industrial;

                    self.mempool
                        .remove(&(tx.payload.from_.clone(), tx.payload.nonce));
                    self.dec_mempool_size();
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
                            consumer: 0,
                            industrial: 0,
                        },
                        nonce: 0,
                        pending: Pending::default(),
                    });
                miner.balance.consumer = miner
                    .balance
                    .consumer
                    .checked_add(coinbase_consumer)
                    .ok_or_else(|| PyValueError::new_err("miner consumer overflow"))?;
                miner.balance.industrial = miner
                    .balance
                    .industrial
                    .checked_add(coinbase_industrial)
                    .ok_or_else(|| PyValueError::new_err("miner industrial overflow"))?;

                self.emission_consumer += reward_consumer.0;
                self.emission_industrial += reward_industrial.0;
                self.block_height += 1;

                self.persist_chain()?;

                self.db.flush();

                return Ok(block);
            }
            nonce = nonce
                .checked_add(1)
                .ok_or_else(|| PyValueError::new_err("Nonce overflow"))?;
        }
    }

    pub fn validate_block(&self, block: &Block) -> PyResult<bool> {
        let expected_prev = if block.index == 0 {
            "0".repeat(64)
        } else if let Some(pb) = self.chain.get(block.index as usize - 1) {
            pb.hash.clone()
        } else {
            return Err(PyValueError::new_err("Missing previous block"));
        };
        if block.previous_hash != expected_prev {
            return Ok(false);
        }

        if block.difficulty != difficulty::expected_difficulty(block.index, self.difficulty) {
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
            block.nonce,
            block.difficulty,
            block.coinbase_consumer,
            block.coinbase_industrial,
            &block.fee_checksum,
            &block.transactions,
        );
        if calc != block.hash {
            return Ok(false);
        }

        let b = hex_to_bytes(&block.hash);
        if leading_zero_bits(&b)
            < difficulty::expected_difficulty(block.index, self.difficulty) as u32
        {
            return Ok(false);
        }

        if block.transactions[0].payload.amount_consumer != block.coinbase_consumer.0
            || block.transactions[0].payload.amount_industrial != block.coinbase_industrial.0
        {
            return Ok(false);
        }

        let mut seen: HashSet<[u8; 32]> = HashSet::new();
        for tx in &block.transactions {
            if !seen.insert(tx.id()) {
                return Ok(false);
            }
        }
        let mut expected: HashMap<String, u64> = HashMap::new();
        let mut seen_nonce: HashSet<(String, u64)> = HashSet::new();
        let mut fee_tot_consumer: u128 = 0;
        let mut fee_tot_industrial: u128 = 0;
        let mut counts: HashMap<String, u64> = HashMap::new();
        for tx in block.transactions.iter().skip(1) {
            *counts.entry(tx.payload.from_.clone()).or_insert(0) += 1;
            match crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee) {
                Ok((fee_consumer, fee_industrial)) => {
                    fee_tot_consumer += fee_consumer as u128;
                    fee_tot_industrial += fee_industrial as u128;
                }
                Err(_) => return Ok(false),
            }
        }
        for (addr, count) in counts {
            let start = self.accounts.get(&addr).map(|a| a.nonce).unwrap_or(0);
            expected.insert(addr, start.saturating_sub(count));
        }
        for tx in block.transactions.iter().skip(1) {
            let next = expected.entry(tx.payload.from_.clone()).or_insert(0);
            *next += 1;
            if tx.payload.nonce != *next {
                return Ok(false);
            }
            if !seen_nonce.insert((tx.payload.from_.clone(), tx.payload.nonce)) {
                return Ok(false);
            }
        }
        let mut h = blake3::Hasher::new();
        let fee_consumer_u64 =
            u64::try_from(fee_tot_consumer).map_err(|_| PyValueError::new_err("Fee overflow"))?;
        let fee_industrial_u64 =
            u64::try_from(fee_tot_industrial).map_err(|_| PyValueError::new_err("Fee overflow"))?;
        h.update(&fee_consumer_u64.to_le_bytes());
        h.update(&fee_industrial_u64.to_le_bytes());
        if h.finalize().to_hex().to_string() != block.fee_checksum {
            return Ok(false);
        }
        let coinbase_consumer_total = block.coinbase_consumer.0 as u128;
        let coinbase_industrial_total = block.coinbase_industrial.0 as u128;
        if coinbase_consumer_total < fee_tot_consumer
            || coinbase_industrial_total < fee_tot_industrial
        {
            return Ok(false);
        }

        Ok(true)
    }

    pub fn import_chain(&mut self, new_chain: Vec<Block>) -> PyResult<()> {
        if new_chain.len() <= self.chain.len() {
            return Err(PyValueError::new_err("Incoming chain not longer"));
        }
        if !Self::is_valid_chain_rust(&new_chain, 0) {
            return Err(PyValueError::new_err("Invalid incoming chain"));
        }

        self.chain.clear();
        self.accounts.clear();
        self.emission_consumer = 0;
        self.emission_industrial = 0;
        self.block_reward_consumer = TokenAmount::new(INITIAL_BLOCK_REWARD_CONSUMER);
        self.block_reward_industrial = TokenAmount::new(INITIAL_BLOCK_REWARD_INDUSTRIAL);
        self.block_height = 0;

        for block in &new_chain {
            let miner_addr = block
                .transactions
                .first()
                .map(|tx| tx.payload.to.clone())
                .unwrap_or_default();
            let mut fee_tot_consumer: u128 = 0;
            let mut fee_tot_industrial: u128 = 0;
            for tx in block.transactions.iter().skip(1) {
                if tx.payload.from_ != "0".repeat(34) {
                    let pk = to_array_32(&tx.public_key)
                        .ok_or_else(|| PyValueError::new_err("Invalid pubkey in chain"))?;
                    let vk = VerifyingKey::from_bytes(&pk)
                        .map_err(|_| PyValueError::new_err("Invalid pubkey in chain"))?;
                    let sig_bytes = to_array_64(&tx.signature)
                        .ok_or_else(|| PyValueError::new_err("Invalid signature in chain"))?;
                    let sig = Signature::from_bytes(&sig_bytes);
                    let mut msg = domain_tag().to_vec();
                    msg.extend(canonical_payload_bytes(&tx.payload));
                    if vk.verify(&msg, &sig).is_err() {
                        return Err(PyValueError::new_err("Bad tx signature in chain"));
                    }
                    if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                        let (fee_consumer, fee_industrial) =
                            crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee)
                                .unwrap_or((0, 0));
                        s.balance.consumer = s
                            .balance
                            .consumer
                            .saturating_sub(tx.payload.amount_consumer + fee_consumer);
                        s.balance.industrial = s
                            .balance
                            .industrial
                            .saturating_sub(tx.payload.amount_industrial + fee_industrial);
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
                            consumer: 0,
                            industrial: 0,
                        },
                        nonce: 0,
                        pending: Pending::default(),
                    });
                r.balance.consumer += tx.payload.amount_consumer;
                r.balance.industrial += tx.payload.amount_industrial;
            }
            let mut h = blake3::Hasher::new();
            let fee_consumer_u64 = u64::try_from(fee_tot_consumer)
                .map_err(|_| PyValueError::new_err("Fee overflow"))?;
            let fee_industrial_u64 = u64::try_from(fee_tot_industrial)
                .map_err(|_| PyValueError::new_err("Fee overflow"))?;
            h.update(&fee_consumer_u64.to_le_bytes());
            h.update(&fee_industrial_u64.to_le_bytes());
            if h.finalize().to_hex().to_string() != block.fee_checksum {
                return Err(PyValueError::new_err("Fee checksum mismatch"));
            }
            let coinbase_consumer_total = block.coinbase_consumer.0 as u128;
            let coinbase_industrial_total = block.coinbase_industrial.0 as u128;
            if coinbase_consumer_total < fee_tot_consumer
                || coinbase_industrial_total < fee_tot_industrial
            {
                return Err(PyValueError::new_err("Fee mismatch"));
            }
            let miner = self.accounts.entry(miner_addr.clone()).or_insert(Account {
                address: miner_addr.clone(),
                balance: TokenBalance {
                    consumer: 0,
                    industrial: 0,
                },
                nonce: 0,
                pending: Pending::default(),
            });
            miner.balance.consumer = miner
                .balance
                .consumer
                .checked_add(block.coinbase_consumer.0)
                .ok_or_else(|| PyValueError::new_err("miner consumer overflow"))?;
            miner.balance.industrial = miner
                .balance
                .industrial
                .checked_add(block.coinbase_industrial.0)
                .ok_or_else(|| PyValueError::new_err("miner industrial overflow"))?;
            if let Some(cb) = block.transactions.first() {
                if cb.payload.amount_consumer != block.coinbase_consumer.0
                    || cb.payload.amount_industrial != block.coinbase_industrial.0
                {
                    // reject forks that tamper with recorded coinbase totals
                    return Err(PyValueError::new_err("Coinbase mismatch"));
                }
            }
            self.block_reward_consumer = TokenAmount::new(
                u64::try_from(
                    (u128::from(self.block_reward_consumer.0) * u128::from(DECAY_NUMERATOR))
                        / u128::from(DECAY_DENOMINATOR),
                )
                .map_err(|_| PyValueError::new_err("reward overflow"))?,
            );
            self.block_reward_industrial = TokenAmount::new(
                u64::try_from(
                    (u128::from(self.block_reward_industrial.0) * u128::from(DECAY_NUMERATOR))
                        / u128::from(DECAY_DENOMINATOR),
                )
                .map_err(|_| PyValueError::new_err("reward overflow"))?,
            );
            self.emission_consumer += block.coinbase_consumer.0;
            self.emission_industrial += block.coinbase_industrial.0;
            self.chain.push(block.clone());
            self.block_height += 1;
        }

        Ok(())
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
    fn is_valid_chain_rust(chain: &[Block], _difficulty: u32) -> bool {
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
            if b.difficulty != difficulty::expected_difficulty(i as u64, 1) {
                return false;
            }
            if b.transactions.is_empty() {
                return false;
            }
            if b.transactions[0].payload.from_ != "0".repeat(34) {
                return false;
            }
            if b.transactions[0].payload.amount_consumer != b.coinbase_consumer.0
                || b.transactions[0].payload.amount_industrial != b.coinbase_industrial.0
            {
                return false;
            }
            let calc = calculate_hash(
                b.index,
                &b.previous_hash,
                b.nonce,
                b.difficulty,
                b.coinbase_consumer,
                b.coinbase_industrial,
                &b.fee_checksum,
                &b.transactions,
            );
            if calc != b.hash {
                return false;
            }
            let bytes = hex_to_bytes(&b.hash);
            if leading_zero_bits(&bytes) < difficulty::expected_difficulty(i as u64, 1) as u32 {
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
                    let sig_bytes = match to_array_64(&tx.signature) {
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
                match crate::fee::decompose(tx.payload.fee_selector, tx.payload.fee) {
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
            let coinbase_consumer_total = b.coinbase_consumer.0 as u128;
            let coinbase_industrial_total = b.coinbase_industrial.0 as u128;
            if coinbase_consumer_total < fee_tot_consumer
                || coinbase_industrial_total < fee_tot_industrial
            {
                return false;
            }
        }
        true
    }
}

impl Drop for Blockchain {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
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
        self.orphan_counter
            .load(std::sync::atomic::Ordering::SeqCst)
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

/// Deterministic block hashing as per `docs/detailed_updates.md`.
/// Field order is fixed; all integers are little-endian.
fn calculate_hash(
    index: u64,
    prev: &str,
    nonce: u64,
    difficulty: u64,
    coin_c: TokenAmount,
    coin_i: TokenAmount,
    fee_checksum: &str,
    txs: &[SignedTransaction],
) -> String {
    let ids: Vec<[u8; 32]> = txs.iter().map(SignedTransaction::id).collect();
    let id_refs: Vec<&[u8]> = ids.iter().map(<[u8; 32]>::as_ref).collect();
    let enc = crate::hashlayout::BlockEncoder {
        index,
        prev,
        nonce,
        difficulty,
        coin_c: coin_c.0,
        coin_i: coin_i.0,
        fee_checksum,
        tx_ids: &id_refs,
    };
    enc.hash()
}

/// Generate a new Ed25519 keypair.
///
/// Returns the private and public key as raw byte vectors. The keys are
/// suitable for both transaction signing and simple message authentication.
#[must_use]
#[pyfunction]
pub fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = OsRng;
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
#[pyfunction]
#[allow(clippy::needless_pass_by_value)]
pub fn sign_message(private: Vec<u8>, message: Vec<u8>) -> PyResult<Vec<u8>> {
    let sk_bytes =
        to_array_32(&private).ok_or_else(|| PyValueError::new_err("Invalid private key length"))?;
    let sk = SigningKey::from_bytes(&sk_bytes);
    Ok(sk.sign(&message).to_bytes().to_vec())
}

/// Verify a message signature produced by [`sign_message`].
#[must_use]
#[pyfunction]
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

/// Return the integer network identifier used in domain separation.
#[must_use]
#[pyfunction]
pub fn chain_id_py() -> u32 {
    CHAIN_ID
}

/// Initialize the Python module.
///
/// # Errors
/// Returns [`PyErr`] if registering classes or functions with the module fails.
#[pymodule]
pub fn the_block(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Blockchain>()?;
    m.add_class::<Block>()?;
    m.add_class::<Account>()?;
    m.add_class::<SignedTransaction>()?;
    m.add_class::<RawTxPayload>()?;
    m.add_class::<TokenBalance>()?;
    m.add_function(wrap_pyfunction!(generate_keypair, m)?)?;
    m.add_function(wrap_pyfunction!(sign_message, m)?)?;
    m.add_function(wrap_pyfunction!(verify_signature, m)?)?;
    m.add_function(wrap_pyfunction!(chain_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(sign_tx_py, m)?)?;
    m.add_function(wrap_pyfunction!(verify_signed_tx_py, m)?)?;
    m.add_function(wrap_pyfunction!(canonical_payload_py, m)?)?;
    m.add_function(wrap_pyfunction!(fee::decompose_py, m)?)?;
    m.add("ErrFeeOverflow", fee::ErrFeeOverflow::type_object(m.py()))?;
    m.add(
        "ErrInvalidSelector",
        fee::ErrInvalidSelector::type_object(m.py()),
    )?;
    m.add("ErrUnknownSender", ErrUnknownSender::type_object(m.py()))?;
    m.add(
        "ErrInsufficientBalance",
        ErrInsufficientBalance::type_object(m.py()),
    )?;
    m.add("ErrBadNonce", ErrBadNonce::type_object(m.py()))?;
    m.add("ErrBadSignature", ErrBadSignature::type_object(m.py()))?;
    m.add("ErrDuplicateTx", ErrDuplicateTx::type_object(m.py()))?;
    m.add("ErrTxNotFound", ErrTxNotFound::type_object(m.py()))?;
    m.add("ErrFeeTooLow", ErrFeeTooLow::type_object(m.py()))?;
    m.add("ErrMempoolFull", ErrMempoolFull::type_object(m.py()))?;
    m.add("ErrLockPoisoned", ErrLockPoisoned::type_object(m.py()))?;
    m.add("ErrPendingLimit", ErrPendingLimit::type_object(m.py()))?;
    Ok(())
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn reservation_rollback_on_panic(cons in 0u64..1000, ind in 0u64..1000) {
            let mut pending = Pending::default();
            let lock = Mutex::new(());
            let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let res = ReservationGuard::new(guard, &mut pending, cons, ind, 1);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                drop(res);
                panic!("boom");
            }));
            assert_eq!(pending.consumer, 0);
            assert_eq!(pending.industrial, 0);
            assert_eq!(pending.nonce, 0);
            assert!(pending.nonces.is_empty());
        }
    }
}
