use serde::Serialize;
use storage::StorageContract;

#[derive(Serialize)]
pub struct StorageContractView {
    pub object_id: String,
    pub provider_id: String,
    pub price_per_block: u64,
}

pub fn render(contracts: &[StorageContract]) -> Vec<StorageContractView> {
    contracts
        .iter()
        .map(|c| StorageContractView {
            object_id: c.object_id.clone(),
            provider_id: c.provider_id.clone(),
            price_per_block: c.price_per_block,
        })
        .collect()
}
