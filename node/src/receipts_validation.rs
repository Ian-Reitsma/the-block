//! Receipt validation with cryptographic signatures and anti-forgery protections
//!
//! This module enforces:
//! - Signature verification via provider registry
//! - Nonce-based replay attack prevention
//! - Receipt deduplication across blocks
//! - Field validation and DoS limits

use crate::block_binary;
use crate::receipt_crypto::{
    verify_receipt_signature, CryptoError, NonceTracker, ProviderRegistry,
};
use crate::receipts::Receipt;
use crypto_suite::hashing::blake3;
use foundation_serialization::{Deserialize, Serialize};

/// Hard ceiling for receipts per block (fuse; budgets are authoritative).
pub const HARD_RECEIPT_CEILING: usize = 50_000;
/// Byte budget per block for receipts (bandwidth + storage pressure).
pub const RECEIPT_BYTE_BUDGET: usize = 10_000_000;
/// Verify-unit budget per block for receipts (deterministic CPU budget).
pub const RECEIPT_VERIFY_BUDGET: u64 = 100_000;
/// Target fraction of the budget used by the EIP-1559-style controller (documented in system_reference.md).
pub const RECEIPT_BUDGET_TARGET_FRACTION: f64 = 0.6;
/// Minimum assumed encoded size per receipt when deriving the budget-based max count.
pub const MIN_RECEIPT_BYTE_FLOOR: usize = 1_000;
/// Minimum assumed verify units per receipt when deriving the budget-based max count.
pub const MIN_RECEIPT_VERIFY_UNITS: u64 = 10;
/// Number of logical receipt shards used when aggregating per-shard roots.
pub const RECEIPT_SHARD_COUNT: u16 = 64;
/// Required data-availability window for receipt blobs/commitments (seconds).
pub const RECEIPT_BLOB_DA_WINDOW_SECS: u64 = 7 * 24 * 60 * 60; // 7 days
const fn min_usize(a: usize, b: usize) -> usize {
    if a < b {
        a
    } else {
        b
    }
}
/// Derived maximum receipts per block from both byte and verify budgets (deterministic, no magic constants).
pub const MAX_RECEIPTS_PER_BLOCK: usize = {
    let byte_bound = RECEIPT_BYTE_BUDGET / MIN_RECEIPT_BYTE_FLOOR;
    let verify_bound = (RECEIPT_VERIFY_BUDGET / MIN_RECEIPT_VERIFY_UNITS) as usize;
    let budget_bound = min_usize(byte_bound, verify_bound);
    min_usize(budget_bound, HARD_RECEIPT_CEILING)
};

/// Maximum length for string fields (contract_id, provider, etc.)
pub const MAX_STRING_FIELD_LENGTH: usize = 256;

/// Minimum BLOCK payment amount to emit a receipt (spam protection)
pub const MIN_PAYMENT_FOR_RECEIPT: u64 = 1;

