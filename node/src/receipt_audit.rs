use crate::receipt_crypto::{ProviderRegistrationSource, ProviderRegistry};
use crate::receipts::{
    AdReceipt, ComputeReceipt, ComputeSlashReceipt, EnergyReceipt, EnergySlashReceipt, Receipt,
    RelayReceipt, StorageReceipt, StorageSlashReceipt,
};
use crypto_suite::hashing::blake3;
use crypto_suite::hex;
use foundation_serialization::{Deserialize, Serialize};

/// Key/value pair exposed by audit queries to capture the settled context.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AuditDetail {
    pub key: String,
    pub value: String,
}

/// Deterministic query describing why a receipt triggered a transfer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AuditQuery {
    pub query_id: [u8; 32],
    pub market: String,
    pub subject: EscrowEntity,
    pub counterparty: EscrowEntity,
    pub amount: u64,
    pub block_height: u64,
    pub reason: String,
    pub details: Vec<AuditDetail>,
}

/// Entities that participate in escrow movements.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum EscrowEntity {
    Provider(String),
    Contract(String),
    Campaign(String),
    Job(String),
    Publisher(String),
    Treasury,
    Unknown(String),
}

impl EscrowEntity {
    fn normalized_bytes(&self) -> Vec<u8> {
        match self {
            EscrowEntity::Provider(id) => [b"provider:", id.as_bytes()].concat(),
            EscrowEntity::Contract(id) => [b"contract:", id.as_bytes()].concat(),
            EscrowEntity::Campaign(id) => [b"campaign:", id.as_bytes()].concat(),
            EscrowEntity::Job(id) => [b"job:", id.as_bytes()].concat(),
            EscrowEntity::Publisher(id) => [b"publisher:", id.as_bytes()].concat(),
            EscrowEntity::Treasury => b"treasury".to_vec(),
            EscrowEntity::Unknown(id) => [b"unknown:", id.as_bytes()].concat(),
        }
    }
}

/// The direction of the escrow movement recorded by the receipt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum CausalityKind {
    DirectSettlement,
    Slash,
}

/// Causality information for evidence-driven audits.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct CausalityEffect {
    pub kind: CausalityKind,
    pub amount: u64,
    pub source: EscrowEntity,
    pub target: EscrowEntity,
    pub context: String,
    pub block_height: u64,
}

/// Severity tiers for invariant violations.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum InvariantSeverity {
    Critical,
    High,
    Medium,
    Low,
}

/// Slashing action to take when an invariant fails.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SlashingOutcome {
    pub reason: String,
    pub amount: u64,
    pub target: EscrowEntity,
}

/// Report describing a receipt invariant and its slashing commitment.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptInvariantReport {
    pub name: String,
    pub description: String,
    pub severity: InvariantSeverity,
    pub satisfied: bool,
    pub slashing: Option<SlashingOutcome>,
}

/// Historical provider key summary exposed to auditors.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderKeyHistory {
    pub key: [u8; 32],
    pub registered_at_block: u64,
    pub retired_at_block: Option<u64>,
    pub evidence: Option<String>,
}

/// Provider identity summary emitted alongside each receipt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderIdentitySummary {
    pub provider_id: String,
    pub stake_reference: Option<String>,
    pub rotation_count: usize,
    pub latest_key: Option<[u8; 32]>,
    pub key_history: Vec<ProviderKeyHistory>,
}

/// Traits describing evidence exposures derived from receipts.
pub trait ReceiptEvidence {
    fn audit_queries(&self) -> Vec<AuditQuery>;
    fn invariants(&self, registry: &ProviderRegistry) -> Vec<ReceiptInvariantReport>;
    fn causality_effect(&self) -> CausalityEffect;
    fn identity(&self, registry: &ProviderRegistry) -> Option<ProviderIdentitySummary>;
}

impl ReceiptEvidence for Receipt {
    fn audit_queries(&self) -> Vec<AuditQuery> {
        match self {
            Receipt::Storage(receipt) => vec![storage_audit(receipt)],
            Receipt::Compute(receipt) => vec![compute_audit(receipt)],
            Receipt::Energy(receipt) => vec![energy_audit(receipt)],
            Receipt::Ad(receipt) => vec![ad_audit(receipt)],
            Receipt::Relay(receipt) => vec![relay_audit(receipt)],
            Receipt::StorageSlash(receipt) => vec![slash_audit(
                "storage_slash",
                receipt.provider.clone(),
                receipt.amount,
                receipt.block_height,
                format!("storage slash {}", receipt.reason),
            )],
            Receipt::ComputeSlash(receipt) => vec![slash_audit(
                "compute_slash",
                receipt.provider.clone(),
                receipt.burned,
                receipt.block_height,
                format!("compute slash {}", receipt.reason),
            )],
            Receipt::EnergySlash(receipt) => vec![slash_audit(
                "energy_slash",
                receipt.provider.clone(),
                receipt.slash_amount,
                receipt.block_height,
                format!("energy slash {}", receipt.reason),
            )],
        }
    }

