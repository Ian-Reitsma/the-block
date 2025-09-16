use std::collections::{HashMap, VecDeque};

use crate::{accounts::AccountValidation, Account, SignedTransaction, TxAdmissionError};

use super::scoring::FeeFloor;

const EVICTION_LOG_LEN: usize = 256;

/// Admission controller responsible for fee-floor tracking and sender slot limits.
pub struct AdmissionState {
    fee_floor: FeeFloor,
    per_sender: HashMap<String, usize>,
    eviction_log: VecDeque<[u8; 32]>,
    lane: &'static str,
    current_floor: u64,
}

impl AdmissionState {
    pub fn new(window: usize, lane: &'static str) -> Self {
        Self {
            fee_floor: FeeFloor::new(window),
            per_sender: HashMap::new(),
            eviction_log: VecDeque::with_capacity(EVICTION_LOG_LEN),
            lane,
            current_floor: 0,
        }
    }

    #[inline]
    pub fn floor(&self) -> u64 {
        self.current_floor
    }

    pub fn reserve_sender<'a>(
        &'a mut self,
        sender: &str,
        limit: usize,
    ) -> Result<AdmissionReservation<'a>, TxAdmissionError> {
        let counter = self.per_sender.entry(sender.to_string()).or_insert(0);
        if *counter >= limit {
            return Err(TxAdmissionError::PendingLimitReached);
        }
        *counter += 1;
        Ok(AdmissionReservation {
            state: self,
            sender: sender.to_string(),
            committed: false,
        })
    }

    pub fn restore_sender(&mut self, sender: &str) {
        *self.per_sender.entry(sender.to_string()).or_insert(0) += 1;
    }

    pub fn release_sender(&mut self, sender: &str) {
        if let Some(counter) = self.per_sender.get_mut(sender) {
            if *counter > 1 {
                *counter -= 1;
            } else {
                self.per_sender.remove(sender);
            }
        }
    }

    pub fn record_admission(&mut self, fee_per_byte: u64) -> u64 {
        let prev = self.current_floor;
        let updated = self.fee_floor.update(fee_per_byte);
        self.current_floor = updated;
        if updated != prev {
            tracing::info!(
                target: "mempool",
                lane = self.lane,
                old = prev,
                new = updated,
                "fee floor updated"
            );
        }
        updated
    }

    pub fn record_eviction(&mut self, hash: [u8; 32]) {
        if self.eviction_log.len() == EVICTION_LOG_LEN {
            self.eviction_log.pop_front();
        }
        self.eviction_log.push_back(hash);
    }

    pub fn eviction_hashes(&self) -> Vec<[u8; 32]> {
        self.eviction_log.iter().copied().collect()
    }
}

pub struct AdmissionReservation<'a> {
    state: &'a mut AdmissionState,
    sender: String,
    committed: bool,
}

impl<'a> AdmissionReservation<'a> {
    pub fn commit(mut self, fee_per_byte: u64) {
        let _ = self.state.record_admission(fee_per_byte);
        self.committed = true;
    }
}

impl Drop for AdmissionReservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.state.release_sender(&self.sender);
        }
    }
}

/// Validate a transaction against the sender's account rules.
pub fn validate_account(acc: &mut Account, tx: &SignedTransaction) -> Result<(), TxAdmissionError> {
    acc.validate_tx(tx)
}