/// Receipt-level validation error
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum ValidationError {
    TooManyReceipts {
        count: usize,
        max: usize,
    },
    ReceiptsTooLarge {
        bytes: usize,
        max: usize,
    },
    VerifyBudgetExceeded {
        units: u64,
        max: u64,
    },
    BlockHeightMismatch {
        receipt_height: u64,
        block_height: u64,
    },
    EmptyStringField {
        field: String,
    },
    StringFieldTooLong {
        field: String,
        length: usize,
        max: usize,
    },
    ZeroValue {
        field: String,
    },
    MissingSignature,
    InvalidSignature {
        reason: String,
    },
    UnknownProvider {
        provider_id: String,
    },
    ReplayedNonce {
        provider_id: String,
        nonce: u64,
    },
    DuplicateReceipt,
    EmptySignature,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::TooManyReceipts { count, max } => {
                write!(f, "Too many receipts: {} (max: {})", count, max)
            }
            ValidationError::ReceiptsTooLarge { bytes, max } => {
                write!(f, "Receipts too large: {} bytes (max: {})", bytes, max)
            }
            ValidationError::VerifyBudgetExceeded { units, max } => {
                write!(
                    f,
                    "Receipt verify budget exceeded: {} units (max: {})",
                    units, max
                )
            }
            ValidationError::BlockHeightMismatch {
                receipt_height,
                block_height,
            } => {
                write!(
                    f,
                    "Receipt height {} != block height {}",
                    receipt_height, block_height
                )
            }
            ValidationError::EmptyStringField { field } => {
                write!(f, "Empty string field: {}", field)
            }
            ValidationError::StringFieldTooLong { field, length, max } => {
                write!(
                    f,
                    "Field {} too long: {} chars (max: {})",
                    field, length, max
                )
            }
            ValidationError::ZeroValue { field } => write!(f, "Zero value: {}", field),
            ValidationError::MissingSignature => write!(f, "Missing signature"),
            ValidationError::InvalidSignature { reason } => {
                write!(f, "Invalid signature: {}", reason)
            }
            ValidationError::UnknownProvider { provider_id } => {
                write!(f, "Unknown provider: {}", provider_id)
            }
            ValidationError::ReplayedNonce { provider_id, nonce } => {
                write!(f, "Replayed nonce {} for provider {}", nonce, provider_id)
            }
            ValidationError::DuplicateReceipt => write!(f, "Duplicate receipt"),
            ValidationError::EmptySignature => write!(f, "Empty signature bytes"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Receipt identity for deduplication
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptId(pub [u8; 32]);

impl ReceiptId {
    pub fn from_receipt(receipt: &Receipt) -> Self {
        let mut hasher = blake3::Hasher::new();

        match receipt {
            Receipt::Storage(r) => {
                hasher.update(b"storage");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.contract_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
            Receipt::Compute(r) => {
                hasher.update(b"compute");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.job_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
            Receipt::ComputeSlash(r) => {
                hasher.update(b"compute_slash");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.job_id.as_bytes());
                hasher.update(&r.burned.to_le_bytes());
                hasher.update(r.reason.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
            }
            Receipt::Energy(r) => {
                hasher.update(b"energy");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.contract_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
            Receipt::EnergySlash(r) => {
                hasher.update(b"energy_slash");
                hasher.update(r.provider.as_bytes());
                hasher.update(&r.meter_hash);
                hasher.update(r.reason.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
            }
            Receipt::Ad(r) => {
                hasher.update(b"ad");
                hasher.update(r.publisher.as_bytes());
                hasher.update(r.campaign_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
        }

        ReceiptId(hasher.finalize().into())
    }
}

/// Receipt registry for deduplication across blocks
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptRegistry {
    ids: std::collections::HashSet<ReceiptId>,
}

impl ReceiptRegistry {
    pub fn new() -> Self {
        Self {
            ids: std::collections::HashSet::new(),
        }
    }

    pub fn register(&mut self, id: ReceiptId) -> Result<(), ValidationError> {
        if !self.ids.insert(id) {
            return Err(ValidationError::DuplicateReceipt);
        }
        Ok(())
    }

    pub fn prune_with<F>(&mut self, mut should_remove: F)
    where
        F: FnMut(&ReceiptId) -> bool,
    {
        self.ids.retain(|id| !should_remove(id));
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

fn receipt_provider_and_nonce(receipt: &Receipt) -> (&str, u64) {
    match receipt {
        Receipt::Storage(r) => (r.provider.as_str(), r.signature_nonce),
        Receipt::Compute(r) => (r.provider.as_str(), r.signature_nonce),
        Receipt::ComputeSlash(r) => (r.provider.as_str(), 0),
        Receipt::Energy(r) => (r.provider.as_str(), r.signature_nonce),
        Receipt::EnergySlash(r) => (r.provider.as_str(), 0),
        Receipt::Ad(r) => (r.publisher.as_str(), r.signature_nonce),
    }
}

/// Validate receipt with full cryptographic checks
pub fn validate_receipt(
    receipt: &Receipt,
    block_height: u64,
    provider_registry: &ProviderRegistry,
    nonce_tracker: &mut NonceTracker,
) -> Result<(), ValidationError> {
    if matches!(receipt, Receipt::EnergySlash(_) | Receipt::ComputeSlash(_)) {
        return Ok(());
    }
    let (provider_id, nonce) = receipt_provider_and_nonce(receipt);
    if nonce_tracker.has_seen_nonce(provider_id, nonce) {
        return Err(ValidationError::ReplayedNonce {
            provider_id: provider_id.to_string(),
            nonce,
        });
    }

    // Check block height
    if receipt.block_height() != block_height {
        return Err(ValidationError::BlockHeightMismatch {
            receipt_height: receipt.block_height(),
            block_height,
        });
    }

    // Field validation
    match receipt {
        Receipt::Storage(r) => {
            validate_string_field("contract_id", &r.contract_id)?;
            validate_string_field("provider", &r.provider)?;
            if r.bytes == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "bytes".to_string(),
                });
            }
            if r.price == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "price".to_string(),
                });
            }
            if r.provider_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
        Receipt::Compute(r) => {
            validate_string_field("job_id", &r.job_id)?;
            validate_string_field("provider", &r.provider)?;
            if r.compute_units == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "compute_units".to_string(),
                });
            }
            if r.payment == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "payment".to_string(),
                });
            }
            if r.provider_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
        Receipt::Energy(r) => {
            validate_string_field("contract_id", &r.contract_id)?;
            validate_string_field("provider", &r.provider)?;
            if r.energy_units == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "energy_units".to_string(),
                });
            }
            if r.price == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "price".to_string(),
                });
            }
            if r.provider_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
        Receipt::EnergySlash(r) => {
            validate_string_field("provider", &r.provider)?;
            validate_string_field("reason", &r.reason)?;
        }
        Receipt::ComputeSlash(r) => {
            validate_string_field("job_id", &r.job_id)?;
            validate_string_field("provider", &r.provider)?;
            validate_string_field("reason", &r.reason)?;
            if r.burned == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "burned".to_string(),
                });
            }
        }
        Receipt::Ad(r) => {
            validate_string_field("campaign_id", &r.campaign_id)?;
            validate_string_field("publisher", &r.publisher)?;
            if r.impressions == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "impressions".to_string(),
                });
            }
            if r.spend == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "spend".to_string(),
                });
            }
            if r.publisher_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
    }

    // Cryptographic signature verification
    verify_receipt_signature(receipt, provider_registry, nonce_tracker, block_height).map_err(|e| {
        match e {
            CryptoError::InvalidSignature { reason } => {
                ValidationError::InvalidSignature { reason }
            }
            CryptoError::UnknownProvider { provider_id } => {
                ValidationError::UnknownProvider { provider_id }
            }
            CryptoError::ReplayedNonce { provider_id, nonce } => {
                ValidationError::ReplayedNonce { provider_id, nonce }
            }
            CryptoError::MalformedSignature { reason } => {
                ValidationError::InvalidSignature { reason }
            }
        }
    })
}

