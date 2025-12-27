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

use foundation_serialization::{Deserialize, Serialize};

/// Market settlement receipt variants.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Receipt {
    Storage(StorageReceipt),
    Compute(ComputeReceipt),
    Energy(EnergyReceipt),
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

/// Ad market settlement receipt.
///
/// Records when ad campaigns settle, capturing impressions served, spend,
/// and conversion events.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReceipt {
    /// Campaign ID
    pub campaign_id: String,
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
    /// Publisher Ed25519 signature over receipt fields (prevents forgery)
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub publisher_signature: Vec<u8>,
    /// Nonce to prevent replay attacks
    pub signature_nonce: u64,
}

impl Receipt {
    /// Get the market domain name for telemetry labeling.
    pub fn market_name(&self) -> &'static str {
        match self {
            Receipt::Storage(_) => "storage",
            Receipt::Compute(_) => "compute",
            Receipt::Energy(_) => "energy",
            Receipt::Ad(_) => "ad",
        }
    }

    /// Get the settlement amount in BLOCK tokens.
    pub fn settlement_amount(&self) -> u64 {
        match self {
            Receipt::Storage(r) => r.price,
            Receipt::Compute(r) => r.payment,
            Receipt::Energy(r) => r.price,
            Receipt::Ad(r) => r.spend,
        }
    }

    /// Get the block height this receipt was recorded at.
    pub fn block_height(&self) -> u64 {
        match self {
            Receipt::Storage(r) => r.block_height,
            Receipt::Compute(r) => r.block_height,
            Receipt::Energy(r) => r.block_height,
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
    fn ad_receipt_serializes() {
        let receipt = Receipt::Ad(AdReceipt {
            campaign_id: "camp_101".into(),
            publisher: "pub_1".into(),
            impressions: 10000,
            spend: 100,
            block_height: 103,
            conversions: 50,
            publisher_signature: vec![0u8; 64],
            signature_nonce: 0,
        });

        assert_eq!(receipt.market_name(), "ad");
        assert_eq!(receipt.settlement_amount(), 100);
        assert_eq!(receipt.block_height(), 103);
    }
}
