use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;

pub mod transaction;
pub use transaction::{
    canonical_payload_bytes, canonical_payload_py as canonical_payload, sign_tx_py as sign_tx,
    verify_signed_tx_py as verify_signed_tx, RawTxPayload, SignedTransaction,
};
use transaction::{canonical_payload_py, sign_tx_py, verify_signed_tx_py};
pub mod constants;
pub use constants::{domain_tag, CHAIN_ID};

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
const DECAY_DENOMINATOR: u64 = 100000;

// === Helpers for Ed25519 v2.x ([u8;32], [u8;64]) ===
pub(crate) fn to_array_32(bytes: &[u8]) -> Option<[u8; 32]> {
    bytes.try_into().ok()
}
pub(crate) fn to_array_64(bytes: &[u8]) -> Option<[u8; 64]> {
    bytes.try_into().ok()
}
fn hex_to_bytes(hex: &str) -> Vec<u8> {
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
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Account {
    #[pyo3(get)]
    pub address: String,
    #[pyo3(get)]
    pub balance: TokenBalance,
    #[pyo3(get, set)]
    #[serde(default)]
    pub nonce: u64,
    #[serde(default)]
    pub pending_consumer: u64,
    #[serde(default)]
    pub pending_industrial: u64,
    #[serde(default)]
    pub pending_nonce: u64,
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
}

#[pyclass]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub accounts: HashMap<String, Account>,
    #[pyo3(get, set)]
    pub difficulty: u64,
    pub mempool: Vec<SignedTransaction>,
    pub mempool_set: std::collections::HashSet<(String, u64)>,
    db: Db,
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
}

