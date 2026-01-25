use ad_market::{MatchOutcome, MeshContext};
use concurrency::Lazy;
use crypto_suite::hashing::blake3;
use crypto_suite::hex;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::{
    relay_economics_mode, relay_max_ack_age_secs, relay_max_bytes_per_bundle,
    relay_max_bytes_per_epoch,
};
use crate::{EPOCH_BLOCKS, ReadAck};
use crate::receipts::RelayReceipt;

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    LABEL_REGISTRATION_ERR, RELAY_JOB_REJECTED_TOTAL, RELAY_RECEIPT_BYTES_TOTAL,
    RELAY_RECEIPTS_TOTAL,
};

#[derive(Clone, Debug)]
pub struct RelayJob {
    pub job_id: String,
    pub provider: String,
    pub campaign_id: Option<String>,
    pub creative_id: Option<String>,
    pub mesh_peer: Option<String>,
    pub mesh_transport: Option<String>,
    pub mesh_latency_ms: Option<u64>,
    pub clearing_price_usd_micros: u64,
    pub resource_floor_usd_micros: u64,
    pub price_per_mib_usd_micros: u64,
    pub total_usd_micros: u64,
    pub bytes: u64,
    pub offered_at_micros: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayEconomicsMode {
    Shadow,
    Trade,
}

impl RelayEconomicsMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelayEconomicsMode::Shadow => "shadow",
            RelayEconomicsMode::Trade => "trade",
        }
    }
}

#[derive(Clone, Debug)]
struct RelayConfig {
    max_bytes_per_bundle: u64,
    max_bytes_per_epoch: u64,
    max_ack_age_secs: u64,
    mode: RelayEconomicsMode,
}

impl RelayConfig {
    fn from_env() -> Self {
        let max_bundle = relay_max_bytes_per_bundle();
        let max_epoch = relay_max_bytes_per_epoch();
        let max_age = relay_max_ack_age_secs();
        let mode = match relay_economics_mode().to_lowercase().as_str() {
            "trade" => RelayEconomicsMode::Trade,
            _ => RelayEconomicsMode::Shadow,
        };
        Self {
            max_bytes_per_bundle: max_bundle,
            max_bytes_per_epoch: max_epoch,
            max_ack_age_secs: max_age,
            mode,
        }
    }
}

#[derive(Default)]
struct RelayState {
    receipts: Vec<RelayReceipt>,
    epoch_id: u64,
    bytes_this_epoch: u64,
}

static RELAY_CONFIG: Lazy<RelayConfig> = Lazy::new(RelayConfig::from_env);
static RELAY_STATE: Lazy<Mutex<RelayState>> = Lazy::new(|| Mutex::new(RelayState::default()));

fn relay_mode() -> RelayEconomicsMode {
    RELAY_CONFIG.mode
}

#[derive(Clone, Debug)]
pub enum RelayOfferError {
    PayloadTooLarge { bytes: u64, max: u64 },
    BudgetExceeded { bytes: u64, available: u64 },
    AckStale { age_secs: u64, max_secs: u64 },
}

impl RelayOfferError {
    pub fn label(&self) -> &'static str {
        match self {
            RelayOfferError::PayloadTooLarge { .. } => "payload_too_large",
            RelayOfferError::BudgetExceeded { .. } => "budget_exhausted",
            RelayOfferError::AckStale { .. } => "ack_stale",
        }
    }
}

