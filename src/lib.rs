use pyo3::prelude::*;
use pyo3::exceptions::PyValueError;
use std::collections::HashMap;
use blake3;
use std::convert::TryInto;
use hex;
use sled::{Db, IVec};
use serde::{Serialize, Deserialize};
use bincode;
use rand::rngs::OsRng;
use rand::RngCore;
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};


const BLOCK_REWARD_CONSUMER: u64 = 50;
const BLOCK_REWARD_INDUSTRIAL: u64 = 50;

// === Helpers for ed25519 v2.x ([u8; 32], [u8; 64]) ===
fn to_array_32(bytes: &[u8]) -> [u8; 32] {
    bytes.try_into().expect("Expected 32 bytes")
}
fn to_array_64(bytes: &[u8]) -> [u8; 64] {
    bytes.try_into().expect("Expected 64 bytes")
}
fn hex_to_bytes_vec(hex: &str) -> Vec<u8> {
    hex::decode(hex).expect("Invalid hex string")
}
fn bytes_to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

// === Basic types ===

#[pyclass]
#[derive(Clone)]
pub struct TokenBalance {
    #[pyo3(get, set)]
    pub consumer: u64,
    #[pyo3(get, set)]
    pub industrial: u64,
}

#[pyclass]
pub struct Account {
    #[pyo3(get)]
    pub address: String,
    #[pyo3(get)]
    pub balance: TokenBalance,
}

#[pyclass]
#[derive(Clone, Serialize, Deserialize)]
pub struct Transaction {
    #[pyo3(get)]
    pub from: String,
    #[pyo3(get)]
    pub to: String,
    #[pyo3(get)]
    pub amount_consumer: u64,
    #[pyo3(get)]
    pub amount_industrial: u64,
    #[pyo3(get)]
    pub fee: u64,
    #[pyo3(get, set)]
    pub public_key: Vec<u8>,
    #[pyo3(get, set)]
    pub signature: Vec<u8>,
}

#[pyclass]
#[derive(Clone, Serialize, Deserialize)]
pub struct Block {
    #[pyo3(get)]
    pub index: u64,
    #[pyo3(get)]
    pub previous_hash: String,
    #[pyo3(get)]
    pub transactions: Vec<Transaction>,
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
    pub mempool: Vec<Transaction>,
    db: Db,
}

#[pymethods]
impl Blockchain {
    #[new]
    pub fn new() -> Self {
        let db = sled::open("chain_db").expect("DB open");
        let chain: Vec<Block> = db
            .get("chain")
            .ok()
            .flatten()
            .and_then(|ivec: IVec| bincode::deserialize(&ivec).ok())
            .unwrap_or_default();

        Blockchain {
            chain,
            accounts: HashMap::new(),
            difficulty: 1_000_000,
            mempool: Vec::new(),
            db,
        }
    }

    pub fn genesis_block(&mut self) -> PyResult<()> {
        let genesis = Block {
            index: 0,
            previous_hash: "0".repeat(64),
            transactions: vec![],
            nonce: 0,
            hash: "genesis_hash_placeholder".to_string(),
        };
        self.chain.push(genesis);
        Ok(())
    }

    pub fn add_account(&mut self, address: String, consumer: u64, industrial: u64) -> PyResult<()> {
        if self.accounts.contains_key(&address) {
            return Err(PyValueError::new_err("Account already exists"));
        }
        let acc = Account {
            address: address.clone(),
            balance: TokenBalance { consumer, industrial },
        };
        self.accounts.insert(address, acc);
        Ok(())
    }

    pub fn get_account_balance(&self, address: String) -> PyResult<TokenBalance> {
        self.accounts
            .get(&address)
            .map(|acc| acc.balance.clone())
            .ok_or_else(|| PyValueError::new_err("Account not found"))
    }

