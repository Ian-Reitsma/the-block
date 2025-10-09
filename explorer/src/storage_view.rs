use foundation_serialization::Serialize;
use storage::StorageContract;
use the_block::storage::pipeline::ManifestSummary;

use crate::ProviderStorageStat;
use the_block::storage::repair::RepairLogEntry;

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

#[derive(Serialize)]
pub struct RepairHistoryView {
    pub manifest: String,
    pub chunk: Option<u32>,
    pub status: String,
    pub bytes: u64,
    pub error: Option<String>,
    pub timestamp: i64,
}

#[derive(Serialize)]
pub struct ManifestAlgorithmView {
    pub manifest: String,
    pub total_len: u64,
    pub chunk_count: u32,
    pub erasure: String,
    pub compression: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub encryption: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub compression_level: Option<i32>,
    pub erasure_fallback: bool,
    pub compression_fallback: bool,
}

#[derive(Serialize)]
pub struct AlgorithmPolicyView {
    pub algorithm: String,
    pub fallback: bool,
    pub emergency: bool,
}

#[derive(Serialize)]
pub struct ManifestPolicyView {
    pub erasure: AlgorithmPolicyView,
    pub compression: AlgorithmPolicyView,
}

#[derive(Serialize)]
pub struct ManifestListingView {
    pub policy: ManifestPolicyView,
    pub manifests: Vec<ManifestAlgorithmView>,
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

pub fn render_repair_history(entries: &[RepairLogEntry]) -> Vec<RepairHistoryView> {
    entries
        .iter()
        .map(|entry| RepairHistoryView {
            manifest: entry.manifest.clone(),
            chunk: entry.chunk,
            status: entry.status.to_string(),
            bytes: entry.bytes,
            error: entry.error.clone(),
            timestamp: entry.timestamp,
        })
        .collect()
}

pub fn render_manifest_listing(
    manifests: &[ManifestSummary],
    policy: ManifestPolicyView,
) -> ManifestListingView {
    let manifests = manifests
        .iter()
        .map(|entry| ManifestAlgorithmView {
            manifest: entry.manifest.clone(),
            total_len: entry.total_len,
            chunk_count: entry.chunk_count,
            erasure: entry.erasure.clone(),
            compression: entry.compression.clone(),
            encryption: entry.encryption.clone(),
            compression_level: entry.compression_level,
            erasure_fallback: entry.erasure_fallback,
            compression_fallback: entry.compression_fallback,
        })
        .collect();
    ManifestListingView { policy, manifests }
}