pub fn offer_job(
    ack: &ReadAck,
    outcome: &MatchOutcome,
    mesh_ctx: Option<&MeshContext>,
    payload_len: usize,
) -> Result<RelayJob, RelayOfferError> {
    let bytes = payload_len as u64;
    let config = &*RELAY_CONFIG;
    if bytes > config.max_bytes_per_bundle {
        record_rejection(&RelayOfferError::PayloadTooLarge {
            bytes,
            max: config.max_bytes_per_bundle,
        });
        return Err(RelayOfferError::PayloadTooLarge {
            bytes,
            max: config.max_bytes_per_bundle,
        });
    }

    let ack_time = UNIX_EPOCH + Duration::from_millis(ack.ts);
    let now = SystemTime::now();
    let age = match now.duration_since(ack_time) {
        Ok(age) => age,
        Err(_) => Duration::from_secs(0),
    };
    if age.as_secs() > config.max_ack_age_secs {
        let err = RelayOfferError::AckStale {
            age_secs: age.as_secs(),
            max_secs: config.max_ack_age_secs,
        };
        record_rejection(&err);
        return Err(err);
    }

    {
        let mut state = RELAY_STATE.lock().unwrap();
        let epoch = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / crate::EPOCH_BLOCKS.max(1);
        if state.epoch_id != epoch {
            state.epoch_id = epoch;
            state.bytes_this_epoch = 0;
        }
        if state.bytes_this_epoch + bytes > config.max_bytes_per_epoch {
            let err = RelayOfferError::BudgetExceeded {
                bytes,
                available: config.max_bytes_per_epoch.saturating_sub(state.bytes_this_epoch),
            };
            record_rejection(&err);
            return Err(err);
        }
        state.bytes_this_epoch = state.bytes_this_epoch.saturating_add(bytes);
    }

    let job_id = relay_job_id(ack, outcome);
    Ok(RelayJob {
        job_id,
        provider: host_address(ack.domain.as_str()),
        campaign_id: Some(outcome.campaign_id.clone()),
        creative_id: Some(outcome.creative_id.clone()),
        mesh_peer: mesh_ctx.and_then(|ctx| ctx.peer_id.clone()),
        mesh_transport: mesh_ctx.and_then(|ctx| ctx.transport.clone()),
        mesh_latency_ms: mesh_ctx.and_then(|ctx| ctx.latency_ms),
        clearing_price_usd_micros: outcome.clearing_price_usd_micros,
        resource_floor_usd_micros: outcome.resource_floor_usd_micros,
        price_per_mib_usd_micros: outcome.price_per_mib_usd_micros,
        total_usd_micros: outcome.total_usd_micros,
        bytes,
        offered_at_micros: ack.ts,
    })
}

fn relay_job_id(ack: &ReadAck, outcome: &MatchOutcome) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&ack.manifest);
    hasher.update(&ack.path_hash);
    hasher.update(&ack.client_hash);
    hasher.update(&ack.ts.to_le_bytes());
    hasher.update(&ack.sig);
    hasher.update(ack.domain.as_bytes());
    hasher.update(ack.provider.as_bytes());
    hasher.update(outcome.campaign_id.as_bytes());
    hasher.update(outcome.creative_id.as_bytes());
    hasher.update(&outcome.clearing_price_usd_micros.to_le_bytes());
    hasher.update(&outcome.total_usd_micros.to_le_bytes());
    hex::encode(&hasher.finalize().as_bytes()[..16])
}

fn host_address(domain: &str) -> String {
    format!("0001:host:{}", domain)
}

fn record_rejection(reason: &RelayOfferError) {
    #[cfg(feature = "telemetry")]
    {
        RELAY_JOB_REJECTED_TOTAL
            .ensure_handle_for_label_values(&[reason.label()])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
    }
}

pub fn record_delivery(job: RelayJob, hop_relays: &[String], payload: &[u8]) {
    #[cfg(feature = "telemetry")]
    {
        RELAY_RECEIPTS_TOTAL.inc();
        RELAY_RECEIPT_BYTES_TOTAL.inc_by(payload.len() as u64);
    }

    if relay_mode() != RelayEconomicsMode::Trade {
        return;
    }

    let mut digest_hasher = blake3::Hasher::new();
    digest_hasher.update(payload);
    let payload_digest = digest_hasher.finalize().into();

    let receipt = RelayReceipt {
        job_id: job.job_id,
        provider: job.provider,
        campaign_id: job.campaign_id,
        creative_id: job.creative_id,
        mesh_peer: job.mesh_peer,
        transport: job.mesh_transport,
        latency_ms: job.mesh_latency_ms,
        payload_digest,
        bytes: job.bytes,
        price_per_mib_usd_micros: job.price_per_mib_usd_micros,
        total_usd_micros: job.total_usd_micros,
        clearing_price_usd_micros: job.clearing_price_usd_micros,
        resource_floor_usd_micros: job.resource_floor_usd_micros,
        hop_proofs: hop_relays.to_vec(),
        delivered_at_micros: job.offered_at_micros,
        block_height: 0,
        provider_signature: vec![],
        signature_nonce: 0,
    };

    let mut state = RELAY_STATE.lock().unwrap();
    state.receipts.push(receipt);
}

pub fn drain_relay_receipts(block_height: u64) -> Vec<RelayReceipt> {
    if relay_mode() != RelayEconomicsMode::Trade {
        let mut state = RELAY_STATE.lock().unwrap();
        state.receipts.clear();
        return Vec::new();
    }

    let mut state = RELAY_STATE.lock().unwrap();
    let mut receipts = Vec::new();
    std::mem::swap(&mut receipts, &mut state.receipts);
    for receipt in receipts.iter_mut() {
        receipt.block_height = block_height;
        receipt.signature_nonce = block_height;
    }
    receipts
}

pub fn reset_epoch(epoch: u64) {
    let mut state = RELAY_STATE.lock().unwrap();
    state.epoch_id = epoch;
    state.bytes_this_epoch = 0;
}