    pub fn submit_transaction(
        &mut self,
        from: String,
        to: String,
        amount_consumer: u64,
        amount_industrial: u64,
        fee: u64,
        public_key: Vec<u8>,
        signature: Vec<u8>,
    ) -> PyResult<()> {
        let sender = self
            .accounts
            .get_mut(&from)
            .ok_or_else(|| PyValueError::new_err("Sender account not found"))?;

        if sender.balance.consumer < amount_consumer + fee
            || sender.balance.industrial < amount_industrial + fee
        {
            return Err(PyValueError::new_err("Insufficient balance"));
        }

        // Construct message for signature check
        let mut msg = Vec::new();
        msg.extend(from.as_bytes());
        msg.extend(to.as_bytes());
        msg.extend(&amount_consumer.to_le_bytes());
        msg.extend(&amount_industrial.to_le_bytes());
        msg.extend(&fee.to_le_bytes());

        let pubkey = VerifyingKey::from_bytes(&to_array_32(&public_key))
            .map_err(|_| PyValueError::new_err("Invalid public key"))?;
        let sig = Signature::from_bytes(&to_array_64(&signature));
        if pubkey.verify(&msg, &sig).is_err() {
            return Err(PyValueError::new_err("Signature verification failed"));
        }

        sender.balance.consumer -= amount_consumer + fee;
        sender.balance.industrial -= amount_industrial + fee;

        let receiver = self.accounts.entry(to.clone()).or_insert(Account {
            address: to.clone(),
            balance: TokenBalance {
                consumer: 0,
                industrial: 0,
            },
        });
        receiver.balance.consumer += amount_consumer;
        receiver.balance.industrial += amount_industrial;

        let tx = Transaction {
            from,
            to,
            amount_consumer,
            amount_industrial,
            fee,
            public_key,
            signature,
        };
        self.add_transaction_to_mempool(tx)?;

        Ok(())
    }

    pub fn current_chain_length(&self) -> usize {
        self.chain.len()
    }

    pub fn add_transaction_to_mempool(&mut self, tx: Transaction) -> PyResult<()> {
        self.mempool.push(tx);
        Ok(())
    }

    pub fn mine_block(&mut self) -> PyResult<Block> {
        let index = self.chain.len() as u64;
        let prev_hash = if index == 0 {
            "0".repeat(64)
        } else {
            self.chain.last().unwrap().hash.clone()
        };

        let mut txs = vec![Transaction {
            from: "0".repeat(34),
            to: "miner".to_string(),  // TODO: real miner addr
            amount_consumer: BLOCK_REWARD_CONSUMER,
            amount_industrial: BLOCK_REWARD_INDUSTRIAL,
            fee: 0,
            public_key: vec![],
            signature: vec![],
        }];
        txs.extend(self.mempool.clone());
        self.mempool.clear();

        let mut nonce = 0u64;
        loop {
            let hash = calculate_hash(index, &prev_hash, nonce, &txs);
            let hash_bytes = hex_to_bytes(&hash);
            let zeros = leading_zero_bits(&hash_bytes);
            if zeros >= self.difficulty as u32 {
                let block = Block {
                    index,
                    previous_hash: prev_hash,
                    transactions: txs,
                    nonce,
                    hash,
                };
                self.chain.push(block.clone());
                for tx in &block.transactions {
                    // If not present, create receiver
                    let receiver = self.accounts.entry(tx.to.clone()).or_insert(Account {
                        address: tx.to.clone(),
                        balance: TokenBalance {
                            consumer: 0,
                            industrial: 0,
                        },
                    });
                    receiver.balance.consumer += tx.amount_consumer;
                    receiver.balance.industrial += tx.amount_industrial;

                    // If not coinbase, subtract from sender
                    if tx.from != "0".repeat(34) {
                        if let Some(sender) = self.accounts.get_mut(&tx.from) {
                            sender.balance.consumer = sender.balance.consumer.saturating_sub(tx.amount_consumer + tx.fee);
                            sender.balance.industrial = sender.balance.industrial.saturating_sub(tx.amount_industrial + tx.fee);
                        }
                    }
                }
                // persist chain
                self.db
                    .insert("chain", bincode::serialize(&self.chain).unwrap())
                    .unwrap();
                self.db.flush().unwrap();

                return Ok(block);
            }
            nonce = nonce.checked_add(1).ok_or_else(|| PyValueError::new_err("Nonce overflow"))?;
        }
    }

