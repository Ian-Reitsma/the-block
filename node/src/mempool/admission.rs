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
    pub fn new(window: usize, percentile: u32, lane: &'static str) -> Self {
        Self {
            fee_floor: FeeFloor::new(window, percentile),
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

    pub fn configure_fee_floor(&mut self, window: usize, percentile: u32) -> bool {
        let changed = self.fee_floor.configure(window, percentile);
        if changed {
            let previous = self.current_floor;
            self.current_floor = self.fee_floor.current();
            log_fee_floor_policy_change(
                self.lane,
                window,
                percentile,
                previous,
                self.current_floor,
            );
        }
        changed
    }

    pub fn policy(&self) -> (usize, u32) {
        self.fee_floor.policy()
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
            log_fee_floor_movement(self.lane, prev, updated);
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

#[cfg(feature = "telemetry")]
fn log_fee_floor_policy_change(
    lane: &str,
    window: usize,
    percentile: u32,
    previous: u64,
    current: u64,
) {
    diagnostics::tracing::info!(
        target: "mempool",
        lane,
        window,
        percentile,
        previous,
        current,
        "fee floor policy updated"
    );
}

#[cfg(not(feature = "telemetry"))]
fn log_fee_floor_policy_change(
    _lane: &str,
    _window: usize,
    _percentile: u32,
    _previous: u64,
    _current: u64,
) {
}

#[cfg(feature = "telemetry")]
fn log_fee_floor_movement(lane: &str, old: u64, new: u64) {
    diagnostics::tracing::info!(
        target: "mempool",
        lane,
        old,
        new,
        "fee floor updated"
    );
}

#[cfg(not(feature = "telemetry"))]
fn log_fee_floor_movement(_lane: &str, _old: u64, _new: u64) {}