fn validate_string_field(field_name: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyStringField {
            field: field_name.to_string(),
        });
    }
    if value.len() > MAX_STRING_FIELD_LENGTH {
        return Err(ValidationError::StringFieldTooLong {
            field: field_name.to_string(),
            length: value.len(),
            max: MAX_STRING_FIELD_LENGTH,
        });
    }
    Ok(())
}

pub fn validate_receipt_count(count: usize) -> Result<(), ValidationError> {
    if count > MAX_RECEIPTS_PER_BLOCK {
        return Err(ValidationError::TooManyReceipts {
            count,
            max: MAX_RECEIPTS_PER_BLOCK,
        });
    }
    Ok(())
}

pub fn validate_receipt_size(bytes: usize) -> Result<(), ValidationError> {
    if bytes > RECEIPT_BYTE_BUDGET {
        return Err(ValidationError::ReceiptsTooLarge {
            bytes,
            max: RECEIPT_BYTE_BUDGET,
        });
    }
    Ok(())
}

/// Aggregate resource usage for a block of receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptBlockUsage {
    pub count: usize,
    pub bytes: usize,
    pub verify_units: u64,
}

impl ReceiptBlockUsage {
    pub fn new(count: usize, bytes: usize, verify_units: u64) -> Self {
        Self {
            count,
            bytes,
            verify_units,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum ReceiptAggregateScheme {
    None,
    BatchEd25519,
}

impl Default for ReceiptAggregateScheme {
    fn default() -> Self {
        Self::None
    }
}

/// Header committed into block/macro-block receipts manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptHeader {
    /// Number of logical shards used when aggregating receipts.
    pub shard_count: u16,
    /// Per-shard receipt Merkle roots (zeroed for empty shards).
    pub shard_roots: Vec<[u8; 32]>,
    /// Per-shard blob/DA commitments (aligned with `shard_roots`).
    pub blob_commitments: Vec<[u8; 32]>,
    /// Wall-clock expiry (ms since epoch) for DA sampling window.
    pub available_until: u64,
    /// Aggregation scheme used for high-volume receipts.
    pub aggregate_scheme: ReceiptAggregateScheme,
    /// Aggregated signature commitment for the selected scheme.
    pub aggregate_sig: [u8; 32],
}

impl ReceiptHeader {
    pub fn is_populated(&self) -> bool {
        self.shard_count > 0 && !self.shard_roots.is_empty()
    }
}

/// Deterministic per-receipt verify-unit estimator (dominant-resource fairness guard).
pub fn receipt_verify_units(receipt: &Receipt) -> u64 {
    // Base signature verification cost for every receipt
    let mut units: u64 = 10;
    match receipt {
        Receipt::Storage(r) => {
            // Slightly scale with payload size to reflect hashing work
            units += (r.bytes / 1_000).min(50) as u64;
        }
        Receipt::Compute(r) => {
            units += 5;
            if r.verified {
                units += 5;
            }
        }
        Receipt::Energy(_) => {
            units += 3;
        }
        Receipt::ComputeSlash(_) => {
            units += 2;
        }
        Receipt::EnergySlash(_) => {
            units += 2;
        }
        Receipt::Ad(r) => {
            // Ad receipts can batch many impressions; cap per-receipt verify units.
            units += (r.impressions / 10_000).min(10) as u64;
        }
    }
    units
}

/// Validate a block-level receipt budget using both byte and verify-unit caps.
pub fn validate_receipt_budget(usage: &ReceiptBlockUsage) -> Result<(), ValidationError> {
    validate_receipt_count(usage.count)?;
    validate_receipt_size(usage.bytes)?;
    if usage.verify_units > RECEIPT_VERIFY_BUDGET {
        return Err(ValidationError::VerifyBudgetExceeded {
            units: usage.verify_units,
            max: RECEIPT_VERIFY_BUDGET,
        });
    }
    Ok(())
}

fn signature_bytes_for_receipt(receipt: &Receipt) -> &[u8] {
    match receipt {
        Receipt::Storage(r) => r.provider_signature.as_slice(),
        Receipt::Compute(r) => r.provider_signature.as_slice(),
        Receipt::Energy(r) => r.provider_signature.as_slice(),
        Receipt::ComputeSlash(_) => &[],
        Receipt::EnergySlash(_) => &[],
        Receipt::Ad(r) => r.publisher_signature.as_slice(),
    }
}

/// Deterministic aggregated signature commitment for high-volume receipts.
pub fn aggregate_signature_digest(
    receipts: &[Receipt],
    scheme: ReceiptAggregateScheme,
) -> [u8; 32] {
    match scheme {
        ReceiptAggregateScheme::None => [0u8; 32],
        ReceiptAggregateScheme::BatchEd25519 => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"receipt_batch_ed25519");
            let mut seen = 0usize;
            for receipt in receipts {
                if matches!(receipt, Receipt::Ad(_) | Receipt::Energy(_)) {
                    hasher.update(&receipt_leaf_hash(receipt));
                    let sig_bytes = signature_bytes_for_receipt(receipt);
                    hasher.update(&(sig_bytes.len() as u32).to_le_bytes());
                    hasher.update(sig_bytes);
                    seen += 1;
                }
            }
            if seen == 0 {
                [0u8; 32]
            } else {
                hasher.finalize().into()
            }
        }
    }
}