#[pyclass]
#[derive(Serialize, Deserialize)]
pub struct ChainDisk {
    pub schema_version: usize,
    pub chain: Vec<Block>,
    pub accounts: HashMap<String, Account>,
    pub emission_consumer: u64,
    pub emission_industrial: u64,
    pub block_reward_consumer: TokenAmount,
    pub block_reward_industrial: TokenAmount,
    pub block_height: u64,
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
        // Open an existing database and auto-migrate to schema v3.
        // See `docs/detailed_updates.md` for layout history.
        let db = sled::open(path).map_err(|e| PyValueError::new_err(format!("DB open: {e}")))?;
        let (chain, accounts, em_c, em_i, br_c, br_i, bh) =
            if let Some(raw) = db.get(DB_CHAIN).ok().flatten() {
                match bincode::deserialize::<ChainDisk>(&raw) {
                    Ok(disk) => {
                        if disk.schema_version > 3 {
                            return Err(PyValueError::new_err("DB schema too new"));
                        }
                        if disk.schema_version < 3 {
                            let migrated = ChainDisk {
                                schema_version: 3,
                                ..disk
                            };
                            db.insert(DB_CHAIN, bincode::serialize(&migrated).unwrap())
                                .unwrap();
                            // Drop legacy column families after migrating to consolidated ChainDisk
                            let _ = db.remove(DB_ACCOUNTS);
                            let _ = db.remove(DB_EMISSION);
                            (
                                migrated.chain,
                                migrated.accounts,
                                migrated.emission_consumer,
                                migrated.emission_industrial,
                                migrated.block_reward_consumer,
                                migrated.block_reward_industrial,
                                migrated.block_height,
                            )
                        } else {
                            (
                                disk.chain,
                                disk.accounts,
                                disk.emission_consumer,
                                disk.emission_industrial,
                                disk.block_reward_consumer,
                                disk.block_reward_industrial,
                                disk.block_height,
                            )
                        }
                    }
                    Err(_) => {
                        let chain: Vec<Block> = bincode::deserialize(&raw).unwrap_or_default();
                        let accounts: HashMap<String, Account> = db
                            .get(DB_ACCOUNTS)
                            .ok()
                            .flatten()
                            .and_then(|iv| bincode::deserialize(&iv).ok())
                            .unwrap_or_default();
                        let (em_c, em_i, br_c, br_i, bh): (u64, u64, u64, u64, u64) = db
                            .get(DB_EMISSION)
                            .ok()
                            .flatten()
                            .and_then(|iv| bincode::deserialize(&iv).ok())
                            .unwrap_or((
                                0,
                                0,
                                INITIAL_BLOCK_REWARD_CONSUMER,
                                INITIAL_BLOCK_REWARD_INDUSTRIAL,
                                0,
                            ));
                        let mut new_chain = chain.clone();
                        for b in &mut new_chain {
                            b.coinbase_consumer = TokenAmount::new(0);
                            b.coinbase_industrial = TokenAmount::new(0);
                        }
                        let disk_new = ChainDisk {
                            schema_version: 3,
                            chain: new_chain.clone(),
                            accounts: accounts.clone(),
                            emission_consumer: em_c,
                            emission_industrial: em_i,
                            block_reward_consumer: TokenAmount::new(br_c),
                            block_reward_industrial: TokenAmount::new(br_i),
                            block_height: bh,
                        };
                        db.insert(DB_CHAIN, bincode::serialize(&disk_new).unwrap())
                            .unwrap();
                        // Remove legacy shard keys; all state now in ChainDisk
                        let _ = db.remove(DB_ACCOUNTS);
                        let _ = db.remove(DB_EMISSION);
                        (
                            disk_new.chain,
                            disk_new.accounts,
                            disk_new.emission_consumer,
                            disk_new.emission_industrial,
                            disk_new.block_reward_consumer,
                            disk_new.block_reward_industrial,
                            disk_new.block_height,
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
                )
            };
        Ok(Blockchain {
            chain,
            accounts,
            difficulty: 8,
            mempool: Vec::new(),
            mempool_set: HashSet::new(),
            db,
            emission_consumer: em_c,
            emission_industrial: em_i,
            block_reward_consumer: br_c,
            block_reward_industrial: br_i,
            block_height: bh,
        })
    }

    /// Return the on-disk schema version
    #[getter]
    pub fn schema_version(&self) -> usize {
        // Bump this constant whenever the serialized `ChainDisk` format changes.
        // Older binaries must refuse to open newer databases.
        3
    }

    /// Persist the entire chain + state under the current schema
    pub fn persist_chain(&self) -> PyResult<()> {
        let disk = ChainDisk {
            schema_version: self.schema_version(),
            chain: self.chain.clone(),
            accounts: self.accounts.clone(),
            emission_consumer: self.emission_consumer,
            emission_industrial: self.emission_industrial,
            block_reward_consumer: self.block_reward_consumer,
            block_reward_industrial: self.block_reward_industrial,
            block_height: self.block_height,
        };
        let bytes = bincode::serialize(&disk)
            .map_err(|e| PyValueError::new_err(format!("Serialization error: {e}")))?;
        self.db
            .insert(DB_CHAIN, bytes)
            .map_err(|e| PyValueError::new_err(format!("DB insert: {e}")))?;
        // ensure no legacy column families linger on disk
        let _ = self.db.remove(DB_ACCOUNTS);
        let _ = self.db.remove(DB_EMISSION);
        self.db
            .flush()
            .map_err(|e| PyValueError::new_err(format!("DB flush: {e}")))?;
        Ok(())
    }

    pub fn circulating_supply(&self) -> (u64, u64) {
        (self.emission_consumer, self.emission_industrial)
    }

    pub fn genesis_block(&mut self) -> PyResult<()> {
        let g = Block {
            index: 0,
            previous_hash: "0".repeat(64),
            transactions: vec![],
            difficulty: self.difficulty,
            nonce: 0,
            hash: "genesis_hash_placeholder".to_string(),
            // genesis carries zero reward; fields included for stable hashing
            coinbase_consumer: TokenAmount::new(0),
            coinbase_industrial: TokenAmount::new(0),
        };
        self.chain.push(g);
        self.block_height = 1;
        self.db
            .insert(DB_CHAIN, bincode::serialize(&self.chain).unwrap())
            .unwrap();
        self.db.flush().unwrap();
        Ok(())
    }

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
            pending_consumer: 0,
            pending_industrial: 0,
            pending_nonce: 0,
        };
        self.accounts.insert(address, acc);
        Ok(())
    }

    pub fn get_account_balance(&self, address: String) -> PyResult<TokenBalance> {
        self.accounts
            .get(&address)
            .map(|a| a.balance.clone())
            .ok_or_else(|| PyValueError::new_err("Account not found"))
    }

    pub fn submit_transaction(&mut self, tx: SignedTransaction) -> PyResult<()> {
        let sender_addr = tx.payload.from_.clone();

        if self
            .mempool_set
            .contains(&(sender_addr.clone(), tx.payload.nonce))
        {
            return Err(PyValueError::new_err("Duplicate transaction"));
        }

        let sender = self
            .accounts
            .get_mut(&sender_addr)
            .ok_or_else(|| PyValueError::new_err("Sender not found"))?;
        let (fee_c, fee_i) = match tx.payload.fee_token {
            0 => (tx.payload.fee, 0),
            1 => (0, tx.payload.fee),
            2 => (tx.payload.fee.div_ceil(2), tx.payload.fee / 2),
            _ => return Err(PyValueError::new_err("Invalid fee_token")),
        };
        if tx.payload.fee >= (1u64 << 63) {
            return Err(PyValueError::new_err("Fee too large"));
        }

        if sender
            .balance
            .consumer
            .saturating_sub(sender.pending_consumer)
            < tx.payload.amount_consumer + fee_c
            || sender
                .balance
                .industrial
                .saturating_sub(sender.pending_industrial)
                < tx.payload.amount_industrial + fee_i
        {
            return Err(PyValueError::new_err("Insufficient balance"));
        }

        if tx.payload.nonce != sender.nonce + sender.pending_nonce + 1 {
            return Err(PyValueError::new_err("Bad nonce"));
        }

        if !verify_signed_tx(tx.clone()) {
            return Err(PyValueError::new_err("Signature verification failed"));
        }

        sender.pending_consumer += tx.payload.amount_consumer + fee_c;
        sender.pending_industrial += tx.payload.amount_industrial + fee_i;
        sender.pending_nonce += 1;

        self.mempool_set.insert((sender_addr, tx.payload.nonce));
        self.mempool.push(tx);
        Ok(())
    }

    pub fn current_chain_length(&self) -> usize {
        self.chain.len()
    }

    pub fn mine_block(&mut self, miner_addr: String) -> PyResult<Block> {
        let index = self.chain.len() as u64;
        let prev_hash = if index == 0 {
            "0".repeat(64)
        } else {
            self.chain.last().unwrap().hash.clone()
        };

        // apply decay first so reward reflects current height
        self.block_reward_consumer = TokenAmount::new(
            ((self.block_reward_consumer.0 as u128 * DECAY_NUMERATOR as u128)
                / DECAY_DENOMINATOR as u128) as u64,
        );
        self.block_reward_industrial = TokenAmount::new(
            ((self.block_reward_industrial.0 as u128 * DECAY_NUMERATOR as u128)
                / DECAY_DENOMINATOR as u128) as u64,
        );
        let mut reward_c = self.block_reward_consumer;
        let mut reward_i = self.block_reward_industrial;
        if self.emission_consumer + reward_c.0 > MAX_SUPPLY_CONSUMER {
            reward_c = TokenAmount::new(MAX_SUPPLY_CONSUMER - self.emission_consumer);
        }
        if self.emission_industrial + reward_i.0 > MAX_SUPPLY_INDUSTRIAL {
            reward_i = TokenAmount::new(MAX_SUPPLY_INDUSTRIAL - self.emission_industrial);
        }

        let coinbase = SignedTransaction {
            payload: RawTxPayload {
                from_: "0".repeat(34),
                to: miner_addr.clone(),
                amount_consumer: reward_c.0,
                amount_industrial: reward_i.0,
                fee: 0,
                fee_token: 0,
                nonce: 0,
                memo: Vec::new(),
            },
            public_key: vec![],
            signature: vec![],
        };
        let mut txs = vec![coinbase.clone()];
        txs.extend(self.mempool.clone());
        self.mempool.clear();
        self.mempool_set.clear();
        let mut block = Block {
            index,
            previous_hash: prev_hash.clone(),
            transactions: txs.clone(),
            difficulty: self.difficulty,
            nonce: 0,
            hash: String::new(),
            coinbase_consumer: reward_c,
            coinbase_industrial: reward_i,
        };

        let mut nonce = 0u64;
        loop {
            let hash = calculate_hash(
                index,
                &prev_hash,
                nonce,
                self.difficulty,
                reward_c,
                reward_i,
                &txs,
            );
            let bytes = hex_to_bytes(&hash);
            if leading_zero_bits(&bytes) >= self.difficulty as u32 {
                block.nonce = nonce;
                block.hash = hash.clone();
                self.chain.push(block.clone());

                for tx in &txs {
                    if tx.payload.from_ != "0".repeat(34) {
                        if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                            let (fee_c, fee_i) = match tx.payload.fee_token {
                                0 => (tx.payload.fee, 0),
                                1 => (0, tx.payload.fee),
                                2 => (tx.payload.fee.div_ceil(2), tx.payload.fee / 2),
                                _ => (0, 0),
                            };
                            let total_c = tx.payload.amount_consumer + fee_c;
                            let total_i = tx.payload.amount_industrial + fee_i;
                            s.balance.consumer = s.balance.consumer.saturating_sub(total_c);
                            s.balance.industrial = s.balance.industrial.saturating_sub(total_i);
                            s.pending_consumer = s.pending_consumer.saturating_sub(total_c);
                            s.pending_industrial = s.pending_industrial.saturating_sub(total_i);
                            s.pending_nonce = s.pending_nonce.saturating_sub(1);
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
                            pending_consumer: 0,
                            pending_industrial: 0,
                            pending_nonce: 0,
                        });
                    r.balance.consumer += tx.payload.amount_consumer;
                    r.balance.industrial += tx.payload.amount_industrial;

                    let (fee_c, fee_i) = match tx.payload.fee_token {
                        0 => (tx.payload.fee, 0),
                        1 => (0, tx.payload.fee),
                        2 => (tx.payload.fee.div_ceil(2), tx.payload.fee / 2),
                        _ => (0, 0),
                    };
                    if let Some(miner) = self.accounts.get_mut(&miner_addr) {
                        miner.balance.consumer += fee_c;
                        miner.balance.industrial += fee_i;
                    }
                    self.mempool_set
                        .remove(&(tx.payload.from_.clone(), tx.payload.nonce));
                }

                self.emission_consumer += reward_c.0;
                self.emission_industrial += reward_i.0;
                self.block_height += 1;

                self.persist_chain()?;

                self.db.flush().unwrap();

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

        if block.difficulty != self.difficulty {
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
            &block.transactions,
        );
        if calc != block.hash {
            return Ok(false);
        }

        let b = hex_to_bytes(&block.hash);
        if leading_zero_bits(&b) < self.difficulty as u32 {
            return Ok(false);
        }

        if block.transactions[0].payload.amount_consumer != block.coinbase_consumer.0
            || block.transactions[0].payload.amount_industrial != block.coinbase_industrial.0
        {
            return Ok(false);
        }

        let mut expected: HashMap<String, u64> = HashMap::new();
        let mut seen: HashSet<[u8; 32]> = HashSet::new();
        for tx in &block.transactions {
            if tx.payload.from_ != "0".repeat(34) {
                let next = expected.entry(tx.payload.from_.clone()).or_insert_with(|| {
                    self.accounts
                        .get(&tx.payload.from_)
                        .map(|a| a.nonce + 1)
                        .unwrap_or(1)
                });
                if tx.payload.nonce != *next {
                    return Ok(false);
                }
                *next += 1;
            }
            if !seen.insert(tx.id()) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub fn import_chain(&mut self, new_chain: Vec<Block>) -> PyResult<()> {
        if new_chain.len() <= self.chain.len() {
            return Err(PyValueError::new_err("Incoming chain not longer"));
        }
        if !Self::is_valid_chain_rust(&new_chain, self.difficulty as u32) {
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
            for tx in &block.transactions {
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
                        let (fee_c, fee_i) = match tx.payload.fee_token {
                            0 => (tx.payload.fee, 0),
                            1 => (0, tx.payload.fee),
                            2 => (tx.payload.fee.div_ceil(2), tx.payload.fee / 2),
                            _ => (0, 0),
                        };
                        s.balance.consumer = s
                            .balance
                            .consumer
                            .saturating_sub(tx.payload.amount_consumer + fee_c);
                        s.balance.industrial = s
                            .balance
                            .industrial
                            .saturating_sub(tx.payload.amount_industrial + fee_i);
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
                        pending_consumer: 0,
                        pending_industrial: 0,
                        pending_nonce: 0,
                    });
                r.balance.consumer += tx.payload.amount_consumer;
                r.balance.industrial += tx.payload.amount_industrial;

                let (fee_c, fee_i) = match tx.payload.fee_token {
                    0 => (tx.payload.fee, 0),
                    1 => (0, tx.payload.fee),
                    2 => (tx.payload.fee.div_ceil(2), tx.payload.fee / 2),
                    _ => (0, 0),
                };
                if let Some(miner) = self.accounts.get_mut(&miner_addr) {
                    miner.balance.consumer += fee_c;
                    miner.balance.industrial += fee_i;
                }
            }
            if let Some(cb) = block.transactions.first() {
                if cb.payload.amount_consumer != block.coinbase_consumer.0
                    || cb.payload.amount_industrial != block.coinbase_industrial.0
                {
                    // reject forks that tamper with recorded coinbase totals
                    return Err(PyValueError::new_err("Coinbase mismatch"));
                }
            }
            self.block_reward_consumer = TokenAmount::new(
                ((self.block_reward_consumer.0 as u128 * DECAY_NUMERATOR as u128)
                    / DECAY_DENOMINATOR as u128) as u64,
            );
            self.block_reward_industrial = TokenAmount::new(
                ((self.block_reward_industrial.0 as u128 * DECAY_NUMERATOR as u128)
                    / DECAY_DENOMINATOR as u128) as u64,
            );
            self.emission_consumer += block.coinbase_consumer.0;
            self.emission_industrial += block.coinbase_industrial.0;
            self.chain.push(block.clone());
            self.block_height += 1;
        }

        Ok(())
    }
}

