use foundation_serialization::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::codec::{BinaryCodec, BinaryWriter, Result as CodecResult};

fn write_bytes(writer: &mut BinaryWriter, data: &[u8]) {
    writer.write_bytes(data);
}

fn read_bytes(reader: &mut crate::codec::BinaryReader<'_>) -> CodecResult<Vec<u8>> {
    reader.read_bytes()
}

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
pub struct SignedExecutionIntent {
    pub disbursement_id: u64,
    pub tx_bytes: Vec<u8>,
    pub tx_hash: String,
    pub staged_at: u64,
    #[serde(default)]
    pub nonce: u64,
}

impl SignedExecutionIntent {
    pub fn new(disbursement_id: u64, tx_bytes: Vec<u8>, tx_hash: String, nonce: u64) -> Self {
        Self {
            disbursement_id,
            tx_bytes,
            tx_hash,
            staged_at: now_ts(),
            nonce,
        }
    }
}

impl BinaryCodec for SignedExecutionIntent {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.disbursement_id.encode(writer);
        write_bytes(writer, &self.tx_bytes);
        self.tx_hash.encode(writer);
        self.staged_at.encode(writer);
        self.nonce.encode(writer);
    }

    fn decode(reader: &mut crate::codec::BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            disbursement_id: u64::decode(reader)?,
            tx_bytes: read_bytes(reader)?,
            tx_hash: String::decode(reader)?,
            staged_at: u64::decode(reader)?,
            nonce: u64::decode(reader).unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryExecutorSnapshot {
    #[serde(default)]
    pub last_tick_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub pending_matured: u64,
    #[serde(default)]
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

impl TreasuryExecutorSnapshot {
    pub fn record_tick(&mut self, pending: u64, staged: u64) {
        self.last_tick_at = now_ts();
        self.pending_matured = pending;
        self.staged_intents = staged;
    }

    pub fn record_success(&mut self, pending: u64, staged: u64) {
        self.record_tick(pending, staged);
        self.last_success_at = Some(self.last_tick_at);
        self.last_error = None;
        self.last_error_at = None;
    }

    pub fn record_error(&mut self, message: String, pending: u64, staged: u64) {
        self.record_tick(pending, staged);
        self.last_error = Some(message);
        self.last_error_at = Some(self.last_tick_at);
    }

    pub fn record_lease(
        &mut self,
        holder: Option<String>,
        expires_at: Option<u64>,
        renewed_at: Option<u64>,
        last_nonce: Option<u64>,
        released: bool,
    ) {
        self.lease_holder = holder;
        self.lease_expires_at = expires_at;
        self.lease_renewed_at = renewed_at;
        self.lease_last_nonce = last_nonce;
        self.lease_released = released;
    }

    pub fn record_nonce(&mut self, nonce: u64) {
        self.last_submitted_nonce = Some(nonce);
    }
}

impl BinaryCodec for TreasuryExecutorSnapshot {
    fn encode(&self, writer: &mut BinaryWriter) {
        self.last_tick_at.encode(writer);
        self.last_success_at.encode(writer);
        self.last_error_at.encode(writer);
        self.last_error.encode(writer);
        self.pending_matured.encode(writer);
        self.staged_intents.encode(writer);
        self.lease_holder.encode(writer);
        self.lease_expires_at.encode(writer);
        self.lease_renewed_at.encode(writer);
        self.last_submitted_nonce.encode(writer);
        self.lease_last_nonce.encode(writer);
        self.lease_released.encode(writer);
    }

    fn decode(reader: &mut crate::codec::BinaryReader<'_>) -> CodecResult<Self> {
        Ok(Self {
            last_tick_at: u64::decode(reader)?,
            last_success_at: Option::<u64>::decode(reader)?,
            last_error_at: Option::<u64>::decode(reader)?,
            last_error: Option::<String>::decode(reader)?,
            pending_matured: u64::decode(reader)?,
            staged_intents: u64::decode(reader)?,
            lease_holder: Option::<String>::decode(reader)?,
            lease_expires_at: Option::<u64>::decode(reader)?,
            lease_renewed_at: Option::<u64>::decode(reader)?,
            last_submitted_nonce: Option::<u64>::decode(reader)?,
            lease_last_nonce: Option::<u64>::decode(reader)?,
            lease_released: bool::decode(reader).unwrap_or(false),
        })
    }
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