/// Encode a single receipt to determine serialized size for budgeting.
pub fn encoded_receipt_len(receipt: &Receipt) -> Result<usize, String> {
    block_binary::encode_receipts(&[receipt.clone()])
        .map(|bytes| bytes.len())
        .map_err(|e| format!("encode receipt: {:?}", e))
}

/// Parameters controlling receipt header derivation/validation.
#[derive(Debug, Clone, Copy)]
pub struct ReceiptHeaderParams {
    pub shard_count: u16,
    pub da_window_secs: u64,
    pub min_region_diversity: u16,
    pub min_asn_diversity: u16,
    pub max_per_provider_per_shard: u16,
    pub aggregate_scheme: ReceiptAggregateScheme,
}

impl ReceiptHeaderParams {
    pub fn new(
        shard_count: u16,
        da_window_secs: u64,
        min_region_diversity: u16,
        min_asn_diversity: u16,
        max_per_provider_per_shard: u16,
        aggregate_scheme: ReceiptAggregateScheme,
    ) -> Self {
        Self {
            shard_count: shard_count.max(1),
            da_window_secs: da_window_secs.max(1),
            min_region_diversity: min_region_diversity.max(1),
            min_asn_diversity: min_asn_diversity.max(1),
            max_per_provider_per_shard: max_per_provider_per_shard.max(1),
            aggregate_scheme,
        }
    }
}

