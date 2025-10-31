use super::RpcError;
use crate::governance::GovStore;
use foundation_serialization::{Deserialize, Serialize};
use governance_spec::treasury::{
    DisbursementStatus as GovDisbursementStatus, TreasuryBalanceEventKind as GovBalanceEventKind,
    TreasuryBalanceSnapshot as GovBalanceSnapshot, TreasuryDisbursement as GovDisbursement,
    TreasuryExecutorSnapshot as GovExecutorSnapshot,
};

const DEFAULT_LIMIT: u64 = 50;
const MAX_LIMIT: u64 = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryDisbursementsRequest {
    #[serde(default, alias = "after_id")]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
    #[serde(default)]
    pub status: Option<TreasuryDisbursementStatusFilter>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub min_epoch: Option<u64>,
    #[serde(default)]
    pub max_epoch: Option<u64>,
    #[serde(default)]
    pub min_amount_ct: Option<u64>,
    #[serde(default)]
    pub max_amount_ct: Option<u64>,
    #[serde(default)]
    pub min_amount_it: Option<u64>,
    #[serde(default)]
    pub max_amount_it: Option<u64>,
    #[serde(default)]
    pub min_created_at: Option<u64>,
    #[serde(default)]
    pub max_created_at: Option<u64>,
    #[serde(default)]
    pub min_status_ts: Option<u64>,
    #[serde(default)]
    pub max_status_ts: Option<u64>,
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
    pub amount_it: u64,
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
    pub balance_it: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_snapshot: Option<TreasuryBalanceSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<TreasuryExecutorSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceHistoryResponse {
    pub snapshots: Vec<TreasuryBalanceSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
    pub current_balance_ct: u64,
    pub current_balance_it: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TreasuryDisbursementStatus {
    Scheduled,
    Executed { tx_hash: String, executed_at: u64 },
    Cancelled { reason: String, cancelled_at: u64 },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(rename_all = "snake_case")]
pub enum TreasuryDisbursementStatusFilter {
    Scheduled,
    Executed,
    Cancelled,
}

impl TreasuryDisbursementStatusFilter {
    fn matches(&self, status: &GovDisbursementStatus) -> bool {
        match (self, status) {
            (Self::Scheduled, GovDisbursementStatus::Scheduled) => true,
            (Self::Executed, GovDisbursementStatus::Executed { .. }) => true,
            (Self::Cancelled, GovDisbursementStatus::Cancelled { .. }) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryBalanceSnapshot {
    pub id: u64,
    pub balance_ct: u64,
    pub delta_ct: i64,
    pub balance_it: u64,
    pub delta_it: i64,
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

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryExecutorSnapshot {
    pub last_tick_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub last_error: Option<String>,
    pub pending_matured: u64,
    pub staged_intents: u64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub lease_holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_renewed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_submitted_nonce: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_last_nonce: Option<u64>,
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
    records.retain(|record| matches_request(record, &request));
    let limit = normalize_limit(request.limit);
    let has_more = records.len() > limit;
    records.truncate(limit);
    let next_cursor = if has_more {
        records.last().map(|entry| entry.id)
    } else {
        None
    };
    let disbursements = records
        .into_iter()
        .map(TreasuryDisbursementRecord::from)
        .collect();
    Ok(TreasuryDisbursementsResponse {
        disbursements,
        next_cursor,
    })
}

pub fn balance(store: &GovStore) -> Result<TreasuryBalanceResponse, RpcError> {
    let balances = store.treasury_balances().map_err(storage_error)?;
    let mut history = store.treasury_balance_history().map_err(storage_error)?;
    history.sort_by(|a, b| b.id.cmp(&a.id));
    let last_snapshot = history
        .into_iter()
        .next()
        .map(TreasuryBalanceSnapshot::from);
    let executor = store
        .executor_snapshot()
        .map_err(storage_error)?
        .map(TreasuryExecutorSnapshot::from);
    Ok(TreasuryBalanceResponse {
        balance_ct: balances.consumer,
        balance_it: balances.industrial,
        last_snapshot,
        executor,
    })
}

pub fn balance_history(
    store: &GovStore,
    request: TreasuryBalanceHistoryRequest,
) -> Result<TreasuryBalanceHistoryResponse, RpcError> {
    let balances = store.treasury_balances().map_err(storage_error)?;
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
        current_balance_ct: balances.consumer,
        current_balance_it: balances.industrial,
    })
}

impl From<GovDisbursement> for TreasuryDisbursementRecord {
    fn from(value: GovDisbursement) -> Self {
        Self {
            id: value.id,
            destination: value.destination,
            amount_ct: value.amount_ct,
            amount_it: value.amount_it,
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
            balance_it: value.balance_it,
            delta_it: value.delta_it,
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

impl From<GovExecutorSnapshot> for TreasuryExecutorSnapshot {
    fn from(value: GovExecutorSnapshot) -> Self {
        Self {
            last_tick_at: value.last_tick_at,
            last_success_at: value.last_success_at,
            last_error_at: value.last_error_at,
            last_error: value.last_error,
            pending_matured: value.pending_matured,
            staged_intents: value.staged_intents,
            lease_holder: value.lease_holder,
            lease_expires_at: value.lease_expires_at,
            lease_renewed_at: value.lease_renewed_at,
            last_submitted_nonce: value.last_submitted_nonce,
            lease_last_nonce: value.lease_last_nonce,
        }
    }
}

fn matches_request(record: &GovDisbursement, request: &TreasuryDisbursementsRequest) -> bool {
    if let Some(filter) = &request.status {
        if !filter.matches(&record.status) {
            return false;
        }
    }
    if let Some(destination) = &request.destination {
        if !record.destination.eq_ignore_ascii_case(destination) {
            return false;
        }
    }
    if let Some(min_epoch) = request.min_epoch {
        if record.scheduled_epoch < min_epoch {
            return false;
        }
    }
    if let Some(max_epoch) = request.max_epoch {
        if record.scheduled_epoch > max_epoch {
            return false;
        }
    }
    if let Some(min_amount_ct) = request.min_amount_ct {
        if record.amount_ct < min_amount_ct {
            return false;
        }
    }
    if let Some(max_amount_ct) = request.max_amount_ct {
        if record.amount_ct > max_amount_ct {
            return false;
        }
    }
    if let Some(min_amount_it) = request.min_amount_it {
        if record.amount_it < min_amount_it {
            return false;
        }
    }
    if let Some(max_amount_it) = request.max_amount_it {
        if record.amount_it > max_amount_it {
            return false;
        }
    }
    if let Some(min_created_at) = request.min_created_at {
        if record.created_at < min_created_at {
            return false;
        }
    }
    if let Some(max_created_at) = request.max_created_at {
        if record.created_at > max_created_at {
            return false;
        }
    }
    let status_timestamp = match &record.status {
        GovDisbursementStatus::Scheduled => record.created_at,
        GovDisbursementStatus::Executed { executed_at, .. } => *executed_at,
        GovDisbursementStatus::Cancelled { cancelled_at, .. } => *cancelled_at,
    };
    if let Some(min_status_ts) = request.min_status_ts {
        if status_timestamp < min_status_ts {
            return false;
        }
    }
    if let Some(max_status_ts) = request.max_status_ts {
        if status_timestamp > max_status_ts {
            return false;
        }
    }
    true
}

fn storage_error(_: sled::Error) -> RpcError {
    RpcError::new(-32092, "treasury storage error")
}

fn default_limit() -> u64 {
    DEFAULT_LIMIT
}

fn normalize_limit(limit: u64) -> usize {
    let effective = if limit == 0 { default_limit() } else { limit };
    effective.clamp(1, MAX_LIMIT) as usize
}
