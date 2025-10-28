use foundation_serialization::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum DisbursementStatus {
    Scheduled,
    Executed { tx_hash: String, executed_at: u64 },
    Cancelled { reason: String, cancelled_at: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryDisbursement {
    pub id: u64,
    pub destination: String,
    pub amount_ct: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub amount_it: u64,
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
        amount_it: u64,
        memo: String,
        scheduled_epoch: u64,
    ) -> Self {
        Self {
            id,
            destination,
            amount_ct,
            amount_it,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum TreasuryBalanceEventKind {
    Accrual,
    Queued,
    Executed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceSnapshot {
    pub id: u64,
    pub balance_ct: u64,
    pub delta_ct: i64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub balance_it: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub delta_it: i64,
    pub recorded_at: u64,
    pub event: TreasuryBalanceEventKind,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub disbursement_id: Option<u64>,
}

impl TreasuryBalanceSnapshot {
    pub fn new(
        id: u64,
        balance_ct: u64,
        delta_ct: i64,
        balance_it: u64,
        delta_it: i64,
        event: TreasuryBalanceEventKind,
        disbursement_id: Option<u64>,
    ) -> Self {
        Self {
            id,
            balance_ct,
            delta_ct,
            balance_it,
            delta_it,
            recorded_at: now_ts(),
            event,
            disbursement_id,
        }
    }
}