/// Derive a receipt header from receipts and enforce shard-level budgets/diversity.
pub fn derive_receipt_header(
    receipts: &[Receipt],
    timestamp_millis: u64,
    params: ReceiptHeaderParams,
    registry: &ProviderRegistry,
) -> Result<ReceiptHeader, String> {
    let mut acc = ReceiptShardAccumulator::new(params.shard_count);
    #[derive(Default)]
    struct ShardDiversity {
        providers: std::collections::HashMap<String, u16>,
        regions: std::collections::HashSet<String>,
        asns: std::collections::HashSet<u32>,
    }
    let mut diversity: std::collections::HashMap<u16, ShardDiversity> =
        std::collections::HashMap::new();

    for receipt in receipts {
        let encoded_len = encoded_receipt_len(receipt)?;
        acc.add(receipt, encoded_len);
        let shard = shard_for_receipt(receipt, params.shard_count);
        let entry = diversity.entry(shard).or_default();
        let (id, region_hint, asn_hint) = match receipt {
            Receipt::Storage(r) => (r.provider.as_str(), None, None),
            Receipt::Compute(r) => (r.provider.as_str(), None, None),
            Receipt::Energy(r) => (r.provider.as_str(), None, None),
            Receipt::EnergySlash(r) => (r.provider.as_str(), None, None),
            Receipt::ComputeSlash(r) => (r.provider.as_str(), None, None),
            Receipt::Ad(r) => (r.publisher.as_str(), None, None),
        };
        let record = registry.get_provider_record(id);
        let region = region_hint
            .clone()
            .or_else(|| record.and_then(|r| r.region.clone()))
            .unwrap_or_else(|| "unknown".to_string());
        let asn = asn_hint.or_else(|| record.and_then(|r| r.asn)).unwrap_or(0);
        let count = entry.providers.entry(id.to_string()).or_insert(0);
        *count = count.saturating_add(1);
        if *count as u16 > params.max_per_provider_per_shard {
            return Err(format!(
                "provider {} exceeds per-shard receipt limit {} on shard {}",
                id, params.max_per_provider_per_shard, shard
            ));
        }
        entry.regions.insert(region);
        entry.asns.insert(asn);
    }

    for (_shard, entry) in &diversity {
        if !entry.providers.is_empty() && entry.regions.len() < params.min_region_diversity as usize
        {
            return Err(format!(
                "region diversity violation: have {} need {}",
                entry.regions.len(),
                params.min_region_diversity
            ));
        }
        if !entry.providers.is_empty() && entry.asns.len() < params.min_asn_diversity as usize {
            return Err(format!(
                "asn diversity violation: have {} need {}",
                entry.asns.len(),
                params.min_asn_diversity
            ));
        }
    }

    for usage in acc.per_shard_usage() {
        if usage.count > 0 {
            validate_receipt_budget(usage).map_err(|e| format!("per-shard budget: {}", e))?;
        }
    }
    let total_usage = acc.total_usage();
    validate_receipt_budget(&total_usage).map_err(|e| format!("total budget: {}", e))?;

    let shard_roots = acc.roots();
    let aggregate_sig = aggregate_signature_digest(receipts, params.aggregate_scheme);
    let available_until =
        timestamp_millis.saturating_add(params.da_window_secs.saturating_mul(1000));
    let blob_commitments = vec![[0u8; 32]; params.shard_count as usize];

    Ok(ReceiptHeader {
        shard_count: params.shard_count,
        shard_roots,
        blob_commitments,
        available_until,
        aggregate_scheme: params.aggregate_scheme,
        aggregate_sig,
    })
}

