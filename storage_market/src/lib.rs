#![forbid(unsafe_code)]

mod codec;
mod engine;
mod importer;
mod legacy;

pub use importer::{
    AuditEntryStatus, AuditReport, ChecksumComparison, ChecksumDigest, ChecksumScope, ImportMode,
    ImportStats, ManifestSource, ManifestSummary, StorageImporter,
};
pub use legacy::{manifest_status, ManifestStatus, LEGACY_MANIFEST_FILE, MIGRATED_MANIFEST_PREFIX};

use codec::{deserialize_contract_record, serialize_contract_record};
use engine::{Engine, Tree};
use foundation_serialization::{Deserialize, Serialize};
use std::path::Path;
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
    pub deposit_ct: u64,
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
        deposit_ct: u64,
    ) -> Self {
        Self {
            provider_id,
            allocated_shares,
            price_per_block,
            deposit_ct,
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
            let slash = self.price_per_block.min(self.deposit_ct);
            self.deposit_ct = self.deposit_ct.saturating_sub(slash);
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofRecord {
    pub object_id: String,
    pub provider_id: String,
    pub outcome: ProofOutcome,
    pub slashed_ct: u64,
    pub amount_accrued_ct: u64,
    pub remaining_deposit_ct: u64,
    pub proof_successes: u64,
    pub proof_failures: u64,
}

#[derive(Clone)]
pub struct StorageMarket {
    engine: Engine,
    contracts: Tree,
}

impl StorageMarket {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let base = path.as_ref();
        let engine = Engine::open(base)?;
        let contracts = engine.open_tree("market/contracts")?;
        legacy::migrate_if_present(base, &contracts)?;
        Ok(Self { engine, contracts })
    }

    pub fn base_path(&self) -> &Path {
        self.engine.base_path()
    }

    pub fn register_contract(
        &self,
        mut contract: StorageContract,
        replicas: Vec<ReplicaIncentive>,
    ) -> Result<ContractRecord> {
        let total_deposit: u64 = replicas.iter().map(|replica| replica.deposit_ct).sum();
        contract.total_deposit_ct = total_deposit;
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
        Ok(())
    }

    pub fn record_proof_outcome(
        &self,
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
                let remaining = replica.deposit_ct;
                (outcome, slashed, successes, failures, remaining)
            };
            if success {
                let pay_block = block.saturating_sub(1);
                let _ = record.contract.pay(pay_block);
            }
            record.contract.total_deposit_ct = record
                .replicas
                .iter()
                .map(|replica| replica.deposit_ct)
                .sum();
            let updated = serialize_contract_record(&record)?;
            match self
                .contracts
                .compare_and_swap(key, current.clone(), Some(updated.clone()))?
            {
                Ok(_) => {
                    return Ok(ProofRecord {
                        object_id: object_id.to_string(),
                        provider_id: provider,
                        outcome,
                        slashed_ct: slashed,
                        amount_accrued_ct: record.contract.accrued,
                        remaining_deposit_ct: remaining_deposit,
                        proof_successes,
                        proof_failures,
                    });
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::hex;
    use std::fs;
    use sys::tempfile::tempdir;

    #[test]
    fn record_proof_updates_replica_and_contract() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("market.db");
        let market = StorageMarket::open(&path).expect("open market");
        let contract = StorageContract {
            object_id: "obj".into(),
            provider_id: "provider-a".into(),
            original_bytes: 1024,
            shares: 4,
            price_per_block: 8,
            start_block: 0,
            retention_blocks: 10,
            next_payment_block: 1,
            accrued: 0,
            total_deposit_ct: 0,
            last_payment_block: None,
        };
        let replica = ReplicaIncentive::new("provider-a".into(), 4, 8, 80);
        market
            .register_contract(contract, vec![replica])
            .expect("register");
        let proof = market
            .record_proof_outcome("obj", None, 2, true)
            .expect("record");
        assert_eq!(proof.amount_accrued_ct, 8);
        assert_eq!(proof.remaining_deposit_ct, 80);
        let failure = market
            .record_proof_outcome("obj", None, 3, false)
            .expect("record failure");
        assert_eq!(failure.slashed_ct, 8);
        assert_eq!(failure.remaining_deposit_ct, 72);
    }

    #[test]
    fn migrates_legacy_manifest_if_present() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("market.db");
        fs::create_dir_all(&path).expect("create legacy dir");
        let manifest_path = path.join("legacy_manifest.json");

        let contract = StorageContract {
            object_id: "legacy-obj".into(),
            provider_id: "legacy-provider".into(),
            original_bytes: 512,
            shares: 2,
            price_per_block: 4,
            start_block: 0,
            retention_blocks: 4,
            next_payment_block: 1,
            accrued: 0,
            total_deposit_ct: 40,
            last_payment_block: None,
        };
        let replica = ReplicaIncentive::new("legacy-provider".into(), 2, 4, 40);
        let record = ContractRecord::with_replicas(contract, vec![replica]);
        let serialized = serialize_contract_record(&record).expect("serialize");
        let key_hex = hex::encode(record.contract.object_id.as_bytes());
        let value_hex = hex::encode(&serialized);
        let manifest = format!(
            "{{\"trees\": {{\"{}\": [{{\"key\": \"{}\", \"value\": \"{}\"}}]}}}}",
            legacy::LEGACY_TREE_HEX,
            key_hex,
            value_hex
        );
        fs::write(&manifest_path, manifest).expect("write manifest");

        let market = StorageMarket::open(&path).expect("open storage market");
        let loaded = market
            .load_contract("legacy-obj")
            .expect("load contract")
            .expect("contract present");
        assert_eq!(loaded.contract.provider_id, "legacy-provider");
        assert!(path.join("legacy_manifest.migrated.json").exists());

        // ensure migration is idempotent
        drop(market);
        let reopened = StorageMarket::open(&path).expect("reopen market");
        assert!(reopened
            .load_contract("legacy-obj")
            .expect("load contract")
            .is_some());
    }

    #[test]
    fn iteration_handles_large_cardinality() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("market.db");
        let market = StorageMarket::open(&path).expect("open market");
        for index in 0..512 {
            let contract = StorageContract {
                object_id: format!("obj-{index:04}"),
                provider_id: "primary".into(),
                original_bytes: 1_024,
                shares: 4,
                price_per_block: 8,
                start_block: 0,
                retention_blocks: 10,
                next_payment_block: 1,
                accrued: 0,
                total_deposit_ct: 0,
                last_payment_block: None,
            };
            let replica = ReplicaIncentive::new("primary".into(), 4, 8, 80);
            market
                .register_contract(contract, vec![replica])
                .expect("register contract");
        }
        let listing = market.contracts().expect("list contracts");
        assert_eq!(listing.len(), 512);
        assert_eq!(listing.first().unwrap().contract.object_id, "obj-0000");
        assert_eq!(listing.last().unwrap().contract.object_id, "obj-0511");
    }

    #[test]
    fn concurrent_record_proof_outcome_is_linearizable() {
        use std::sync::Arc;

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("market.db");
        let market = StorageMarket::open(&path).expect("open market");
        let contract = StorageContract {
            object_id: "shared".into(),
            provider_id: "primary".into(),
            original_bytes: 4_096,
            shares: 8,
            price_per_block: 12,
            start_block: 0,
            retention_blocks: 20,
            next_payment_block: 1,
            accrued: 0,
            total_deposit_ct: 0,
            last_payment_block: None,
        };
        let replica_a = ReplicaIncentive::new("primary".into(), 8, 12, 120);
        let replica_b = ReplicaIncentive::new("backup".into(), 4, 12, 60);
        market
            .register_contract(contract, vec![replica_a, replica_b])
            .expect("register contract");

        let concurrent = Arc::new(market);
        let mut handles = Vec::new();
        for thread_id in 0..8 {
            let market = Arc::clone(&concurrent);
            handles.push(std::thread::spawn(move || {
                for iteration in 0..100 {
                    let success = (thread_id + iteration) % 2 == 0;
                    let provider = if iteration % 3 == 0 {
                        "backup"
                    } else {
                        "primary"
                    };
                    let _ = market.record_proof_outcome(
                        "shared",
                        Some(provider),
                        iteration as u64,
                        success,
                    );
                }
            }));
        }
        for handle in handles {
            handle.join().expect("join worker");
        }

        let contract = concurrent
            .load_contract("shared")
            .expect("load contract")
            .expect("contract present");
        let totals: u64 = contract
            .replicas
            .iter()
            .map(|replica| replica.proof_successes + replica.proof_failures)
            .sum();
        assert!(totals > 0, "expected proof outcomes to be recorded");
        assert!(contract
            .replicas
            .iter()
            .any(|replica| replica.proof_successes > 0));
    }
}