impl Default for Blockchain {
    fn default() -> Self {
        Self::new()
    }
}

impl Blockchain {
    /// Open the default ./chain_db path
    pub fn new() -> Self {
        let db = sled::Config::new().temporary(true).open().expect("DB open");
        let (chain, accounts, em_c, em_i, br_c, br_i, bh) =
            if let Some(raw) = db.get(DB_CHAIN).ok().flatten() {
                if let Ok(disk) = bincode::deserialize::<ChainDisk>(&raw) {
                    (
                        disk.chain,
                        disk.accounts,
                        disk.emission_consumer,
                        disk.emission_industrial,
                        disk.block_reward_consumer,
                        disk.block_reward_industrial,
                        disk.block_height,
                    )
                } else {
                    let chain: Vec<Block> = bincode::deserialize(&raw).unwrap_or_default();
                    let accounts: HashMap<String, Account> = db
                        .get(DB_ACCOUNTS)
                        .ok()
                        .flatten()
                        .and_then(|iv| bincode::deserialize(&iv).ok())
                        .unwrap_or_default();
                    let (em_c, em_i, br_c_u64, br_i_u64, bh): (u64, u64, u64, u64, u64) = db
                        .get(DB_EMISSION)
                        .ok()
                        .flatten()
                        .and_then(|iv| bincode::deserialize(&iv).ok())
                        .unwrap_or((
                            0,
                            0,
                            INITIAL_BLOCK_REWARD_CONSUMER,
                            INITIAL_BLOCK_REWARD_INDUSTRIAL,
                            0,
                        ));
                    (
                        chain,
                        accounts,
                        em_c,
                        em_i,
                        TokenAmount::new(br_c_u64),
                        TokenAmount::new(br_i_u64),
                        bh,
                    )
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
                )
            };
        Blockchain {
            chain,
            accounts,
            difficulty: 8,
            mempool: Vec::new(),
            mempool_set: HashSet::new(),
            db,
            emission_consumer: em_c,
            emission_industrial: em_i,
            block_reward_consumer: br_c,
            block_reward_industrial: br_i,
            block_height: bh,
        }
    }

