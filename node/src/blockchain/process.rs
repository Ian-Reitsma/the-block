use std::collections::{HashMap, HashSet};

use crate::{
    simple_db::DbDelta, transaction::verify_stateless, Account, Block, Blockchain, TokenBalance,
    TxAdmissionError,
};
use ledger::{address, shard::ShardState};
use state::MerkleTrie;

#[cfg(feature = "telemetry")]
use crate::telemetry::BLOCK_APPLY_FAIL_TOTAL;

/// State change for a single account.
#[derive(Clone)]
pub struct StateDelta {
    pub address: String,
    pub account: Account,
    pub shard: address::ShardId,
}

/// Validate all transactions in `block` against `chain` without
/// mutating state, returning the updated accounts on success.
pub fn validate_and_apply(
    chain: &Blockchain,
    block: &Block,
) -> Result<Vec<StateDelta>, TxAdmissionError> {
    let mut accounts = chain.accounts.clone();
    let mut touched: HashSet<String> = HashSet::new();
    for tx in block.transactions.iter().skip(1) {
        verify_stateless(tx)?;
        let (fee_c, fee_i) = crate::fee::decompose(tx.payload.pct_ct, tx.payload.fee)
            .map_err(|_| TxAdmissionError::FeeOverflow)?;
        if tx.payload.from_ != "0".repeat(34) {
            let sender = accounts
                .get_mut(&tx.payload.from_)
                .ok_or(TxAdmissionError::UnknownSender)?;
            let total_c = tx.payload.amount_consumer + fee_c;
            let total_i = tx.payload.amount_industrial + fee_i;
            if sender.balance.consumer < total_c || sender.balance.industrial < total_i {
                #[cfg(feature = "telemetry")]
                BLOCK_APPLY_FAIL_TOTAL.inc();
                return Err(TxAdmissionError::InsufficientBalance);
            }
            sender.balance.consumer -= total_c;
            sender.balance.industrial -= total_i;
            sender.nonce = tx.payload.nonce;
            touched.insert(tx.payload.from_.clone());
        }
        let recv = accounts.entry(tx.payload.to.clone()).or_insert(Account {
            address: tx.payload.to.clone(),
            balance: TokenBalance {
                consumer: 0,
                industrial: 0,
            },
            nonce: 0,
            pending_consumer: 0,
            pending_industrial: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
            sessions: Vec::new(),
        });
        recv.balance.consumer += tx.payload.amount_consumer;
        recv.balance.industrial += tx.payload.amount_industrial;
        touched.insert(tx.payload.to.clone());
    }
    let mut deltas = Vec::new();
    for addr in touched {
        if let Some(acc) = accounts.get(&addr) {
            deltas.push(StateDelta {
                address: addr,
                account: acc.clone(),
                shard: address::shard_id(&acc.address),
            });
        }
    }
    Ok(deltas)
}

/// RAII guard for block execution. Mutations are rolled back on drop unless
/// [`commit`](ExecutionContext::commit) is called.
pub struct ExecutionContext<'a> {
    chain: &'a mut Blockchain,
    /// Prior account states for rollback.
    account_deltas: Vec<(String, Option<Account>)>,
    /// Database mutations for rollback.
    db_deltas: Vec<DbDelta>,
    /// Prior shard roots for rollback.
    shard_root_deltas: Vec<(address::ShardId, Option<[u8; 32]>)>,
    /// Prior shard heights for rollback.
    shard_height_deltas: Vec<(address::ShardId, Option<u64>)>,
    committed: bool,
}

impl<'a> ExecutionContext<'a> {
    pub fn new(chain: &'a mut Blockchain) -> Self {
        Self {
            chain,
            account_deltas: Vec::new(),
            db_deltas: Vec::new(),
            shard_root_deltas: Vec::new(),
            shard_height_deltas: Vec::new(),
            committed: false,
        }
    }

    /// Apply state deltas and persist them to the database. Any I/O failure
    /// triggers an automatic rollback when the context drops.
    pub fn apply(&mut self, deltas: Vec<StateDelta>) -> std::io::Result<()> {
        let mut touched_shards: HashSet<address::ShardId> = HashSet::new();
        for delta in deltas {
            let prev = self
                .chain
                .accounts
                .insert(delta.address.clone(), delta.account.clone());
            self.account_deltas.push((delta.address.clone(), prev));
            let key = format!("acct:{}", delta.address);
            let bytes = bincode::serialize(&delta.account)
                .unwrap_or_else(|e| panic!("serialize account: {e}"));
            self.chain
                .write_shard_state(delta.shard, &key, bytes, &mut self.db_deltas)?;
            touched_shards.insert(delta.shard);
        }
        for shard in touched_shards {
            let root = shard_state_root(&self.chain.accounts, shard);
            let prev_root = self.chain.shard_roots.insert(shard, root);
            self.shard_root_deltas.push((shard, prev_root));
            let prev_height = self.chain.shard_heights.get(&shard).copied();
            self.chain
                .shard_heights
                .insert(shard, prev_height.unwrap_or(0) + 1);
            self.shard_height_deltas.push((shard, prev_height));
            let key = ShardState::db_key();
            let bytes = ShardState::new(shard, root).to_bytes();
            self.chain
                .write_shard_state(shard, key, bytes, &mut self.db_deltas)?;
        }
        Ok(())
    }

    /// Finalise block execution by flushing the database.
    pub fn commit(mut self) -> std::io::Result<()> {
        self.chain.db.flush();
        self.committed = true;
        Ok(())
    }
}

impl Drop for ExecutionContext<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.chain.db.rollback(std::mem::take(&mut self.db_deltas));
            for (addr, prev) in self.account_deltas.drain(..).rev() {
                match prev {
                    Some(acc) => {
                        self.chain.accounts.insert(addr, acc);
                    }
                    None => {
                        self.chain.accounts.remove(&addr);
                    }
                }
            }
            for (shard, prev) in self.shard_root_deltas.drain(..).rev() {
                match prev {
                    Some(root) => {
                        self.chain.shard_roots.insert(shard, root);
                    }
                    None => {
                        self.chain.shard_roots.remove(&shard);
                    }
                }
            }
            for (shard, prev) in self.shard_height_deltas.drain(..).rev() {
                match prev {
                    Some(h) => {
                        self.chain.shard_heights.insert(shard, h);
                    }
                    None => {
                        self.chain.shard_heights.remove(&shard);
                    }
                }
            }
        }
    }
}

/// Commit validated state deltas to the chain atomically.
pub fn commit(chain: &mut Blockchain, deltas: Vec<StateDelta>) -> std::io::Result<()> {
    let mut ctx = ExecutionContext::new(chain);
    ctx.apply(deltas)?;
    ctx.commit()
}

pub(crate) fn shard_state_root(
    accounts: &HashMap<String, Account>,
    shard: address::ShardId,
) -> [u8; 32] {
    let mut trie = MerkleTrie::new();
    for (addr, acc) in accounts
        .iter()
        .filter(|(addr, _)| address::shard_id(addr) == shard)
    {
        let mut data = Vec::new();
        data.extend_from_slice(&acc.balance.consumer.to_le_bytes());
        data.extend_from_slice(&acc.balance.industrial.to_le_bytes());
        data.extend_from_slice(&acc.nonce.to_le_bytes());
        trie.insert(addr.as_bytes(), &data);
    }
    trie.root_hash()
}
