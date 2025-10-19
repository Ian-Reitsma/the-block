use super::RpcError;
use crate::governance::GovStore;
use foundation_serialization::{Deserialize, Serialize};
use governance_spec::treasury::{
    DisbursementStatus as GovDisbursementStatus, TreasuryBalanceEventKind as GovBalanceEventKind,
    TreasuryBalanceSnapshot as GovBalanceSnapshot, TreasuryDisbursement as GovDisbursement,
};

const DEFAULT_LIMIT: u64 = 50;
const MAX_LIMIT: u64 = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryDisbursementsRequest {
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceHistoryRequest {
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryDisbursementRecord {
    pub id: u64,
    pub destination: String,
    pub amount_ct: u64,
    pub memo: String,
    pub scheduled_epoch: u64,
    pub created_at: u64,
    pub status: TreasuryDisbursementStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryDisbursementsResponse {
    pub disbursements: Vec<TreasuryDisbursementRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceResponse {
    pub balance_ct: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_snapshot: Option<TreasuryBalanceSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceHistoryResponse {
    pub snapshots: Vec<TreasuryBalanceSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
    pub current_balance_ct: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TreasuryDisbursementStatus {
    Scheduled,
    Executed { tx_hash: String, executed_at: u64 },
    Cancelled { reason: String, cancelled_at: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceSnapshot {
    pub id: u64,
    pub balance_ct: u64,
    pub delta_ct: i64,
    pub recorded_at: u64,
    pub event: TreasuryBalanceEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disbursement_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(rename_all = "snake_case")]
pub enum TreasuryBalanceEventKind {
    Accrual,
    Queued,
    Executed,
    Cancelled,
}

pub fn disbursements(
    store: &GovStore,
    request: TreasuryDisbursementsRequest,
) -> Result<TreasuryDisbursementsResponse, RpcError> {
    let mut records = store.disbursements().map_err(storage_error)?;
    records.sort_by(|a, b| b.id.cmp(&a.id));
    if let Some(cursor) = request.cursor {
        records.retain(|record| record.id < cursor);
    }
    let limit = normalize_limit(request.limit);
    let total_records = records.len();
    let mut page: Vec<TreasuryDisbursementRecord> = records
        .into_iter()
        .take(limit)
        .map(TreasuryDisbursementRecord::from)
        .collect();
    let next_cursor = if total_records > page.len() {
        page.last().map(|entry| entry.id)
    } else {
        None
    };
    Ok(TreasuryDisbursementsResponse {
        disbursements: page.drain(..).collect(),
        next_cursor,
    })
}

pub fn balance(store: &GovStore) -> Result<TreasuryBalanceResponse, RpcError> {
    let balance_ct = store.treasury_balance().map_err(storage_error)?;
    let mut history = store.treasury_balance_history().map_err(storage_error)?;
    history.sort_by(|a, b| b.id.cmp(&a.id));
    let last_snapshot = history
        .into_iter()
        .next()
        .map(TreasuryBalanceSnapshot::from);
    Ok(TreasuryBalanceResponse {
        balance_ct,
        last_snapshot,
    })
}

pub fn balance_history(
    store: &GovStore,
    request: TreasuryBalanceHistoryRequest,
) -> Result<TreasuryBalanceHistoryResponse, RpcError> {
    let balance_ct = store.treasury_balance().map_err(storage_error)?;
    let mut history = store.treasury_balance_history().map_err(storage_error)?;
    history.sort_by(|a, b| b.id.cmp(&a.id));
    if let Some(cursor) = request.cursor {
        history.retain(|snapshot| snapshot.id < cursor);
    }
    let limit = normalize_limit(request.limit);
    let total = history.len();
    let mut page: Vec<TreasuryBalanceSnapshot> = history
        .into_iter()
        .take(limit)
        .map(TreasuryBalanceSnapshot::from)
        .collect();
    let next_cursor = if total > page.len() {
        page.last().map(|snapshot| snapshot.id)
    } else {
        None
    };
    Ok(TreasuryBalanceHistoryResponse {
        snapshots: page.drain(..).collect(),
        next_cursor,
        current_balance_ct: balance_ct,
    })
}

impl From<GovDisbursement> for TreasuryDisbursementRecord {
    fn from(value: GovDisbursement) -> Self {
        Self {
            id: value.id,
            destination: value.destination,
            amount_ct: value.amount_ct,
            memo: value.memo,
            scheduled_epoch: value.scheduled_epoch,
            created_at: value.created_at,
            status: value.status.into(),
        }
    }
}

impl From<GovDisbursementStatus> for TreasuryDisbursementStatus {
    fn from(value: GovDisbursementStatus) -> Self {
        match value {
            GovDisbursementStatus::Scheduled => TreasuryDisbursementStatus::Scheduled,
            GovDisbursementStatus::Executed {
                tx_hash,
                executed_at,
            } => TreasuryDisbursementStatus::Executed {
                tx_hash,
                executed_at,
            },
            GovDisbursementStatus::Cancelled {
                reason,
                cancelled_at,
            } => TreasuryDisbursementStatus::Cancelled {
                reason,
                cancelled_at,
            },
        }
    }
}

impl From<GovBalanceSnapshot> for TreasuryBalanceSnapshot {
    fn from(value: GovBalanceSnapshot) -> Self {
        Self {
            id: value.id,
            balance_ct: value.balance_ct,
            delta_ct: value.delta_ct,
            recorded_at: value.recorded_at,
            event: value.event.into(),
            disbursement_id: value.disbursement_id,
        }
    }
}

impl From<GovBalanceEventKind> for TreasuryBalanceEventKind {
    fn from(value: GovBalanceEventKind) -> Self {
        match value {
            GovBalanceEventKind::Accrual => TreasuryBalanceEventKind::Accrual,
            GovBalanceEventKind::Queued => TreasuryBalanceEventKind::Queued,
            GovBalanceEventKind::Executed => TreasuryBalanceEventKind::Executed,
            GovBalanceEventKind::Cancelled => TreasuryBalanceEventKind::Cancelled,
        }
    }
}

fn storage_error(_: sled::Error) -> RpcError {
    RpcError::new(-32092, "treasury storage error")
}

fn default_limit() -> u64 {
    DEFAULT_LIMIT
}

fn normalize_limit(limit: u64) -> usize {
    let bounded = limit.clamp(1, MAX_LIMIT);
    bounded as usize
}