    pub fn validate_block(&self, block: &Block) -> PyResult<bool> {
        let index = block.index;
        let prev_hash_expected = if index == 0 {
            "0".repeat(64)
        } else if let Some(prev_block) = self.chain.get(index as usize - 1) {
            prev_block.hash.clone()
        } else {
            return Err(PyValueError::new_err("Previous block not found"));
        };

        if block.previous_hash != prev_hash_expected {
            return Ok(false);
        }

        let hash_check = calculate_hash(
            block.index,
            &block.previous_hash,
            block.nonce,
            &block.transactions,
        );
        if hash_check != block.hash {
            return Ok(false);
        }

        let hash_val = u64::from_be_bytes(block.hash.as_bytes()[..8].try_into().unwrap_or([0u8; 8]));
        Ok(hash_val < self.difficulty)
    }
}

fn leading_zero_bits(hash: &[u8]) -> u32 {
    let mut count = 0;
    for byte in hash {
        if *byte == 0 {
            count += 8;
        } else {
            count += byte.leading_zeros();
            break;
        }
    }
    count
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex::decode(hex).expect("Invalid hex string")
}

// === Ed25519 keygen/sign/verify - CORRECT FOR V2 ===

#[pyfunction]
pub fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    use rand::RngCore;
    let mut rng = rand::rngs::OsRng;
    let mut priv_bytes = [0u8; 32];
    rng.fill_bytes(&mut priv_bytes);
    let signing = SigningKey::from_bytes(&priv_bytes);
    let verifying = signing.verifying_key();
    (priv_bytes.to_vec(), verifying.to_bytes().to_vec())
}

#[pyfunction]
pub fn sign_message(private: Vec<u8>, message: Vec<u8>) -> Vec<u8> {
    let signing = SigningKey::from_bytes(&to_array_32(&private));
    let sig: Signature = signing.sign(&message);
    sig.to_bytes().to_vec()
}

#[pyfunction]
pub fn verify_signature(public: Vec<u8>, message: Vec<u8>, signature: Vec<u8>) -> bool {
    let verifying = match VerifyingKey::from_bytes(&to_array_32(&public)) {
        Ok(vk) => vk,
        Err(_) => return false,
    };
    let sig = Signature::from_bytes(&to_array_64(&signature));
    verifying.verify(&message, &sig).is_ok()
}

// === Python module ===

#[pymodule]
fn the_block(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Blockchain>()?;
    m.add_class::<Block>()?;
    m.add_class::<Account>()?;
    m.add_class::<Transaction>()?;
    m.add_class::<TokenBalance>()?;

    m.add_function(pyo3::wrap_pyfunction!(generate_keypair, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(sign_message, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(verify_signature, m)?)?;
    Ok(())
}

// === Optional bonus: Free function variant of the hash calculator ===

fn calculate_hash(index: u64, prev_hash: &str, nonce: u64, txs: &[Transaction]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&index.to_be_bytes());
    hasher.update(prev_hash.as_bytes());
    hasher.update(&nonce.to_be_bytes());

    for tx in txs {
        hasher.update(tx.from.as_bytes());
        hasher.update(tx.to.as_bytes());
        hasher.update(&tx.amount_consumer.to_be_bytes());
        hasher.update(&tx.amount_industrial.to_be_bytes());
        hasher.update(&tx.fee.to_be_bytes());
    }

    hasher.finalize().to_hex().to_string()
}