    fn invariants(&self, registry: &ProviderRegistry) -> Vec<ReceiptInvariantReport> {
        match self {
            Receipt::Storage(receipt) => storage_invariants(receipt, registry),
            Receipt::Compute(receipt) => compute_invariants(receipt, registry),
            Receipt::Energy(receipt) => energy_invariants(receipt, registry),
            Receipt::Ad(receipt) => ad_invariants(receipt, registry),
            Receipt::Relay(receipt) => relay_invariants(receipt, registry),
            Receipt::StorageSlash(receipt) => slash_invariants(receipt, registry),
            Receipt::ComputeSlash(receipt) => slash_invariants(receipt, registry),
            Receipt::EnergySlash(receipt) => slash_invariants(receipt, registry),
        }
    }

    fn causality_effect(&self) -> CausalityEffect {
        match self {
            Receipt::Storage(receipt) => storage_causality(receipt),
            Receipt::Compute(receipt) => compute_causality(receipt),
            Receipt::Energy(receipt) => energy_causality(receipt),
            Receipt::Ad(receipt) => ad_causality(receipt),
            Receipt::Relay(receipt) => relay_causality(receipt),
            Receipt::StorageSlash(receipt) => slash_causality(
                receipt.provider.clone(),
                receipt.amount,
                receipt.block_height,
                "storage slash",
            ),
            Receipt::ComputeSlash(receipt) => slash_causality(
                receipt.provider.clone(),
                receipt.burned,
                receipt.block_height,
                "compute slash",
            ),
            Receipt::EnergySlash(receipt) => slash_causality(
                receipt.provider.clone(),
                receipt.slash_amount,
                receipt.block_height,
                "energy slash",
            ),
        }
    }

    fn identity(&self, registry: &ProviderRegistry) -> Option<ProviderIdentitySummary> {
        match self {
            Receipt::Storage(r) => provider_identity_summary(registry, &r.provider),
            Receipt::Compute(r) => provider_identity_summary(registry, &r.provider),
            Receipt::Energy(r) => provider_identity_summary(registry, &r.provider),
            Receipt::Ad(r) => provider_identity_summary(registry, &r.publisher),
            Receipt::Relay(r) => provider_identity_summary(registry, &r.provider),
            Receipt::StorageSlash(r) => provider_identity_summary(registry, &r.provider),
            Receipt::ComputeSlash(r) => provider_identity_summary(registry, &r.provider),
            Receipt::EnergySlash(r) => provider_identity_summary(registry, &r.provider),
        }
    }
}

fn build_audit_query(
    market: impl Into<String>,
    subject: EscrowEntity,
    counterparty: EscrowEntity,
    amount: u64,
    block_height: u64,
    reason: impl Into<String>,
    details: Vec<AuditDetail>,
) -> AuditQuery {
    let market_string = market.into();
    let reason_string = reason.into();
    let subject_bytes = subject.normalized_bytes();
    let counterparty_bytes = counterparty.normalized_bytes();
    let mut hasher = blake3::Hasher::new();
    hasher.update(market_string.as_bytes());
    hasher.update(&subject_bytes);
    hasher.update(&counterparty_bytes);
    hasher.update(reason_string.as_bytes());
    hasher.update(&amount.to_le_bytes());
    hasher.update(&block_height.to_le_bytes());
    for detail in &details {
        hasher.update(detail.key.as_bytes());
        hasher.update(detail.value.as_bytes());
    }
    AuditQuery {
        query_id: hasher.finalize().into(),
        market: market_string,
        subject,
        counterparty,
        amount,
        block_height,
        reason: reason_string,
        details,
    }
}

fn audit_detail(key: impl Into<String>, value: impl ToString) -> AuditDetail {
    AuditDetail {
        key: key.into(),
        value: value.to_string(),
    }
}

