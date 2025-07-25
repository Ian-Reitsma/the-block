use bincode;
use blake3;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hex;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::HashMap;
use std::convert::TryInto;

pub mod transaction;
pub use transaction::{
    canonical_payload_bytes, py_sign_tx as sign_tx, py_verify_signed_tx as verify_signed_tx,
    RawTxPayload, SignedTransaction,
};

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
pub(crate) fn to_array_32(bytes: &[u8]) -> [u8; 32] {
    bytes.try_into().expect("Expected 32 bytes")
}
pub(crate) fn to_array_64(bytes: &[u8]) -> [u8; 64] {
    bytes.try_into().expect("Expected 64 bytes")
}
fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex::decode(hex).expect("Invalid hex string")
}

// === Data types ===

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
}

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
    pub nonce: u64,
    #[pyo3(get)]
    pub hash: String,
}

#[pyclass]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub accounts: HashMap<String, Account>,
    #[pyo3(get, set)]
    pub difficulty: u64,
    pub mempool: Vec<SignedTransaction>,
    db: Db,
    #[pyo3(get, set)]
    pub emission_consumer: u64,
    #[pyo3(get, set)]
    pub emission_industrial: u64,
    #[pyo3(get, set)]
    pub block_reward_consumer: u64,
    #[pyo3(get, set)]
    pub block_reward_industrial: u64,
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
    pub block_reward_consumer: u64,
    pub block_reward_industrial: u64,
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
        // exactly the same as `new()`, but open sled::open(path)
        let db = sled::open(path).map_err(|e| PyValueError::new_err(format!("DB open: {}", e)))?;
        let chain: Vec<Block> = db
            .get(DB_CHAIN)
            .ok()
            .flatten()
            .and_then(|iv| bincode::deserialize(&iv).ok())
            .unwrap_or_default();
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
        Ok(Blockchain {
            chain,
            accounts,
            difficulty: 8,
            mempool: Vec::new(),
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
        // bump this if you ever change the on-disk format
        1
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
            .map_err(|e| PyValueError::new_err(format!("Serialization error: {}", e)))?;
        self.db
            .insert(DB_CHAIN, bytes)
            .map_err(|e| PyValueError::new_err(format!("DB insert: {}", e)))?;
        self.db
            .flush()
            .map_err(|e| PyValueError::new_err(format!("DB flush: {}", e)))?;
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
            nonce: 0,
            hash: "genesis_hash_placeholder".to_string(),
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
        let receiver_addr = tx.payload.to.clone();

        let sender = self
            .accounts
            .get_mut(&sender_addr)
            .ok_or_else(|| PyValueError::new_err("Sender not found"))?;
        if sender.balance.consumer < tx.payload.amount_consumer + tx.payload.fee
            || sender.balance.industrial < tx.payload.amount_industrial + tx.payload.fee
        {
            return Err(PyValueError::new_err("Insufficient balance"));
        }

        if !verify_signed_tx(tx.clone()) {
            return Err(PyValueError::new_err("Signature verification failed"));
        }

        sender.balance.consumer -= tx.payload.amount_consumer + tx.payload.fee;
        sender.balance.industrial -= tx.payload.amount_industrial + tx.payload.fee;

        let recv = self
            .accounts
            .entry(receiver_addr.clone())
            .or_insert(Account {
                address: receiver_addr.clone(),
                balance: TokenBalance {
                    consumer: 0,
                    industrial: 0,
                },
            });
        recv.balance.consumer += tx.payload.amount_consumer;
        recv.balance.industrial += tx.payload.amount_industrial;

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

        let mut reward_c = self.block_reward_consumer;
        let mut reward_i = self.block_reward_industrial;
        if self.emission_consumer + reward_c > MAX_SUPPLY_CONSUMER {
            reward_c = MAX_SUPPLY_CONSUMER - self.emission_consumer;
        }
        if self.emission_industrial + reward_i > MAX_SUPPLY_INDUSTRIAL {
            reward_i = MAX_SUPPLY_INDUSTRIAL - self.emission_industrial;
        }

        let mut txs = vec![SignedTransaction {
            payload: RawTxPayload {
                from_: "0".repeat(34),
                to: miner_addr.clone(),
                amount_consumer: reward_c,
                amount_industrial: reward_i,
                fee: 0,
            },
            public_key: vec![],
            signature: vec![],
        }];
        txs.extend(self.mempool.clone());
        self.mempool.clear();

        let mut nonce = 0u64;
        loop {
            let hash = calculate_hash(index, &prev_hash, nonce, &txs);
            let bytes = hex_to_bytes(&hash);
            if leading_zero_bits(&bytes) >= self.difficulty as u32 {
                let block = Block {
                    index,
                    previous_hash: prev_hash.clone(),
                    transactions: txs.clone(),
                    nonce,
                    hash: hash.clone(),
                };
                self.chain.push(block.clone());

                for tx in &txs {
                    if tx.payload.from_ != "0".repeat(34) {
                        if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                            s.balance.consumer = s
                                .balance
                                .consumer
                                .saturating_sub(tx.payload.amount_consumer + tx.payload.fee);
                            s.balance.industrial = s
                                .balance
                                .industrial
                                .saturating_sub(tx.payload.amount_industrial + tx.payload.fee);
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
                        });
                    r.balance.consumer += tx.payload.amount_consumer;
                    r.balance.industrial += tx.payload.amount_industrial;
                }

                self.emission_consumer += reward_c;
                self.emission_industrial += reward_i;
                self.block_height += 1;
                self.block_reward_consumer =
                    ((self.block_reward_consumer as u128) * DECAY_NUMERATOR as u128
                        / DECAY_DENOMINATOR as u128) as u64;
                self.block_reward_industrial =
                    ((self.block_reward_industrial as u128) * DECAY_NUMERATOR as u128
                        / DECAY_DENOMINATOR as u128) as u64;

                self.db
                    .insert(DB_CHAIN, bincode::serialize(&self.chain).unwrap())
                    .unwrap();
                self.db
                    .insert(DB_ACCOUNTS, bincode::serialize(&self.accounts).unwrap())
                    .unwrap();
                let state = (
                    self.emission_consumer,
                    self.emission_industrial,
                    self.block_reward_consumer,
                    self.block_reward_industrial,
                    self.block_height,
                );
                self.db
                    .insert(DB_EMISSION, bincode::serialize(&state).unwrap())
                    .unwrap();
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

        let calc = calculate_hash(
            block.index,
            &block.previous_hash,
            block.nonce,
            &block.transactions,
        );
        if calc != block.hash {
            return Ok(false);
        }

        let b = hex_to_bytes(&block.hash);
        if leading_zero_bits(&b) < self.difficulty as u32 {
            return Ok(false);
        }

        Ok(true)
    }

    pub fn import_chain(&mut self, new_chain: Vec<Block>) -> PyResult<()> {
        if new_chain.len() <= self.chain.len() {
            return Err(PyValueError::new_err("Incoming chain not longer"));
        }
        if !Self::is_valid_chain_rust(&new_chain) {
            return Err(PyValueError::new_err("Invalid incoming chain"));
        }

        self.chain.clear();
        self.accounts.clear();
        self.emission_consumer = 0;
        self.emission_industrial = 0;
        self.block_reward_consumer = INITIAL_BLOCK_REWARD_CONSUMER;
        self.block_reward_industrial = INITIAL_BLOCK_REWARD_INDUSTRIAL;
        self.block_height = 0;

        for block in &new_chain {
            for tx in &block.transactions {
                if tx.payload.from_ != "0".repeat(34) {
                    let mut msg = Vec::new();
                    msg.extend(tx.payload.from_.as_bytes());
                    msg.extend(tx.payload.to.as_bytes());
                    msg.extend(&tx.payload.amount_consumer.to_le_bytes());
                    msg.extend(&tx.payload.amount_industrial.to_le_bytes());
                    msg.extend(&tx.payload.fee.to_le_bytes());
                    let vk = VerifyingKey::from_bytes(&to_array_32(&tx.public_key))
                        .map_err(|_| PyValueError::new_err("Invalid pubkey in chain"))?;
                    let sig = Signature::from_bytes(&to_array_64(&tx.signature));
                    if vk.verify(&msg, &sig).is_err() {
                        return Err(PyValueError::new_err("Bad tx signature in chain"));
                    }
                    if let Some(s) = self.accounts.get_mut(&tx.payload.from_) {
                        s.balance.consumer = s
                            .balance
                            .consumer
                            .saturating_sub(tx.payload.amount_consumer + tx.payload.fee);
                        s.balance.industrial = s
                            .balance
                            .industrial
                            .saturating_sub(tx.payload.amount_industrial + tx.payload.fee);
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
                    });
                r.balance.consumer += tx.payload.amount_consumer;
                r.balance.industrial += tx.payload.amount_industrial;
            }
            if let Some(cb) = block.transactions.first() {
                self.emission_consumer += cb.payload.amount_consumer;
                self.emission_industrial += cb.payload.amount_industrial;
            }
            self.chain.push(block.clone());
            self.block_height += 1;
            self.block_reward_consumer = ((self.block_reward_consumer as u128)
                * DECAY_NUMERATOR as u128
                / DECAY_DENOMINATOR as u128) as u64;
            self.block_reward_industrial = ((self.block_reward_industrial as u128)
                * DECAY_NUMERATOR as u128
                / DECAY_DENOMINATOR as u128) as u64;
        }

        Ok(())
    }
}

impl Blockchain {
    /// Open the default ./chain_db path
    pub fn new() -> Self {
        let db = sled::Config::new().temporary(true).open().expect("DB open");
        let chain: Vec<Block> = db
            .get(DB_CHAIN)
            .ok()
            .flatten()
            .and_then(|iv| bincode::deserialize(&iv).ok())
            .unwrap_or_default();
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
        Blockchain {
            chain,
            accounts,
            difficulty: 8,
            mempool: Vec::new(),
            db,
            emission_consumer: em_c,
            emission_industrial: em_i,
            block_reward_consumer: br_c,
            block_reward_industrial: br_i,
            block_height: bh,
        }
    }

    #[allow(dead_code)]
    fn is_valid_chain_rust(chain: &[Block]) -> bool {
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
            let calc = calculate_hash(b.index, &b.previous_hash, b.nonce, &b.transactions);
            if calc != b.hash {
                return false;
            }
            let bytes = hex_to_bytes(&b.hash);
            if u64::from(leading_zero_bits(&bytes)) < chain[0].index
            /* or difficulty */
            {
                return false;
            }
            for tx in &b.transactions {
                if tx.payload.from_ != "0".repeat(34) {
                    let mut msg = Vec::new();
                    msg.extend(tx.payload.from_.as_bytes());
                    msg.extend(tx.payload.to.as_bytes());
                    msg.extend(&tx.payload.amount_consumer.to_le_bytes());
                    msg.extend(&tx.payload.amount_industrial.to_le_bytes());
                    msg.extend(&tx.payload.fee.to_le_bytes());
                    let vk = match VerifyingKey::from_bytes(&to_array_32(&tx.public_key)) {
                        Ok(vk) => vk,
                        Err(_) => return false,
                    };
                    let sig = Signature::from_bytes(&to_array_64(&tx.signature));
                    if vk.verify(&msg, &sig).is_err() {
                        return false;
                    }
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

fn calculate_hash(index: u64, prev: &str, nonce: u64, txs: &[SignedTransaction]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&index.to_be_bytes());
    hasher.update(prev.as_bytes());
    hasher.update(&nonce.to_be_bytes());
    for tx in txs {
        hasher.update(tx.payload.from_.as_bytes());
        hasher.update(tx.payload.to.as_bytes());
        hasher.update(&tx.payload.amount_consumer.to_le_bytes());
        hasher.update(&tx.payload.amount_industrial.to_le_bytes());
        hasher.update(&tx.payload.fee.to_le_bytes());
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
pub fn sign_message(private: Vec<u8>, message: Vec<u8>) -> Vec<u8> {
    let sk = SigningKey::from_bytes(&to_array_32(&private));
    sk.sign(&message).to_bytes().to_vec()
}

#[pyfunction]
pub fn verify_signature(public: Vec<u8>, message: Vec<u8>, signature: Vec<u8>) -> bool {
    if let Ok(vk) = VerifyingKey::from_bytes(&to_array_32(&public)) {
        let sig = Signature::from_bytes(&to_array_64(&signature));
        return vk.verify(&message, &sig).is_ok();
    }
    false
}

#[pymodule]
pub fn the_block(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Blockchain>()?;
    m.add_class::<Block>()?;
    m.add_class::<Account>()?;
    m.add_class::<SignedTransaction>()?;
    m.add_class::<RawTxPayload>()?;
    m.add_class::<TokenBalance>()?;
    m.add_function(wrap_pyfunction!(generate_keypair, m)?)?;
    m.add_function(wrap_pyfunction!(sign_message, m)?)?;
    m.add_function(wrap_pyfunction!(verify_signature, m)?)?;
    m.add_function(wrap_pyfunction!(sign_tx, m)?)?;
    m.add_function(wrap_pyfunction!(verify_signed_tx, m)?)?;
    Ok(())
}
