//! Market settlement receipts for deterministic economics derivation.
//!
//! These receipts are embedded in blocks to allow deterministic replay of
//! market metrics without relying on live market state. Each market domain
//! (storage, compute, energy, ad) produces settlement receipts that are
//! collected at epoch boundaries for metrics derivation.
//!
//! # Determinism
//! Receipts are cryptographic commitments to actual market activity on-chain.
//! Two nodes that see the same blocks will see the same receipts and compute
//! identical market metrics deterministically.

use foundation_serialization::binary;
use foundation_serialization::{Deserialize, Serialize};

/// Market settlement receipt variants.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Receipt {
    Storage(StorageReceipt),
    Compute(ComputeReceipt),
    ComputeSlash(ComputeSlashReceipt),
    Energy(EnergyReceipt),
    EnergySlash(EnergySlashReceipt),
    Ad(AdReceipt),
}

/// Storage market settlement receipt.
///
/// Records when a storage contract settles, capturing the bytes contracted,
/// price paid, and provider escrow state at settlement time.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct StorageReceipt {
    /// Storage contract ID
    pub contract_id: String,
    /// Provider address
    pub provider: String,
    /// Bytes contracted in this settlement
    pub bytes: u64,
    /// Price paid to provider
    pub price: u64,
    /// Settlement block height
    pub block_height: u64,
    /// Provider's total escrow balance at settlement
    pub provider_escrow: u64,
    /// Provider Ed25519 signature over receipt fields (prevents forgery)
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub provider_signature: Vec<u8>,
    /// Nonce to prevent replay attacks
    pub signature_nonce: u64,
}

/// Compute market settlement receipt.
///
/// Records when a compute job settles, capturing units consumed, payment,
/// and SNARK verification success.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeReceipt {
    /// Job ID
    pub job_id: String,
    /// Provider address
    pub provider: String,
    /// Compute units consumed
    pub compute_units: u64,
    /// Payment to provider
    pub payment: u64,
    /// Settlement block height
    pub block_height: u64,
    /// SNARK verification success
    pub verified: bool,
    /// Provider Ed25519 signature over receipt fields (prevents forgery)
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub provider_signature: Vec<u8>,
    /// Nonce to prevent replay attacks
    pub signature_nonce: u64,
}

/// Energy market settlement receipt.
///
/// Records when energy is settled on-chain, capturing units delivered,
/// price paid, and grid verification proof.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyReceipt {
    /// Energy contract ID
    pub contract_id: String,
    /// Provider address (grid operator)
    pub provider: String,
    /// Energy units delivered (kWh * 1000 for fixed-point)
    pub energy_units: u64,
    /// Price paid
    pub price: u64,
    /// Settlement block height
    pub block_height: u64,
    /// Grid verification proof hash
    pub proof_hash: [u8; 32],
    /// Provider Ed25519 signature over receipt fields (prevents forgery)
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub provider_signature: Vec<u8>,
    /// Nonce to prevent replay attacks
    pub signature_nonce: u64,
}

/// Energy market slashing receipt capturing invalid readings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergySlashReceipt {
    /// Provider address (grid operator)
    pub provider: String,
    /// Meter reading hash that triggered the slash
    pub meter_hash: [u8; 32],
    /// Slashed amount in BLOCK
    pub slash_amount: u64,
    /// Reason for the slash (quorum/expiry/conflict)
    pub reason: String,
    /// Block height when the slash was recorded
    pub block_height: u64,
}

/// Compute SLA slash receipt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeSlashReceipt {
    /// Job identifier for the SLA violation.
    pub job_id: String,
    /// Provider address that was slashed.
    pub provider: String,
    /// Buyer account associated with the job.
    pub buyer: String,
    /// Burned amount in BLOCK.
    pub burned: u64,
    /// Reason for the slash (deadline_missed, provider_fault, etc.).
    pub reason: String,
    /// SLA deadline for the job.
    pub deadline: u64,
    /// Timestamp when the SLA was resolved (seconds since epoch).
    pub resolved_at: u64,
    /// Block height when the slash receipt is included.
    pub block_height: u64,
}

/// Ad market settlement receipt.
///
/// Records when ad campaigns settle, capturing impressions served, spend,
/// and conversion events.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReceipt {
    /// Campaign ID
    pub campaign_id: String,
    /// Creative ID
    #[serde(default = "foundation_serialization::defaults::default")]
    pub creative_id: String,
    /// Publisher address
    pub publisher: String,
    /// Impressions delivered
    pub impressions: u64,
    /// Ad spend
    pub spend: u64,
    /// Settlement block height
    pub block_height: u64,
    /// Conversion events recorded
    pub conversions: u32,
    #[serde(default)]
    pub claim_routes: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub role_breakdown: Option<AdRoleBreakdown>,
    #[serde(default)]
    pub device_links: Vec<ad_market::DeviceLinkOptIn>,
    /// Publisher Ed25519 signature over receipt fields (prevents forgery)
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub publisher_signature: Vec<u8>,
    /// Nonce to prevent replay attacks
    pub signature_nonce: u64,
}