/// Validate receipt header against receipts and limits.
pub fn validate_receipt_header(
    header: &ReceiptHeader,
    receipts: &[Receipt],
    params: ReceiptHeaderParams,
    registry: &ProviderRegistry,
    now_millis: u64,
) -> Result<(), String> {
    if header.shard_count == 0
        || header.shard_roots.len() != header.shard_count as usize
        || header.blob_commitments.len() != header.shard_count as usize
    {
        return Err("receipt header malformed".into());
    }
    if now_millis > header.available_until {
        return Err("receipt header expired".into());
    }
    let expected = derive_receipt_header(receipts, now_millis, params, registry)?;
    if expected.shard_roots != header.shard_roots {
        return Err("receipt shard roots mismatch".into());
    }
    if expected.aggregate_sig != header.aggregate_sig {
        return Err("receipt aggregate signature mismatch".into());
    }
    Ok(())
}

/// Per-shard aggregation of receipt roots and resource usage. This is a
/// stateless, append-only accumulator intended for builders and macro-block
/// assembly to avoid a single global receipts Vec bottleneck.
#[derive(Debug, Clone)]
pub struct ReceiptShardAccumulator {
    shard_count: u16,
    leaves: Vec<Vec<[u8; 32]>>,
    usage: Vec<ReceiptBlockUsage>,
}

impl ReceiptShardAccumulator {
    pub fn new(shard_count: u16) -> Self {
        let len = shard_count as usize;
        Self {
            shard_count,
            leaves: vec![Vec::new(); len],
            usage: vec![
                ReceiptBlockUsage {
                    count: 0,
                    bytes: 0,
                    verify_units: 0
                };
                len
            ],
        }
    }

    pub fn add(&mut self, receipt: &Receipt, encoded_size: usize) {
        let shard = shard_for_receipt(receipt, self.shard_count);
        let idx = shard as usize;
        self.leaves[idx].push(receipt_leaf_hash(receipt));
        let units = receipt_verify_units(receipt);
        let usage = &mut self.usage[idx];
        usage.count += 1;
        usage.bytes += encoded_size;
        usage.verify_units += units;
    }

    /// Compute per-shard Merkle roots (empty shards use the zero hash).
    pub fn roots(&self) -> Vec<[u8; 32]> {
        self.leaves
            .iter()
            .map(|leaves| {
                if leaves.is_empty() {
                    [0u8; 32]
                } else {
                    merkle_root(leaves)
                }
            })
            .collect()
    }

    pub fn per_shard_usage(&self) -> &[ReceiptBlockUsage] {
        &self.usage
    }

    pub fn total_usage(&self) -> ReceiptBlockUsage {
        self.usage
            .iter()
            .fold(ReceiptBlockUsage::new(0, 0, 0), |mut acc, shard| {
                acc.count += shard.count;
                acc.bytes += shard.bytes;
                acc.verify_units += shard.verify_units;
                acc
            })
    }
}

/// Compute a stable shard assignment for a receipt based on provider/publisher
/// identifier. This is deterministic and domain-separated so all nodes agree.
pub fn shard_for_receipt(receipt: &Receipt, shard_count: u16) -> u16 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"receipt_shard");
    match receipt {
        Receipt::Storage(r) => hasher.update(r.provider.as_bytes()),
        Receipt::Compute(r) => hasher.update(r.provider.as_bytes()),
        Receipt::Energy(r) => hasher.update(r.provider.as_bytes()),
        Receipt::EnergySlash(r) => hasher.update(r.provider.as_bytes()),
        Receipt::ComputeSlash(r) => hasher.update(r.provider.as_bytes()),
        Receipt::Ad(r) => hasher.update(r.publisher.as_bytes()),
    }
    let hash = hasher.finalize();
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&hash.as_bytes()[..2]);
    u16::from_le_bytes(bytes) % shard_count.max(1)
}

