use crate::transaction::FeeLane;
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Receipt emitted by the dry-run matcher.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Receipt {
    pub version: u16,
    pub job_id: String,
    pub buyer: String,
    pub provider: String,
    /// Quoted price per compute unit.
    pub quote_price: u64,
    /// Total compute units covered by this receipt.
    pub units: u64,
    pub dry_run: bool,
    pub issued_at: u64,
    pub idempotency_key: [u8; 32],
    pub lane: FeeLane,
}

impl Receipt {
    /// Build a new receipt and derive an idempotency key from its fields.
    pub fn new(
        job_id: String,
        buyer: String,
        provider: String,
        quote_price: u64,
        units: u64,
        dry_run: bool,
        lane: FeeLane,
    ) -> Self {
        let version = 1u16;
        let mut h = Hasher::new();
        h.update(job_id.as_bytes());
        h.update(buyer.as_bytes());
        h.update(provider.as_bytes());
        h.update(&quote_price.to_be_bytes());
        h.update(&units.to_be_bytes());
        h.update(&version.to_be_bytes());
        h.update(&[lane as u8]);
        let idempotency_key = *h.finalize().as_bytes();
        let issued_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time error: {e}"))
            .as_secs();
        Self {
            version,
            job_id,
            buyer,
            provider,
            quote_price,
            units,
            dry_run,
            issued_at,
            idempotency_key,
            lane,
        }
    }
}
