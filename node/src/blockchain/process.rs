use std::collections::HashSet;

use crate::{
    transaction::verify_stateless, Account, Block, Blockchain, TokenBalance, TxAdmissionError,
};

#[cfg(feature = "telemetry")]
use crate::telemetry::BLOCK_APPLY_FAIL_TOTAL;

/// State change for a single account.
#[derive(Clone)]
pub struct StateDelta {
    pub address: String,
    pub account: Account,
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
            });
        }
    }
    Ok(deltas)
}

/// Commit validated state deltas to the chain.
pub fn commit(chain: &mut Blockchain, deltas: Vec<StateDelta>) {
    for delta in deltas {
        chain.accounts.insert(delta.address, delta.account);
    }
}