fn hex_compress(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn storage_audit(receipt: &StorageReceipt) -> AuditQuery {
    let subject = EscrowEntity::Provider(receipt.provider.clone());
    let counterparty = EscrowEntity::Contract(receipt.contract_id.clone());
    let mut details = vec![
        audit_detail("bytes", receipt.bytes),
        audit_detail("price", receipt.price),
        audit_detail(
            "region",
            receipt.region.clone().unwrap_or_else(|| "unknown".into()),
        ),
    ];
    if let Some(chunk) = &receipt.chunk_hash {
        details.push(audit_detail("chunk_hash", hex_compress(chunk)));
    }
    build_audit_query(
        "storage",
        subject,
        counterparty,
        receipt.price,
        receipt.block_height,
        "storage settlement",
        details,
    )
}

fn compute_audit(receipt: &ComputeReceipt) -> AuditQuery {
    let subject = EscrowEntity::Provider(receipt.provider.clone());
    let counterparty = EscrowEntity::Job(receipt.job_id.clone());
    let mut details = vec![
        audit_detail("units", receipt.compute_units),
        audit_detail("payment", receipt.payment),
        audit_detail("verified", receipt.verified),
    ];
    if let Some(meta) = &receipt.blocktorch {
        details.push(audit_detail("latency_ms", meta.proof_latency_ms));
        details.push(audit_detail(
            "kernel_digest",
            hex_compress(&meta.kernel_variant_digest),
        ));
        details.push(audit_detail(
            "output_digest",
            hex_compress(&meta.output_digest),
        ));
    }
    build_audit_query(
        "compute",
        subject,
        counterparty,
        receipt.payment,
        receipt.block_height,
        "compute settlement",
        details,
    )
}

fn energy_audit(receipt: &EnergyReceipt) -> AuditQuery {
    let subject = EscrowEntity::Provider(receipt.provider.clone());
    let counterparty = EscrowEntity::Contract(receipt.contract_id.clone());
    let details = vec![
        audit_detail("kwh_units", receipt.energy_units),
        audit_detail("price", receipt.price),
        audit_detail("proof_hash", hex_compress(&receipt.proof_hash)),
    ];
    build_audit_query(
        "energy",
        subject,
        counterparty,
        receipt.price,
        receipt.block_height,
        "energy settlement",
        details,
    )
}

fn ad_audit(receipt: &AdReceipt) -> AuditQuery {
    let subject = EscrowEntity::Publisher(receipt.publisher.clone());
    let counterparty = EscrowEntity::Campaign(receipt.campaign_id.clone());
    let details = vec![
        audit_detail("impressions", receipt.impressions),
        audit_detail("spend", receipt.spend),
        audit_detail("conversions", receipt.conversions),
    ];
    build_audit_query(
        "ad",
        subject,
        counterparty,
        receipt.spend,
        receipt.block_height,
        "ad settlement",
        details,
    )
}

fn relay_audit(receipt: &RelayReceipt) -> AuditQuery {
    let subject = EscrowEntity::Provider(receipt.provider.clone());
    let mut details = vec![
        audit_detail("bytes", receipt.bytes),
        audit_detail("usd_total", receipt.total_usd_micros),
        audit_detail("clearing_price", receipt.clearing_price_usd_micros),
        audit_detail("resource_floor", receipt.resource_floor_usd_micros),
    ];
    if let Some(mesh) = &receipt.mesh_peer {
        details.push(audit_detail("mesh_peer", mesh));
    }
    build_audit_query(
        "relay",
        subject,
        EscrowEntity::Job(receipt.job_id.clone()),
        receipt.total_usd_micros,
        receipt.block_height,
        "relay settlement",
        details,
    )
}

fn slash_audit(
    market: &'static str,
    provider: String,
    amount: u64,
    block_height: u64,
    reason: String,
) -> AuditQuery {
    let reason_static = match market {
        "storage_slash" => "storage slash",
        "compute_slash" => "compute slash",
        "energy_slash" => "energy slash",
        _ => "slash",
    };
    build_audit_query(
        market,
        EscrowEntity::Provider(provider.clone()),
        EscrowEntity::Treasury,
        amount,
        block_height,
        reason_static,
        vec![audit_detail("reason", reason)],
    )
}

fn storage_invariants(
    receipt: &StorageReceipt,
    registry: &ProviderRegistry,
) -> Vec<ReceiptInvariantReport> {
    let mut invariants = Vec::new();
    invariants.push(identity_invariant(
        &receipt.provider,
        receipt.price,
        registry,
    ));
    invariants.push(ReceiptInvariantReport {
        name: "storage_escrow_coverage".into(),
        description: "Provider escrow must cover the settled BLOCK".into(),
        severity: InvariantSeverity::High,
        satisfied: receipt.provider_escrow >= receipt.price,
        slashing: if receipt.provider_escrow >= receipt.price {
            None
        } else {
            Some(SlashingOutcome {
                reason: "insufficient escrow".into(),
                amount: receipt.price,
                target: EscrowEntity::Provider(receipt.provider.clone()),
            })
        },
    });
    invariants.push(ReceiptInvariantReport {
        name: "storage_chunk_fingerprint".into(),
        description: "Storage receipts must cite the chunk fingerprint for repairs".into(),
        severity: InvariantSeverity::High,
        satisfied: receipt.chunk_hash.is_some(),
        slashing: if receipt.chunk_hash.is_some() {
            None
        } else {
            Some(SlashingOutcome {
                reason: "missing chunk fingerprint".into(),
                amount: receipt.price,
                target: EscrowEntity::Provider(receipt.provider.clone()),
            })
        },
    });
    invariants
}

fn compute_invariants(
    receipt: &ComputeReceipt,
    registry: &ProviderRegistry,
) -> Vec<ReceiptInvariantReport> {
    let mut invariants = Vec::new();
    invariants.push(identity_invariant(
        &receipt.provider,
        receipt.payment,
        registry,
    ));
    let has_meta = receipt.blocktorch.as_ref().map(|meta| {
        meta.kernel_variant_digest != [0u8; 32]
            && meta.descriptor_digest != [0u8; 32]
            && meta.output_digest != [0u8; 32]
            && meta.proof_latency_ms > 0
            && meta
                .benchmark_commit
                .as_ref()
                .map(|value| !value.is_empty())
                .unwrap_or(false)
            && meta
                .tensor_profile_epoch
                .as_ref()
                .map(|value| !value.is_empty())
                .unwrap_or(false)
    });
    invariants.push(ReceiptInvariantReport {
        name: "compute_blocktorch_metadata".into(),
        description: "Compute receipts must list the BlockTorch provenance bundle".into(),
        severity: InvariantSeverity::Critical,
        satisfied: has_meta.unwrap_or(false),
        slashing: if has_meta.unwrap_or(false) {
            None
        } else {
            Some(SlashingOutcome {
                reason: "missing BlockTorch metadata".into(),
                amount: receipt.payment,
                target: EscrowEntity::Provider(receipt.provider.clone()),
            })
        },
    });
    invariants
}

fn energy_invariants(
    receipt: &EnergyReceipt,
    registry: &ProviderRegistry,
) -> Vec<ReceiptInvariantReport> {
    let mut invariants = Vec::new();
    invariants.push(identity_invariant(
        &receipt.provider,
        receipt.price,
        registry,
    ));
    invariants.push(ReceiptInvariantReport {
        name: "energy_proof_hash".into(),
        description: "Energy receipts require a non-zero proof hash".into(),
        severity: InvariantSeverity::Critical,
        satisfied: receipt.proof_hash != [0u8; 32],
        slashing: if receipt.proof_hash != [0u8; 32] {
            None
        } else {
            Some(SlashingOutcome {
                reason: "missing proof hash".into(),
                amount: receipt.price,
                target: EscrowEntity::Provider(receipt.provider.clone()),
            })
        },
    });
    invariants
}

fn ad_invariants(receipt: &AdReceipt, registry: &ProviderRegistry) -> Vec<ReceiptInvariantReport> {
    let mut invariants = Vec::new();
    invariants.push(identity_invariant(
        &receipt.publisher,
        receipt.spend,
        registry,
    ));
    invariants.push(ReceiptInvariantReport {
        name: "ad_conversion_bounds".into(),
        description: "Conversions may not exceed impressions".into(),
        severity: InvariantSeverity::Medium,
        satisfied: receipt.conversions as u64 <= receipt.impressions,
        slashing: if receipt.conversions as u64 <= receipt.impressions {
            None
        } else {
            Some(SlashingOutcome {
                reason: "invalid conversion count".into(),
                amount: receipt.spend,
                target: EscrowEntity::Publisher(receipt.publisher.clone()),
            })
        },
    });
    invariants
}

fn relay_invariants(
    receipt: &RelayReceipt,
    registry: &ProviderRegistry,
) -> Vec<ReceiptInvariantReport> {
    let mut invariants = Vec::new();
    invariants.push(identity_invariant(
        &receipt.provider,
        receipt.total_usd_micros,
        registry,
    ));
    invariants.push(ReceiptInvariantReport {
        name: "relay_clearing_floor".into(),
        description: "Relay receipts must respect the clearing price and resource floor".into(),
        severity: InvariantSeverity::High,
        satisfied: receipt.total_usd_micros >= receipt.clearing_price_usd_micros
            && receipt.total_usd_micros >= receipt.resource_floor_usd_micros,
        slashing: if receipt.total_usd_micros >= receipt.clearing_price_usd_micros
            && receipt.total_usd_micros >= receipt.resource_floor_usd_micros
        {
            None
        } else {
            Some(SlashingOutcome {
                reason: "relay floor violation".into(),
                amount: receipt.total_usd_micros,
                target: EscrowEntity::Provider(receipt.provider.clone()),
            })
        },
    });
    invariants
}

fn slash_invariants(
    receipt: &impl ReceiptProvider,
    registry: &ProviderRegistry,
) -> Vec<ReceiptInvariantReport> {
    vec![identity_invariant(
        receipt.provider_id(),
        receipt.slash_amount(),
        registry,
    )]
}

trait ReceiptProvider {
    fn provider_id(&self) -> &str;
    fn slash_amount(&self) -> u64;
}

impl ReceiptProvider for StorageSlashReceipt {
    fn provider_id(&self) -> &str {
        &self.provider
    }
    fn slash_amount(&self) -> u64 {
        self.amount
    }
}

impl ReceiptProvider for ComputeSlashReceipt {
    fn provider_id(&self) -> &str {
        &self.provider
    }
    fn slash_amount(&self) -> u64 {
        self.burned
    }
}

impl ReceiptProvider for EnergySlashReceipt {
    fn provider_id(&self) -> &str {
        &self.provider
    }
    fn slash_amount(&self) -> u64 {
        self.slash_amount
    }
}

fn storage_causality(receipt: &StorageReceipt) -> CausalityEffect {
    CausalityEffect {
        kind: CausalityKind::DirectSettlement,
        amount: receipt.price,
        source: EscrowEntity::Contract(receipt.contract_id.clone()),
        target: EscrowEntity::Provider(receipt.provider.clone()),
        context: "storage settlement".into(),
        block_height: receipt.block_height,
    }
}

fn compute_causality(receipt: &ComputeReceipt) -> CausalityEffect {
    CausalityEffect {
        kind: CausalityKind::DirectSettlement,
        amount: receipt.payment,
        source: EscrowEntity::Job(receipt.job_id.clone()),
        target: EscrowEntity::Provider(receipt.provider.clone()),
        context: "compute settlement".into(),
        block_height: receipt.block_height,
    }
}

fn energy_causality(receipt: &EnergyReceipt) -> CausalityEffect {
    CausalityEffect {
        kind: CausalityKind::DirectSettlement,
        amount: receipt.price,
        source: EscrowEntity::Contract(receipt.contract_id.clone()),
        target: EscrowEntity::Provider(receipt.provider.clone()),
        context: "energy settlement".into(),
        block_height: receipt.block_height,
    }
}

fn ad_causality(receipt: &AdReceipt) -> CausalityEffect {
    CausalityEffect {
        kind: CausalityKind::DirectSettlement,
        amount: receipt.spend,
        source: EscrowEntity::Campaign(receipt.campaign_id.clone()),
        target: EscrowEntity::Publisher(receipt.publisher.clone()),
        context: "ad settlement".into(),
        block_height: receipt.block_height,
    }
}

fn relay_causality(receipt: &RelayReceipt) -> CausalityEffect {
    CausalityEffect {
        kind: CausalityKind::DirectSettlement,
        amount: receipt.total_usd_micros,
        source: EscrowEntity::Job(receipt.job_id.clone()),
        target: EscrowEntity::Provider(receipt.provider.clone()),
        context: "relay settlement".into(),
        block_height: receipt.block_height,
    }
}

fn slash_causality(
    provider: String,
    amount: u64,
    block_height: u64,
    context: impl Into<String>,
) -> CausalityEffect {
    let context_string = context.into();
    CausalityEffect {
        kind: CausalityKind::Slash,
        amount,
        source: EscrowEntity::Provider(provider.clone()),
        target: EscrowEntity::Treasury,
        context: context_string,
        block_height,
    }
}

fn identity_invariant(
    provider_id: &str,
    amount: u64,
    registry: &ProviderRegistry,
) -> ReceiptInvariantReport {
    let record = registry.get_provider_record(provider_id);
    let satisfied = record
        .map(|r| match &r.registration_source {
            ProviderRegistrationSource::StakeLinked { .. } => true,
            _ => false,
        })
        .unwrap_or(false);
    ReceiptInvariantReport {
        name: "stake_linked_identity".into(),
        description: "Service providers must remain stake linked across rotations".into(),
        severity: InvariantSeverity::Critical,
        satisfied,
        slashing: if satisfied {
            None
        } else {
            Some(SlashingOutcome {
                reason: "provider not stake-linked".into(),
                amount,
                target: EscrowEntity::Provider(provider_id.to_string()),
            })
        },
    }
}

fn provider_identity_summary(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> Option<ProviderIdentitySummary> {
    registry.get_provider_record(provider_id).map(|record| {
        let stake_reference = match &record.registration_source {
            ProviderRegistrationSource::StakeLinked { stake_id } => Some(stake_id.clone()),
            _ => None,
        };
        let key_history = record
            .key_versions
            .iter()
            .map(|version| ProviderKeyHistory {
                key: version.verifying_key.to_bytes(),
                registered_at_block: version.registered_at_block,
                retired_at_block: version.retired_at_block,
                evidence: version.evidence.clone(),
            })
            .collect();
        let latest_key = record
            .key_versions
            .last()
            .map(|version| version.verifying_key.to_bytes());
        ProviderIdentitySummary {
            provider_id: record.provider_id.clone(),
            stake_reference,
            rotation_count: record.key_versions.len(),
            latest_key,
            key_history,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt_crypto::ProviderRegistry;
    use crypto_suite::signatures::ed25519::SigningKey;
    use rand::{rngs::StdRng, SeedableRng};

    fn seeded_registry(provider: &str) -> ProviderRegistry {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();
        let mut registry = ProviderRegistry::new();
        registry
            .register_provider_with_source(
                provider.to_string(),
                vk,
                0,
                None,
                None,
                ProviderRegistrationSource::StakeLinked {
                    stake_id: format!("stake-{provider}"),
                },
            )
            .unwrap();
        registry
    }

    #[test]
    fn storage_audit_queries_reference_contract() {
        let receipt = StorageReceipt {
            contract_id: "contract-42".into(),
            provider: "stor-1".into(),
            bytes: 2048,
            price: 100,
            block_height: 123,
            provider_escrow: 200,
            region: Some("us-west".into()),
            chunk_hash: Some([1u8; 32]),
            provider_signature: vec![0u8; 64],
            signature_nonce: 1,
        };
        let queries = Receipt::Storage(receipt.clone()).audit_queries();
        assert_eq!(queries.len(), 1);
        let query = &queries[0];
        assert_eq!(query.market, "storage");
        assert_eq!(query.amount, receipt.price);
        assert!(query
            .details
            .iter()
            .any(|detail| detail.key == "bytes" && detail.value == "2048"));
    }

    #[test]
    fn storage_invariant_slashes_when_escrow_short() {
        let receipt = StorageReceipt {
            contract_id: "contract-escrow".into(),
            provider: "stor-escrow".into(),
            bytes: 1024,
            price: 500,
            block_height: 100,
            provider_escrow: 100,
            region: None,
            chunk_hash: Some([0u8; 32]),
            provider_signature: vec![0u8; 64],
            signature_nonce: 2,
        };
        let registry = seeded_registry("stor-escrow");
        let invariants = Receipt::Storage(receipt.clone()).invariants(&registry);
        let escrow_invariant = invariants
            .iter()
            .find(|inv| inv.name == "storage_escrow_coverage")
            .expect("escrow coverage invariant missing");
        assert!(!escrow_invariant.satisfied);
        assert_eq!(
            escrow_invariant.slashing.as_ref().map(|slash| slash.amount),
            Some(receipt.price)
        );
    }

    #[test]
    fn identity_summary_refs_stake_linked_provider() {
        let registry = seeded_registry("rotating");
        let summary = Receipt::Storage(StorageReceipt {
            contract_id: "c".into(),
            provider: "rotating".into(),
            bytes: 1,
            price: 1,
            block_height: 1,
            provider_escrow: 1,
            region: None,
            chunk_hash: Some([0; 32]),
            provider_signature: vec![0; 64],
            signature_nonce: 1,
        })
        .identity(&registry)
        .expect("identity present");
        assert_eq!(summary.stake_reference, Some("stake-rotating".into()));
        assert!(summary.rotation_count >= 1);
    }
}
