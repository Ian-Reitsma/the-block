use serde::{Serialize, Deserialize};

/// StorageOffer advertises available storage capacity
/// along with pricing and retention policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageOffer {
    /// Unique identifier of the provider
    pub provider_id: String,
    /// Available bytes for allocation
    pub capacity_bytes: u64,
    /// Price per byte per block in CT
    pub price_per_byte: u64,
    /// Retention policy in blocks
    pub retention_blocks: u64,
}

impl StorageOffer {
    /// Create a new offer.
    pub fn new(provider_id: String, capacity_bytes: u64, price_per_byte: u64, retention_blocks: u64) -> Self {
        Self { provider_id, capacity_bytes, price_per_byte, retention_blocks }
    }
}

/// Allocate `shares` across offers using capacity-weighted proportions.
pub fn allocate_shards(offers: &[StorageOffer], shares: u16) -> Vec<(String, u16)> {
    if offers.is_empty() || shares == 0 {
        return Vec::new();
    }
    let total_cap: u64 = offers.iter().map(|o| o.capacity_bytes).sum();
    offers
        .iter()
        .map(|o| {
            let frac = (o.capacity_bytes as f64) / (total_cap as f64);
            let alloc = (frac * shares as f64).round() as u16;
            (o.provider_id.clone(), alloc)
        })
        .collect()
}
