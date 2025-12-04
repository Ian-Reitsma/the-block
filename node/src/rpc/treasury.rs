use super::RpcError;
use crate::governance::GovStore;
use foundation_serialization::{Deserialize, Serialize};
use governance_spec::treasury::{
    validate_disbursement_payload, DisbursementPayload,
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
    Draft {
        created_at: u64,
    },
    Voting {
        vote_deadline_epoch: u64,
    },
    Queued {
        queued_at: u64,
        activation_epoch: u64,
    },
    Timelocked {
        ready_epoch: u64,
    },
    Executed {
        tx_hash: String,
        executed_at: u64,
    },
    Finalized {
        tx_hash: String,
        executed_at: u64,
        finalized_at: u64,
    },
    RolledBack {
        reason: String,
        rolled_back_at: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        prior_tx: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(rename_all = "snake_case")]
pub enum TreasuryDisbursementStatusFilter {
    Draft,
    Voting,
    Queued,
    Timelocked,
    Executed,
    Finalized,
    RolledBack,
    #[serde(alias = "scheduled")]
    Scheduled,
    #[serde(alias = "cancelled")]
    Cancelled,
}

impl TreasuryDisbursementStatusFilter {
    fn matches(&self, status: &GovDisbursementStatus) -> bool {
        match (self, status) {
            (Self::Draft, GovDisbursementStatus::Draft { .. }) => true,
            (Self::Voting, GovDisbursementStatus::Voting { .. }) => true,
            (Self::Queued, GovDisbursementStatus::Queued { .. }) => true,
            (Self::Timelocked, GovDisbursementStatus::Timelocked { .. }) => true,
            (Self::Executed, GovDisbursementStatus::Executed { .. }) => true,
            (Self::Finalized, GovDisbursementStatus::Finalized { .. }) => true,
            (Self::RolledBack, GovDisbursementStatus::RolledBack { .. }) => true,
            (
                Self::Scheduled,
                GovDisbursementStatus::Draft { .. }
                | GovDisbursementStatus::Voting { .. }
                | GovDisbursementStatus::Queued { .. }
                | GovDisbursementStatus::Timelocked { .. },
            ) => true,
            (Self::Cancelled, GovDisbursementStatus::RolledBack { .. }) => true,
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
    #[serde(default)]
    pub lease_released: bool,
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
            GovDisbursementStatus::Draft { created_at } => {
                TreasuryDisbursementStatus::Draft { created_at }
            }
            GovDisbursementStatus::Voting {
                vote_deadline_epoch,
            } => TreasuryDisbursementStatus::Voting {
                vote_deadline_epoch,
            },
            GovDisbursementStatus::Queued {
                queued_at,
                activation_epoch,
            } => TreasuryDisbursementStatus::Queued {
                queued_at,
                activation_epoch,
            },
            GovDisbursementStatus::Timelocked { ready_epoch } => {
                TreasuryDisbursementStatus::Timelocked { ready_epoch }
            }
            GovDisbursementStatus::Executed {
                tx_hash,
                executed_at,
            } => TreasuryDisbursementStatus::Executed {
                tx_hash,
                executed_at,
            },
            GovDisbursementStatus::Finalized {
                tx_hash,
                executed_at,
                finalized_at,
            } => TreasuryDisbursementStatus::Finalized {
                tx_hash,
                executed_at,
                finalized_at,
            },
            GovDisbursementStatus::RolledBack {
                reason,
                rolled_back_at,
                prior_tx,
            } => TreasuryDisbursementStatus::RolledBack {
                reason,
                rolled_back_at,
                prior_tx,
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
            lease_released: value.lease_released,
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
        GovDisbursementStatus::Draft { created_at } => *created_at,
        GovDisbursementStatus::Voting {
            vote_deadline_epoch,
        } => *vote_deadline_epoch,
        GovDisbursementStatus::Queued { queued_at, .. } => *queued_at,
        GovDisbursementStatus::Timelocked { ready_epoch } => *ready_epoch,
        GovDisbursementStatus::Executed { executed_at, .. } => *executed_at,
        GovDisbursementStatus::Finalized { finalized_at, .. } => *finalized_at,
        GovDisbursementStatus::RolledBack { rolled_back_at, .. } => *rolled_back_at,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubmitDisbursementRequest {
    pub payload: DisbursementPayload,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubmitDisbursementResponse {
    pub id: u64,
    pub created_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct GetDisbursementRequest {
    pub id: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct GetDisbursementResponse {
    pub disbursement: TreasuryDisbursementRecord,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct QueueDisbursementRequest {
    pub id: u64,
    #[serde(default, alias = "epoch")]
    pub current_epoch: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ExecuteDisbursementRequest {
    pub id: u64,
    pub tx_hash: String,
    pub receipts: Vec<DisbursementReceiptInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementReceiptInput {
    pub account: String,
    pub amount_ct: u64,
    pub amount_it: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RollbackDisbursementRequest {
    pub id: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementOperationResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn submit_disbursement(
    store: &GovStore,
    request: SubmitDisbursementRequest,
) -> Result<SubmitDisbursementResponse, RpcError> {
    // Validate payload
    validate_disbursement_payload(&request.payload)
        .map_err(|e| RpcError::new(-32600, format!("disbursement validation failed: {e}")))?;

    // Queue disbursement using existing store method
    let disbursement = store
        .queue_disbursement(request.payload)
        .map_err(storage_error)?;

    Ok(SubmitDisbursementResponse {
        id: disbursement.id,
        created_at: disbursement.created_at,
    })
}

pub fn get_disbursement(
    store: &GovStore,
    request: GetDisbursementRequest,
) -> Result<GetDisbursementResponse, RpcError> {
    let all_disbursements = store.disbursements().map_err(storage_error)?;

    let disbursement = all_disbursements
        .into_iter()
        .find(|d| d.id == request.id)
        .ok_or_else(|| RpcError::new(-32001, format!("disbursement {} not found", request.id)))?;

    Ok(GetDisbursementResponse {
        disbursement: TreasuryDisbursementRecord::from(disbursement),
    })
}

pub fn queue_disbursement(
    store: &GovStore,
    request: QueueDisbursementRequest,
) -> Result<DisbursementOperationResponse, RpcError> {
    let record = store
        .advance_disbursement_status(request.id, request.current_epoch)
        .map_err(storage_error)?;
    Ok(DisbursementOperationResponse {
        ok: true,
        message: Some(format!(
            "disbursement {} advanced to {:?}",
            record.id, record.status
        )),
    })
}

pub fn execute_disbursement(
    store: &GovStore,
    request: ExecuteDisbursementRequest,
) -> Result<DisbursementOperationResponse, RpcError> {
    let receipts: Vec<governance_spec::treasury::DisbursementReceipt> = request
        .receipts
        .into_iter()
        .map(|r| governance_spec::treasury::DisbursementReceipt {
            account: r.account,
            amount_ct: r.amount_ct,
            amount_it: r.amount_it,
        })
        .collect();
    let disbursement = store
        .execute_disbursement(request.id, &request.tx_hash, receipts)
        .map_err(storage_error)?;

    Ok(DisbursementOperationResponse {
        ok: true,
        message: Some(format!(
            "disbursement {} executed: {}",
            disbursement.id, request.tx_hash
        )),
    })
}

pub fn rollback_disbursement(
    store: &GovStore,
    request: RollbackDisbursementRequest,
) -> Result<DisbursementOperationResponse, RpcError> {
    let disbursement = store
        .cancel_disbursement(request.id, &request.reason)
        .map_err(storage_error)?;

    Ok(DisbursementOperationResponse {
        ok: true,
        message: Some(format!("disbursement {} cancelled", disbursement.id)),
    })
}