/// Optional role-level breakdown for ad receipts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdRoleBreakdown {
    pub viewer: u64,
    pub host: u64,
    pub hardware: u64,
    pub verifier: u64,
    pub liquidity: u64,
    pub miner: u64,
    #[serde(default)]
    pub price_usd_micros: u64,
    #[serde(default)]
    pub clearing_price_usd_micros: u64,
}

impl Receipt {
    /// Get the market domain name for telemetry labeling.
    pub fn market_name(&self) -> &'static str {
        match self {
            Receipt::Storage(_) => "storage",
            Receipt::Compute(_) => "compute",
            Receipt::ComputeSlash(_) => "compute_slash",
            Receipt::Energy(_) => "energy",
            Receipt::EnergySlash(_) => "energy",
            Receipt::Ad(_) => "ad",
        }
    }

    /// Get the settlement amount in BLOCK tokens.
    pub fn settlement_amount(&self) -> u64 {
        match self {
            Receipt::Storage(r) => r.price,
            Receipt::Compute(r) => r.payment,
            Receipt::ComputeSlash(r) => r.burned,
            Receipt::Energy(r) => r.price,
            Receipt::EnergySlash(r) => r.slash_amount,
            Receipt::Ad(r) => r.spend,
        }
    }

    /// Get the block height this receipt was recorded at.
    pub fn block_height(&self) -> u64 {
        match self {
            Receipt::Storage(r) => r.block_height,
            Receipt::Compute(r) => r.block_height,
            Receipt::ComputeSlash(r) => r.block_height,
            Receipt::Energy(r) => r.block_height,
            Receipt::EnergySlash(r) => r.block_height,
            Receipt::Ad(r) => r.block_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_receipt_serializes() {
        let receipt = Receipt::Storage(StorageReceipt {
            contract_id: "sc_123".into(),
            provider: "provider_1".into(),
            bytes: 1000,
            price: 500,
            block_height: 100,
            provider_escrow: 10000,
            provider_signature: vec![0u8; 64],
            signature_nonce: 0,
        });

        assert_eq!(receipt.market_name(), "storage");
        assert_eq!(receipt.settlement_amount(), 500);
        assert_eq!(receipt.block_height(), 100);
    }

    #[test]
    fn compute_receipt_serializes() {
        let receipt = Receipt::Compute(ComputeReceipt {
            job_id: "job_456".into(),
            provider: "provider_2".into(),
            compute_units: 1000,
            payment: 200,
            block_height: 101,
            verified: true,
            provider_signature: vec![0u8; 64],
            signature_nonce: 0,
        });

        assert_eq!(receipt.market_name(), "compute");
        assert_eq!(receipt.settlement_amount(), 200);
        assert_eq!(receipt.block_height(), 101);
    }

    #[test]
    fn energy_receipt_serializes() {
        let receipt = Receipt::Energy(EnergyReceipt {
            contract_id: "ec_789".into(),
            provider: "grid_operator_1".into(),
            energy_units: 5000,
            price: 250,
            block_height: 102,
            proof_hash: [0u8; 32],
            provider_signature: vec![0u8; 64],
            signature_nonce: 0,
        });

        assert_eq!(receipt.market_name(), "energy");
        assert_eq!(receipt.settlement_amount(), 250);
        assert_eq!(receipt.block_height(), 102);
    }

    #[test]
    fn energy_receipt_roundtrip_binary() {
        let original = EnergyReceipt {
            contract_id: "ec_rt".into(),
            provider: "grid_operator_rt".into(),
            energy_units: 2500,
            price: 125,
            block_height: 123,
            proof_hash: [7u8; 32],
            provider_signature: vec![1u8; 64],
            signature_nonce: 42,
        };
        let encoded =
            binary::encode(&original).expect("energy receipt should encode without panicking");
        let decoded =
            binary::decode::<EnergyReceipt>(&encoded).expect("decoding should return the receipt");
        assert_eq!(original, decoded);
    }

    #[test]
    fn energy_slash_receipt_serializes() {
        let receipt = Receipt::EnergySlash(EnergySlashReceipt {
            provider: "grid_operator_1".into(),
            meter_hash: [1u8; 32],
            slash_amount: 75,
            reason: "quorum".into(),
            block_height: 104,
        });

        assert_eq!(receipt.market_name(), "energy");
        assert_eq!(receipt.settlement_amount(), 75);
        assert_eq!(receipt.block_height(), 104);
    }

    #[test]
    fn ad_receipt_serializes() {
        let receipt = Receipt::Ad(AdReceipt {
            campaign_id: "camp_101".into(),
            creative_id: "creative_101".into(),
            publisher: "pub_1".into(),
            impressions: 10000,
            spend: 100,
            block_height: 103,
            conversions: 50,
            claim_routes: std::collections::HashMap::new(),
            role_breakdown: None,
            device_links: Vec::new(),
            publisher_signature: vec![0u8; 64],
            signature_nonce: 0,
        });

        assert_eq!(receipt.market_name(), "ad");
        assert_eq!(receipt.settlement_amount(), 100);
        assert_eq!(receipt.block_height(), 103);
    }
}