/// Leaf hash used for per-shard receipt Merkle trees.
pub fn receipt_leaf_hash(receipt: &Receipt) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"receipt_leaf");
    match receipt {
        Receipt::Storage(r) => {
            hasher.update(r.provider.as_bytes());
            hasher.update(r.contract_id.as_bytes());
            hasher.update(&r.block_height.to_le_bytes());
            hasher.update(&r.signature_nonce.to_le_bytes());
            hasher.update(&r.bytes.to_le_bytes());
            hasher.update(&r.price.to_le_bytes());
        }
        Receipt::Compute(r) => {
            hasher.update(r.provider.as_bytes());
            hasher.update(r.job_id.as_bytes());
            hasher.update(&r.block_height.to_le_bytes());
            hasher.update(&r.signature_nonce.to_le_bytes());
            hasher.update(&r.compute_units.to_le_bytes());
            hasher.update(&r.payment.to_le_bytes());
            hasher.update(&[u8::from(r.verified)]);
        }
        Receipt::ComputeSlash(r) => {
            hasher.update(r.provider.as_bytes());
            hasher.update(r.job_id.as_bytes());
            hasher.update(&r.block_height.to_le_bytes());
            hasher.update(&r.burned.to_le_bytes());
            hasher.update(r.reason.as_bytes());
            hasher.update(&r.deadline.to_le_bytes());
            hasher.update(&r.resolved_at.to_le_bytes());
        }
        Receipt::Energy(r) => {
            hasher.update(r.provider.as_bytes());
            hasher.update(r.contract_id.as_bytes());
            hasher.update(&r.block_height.to_le_bytes());
            hasher.update(&r.signature_nonce.to_le_bytes());
            hasher.update(&r.energy_units.to_le_bytes());
            hasher.update(&r.price.to_le_bytes());
        }
        Receipt::EnergySlash(r) => {
            hasher.update(r.provider.as_bytes());
            hasher.update(&r.block_height.to_le_bytes());
            hasher.update(&r.meter_hash);
            hasher.update(&r.slash_amount.to_le_bytes());
            hasher.update(r.reason.as_bytes());
        }
        Receipt::Ad(r) => {
            hasher.update(r.publisher.as_bytes());
            hasher.update(r.campaign_id.as_bytes());
            hasher.update(&r.block_height.to_le_bytes());
            hasher.update(&r.signature_nonce.to_le_bytes());
            hasher.update(&r.impressions.to_le_bytes());
            hasher.update(&r.spend.to_le_bytes());
        }
    }
    hasher.finalize().into()
}

