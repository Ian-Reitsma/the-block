use std::collections::{HashMap, HashSet};

use crate::utxo::{OutPoint, Transaction};

#[derive(Default)]
pub struct TxScheduler {
    running: HashMap<[u8; 32], TxRwSet>,
}

#[derive(Clone)]
struct TxRwSet {
    reads: HashSet<OutPoint>,
    writes: HashSet<OutPoint>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScheduleError {
    Conflict([u8; 32]),
}

impl TxScheduler {
    pub fn schedule(&mut self, tx: &Transaction) -> Result<(), ScheduleError> {
        let txid = tx.txid();
        let ours = TxRwSet::from_tx(tx);
        for (other_id, other) in &self.running {
            if ours.conflicts(other) {
                return Err(ScheduleError::Conflict(*other_id));
            }
        }
        self.running.insert(txid, ours);
        Ok(())
    }

    pub fn complete(&mut self, tx: &Transaction) {
        self.running.remove(&tx.txid());
    }
}

impl TxRwSet {
    fn from_tx(tx: &Transaction) -> Self {
        let reads = tx
            .inputs
            .iter()
            .map(|i| i.previous_output.clone())
            .collect();
        let writes = tx
            .outputs
            .iter()
            .enumerate()
            .map(|(i, _)| OutPoint {
                txid: tx.txid(),
                index: i as u32,
            })
            .collect();
        Self { reads, writes }
    }

    fn conflicts(&self, other: &TxRwSet) -> bool {
        !self.reads.is_disjoint(&other.writes)
            || !self.writes.is_disjoint(&other.reads)
            || !self.writes.is_disjoint(&other.writes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::{Script, Transaction};
    use crate::utxo::{TxIn, TxOut};

    fn mk_tx(read: Option<OutPoint>, _write_idx: u32) -> Transaction {
        let inputs = read
            .into_iter()
            .map(|previous_output| TxIn {
                previous_output,
                script_sig: Script(vec![]),
            })
            .collect();
        Transaction {
            inputs,
            outputs: vec![TxOut {
                value: 1,
                script_pubkey: Script(vec![]),
            }],
        }
    }

    #[test]
    fn detects_conflict() {
        let mut sched = TxScheduler::default();
        let op = OutPoint {
            txid: [1; 32],
            index: 0,
        };
        let tx1 = mk_tx(Some(op.clone()), 0);
        let tx2 = mk_tx(Some(op), 1);
        assert!(sched.schedule(&tx1).is_ok());
        assert_eq!(
            sched.schedule(&tx2),
            Err(ScheduleError::Conflict(tx1.txid()))
        );
    }
}
