use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DisbursementStatus {
    Scheduled,
    Executed { tx_hash: String, executed_at: u64 },
    Cancelled { reason: String, cancelled_at: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreasuryDisbursement {
    pub id: u64,
    pub destination: String,
    pub amount_ct: u64,
    pub memo: String,
    pub scheduled_epoch: u64,
    pub created_at: u64,
    pub status: DisbursementStatus,
}

impl TreasuryDisbursement {
    pub fn new(
        id: u64,
        destination: String,
        amount_ct: u64,
        memo: String,
        scheduled_epoch: u64,
    ) -> Self {
        Self {
            id,
            destination,
            amount_ct,
            memo,
            scheduled_epoch,
            created_at: now_ts(),
            status: DisbursementStatus::Scheduled,
        }
    }
}

pub fn mark_executed(disbursement: &mut TreasuryDisbursement, tx_hash: String) {
    disbursement.status = DisbursementStatus::Executed {
        tx_hash,
        executed_at: now_ts(),
    };
}

pub fn mark_cancelled(disbursement: &mut TreasuryDisbursement, reason: String) {
    disbursement.status = DisbursementStatus::Cancelled {
        reason,
        cancelled_at: now_ts(),
    };
}
