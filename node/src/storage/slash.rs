use concurrency::{mutex, Lazy, MutexExt, MutexT};
use std::sync::Arc;
use storage_market::slashing::{
    Config as StorageSlashingConfig, ReceiptMetadata, RepairKey, RepairReport, SlashingController,
    StorageSlash,
};

use crate::receipts::StorageSlashReceipt;

type StorageSlashHandle = Arc<MutexT<SlashingController>>;

static SLASH_CONTROLLER: Lazy<StorageSlashHandle> = Lazy::new(|| {
    let config = StorageSlashingConfig::default();
    Arc::new(mutex(SlashingController::new(config)))
});

/// Drain slashes that are due for inclusion, returning the raw events.
pub fn drain_slash_events(block_height: u64) -> Vec<StorageSlash> {
    SLASH_CONTROLLER.guard().drain_slashes(block_height)
}

/// Record a storage receipt so the controller can update nonces and repairs.
pub fn record_receipt(metadata: ReceiptMetadata) -> Vec<StorageSlash> {
    SLASH_CONTROLLER.guard().record_receipt(metadata)
}

/// Record a newly discovered missing chunk so the controller can start the deadline.
pub fn report_missing_chunk(report: RepairReport) {
    SLASH_CONTROLLER.guard().report_missing_chunk(report);
}

/// Remove an outstanding repair deadline when the chunk is restored.
pub fn cancel_pending_repair(key: RepairKey) {
    SLASH_CONTROLLER.guard().cancel_repair(&key);
}

/// Convert a slashing event into the ledger receipt format.
pub fn storage_slash_to_receipt(slash: StorageSlash) -> StorageSlashReceipt {
    let (contract_id, chunk_hash, reason) = match slash.reason {
        storage_market::slashing::SlashingReason::MissingRepair {
            contract_id,
            chunk_hash,
        } => (
            Some(contract_id),
            Some(chunk_hash),
            "missing_repair".to_string(),
        ),
        storage_market::slashing::SlashingReason::ReplayedNonce { nonce } => {
            (None, None, format!("replayed_nonce:{}", nonce))
        }
        storage_market::slashing::SlashingReason::RegionDark { region } => {
            (None, None, "region_dark".to_string())
        }
    };
    StorageSlashReceipt {
        provider: slash.provider,
        amount: slash.amount,
        region: slash.region,
        contract_id,
        chunk_hash,
        reason,
        block_height: slash.block_height,
    }
}
