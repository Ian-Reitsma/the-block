#![allow(
    clippy::useless_conversion,
    clippy::type_complexity,
    clippy::manual_map
)]
#![forbid(unsafe_code)]

mod codec;
mod engine;
mod importer;
mod legacy;
pub mod receipts;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofRecord {
    pub object_id: String,
    pub provider_id: String,
    pub outcome: ProofOutcome,
    pub slashed_ct: u64,
    pub amount_accrued_ct: u64,
    pub remaining_deposit: u64,
    pub proof_successes: u64,
    pub proof_failures: u64,
}

#[derive(Clone)]
pub struct StorageMarket {
    engine: Engine,
    contracts: Tree,
    /// Pending settlement receipts for block inclusion
    pending_receipts: Vec<receipts::StorageSettlementReceipt>,
}

impl StorageMarket {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let base = path.as_ref();
        let engine = Engine::open(base)?;
        let contracts = engine.open_tree("market/contracts")?;
        legacy::migrate_if_present(base, &contracts)?;
        Ok(Self {
            engine,
            contracts,
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
                        slashed_ct: slashed,
                        amount_accrued_ct: record.contract.accrued,
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
}
