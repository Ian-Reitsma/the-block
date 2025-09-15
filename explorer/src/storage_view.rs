use serde::Serialize;
use storage::StorageContract;

use crate::ProviderStorageStat;

#[derive(Serialize)]
pub struct StorageContractView {
    pub object_id: String,
    pub provider_id: String,
    pub price_per_block: u64,
}

#[derive(Serialize)]
pub struct ProviderStatsView {
    pub provider_id: String,
    pub contracts: u64,
    pub capacity_bytes: u64,
    pub reputation: i64,
}

pub fn render_contracts(contracts: &[StorageContract]) -> Vec<StorageContractView> {
    contracts
        .iter()
        .map(|c| StorageContractView {
            object_id: c.object_id.clone(),
            provider_id: c.provider_id.clone(),
            price_per_block: c.price_per_block,
        })
        .collect()
}

pub fn render_provider_stats(stats: &[ProviderStorageStat]) -> Vec<ProviderStatsView> {
    stats
        .iter()
        .map(|s| ProviderStatsView {
            provider_id: s.provider_id.clone(),
            contracts: s.contracts,
            capacity_bytes: s.capacity_bytes,
            reputation: s.reputation,
        })
        .collect()
}
