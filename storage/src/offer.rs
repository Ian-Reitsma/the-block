use foundation_serialization::{Deserialize, Serialize};

/// StorageOffer advertises available storage capacity
/// along with pricing and retention policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
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
    pub fn new(
        provider_id: String,
        capacity_bytes: u64,
        price_per_byte: u64,
        retention_blocks: u64,
    ) -> Self {
        Self {
            provider_id,
            capacity_bytes,
            price_per_byte,
            retention_blocks,
        }
    }
}

/// Allocate `shares` across offers using capacity-weighted proportions.
pub fn allocate_shards(offers: &[StorageOffer], shares: u16) -> Vec<(String, u16)> {
    if offers.is_empty() || shares == 0 {
        return Vec::new();
    }
    let total_cap: u128 = offers.iter().map(|o| o.capacity_bytes as u128).sum();
    let mut allocs: Vec<(String, u16, u128)> = offers
        .iter()
        .map(|o| {
            let prod = (o.capacity_bytes as u128) * (shares as u128);
            let base = (prod / total_cap) as u16;
            let rem = prod % total_cap;
            (o.provider_id.clone(), base, rem)
        })
        .collect();
    let allocated: u16 = allocs.iter().map(|(_, a, _)| *a).sum();
    let mut remaining = shares - allocated;
    allocs.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let n = allocs.len();
    let mut i = 0;
    while remaining > 0 {
        allocs[i % n].1 += 1;
        remaining -= 1;
        i += 1;
    }
    allocs.into_iter().map(|(id, a, _)| (id, a)).collect()
}

#[cfg(test)]
mod tests {
    use super::{allocate_shards, StorageOffer};

    #[test]
    fn deterministic_allocation() {
        let offers = vec![
            StorageOffer::new("a".into(), 1, 1, 1),
            StorageOffer::new("b".into(), 3, 1, 1),
        ];
        let alloc = allocate_shards(&offers, 10);
        assert_eq!(alloc, vec![("a".into(), 3), ("b".into(), 7)]);
    }

    #[test]
    fn extreme_capacities() {
        let offers = vec![
            StorageOffer::new("x".into(), u64::MAX - 1, 1, 1),
            StorageOffer::new("y".into(), 1, 1, 1),
            StorageOffer::new("z".into(), 2, 1, 1),
        ];
        let alloc = allocate_shards(&offers, 7);
        let total: u16 = alloc.iter().map(|(_, s)| *s).sum();
        assert_eq!(total, 7);
        // largest capacity should dominate
        assert!(alloc.iter().any(|(id, s)| id == "x" && *s > 0));
    }
}
