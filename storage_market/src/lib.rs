#![allow(
    clippy::useless_conversion,
    clippy::type_complexity,
    clippy::manual_map
)]
#![forbid(unsafe_code)]

pub mod codec;
mod engine;
mod importer;
mod legacy;
pub mod receipts;

pub use importer::{
    AuditEntryStatus, AuditReport, ChecksumComparison, ChecksumDigest, ChecksumScope, ImportMode,
    ImportStats, ManifestSource, ManifestSummary, StorageImporter,
};
pub use legacy::{manifest_status, ManifestStatus, LEGACY_MANIFEST_FILE, MIGRATED_MANIFEST_PREFIX};

use codec::{
    deserialize_contract_record, deserialize_provider_profile, serialize_contract_record,
    serialize_provider_profile,
};
use engine::{Engine, Tree};
use foundation_serialization::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;
use storage::StorageContract;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageMarketError {
    #[error("storage engine error: {0}")]
    Engine(#[from] storage_engine::StorageError),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("contract not found: {0}")]
    ContractMissing(String),
    #[error("no replicas registered for contract {0}")]
    NoReplicas(String),
    #[error("replica {provider_id} missing for contract {object_id}")]
    ReplicaMissing {
        object_id: String,
        provider_id: String,
    },
    #[error("legacy storage manifest error: {0}")]
    LegacyManifest(String),
}

pub type Result<T> = std::result::Result<T, StorageMarketError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProofOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicaIncentive {
    pub provider_id: String,
    pub allocated_shares: u16,
    pub price_per_block: u64,
    pub deposit: u64,
    #[serde(default)]
    pub proof_successes: u64,
    #[serde(default)]
    pub proof_failures: u64,
    #[serde(default)]
    pub last_proof_block: Option<u64>,
    #[serde(default)]
    pub last_outcome: Option<ProofOutcome>,
}

impl ReplicaIncentive {
    pub fn new(
        provider_id: String,
        allocated_shares: u16,
        price_per_block: u64,
        deposit: u64,
    ) -> Self {
        Self {
            provider_id,
            allocated_shares,
            price_per_block,
            deposit,
            proof_successes: 0,
            proof_failures: 0,
            last_proof_block: None,
            last_outcome: None,
        }
    }

    fn record_outcome(&mut self, block: u64, success: bool) -> (ProofOutcome, u64) {
        self.last_proof_block = Some(block);
        if success {
            self.proof_successes = self.proof_successes.saturating_add(1);
            self.last_outcome = Some(ProofOutcome::Success);
            (ProofOutcome::Success, 0)
        } else {
            self.proof_failures = self.proof_failures.saturating_add(1);
            self.last_outcome = Some(ProofOutcome::Failure);
            let slash = self.price_per_block.min(self.deposit);
            self.deposit = self.deposit.saturating_sub(slash);
            (ProofOutcome::Failure, slash)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractRecord {
    pub contract: StorageContract,
    #[serde(default)]
    pub replicas: Vec<ReplicaIncentive>,
}

impl ContractRecord {
    pub fn with_replicas(contract: StorageContract, replicas: Vec<ReplicaIncentive>) -> Self {
        Self { contract, replicas }
    }
}

/// Provider metadata published into the DHT-backed catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProfile {
    pub provider_id: String,
    #[serde(default)]
    pub region: Option<String>,
    pub max_capacity_bytes: u64,
    pub price_per_block: u64,
    pub escrow_deposit: u64,
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub proof_successes: u64,
    #[serde(default)]
    pub proof_failures: u64,
    #[serde(default)]
    pub last_seen_block: Option<u64>,
}

impl ProviderProfile {
    pub fn new(
        provider_id: String,
        max_capacity_bytes: u64,
        price_per_block: u64,
        escrow_deposit: u64,
    ) -> Self {
        Self {
            provider_id,
            region: None,
            max_capacity_bytes,
            price_per_block,
            escrow_deposit,
            version: 0,
            expires_at: None,
            latency_ms: None,
            tags: Vec::new(),
            proof_successes: 0,
            proof_failures: 0,
            last_seen_block: None,
        }
    }

    pub fn mark_version(&mut self, version: u64) {
        self.version = version;
    }

    pub fn set_expiry(&mut self, expires_at: u64) {
        self.expires_at = Some(expires_at);
    }

    pub fn is_expired(&self, now: u64) -> bool {
        match self.expires_at {
            Some(expiry) => expiry <= now,
            None => false,
        }
    }

    pub fn success_rate_ppm(&self) -> u64 {
        let total = self.proof_successes.saturating_add(self.proof_failures);
        if total == 0 {
            1_000_000
        } else {
            self.proof_successes.saturating_mul(1_000_000) / total
        }
    }
}

/// Request that powers DHT provider discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DiscoveryRequest {
    pub object_size: u64,
    pub shares: u16,
    pub region: Option<String>,
    pub max_price_per_block: Option<u64>,
    pub min_success_rate_ppm: Option<u64>,
    pub limit: usize,
}

impl DiscoveryRequest {
    fn normalized_limit(&self) -> usize {
        self.limit.clamp(1, 200)
    }

    pub fn required_capacity_bytes(&self) -> u64 {
        let shares = (self.shares.max(1)) as u128;
        let bytes = (self.object_size.max(1)) as u128;
        let chunk = (bytes + shares - 1) / shares;
        let total = chunk.saturating_mul(shares);
        total.min(u128::from(u64::MAX)) as u64
    }
}

pub trait ProviderDirectory: Send + Sync {
    fn publish(&self, profile: ProviderProfile);

    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<ProviderProfile>>;
}

static PROVIDER_DIRECTORY: OnceLock<Arc<dyn ProviderDirectory>> = OnceLock::new();

pub fn install_provider_directory(directory: Arc<dyn ProviderDirectory>) {
    let _ = PROVIDER_DIRECTORY.set(directory);
}

fn provider_directory() -> Option<Arc<dyn ProviderDirectory>> {
    PROVIDER_DIRECTORY.get().cloned()
}

const DEFAULT_PROFILE_TTL_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofRecord {
    pub object_id: String,
    pub provider_id: String,
    pub outcome: ProofOutcome,
    pub slashed: u64,
    pub amount_accrued: u64,
    pub remaining_deposit: u64,
    pub proof_successes: u64,
    pub proof_failures: u64,
}

#[derive(Clone)]
pub struct StorageMarket {
    engine: Engine,
    contracts: Tree,
    providers: Tree,
    /// Pending settlement receipts for block inclusion
    pending_receipts: Vec<receipts::StorageSettlementReceipt>,
}

impl StorageMarket {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let base = path.as_ref();
        let engine = Engine::open(base)?;
        let contracts = engine.open_tree("market/contracts")?;
        let providers = engine.open_tree("market/providers")?;
        legacy::migrate_if_present(base, &contracts)?;
        Ok(Self {
            engine,
            contracts,
            providers,
            pending_receipts: Vec::new(),
        })
    }

    pub fn base_path(&self) -> &Path {
        self.engine.base_path()
    }

    /// Drain all pending receipts for block inclusion
    pub fn drain_receipts(&mut self) -> Vec<receipts::StorageSettlementReceipt> {
        std::mem::take(&mut self.pending_receipts)
    }

    pub fn push_receipt_for_test(&mut self, receipt: receipts::StorageSettlementReceipt) {
        self.pending_receipts.push(receipt);
    }

    pub fn register_contract(
        &self,
        mut contract: StorageContract,
        replicas: Vec<ReplicaIncentive>,
    ) -> Result<ContractRecord> {
        let total_deposit: u64 = replicas.iter().map(|replica| replica.deposit).sum();
        contract.total_deposit = total_deposit;
        let record = ContractRecord::with_replicas(contract, replicas);
        let key = record.contract.object_id.as_bytes();
        let value = serialize_contract_record(&record)?;
        let _ = self.contracts.insert(key, value)?;
        Ok(record)
    }

    pub fn load_contract(&self, object_id: &str) -> Result<Option<ContractRecord>> {
        let maybe = self.contracts.get(object_id.as_bytes())?;
        maybe
            .map(|bytes| deserialize_contract_record(&bytes))
            .transpose()
    }

    pub fn contracts(&self) -> Result<Vec<ContractRecord>> {
        let mut records: Vec<ContractRecord> = Vec::new();
        for entry in self.contracts.iter() {
            let (_, value) = entry?;
            records.push(deserialize_contract_record(&value)?);
        }
        records.sort_by(|a, b| a.contract.object_id.cmp(&b.contract.object_id));
        Ok(records)
    }

    pub fn clear(&self) -> Result<()> {
        self.contracts.clear()?;
        self.providers.clear()?;
        Ok(())
    }

    pub fn record_proof_outcome(
        &mut self,
        object_id: &str,
        provider_id: Option<&str>,
        block: u64,
        success: bool,
    ) -> Result<ProofRecord> {
        let key = object_id.as_bytes();
        loop {
            let current = self.contracts.get(key)?;
            let Some(bytes) = current.clone() else {
                return Err(StorageMarketError::ContractMissing(object_id.to_string()));
            };
            let mut record: ContractRecord = deserialize_contract_record(&bytes)?;
            let provider = if let Some(id) = provider_id {
                id.to_string()
            } else {
                record
                    .replicas
                    .first()
                    .map(|replica| replica.provider_id.clone())
                    .ok_or_else(|| StorageMarketError::NoReplicas(object_id.to_string()))?
            };
            let (outcome, slashed, proof_successes, proof_failures, remaining_deposit) = {
                let replica = record
                    .replicas
                    .iter_mut()
                    .find(|replica| replica.provider_id == provider)
                    .ok_or_else(|| StorageMarketError::ReplicaMissing {
                        object_id: object_id.to_string(),
                        provider_id: provider.clone(),
                    })?;
                let (outcome, slashed) = replica.record_outcome(block, success);
                let successes = replica.proof_successes;
                let failures = replica.proof_failures;
                let remaining = replica.deposit;
                (outcome, slashed, successes, failures, remaining)
            };
            if success {
                let pay_block = block.saturating_sub(1);
                let _ = record.contract.pay(pay_block);
            }
            record.contract.total_deposit =
                record.replicas.iter().map(|replica| replica.deposit).sum();
            let updated = serialize_contract_record(&record)?;
            match self
                .contracts
                .compare_and_swap(key, current.clone(), Some(updated.clone()))?
            {
                Ok(_) => {
                    let proof = ProofRecord {
                        object_id: object_id.to_string(),
                        provider_id: provider,
                        outcome,
                        slashed,
                        amount_accrued: record.contract.accrued,
                        remaining_deposit,
                        proof_successes,
                        proof_failures,
                    };

                    // Emit receipt for successful settlements with payment
                    if let Some(receipt) = receipts::StorageSettlementReceipt::from_proof(
                        &proof,
                        record.contract.original_bytes,
                        record.contract.price_per_block,
                        block,
                    ) {
                        self.pending_receipts.push(receipt);
                    }

                    let _ = self.record_provider_feedback(&proof.provider_id, success, block);

                    return Ok(proof);
                }
                Err(_) => continue,
            }
        }
    }

    pub fn replicas_for(&self, object_id: &str) -> Result<Vec<ReplicaIncentive>> {
        let record = self
            .load_contract(object_id)?
            .ok_or_else(|| StorageMarketError::ContractMissing(object_id.to_string()))?;
        Ok(record.replicas)
    }

    fn default_expiry() -> u64 {
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_add(DEFAULT_PROFILE_TTL_SECS)
    }

    pub fn register_provider_profile(
        &self,
        mut profile: ProviderProfile,
    ) -> Result<ProviderProfile> {
        let key = profile.provider_id.as_bytes();
        if let Some(existing) = self.providers.get(key)? {
            let existing_profile = deserialize_provider_profile(&existing)?;
            let next_version = existing_profile.version.saturating_add(1);
            profile.proof_successes = existing_profile.proof_successes;
            profile.proof_failures = existing_profile.proof_failures;
            profile.last_seen_block = existing_profile.last_seen_block;
            if profile.version == 0 {
                profile.version = next_version;
            }
        } else if profile.version == 0 {
            profile.version = 1;
        }
        if profile.expires_at.is_none() {
            profile.expires_at = Some(Self::default_expiry());
        }
        let bytes = serialize_provider_profile(&profile)?;
        let _ = self.providers.insert(key, bytes)?;
        if let Some(directory) = provider_directory() {
            directory.publish(profile.clone());
        }
        Ok(profile)
    }

    pub fn cache_provider_profile(
        &self,
        mut profile: ProviderProfile,
    ) -> Result<Option<ProviderProfile>> {
        let key = profile.provider_id.as_bytes();
        if let Some(existing) = self.providers.get(key)? {
            let existing_profile = deserialize_provider_profile(&existing)?;
            let newer = profile.version > existing_profile.version
                || (profile.version == existing_profile.version
                    && profile.expires_at.unwrap_or(0) > existing_profile.expires_at.unwrap_or(0));
            if !newer {
                return Ok(None);
            }
            profile.proof_successes = existing_profile.proof_successes;
            profile.proof_failures = existing_profile.proof_failures;
            profile.last_seen_block = existing_profile.last_seen_block;
        }
        if profile.expires_at.is_none() {
            profile.expires_at = Some(Self::default_expiry());
        }
        let bytes = serialize_provider_profile(&profile)?;
        let _ = self.providers.insert(key, bytes)?;
        Ok(Some(profile))
    }

    pub fn provider_profile(&self, provider_id: &str) -> Result<Option<ProviderProfile>> {
        let key = provider_id.as_bytes();
        self.providers
            .get(key)?
            .map(|bytes| deserialize_provider_profile(&bytes))
            .transpose()
    }

    pub fn provider_profiles(&self) -> Result<Vec<ProviderProfile>> {
        let mut profiles = Vec::new();
        for entry in self.providers.iter() {
            let (_, value) = entry?;
            profiles.push(deserialize_provider_profile(&value)?);
        }
        Ok(profiles)
    }

    pub fn discover_providers(&self, request: DiscoveryRequest) -> Result<Vec<ProviderProfile>> {
        let limit = request.normalized_limit();
        let min_capacity = request.required_capacity_bytes();
        let mut candidates = Vec::new();
        if let Some(directory) = provider_directory() {
            let fetched = directory.discover(&request)?;
            for profile in fetched {
                let _ = self.cache_provider_profile(profile.clone())?;
                candidates.push(profile);
            }
        }
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        for entry in self.providers.iter() {
            let (_, value) = entry?;
            let profile = deserialize_provider_profile(&value)?;
            if profile.is_expired(now) {
                continue;
            }
            if profile.max_capacity_bytes < min_capacity {
                continue;
            }
            if let Some(region) = &request.region {
                if profile.region.as_deref() != Some(region.as_str()) {
                    continue;
                }
            }
            if let Some(max_price) = request.max_price_per_block {
                if profile.price_per_block > max_price {
                    continue;
                }
            }
            if let Some(min_success) = request.min_success_rate_ppm {
                if profile.success_rate_ppm() < min_success {
                    continue;
                }
            }
            candidates.push(profile);
        }
        candidates.sort_by(|a, b| {
            a.price_per_block
                .cmp(&b.price_per_block)
                .then_with(|| b.last_seen_block.cmp(&a.last_seen_block))
                .then_with(|| b.version.cmp(&a.version))
                .then_with(|| a.provider_id.cmp(&b.provider_id))
        });
        if candidates.len() > limit {
            candidates.truncate(limit);
        }
        Ok(candidates)
    }

    fn record_provider_feedback(&self, provider_id: &str, success: bool, block: u64) -> Result<()> {
        let key = provider_id.as_bytes();
        let mut profile = self
            .providers
            .get(key)?
            .map(|bytes| deserialize_provider_profile(&bytes))
            .transpose()?
            .unwrap_or_else(|| ProviderProfile::new(provider_id.to_string(), 0, 0, 0));
        if success {
            profile.proof_successes = profile.proof_successes.saturating_add(1);
        } else {
            profile.proof_failures = profile.proof_failures.saturating_add(1);
        }
        profile.last_seen_block = Some(block);
        profile.version = profile.version.saturating_add(1);
        profile.expires_at = Some(Self::default_expiry());
        let bytes = serialize_provider_profile(&profile)?;
        let _ = self.providers.insert(key, bytes)?;
        if let Some(directory) = provider_directory() {
            directory.publish(profile);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

    fn temp_market() -> StorageMarket {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        Box::leak(Box::new(dir));
        StorageMarket::open(&path).expect("open market")
    }

    #[test]
    fn discovery_filters_on_price() {
        let market = temp_market();
        let cheap = ProviderProfile::new("cheap".into(), 4096, 5, 100);
        let pricey = ProviderProfile::new("expensive".into(), 4096, 12, 100);
        market
            .register_provider_profile(cheap)
            .expect("register cheap");
        market
            .register_provider_profile(pricey)
            .expect("register pricey");
        let request = DiscoveryRequest {
            object_size: 32,
            shares: 2,
            region: None,
            max_price_per_block: Some(8),
            min_success_rate_ppm: None,
            limit: 10,
        };
        let providers = market.discover_providers(request).expect("discover");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider_id, "cheap");
    }

    #[test]
    fn discovery_respects_region_and_success_rate() {
        let market = temp_market();
        let mut good = ProviderProfile::new("good".into(), 4096, 7, 100);
        good.region = Some("us".into());
        let mut ok = ProviderProfile::new("ok".into(), 4096, 7, 100);
        ok.region = Some("us".into());
        market
            .register_provider_profile(good)
            .expect("register good");
        market.register_provider_profile(ok).expect("register ok");
        market
            .record_provider_feedback("good", true, 1)
            .expect("feedback good");
        market
            .record_provider_feedback("ok", true, 1)
            .expect("feedback ok");
        market
            .record_provider_feedback("ok", false, 2)
            .expect("feedback fail");

        let request = DiscoveryRequest {
            object_size: 64,
            shares: 4,
            region: Some("us".into()),
            max_price_per_block: None,
            min_success_rate_ppm: Some(950_000),
            limit: 5,
        };

        let providers = market.discover_providers(request).expect("discover");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider_id, "good");
    }

    #[test]
    fn provider_profile_serialization_roundtrip() {
        let mut profile = ProviderProfile::new("prov-123".into(), 8 * 1024, 10, 250);
        profile.region = Some("europe".into());
        profile.latency_ms = Some(42);
        profile.tags = vec!["gpu".into(), "ssd".into()];
        profile.version = 3;
        profile.expires_at = Some(9_999);
        profile.proof_successes = 9;
        profile.proof_failures = 1;
        profile.last_seen_block = Some(1337);

        let bytes = serialize_provider_profile(&profile).expect("serialize profile");
        let recovered = deserialize_provider_profile(&bytes).expect("deserialize profile");
        assert_eq!(profile, recovered);
    }
}