/// Simple binary Merkle root over fixed-size leaf hashes.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for pair in layer.chunks(2) {
            let combined = if pair.len() == 2 {
                let mut hasher = blake3::Hasher::new();
                hasher.update(&pair[0]);
                hasher.update(&pair[1]);
                hasher.finalize().into()
            } else {
                pair[0]
            };
            next.push(combined);
        }
        layer = next;
    }
    layer[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt_crypto::ProviderRegistry;
    use crate::receipts::{AdReceipt, ComputeReceipt, StorageReceipt};
    use crypto_suite::signatures::ed25519::SigningKey;
    use rand::{rngs::StdRng, SeedableRng};

    fn create_signed_storage_receipt(
        sk: &SigningKey,
        block_height: u64,
        nonce: u64,
    ) -> StorageReceipt {
        let mut receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height,
            provider_escrow: 10000,
            provider_signature: vec![],
            signature_nonce: nonce,
        };

        // Build preimage
        use crypto_suite::hashing::blake3;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"storage");
        hasher.update(&receipt.block_height.to_le_bytes());
        hasher.update(receipt.contract_id.as_bytes());
        hasher.update(receipt.provider.as_bytes());
        hasher.update(&receipt.bytes.to_le_bytes());
        hasher.update(&receipt.price.to_le_bytes());
        hasher.update(&receipt.provider_escrow.to_le_bytes());
        hasher.update(&receipt.signature_nonce.to_le_bytes());
        let preimage = hasher.finalize();

        let signature = sk.sign(preimage.as_bytes());
        receipt.provider_signature = signature.to_bytes().to_vec();
        receipt
    }

    #[test]
    fn valid_receipt_passes() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let receipt = create_signed_storage_receipt(&sk, 100, 1);

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        assert!(validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker
        )
        .is_ok());
    }

    #[test]
    fn forged_signature_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let mut receipt = create_signed_storage_receipt(&sk, 100, 1);
        // Corrupt signature
        receipt.provider_signature[0] ^= 0xFF;

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(
            result,
            Err(ValidationError::InvalidSignature { .. })
        ));
    }

    #[test]
    fn unsigned_receipt_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height: 100,
            provider_escrow: 10000,
            provider_signature: vec![], // Empty
            signature_nonce: 1,
        };

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(result, Err(ValidationError::EmptySignature)));
    }

    #[test]
    fn replay_attack_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let receipt = create_signed_storage_receipt(&sk, 100, 1);

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        // First validation succeeds
        assert!(validate_receipt(
            &Receipt::Storage(receipt.clone()),
            100,
            &registry,
            &mut nonce_tracker
        )
        .is_ok());

        // Second validation with same nonce fails
        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(result, Err(ValidationError::ReplayedNonce { .. })));
    }

    #[test]
    fn duplicate_receipt_detected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);

        let receipt = create_signed_storage_receipt(&sk, 100, 1);
        let id = ReceiptId::from_receipt(&Receipt::Storage(receipt));

        let mut registry = ReceiptRegistry::new();
        assert!(registry.register(id).is_ok());
        assert!(matches!(
            registry.register(id),
            Err(ValidationError::DuplicateReceipt)
        ));
    }

    #[test]
    fn unknown_provider_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);

        let receipt = create_signed_storage_receipt(&sk, 100, 1);

        let registry = ProviderRegistry::new(); // Empty registry
        let mut nonce_tracker = NonceTracker::new(100);

        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(
            result,
            Err(ValidationError::UnknownProvider { .. })
        ));
    }

    fn dummy_ad_receipt(publisher: &str, block_height: u64) -> Receipt {
        Receipt::Ad(AdReceipt {
            campaign_id: "camp".into(),
            creative_id: "creative".into(),
            publisher: publisher.to_string(),
            impressions: 10,
            spend: 5,
            block_height,
            conversions: 1,
            claim_routes: Default::default(),
            role_breakdown: None,
            device_links: Vec::new(),
            publisher_signature: vec![1, 2, 3],
            signature_nonce: block_height,
        })
    }

    #[test]
    fn derive_header_enforces_provider_cap() {
        let receipts = vec![dummy_ad_receipt("p1", 1), dummy_ad_receipt("p1", 1)];
        let registry = ProviderRegistry::new();
        let params = ReceiptHeaderParams::new(1, 10, 1, 1, 1, ReceiptAggregateScheme::BatchEd25519);
        let err = derive_receipt_header(&receipts, 1_000, params, &registry)
            .expect_err("should fail provider cap");
        assert!(err.contains("per-shard receipt limit"));
    }

    #[test]
    fn aggregate_sig_mismatch_detected() {
        let receipts = vec![dummy_ad_receipt("p1", 1)];
        let mut registry = ProviderRegistry::new();
        let sk = SigningKey::generate(&mut StdRng::seed_from_u64(1));
        let vk = sk.verifying_key();
        registry
            .register_provider_with_metadata("p1".into(), vk, 0, Some("r1".into()), Some(10))
            .unwrap();
        let params = ReceiptHeaderParams::new(2, 10, 1, 1, 2, ReceiptAggregateScheme::BatchEd25519);
        let mut header =
            derive_receipt_header(&receipts, 10_000, params, &registry).expect("derive header");
        // Tamper aggregate signature
        header.aggregate_sig[0] ^= 0xFF;
        let err = validate_receipt_header(&header, &receipts, params, &registry, 10_000)
            .expect_err("should detect agg mismatch");
        assert!(err.contains("aggregate signature"));
    }

    #[test]
    fn header_round_trip_validates() {
        let receipts = vec![dummy_ad_receipt("pA", 5), dummy_ad_receipt("pB", 5)];
        let mut registry = ProviderRegistry::new();
        let sk = SigningKey::generate(&mut StdRng::seed_from_u64(2));
        let vk = sk.verifying_key();
        registry
            .register_provider_with_metadata(
                "pA".into(),
                vk.clone(),
                0,
                Some("r1".into()),
                Some(10),
            )
            .unwrap();
        registry
            .register_provider_with_metadata("pB".into(), vk, 0, Some("r2".into()), Some(20))
            .unwrap();
        let params = ReceiptHeaderParams::new(1, 15, 2, 2, 3, ReceiptAggregateScheme::BatchEd25519);
        let header =
            derive_receipt_header(&receipts, 50_000, params, &registry).expect("derive header");
        validate_receipt_header(&header, &receipts, params, &registry, 50_000)
            .expect("validate header");
    }
}