    #[allow(dead_code)]
    fn is_valid_chain_rust(chain: &[Block], difficulty: u32) -> bool {
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
            if b.difficulty != difficulty as u64 {
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
                &b.transactions,
            );
            if calc != b.hash {
                return false;
            }
            let bytes = hex_to_bytes(&b.hash);
            if leading_zero_bits(&bytes) < difficulty {
                return false;
            }
            let mut expected_nonce: HashMap<String, u64> = HashMap::new();
            let mut seen: HashSet<[u8; 32]> = HashSet::new();
            for tx in &b.transactions {
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
                    let next = expected_nonce
                        .entry(tx.payload.from_.clone())
                        .or_insert_with(|| tx.payload.nonce);
                    if tx.payload.nonce != *next {
                        return false;
                    }
                    *next += 1;
                }
                if !seen.insert(tx.id()) {
                    return false;
                }
            }
        }
        true
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
    txs: &[SignedTransaction],
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&index.to_le_bytes());
    hasher.update(prev.as_bytes());
    hasher.update(&nonce.to_le_bytes());
    hasher.update(&difficulty.to_le_bytes());
    hasher.update(&coin_c.0.to_le_bytes());
    hasher.update(&coin_i.0.to_le_bytes());
    for tx in txs {
        hasher.update(&tx.id());
    }
    hasher.finalize().to_hex().to_string()
}

#[pyfunction]
pub fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = OsRng;
    let mut priv_bytes = [0u8; 32];
    rng.fill_bytes(&mut priv_bytes);
    let sk = SigningKey::from_bytes(&priv_bytes);
    let vk = sk.verifying_key();
    (priv_bytes.to_vec(), vk.to_bytes().to_vec())
}

#[pyfunction]
pub fn sign_message(private: Vec<u8>, message: Vec<u8>) -> PyResult<Vec<u8>> {
    let sk_bytes =
        to_array_32(&private).ok_or_else(|| PyValueError::new_err("Invalid private key length"))?;
    let sk = SigningKey::from_bytes(&sk_bytes);
    Ok(sk.sign(&message).to_bytes().to_vec())
}

#[pyfunction]
pub fn verify_signature(public: Vec<u8>, message: Vec<u8>, signature: Vec<u8>) -> bool {
    if let (Some(pk), Some(sig_bytes)) = (to_array_32(&public), to_array_64(&signature)) {
        if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
            let sig = Signature::from_bytes(&sig_bytes);
            return vk.verify(&message, &sig).is_ok();
        }
    }
    false
}

#[pyfunction]
pub fn chain_id_py() -> u32 {
    CHAIN_ID
}

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
    Ok(())
}
