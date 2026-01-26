use std::collections::HashMap;

/// Reasons for storing a slash event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlashingReason {
    MissingRepair {
        contract_id: String,
        chunk_hash: [u8; 32],
    },
    ReplayedNonce {
        nonce: u64,
    },
    RegionDark {
        region: String,
    },
}

/// A summary of a slash issued by the storage market.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageSlash {
    pub provider: String,
    pub amount: u64,
    pub region: Option<String>,
    pub reason: SlashingReason,
    pub block_height: u64,
}

/// Configuration knobs exposed by the storage slashing controller.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// How many blocks providers have to re-upload a missing chunk.
    pub repair_window: u64,
    /// How many blocks without receipts before a region is considered dark.
    pub dark_threshold: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repair_window: 10,
            dark_threshold: 15,
        }
    }
}

/// Metadata emitted by storage receipts so slashing rules can observe them.
#[derive(Clone, Debug)]
pub struct ReceiptMetadata {
    pub provider: String,
    pub signature_nonce: u64,
    pub block_height: u64,
    pub contract_id: String,
    pub region: Option<String>,
    pub chunk_hash: Option<[u8; 32]>,
}

/// Unique key for a chunk that must be repaired.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepairKey {
    pub contract_id: String,
    pub provider: String,
    pub chunk_hash: [u8; 32],
}

/// A pending repair that must be resolved before the deadline.
#[derive(Clone, Debug)]
struct RepairRecord {
    due_block: u64,
    amount: u64,
    region: Option<String>,
}

/// Report that a chunk is missing and must be repaired.
#[derive(Clone, Debug)]
pub struct RepairReport {
    pub key: RepairKey,
    pub block_height: u64,
    pub missing_bytes: u64,
    pub provider_escrow: u64,
    pub rent_per_byte: u64,
    pub region: Option<String>,
}

/// Status information for a region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionStatus {
    pub last_seen_block: u64,
    pub dark_since: Option<u64>,
}

impl RegionStatus {
    pub fn is_dark(&self) -> bool {
        self.dark_since.is_some()
    }
}

/// Controller that wires auditor discoveries, receipts, and region indicators
/// into a deterministic slashing history.
pub struct SlashingController {
    config: Config,
    seen_nonces: HashMap<(String, u64), u64>,
    pending_repairs: HashMap<RepairKey, RepairRecord>,
    region_status: HashMap<String, RegionStatus>,
    provider_reputation: HashMap<String, i64>,
}

impl SlashingController {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            seen_nonces: HashMap::new(),
            pending_repairs: HashMap::new(),
            region_status: HashMap::new(),
            provider_reputation: HashMap::new(),
        }
    }

    /// Record an incoming storage receipt so the controller can track nonces,
    /// repairs, and regional liveness.
    pub fn record_receipt(&mut self, metadata: ReceiptMetadata) -> Vec<StorageSlash> {
        let mut slashes = Vec::new();

        if let Some(region) = metadata.region.as_deref() {
            let status = self
                .region_status
                .entry(region.to_string())
                .or_insert(RegionStatus {
                    last_seen_block: metadata.block_height,
                    dark_since: None,
                });
            status.last_seen_block = metadata.block_height;
            status.dark_since = None;
        }

        let key = (metadata.provider.clone(), metadata.signature_nonce);
        match self.seen_nonces.get(&key).copied() {
            Some(existing_block) if existing_block == metadata.block_height => {
                // Already processed this receipt for the same block height.
            }
            Some(_) => {
                slashes.push(StorageSlash {
                    provider: metadata.provider.clone(),
                    amount: 0,
                    region: metadata.region.clone(),
                    reason: SlashingReason::ReplayedNonce {
                        nonce: metadata.signature_nonce,
                    },
                    block_height: metadata.block_height,
                });
            }
            None => {
                self.seen_nonces.insert(key.clone(), metadata.block_height);
            }
        }

        if let Some(chunk_hash) = metadata.chunk_hash {
            let repair_key = RepairKey {
                contract_id: metadata.contract_id.clone(),
                provider: metadata.provider.clone(),
                chunk_hash,
            };
            self.pending_repairs.remove(&repair_key);
        }

        slashes
    }

    /// Schedule a repair after an audit report or repair scheduler discovery.
    pub fn report_missing_chunk(&mut self, report: RepairReport) {
        let due_block = report
            .block_height
            .saturating_add(self.config.repair_window);
        let amount = report
            .provider_escrow
            .max(report.rent_per_byte.saturating_mul(report.missing_bytes));
        let key = report.key.clone();
        self.pending_repairs.insert(
            key.clone(),
            RepairRecord {
                due_block,
                amount,
                region: report.region,
            },
        );
    }

    /// Emit slashes for any overdue repair deadlines.
    pub fn resolve_overdue(&mut self, current_block: u64) -> Vec<StorageSlash> {
        let mut slashes = Vec::new();
        let expired_records: Vec<(RepairKey, RepairRecord)> = self
            .pending_repairs
            .iter()
            .filter(|(_, record)| record.due_block <= current_block)
            .map(|(key, record)| (key.clone(), record.clone()))
            .collect();

        for (key, record) in expired_records {
            self.apply_slash(&key.provider, record.amount);
            slashes.push(StorageSlash {
                provider: key.provider.clone(),
                amount: record.amount,
                region: record.region.clone(),
                reason: SlashingReason::MissingRepair {
                    contract_id: key.contract_id.clone(),
                    chunk_hash: key.chunk_hash,
                },
                block_height: current_block,
            });
            self.pending_repairs.remove(&key);
        }

        slashes
    }

    /// Mark regions as dark when they miss `dark_threshold` blocks with receipts.
    pub fn check_dark_regions(&mut self, current_block: u64) -> Vec<String> {
        let mut darkened = Vec::new();
        for (region, status) in self.region_status.iter_mut() {
            if status.last_seen_block + self.config.dark_threshold <= current_block {
                if status.dark_since.is_none() {
                    status.dark_since = Some(current_block);
                    darkened.push(region.clone());
                }
            }
        }
        darkened
    }

    /// Drain all pending slash events (repairs + dark regions) for inclusion.
    pub fn drain_slashes(&mut self, current_block: u64) -> Vec<StorageSlash> {
        let mut slashes = self.resolve_overdue(current_block);
        let dark_regions = self.check_dark_regions(current_block);
        for region in dark_regions {
            slashes.push(StorageSlash {
                provider: format!("region:{}", region),
                amount: 0,
                region: Some(region.clone()),
                reason: SlashingReason::RegionDark { region },
                block_height: current_block,
            });
        }
        slashes
    }

    /// Query the status for a region.
    pub fn region_status(&self, region: &str) -> Option<&RegionStatus> {
        self.region_status.get(region)
    }

    /// Query the current reputation for a provider.
    pub fn reputation(&self, provider: &str) -> i64 {
        *self.provider_reputation.get(provider).unwrap_or(&1_000)
    }

    pub fn cancel_repair(&mut self, key: &RepairKey) {
        self.pending_repairs.remove(key);
    }

    fn apply_slash(&mut self, provider: &str, amount: u64) {
        if amount == 0 {
            return;
        }
        let entry = self
            .provider_reputation
            .entry(provider.to_string())
            .or_insert(1_000);
        *entry = entry.saturating_sub(amount as i64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let controller = SlashingController::new(Config::default());
        assert_eq!(controller.config.repair_window, 10);
        assert_eq!(controller.config.dark_threshold, 15);
    }
}
