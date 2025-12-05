//! Sled-backed cache for PresenceReceipt entries.
//!
//! This module provides a persistent cache for presence proofs originating from LocalNet
//! and Range Boost receipts. Receipts are keyed by `{beacon_id, bucket_id}` with TTL,
//! radius, and confidence metadata.
//!
//! The cache enforces governance knobs:
//! - `TB_PRESENCE_TTL_SECS`: Maximum age of presence proofs before expiry
//! - `TB_PRESENCE_RADIUS_METERS`: Default radius for bucket aggregation
//! - `TB_PRESENCE_PROOF_CACHE_SIZE`: Maximum cached entries per node

use ad_market::{PresenceBucketRef, PresenceKind};
use crypto_suite::hashing::blake3;
use foundation_serialization::{json, Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const PRESENCE_TREE_NAME: &str = "presence_receipts";

/// A presence receipt representing proof of physical presence at a location.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PresenceReceipt {
    /// Unique receipt identifier (hash of beacon + device + timestamp)
    pub receipt_id: String,
    /// Beacon or venue identifier
    pub beacon_id: String,
    /// Device key (anonymized)
    pub device_key: String,
    /// Mesh node that witnessed the presence
    pub mesh_node: Option<String>,
    /// Location bucket identifier for privacy-preserving aggregation
    pub location_bucket: String,
    /// Radius in meters for the presence proof
    pub radius_meters: u16,
    /// Confidence in basis points (0-10000)
    pub confidence_bps: u16,
    /// When the receipt was minted (Unix timestamp in microseconds)
    pub minted_at_micros: u64,
    /// When the receipt expires (Unix timestamp in microseconds)
    pub expires_at_micros: u64,
    /// Optional venue identifier for venue-grade attestations
    pub venue_id: Option<String>,
    /// Optional crowd size hint for k-anonymity
    pub crowd_size_hint: Option<u64>,
    /// Source of the presence proof
    pub kind: PresenceKind,
    /// Optional badge token for venue-grade attestations
    pub presence_badge: Option<String>,
}

impl PresenceReceipt {
    /// Create a new presence receipt with the given parameters.
    pub fn new(
        beacon_id: String,
        device_key: String,
        location_bucket: String,
        radius_meters: u16,
        confidence_bps: u16,
        ttl_secs: u64,
        kind: PresenceKind,
    ) -> Self {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);

        let mut hasher = blake3::Hasher::new();
        hasher.update(beacon_id.as_bytes());
        hasher.update(device_key.as_bytes());
        hasher.update(&now_micros.to_le_bytes());
        let receipt_id = crypto_suite::hex::encode(&hasher.finalize().as_bytes()[..16]);

        Self {
            receipt_id,
            beacon_id,
            device_key,
            mesh_node: None,
            location_bucket,
            radius_meters,
            confidence_bps,
            minted_at_micros: now_micros,
            expires_at_micros: now_micros + (ttl_secs * 1_000_000),
            venue_id: None,
            crowd_size_hint: None,
            kind,
            presence_badge: None,
        }
    }

    /// Check if the receipt has expired.
    pub fn is_expired(&self) -> bool {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        self.expires_at_micros < now_micros
    }

    /// Convert this receipt to a PresenceBucketRef for use in ad market cohorts.
    pub fn to_bucket_ref(&self) -> PresenceBucketRef {
        PresenceBucketRef {
            bucket_id: self.location_bucket.clone(),
            kind: self.kind.clone(),
            region: None, // Would be derived from beacon_id in production
            radius_meters: self.radius_meters,
            confidence_bps: self.confidence_bps,
            minted_at_micros: Some(self.minted_at_micros),
            expires_at_micros: Some(self.expires_at_micros),
        }
    }

    /// Generate the cache key for this receipt.
    pub fn cache_key(&self) -> String {
        format!("{}:{}", self.beacon_id, self.location_bucket)
    }
}

/// Configuration for the presence cache.
#[derive(Clone, Debug)]
pub struct PresenceCacheConfig {
    /// Maximum number of entries in the cache
    pub max_entries: usize,
    /// Default TTL in seconds for new receipts
    pub default_ttl_secs: u64,
    /// Default radius in meters for new receipts
    pub default_radius_meters: u16,
    /// Minimum confidence in basis points for valid receipts
    pub min_confidence_bps: u16,
    /// Minimum crowd size for venue-grade attestations
    pub min_crowd_size: u64,
}

impl Default for PresenceCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            default_ttl_secs: 86_400, // 24 hours
            default_radius_meters: 500,
            min_confidence_bps: 8_000, // 80%
            min_crowd_size: 5,
        }
    }
}

impl PresenceCacheConfig {
    /// Create a config from governance params.
    pub fn from_governance(
        ttl_secs: i64,
        radius_meters: i64,
        cache_size: i64,
        min_confidence_bps: i64,
        min_crowd_size: i64,
    ) -> Self {
        Self {
            max_entries: cache_size.max(100) as usize,
            default_ttl_secs: ttl_secs.max(60) as u64,
            default_radius_meters: radius_meters.clamp(10, 10_000) as u16,
            min_confidence_bps: min_confidence_bps.clamp(0, 10_000) as u16,
            min_crowd_size: min_crowd_size.max(1) as u64,
        }
    }
}

/// Sled-backed cache for presence receipts.
pub struct PresenceCache {
    tree: sled::Tree,
    config: PresenceCacheConfig,
}

impl PresenceCache {
    /// Open or create a presence cache in the given sled database.
    pub fn open(db: &sled::Db, config: PresenceCacheConfig) -> sled::Result<Self> {
        let tree = db.open_tree(PRESENCE_TREE_NAME)?;
        Ok(Self { tree, config })
    }

    /// Insert a presence receipt into the cache.
    pub fn insert(&self, receipt: &PresenceReceipt) -> sled::Result<()> {
        // Validate confidence
        if receipt.confidence_bps < self.config.min_confidence_bps {
            return Ok(()); // Silently reject low-confidence receipts
        }

        // Check if expired
        if receipt.is_expired() {
            return Ok(()); // Don't insert expired receipts
        }

        let key = receipt.cache_key();
        let value = json::to_vec(receipt)
            .map_err(|e| sled::Error::Io(format!("serialization error: {e}")))?;

        self.tree.insert(key.as_bytes(), value)?;

        // Prune if over capacity
        if self.tree.len() > self.config.max_entries {
            self.prune_expired()?;
        }

        Ok(())
    }

    /// Get a presence receipt by its cache key.
    pub fn get(&self, beacon_id: &str, bucket_id: &str) -> sled::Result<Option<PresenceReceipt>> {
        let key = format!("{beacon_id}:{bucket_id}");
        match self.tree.get(key.as_bytes())? {
            Some(bytes) => {
                let receipt: PresenceReceipt = json::from_slice(&bytes)
                    .map_err(|e| sled::Error::Io(format!("deserialization error: {e}")))?;

                // Check expiry
                if receipt.is_expired() {
                    // Remove expired entry
                    self.tree.remove(key.as_bytes())?;
                    return Ok(None);
                }

                Ok(Some(receipt))
            }
            None => Ok(None),
        }
    }

    /// List all non-expired presence receipts.
    pub fn list(&self) -> sled::Result<Vec<PresenceReceipt>> {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);

        let mut receipts = Vec::new();
        for result in self.tree.iter() {
            let (_, value) = result?;
            if let Ok(receipt) = json::from_slice::<PresenceReceipt>(&value) {
                if receipt.expires_at_micros >= now_micros {
                    receipts.push(receipt);
                }
            }
        }
        Ok(receipts)
    }

    /// List presence receipts matching a filter.
    pub fn list_filtered(
        &self,
        kind: Option<&PresenceKind>,
        min_confidence_bps: Option<u16>,
        max_radius_meters: Option<u16>,
    ) -> sled::Result<Vec<PresenceReceipt>> {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);

        let mut receipts = Vec::new();
        for result in self.tree.iter() {
            let (_, value) = result?;
            if let Ok(receipt) = json::from_slice::<PresenceReceipt>(&value) {
                // Check expiry
                if receipt.expires_at_micros < now_micros {
                    continue;
                }

                // Apply filters
                if let Some(k) = kind {
                    if &receipt.kind != k {
                        continue;
                    }
                }
                if let Some(min_conf) = min_confidence_bps {
                    if receipt.confidence_bps < min_conf {
                        continue;
                    }
                }
                if let Some(max_rad) = max_radius_meters {
                    if receipt.radius_meters > max_rad {
                        continue;
                    }
                }

                receipts.push(receipt);
            }
        }
        Ok(receipts)
    }

    /// Remove a presence receipt by its cache key.
    pub fn remove(&self, beacon_id: &str, bucket_id: &str) -> sled::Result<bool> {
        let key = format!("{beacon_id}:{bucket_id}");
        Ok(self.tree.remove(key.as_bytes())?.is_some())
    }

    /// Remove all expired entries from the cache.
    pub fn prune_expired(&self) -> sled::Result<usize> {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);

        let mut to_remove = Vec::new();
        for result in self.tree.iter() {
            let (key, value) = result?;
            if let Ok(receipt) = json::from_slice::<PresenceReceipt>(&value) {
                if receipt.expires_at_micros < now_micros {
                    to_remove.push(key);
                }
            }
        }

        let count = to_remove.len();
        for key in to_remove {
            self.tree.remove(key)?;
        }

        Ok(count)
    }

    /// Get the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.tree.len() == 0
    }

    /// Clear all entries from the cache.
    pub fn clear(&self) -> sled::Result<()> {
        self.tree.clear()?;
        Ok(())
    }

    /// Get freshness histogram for analytics.
    /// Returns counts in buckets: <1h, 1-6h, 6-24h, >24h
    pub fn freshness_histogram(&self) -> sled::Result<FreshnessHistogram> {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);

        let one_hour = 3_600_000_000u64;
        let six_hours = 6 * one_hour;
        let twenty_four_hours = 24 * one_hour;

        let mut histogram = FreshnessHistogram::default();
        let mut total = 0u64;

        for result in self.tree.iter() {
            let (_, value) = result?;
            if let Ok(receipt) = json::from_slice::<PresenceReceipt>(&value) {
                if receipt.expires_at_micros < now_micros {
                    continue; // Skip expired
                }

                let age = now_micros.saturating_sub(receipt.minted_at_micros);
                total += 1;

                if age < one_hour {
                    histogram.under_1h += 1;
                } else if age < six_hours {
                    histogram.hours_1_to_6 += 1;
                } else if age < twenty_four_hours {
                    histogram.hours_6_to_24 += 1;
                } else {
                    histogram.over_24h += 1;
                }
            }
        }

        // Convert to ppm
        if total > 0 {
            histogram.under_1h_ppm = ((histogram.under_1h as u64 * 1_000_000) / total) as u32;
            histogram.hours_1_to_6_ppm =
                ((histogram.hours_1_to_6 as u64 * 1_000_000) / total) as u32;
            histogram.hours_6_to_24_ppm =
                ((histogram.hours_6_to_24 as u64 * 1_000_000) / total) as u32;
            histogram.over_24h_ppm = ((histogram.over_24h as u64 * 1_000_000) / total) as u32;
        }

        Ok(histogram)
    }
}

/// Freshness histogram for presence receipts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct FreshnessHistogram {
    pub under_1h: u32,
    pub hours_1_to_6: u32,
    pub hours_6_to_24: u32,
    pub over_24h: u32,
    pub under_1h_ppm: u32,
    pub hours_1_to_6_ppm: u32,
    pub hours_6_to_24_ppm: u32,
    pub over_24h_ppm: u32,
}

/// Handle for managing presence receipts across the node.
#[derive(Clone)]
pub struct PresenceHandle {
    cache: Arc<PresenceCache>,
}

impl PresenceHandle {
    /// Create a new presence handle with the given cache.
    pub fn new(cache: Arc<PresenceCache>) -> Self {
        Self { cache }
    }

    /// Insert a presence receipt.
    pub fn insert(&self, receipt: &PresenceReceipt) -> sled::Result<()> {
        self.cache.insert(receipt)
    }

    /// Get a presence receipt by beacon and bucket ID.
    pub fn get(&self, beacon_id: &str, bucket_id: &str) -> sled::Result<Option<PresenceReceipt>> {
        self.cache.get(beacon_id, bucket_id)
    }

    /// List all valid presence receipts.
    pub fn list(&self) -> sled::Result<Vec<PresenceReceipt>> {
        self.cache.list()
    }

    /// List presence receipts as bucket refs for ad market integration.
    pub fn list_bucket_refs(&self) -> sled::Result<Vec<PresenceBucketRef>> {
        let receipts = self.cache.list()?;
        Ok(receipts.iter().map(|r| r.to_bucket_ref()).collect())
    }

    /// Remove a presence receipt.
    pub fn remove(&self, beacon_id: &str, bucket_id: &str) -> sled::Result<bool> {
        self.cache.remove(beacon_id, bucket_id)
    }

    /// Prune expired entries.
    pub fn prune_expired(&self) -> sled::Result<usize> {
        self.cache.prune_expired()
    }

    /// Get freshness histogram.
    pub fn freshness_histogram(&self) -> sled::Result<FreshnessHistogram> {
        self.cache.freshness_histogram()
    }

    /// Get cache size.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile;

    #[test]
    fn test_presence_receipt_creation() {
        let receipt = PresenceReceipt::new(
            "beacon-123".into(),
            "device-456".into(),
            "bucket-789".into(),
            500,
            9000,
            3600,
            PresenceKind::LocalNet,
        );

        assert!(!receipt.receipt_id.is_empty());
        assert_eq!(receipt.beacon_id, "beacon-123");
        assert_eq!(receipt.confidence_bps, 9000);
        assert!(!receipt.is_expired());
    }

    #[test]
    #[ignore] // Serialization requires runtime serde feature
    fn test_presence_cache_basic() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let config = PresenceCacheConfig::default();
        let cache = PresenceCache::open(&db, config).unwrap();

        let receipt = PresenceReceipt::new(
            "beacon-123".into(),
            "device-456".into(),
            "bucket-789".into(),
            500,
            9000,
            3600,
            PresenceKind::LocalNet,
        );

        cache.insert(&receipt).unwrap();
        assert_eq!(cache.len(), 1);

        let retrieved = cache.get("beacon-123", "bucket-789").unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().receipt_id, receipt.receipt_id);
    }

    #[test]
    #[ignore] // Serialization requires runtime serde feature
    fn test_presence_cache_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let config = PresenceCacheConfig::default();
        let cache = PresenceCache::open(&db, config).unwrap();

        // Create an already-expired receipt
        let mut receipt = PresenceReceipt::new(
            "beacon-123".into(),
            "device-456".into(),
            "bucket-789".into(),
            500,
            9000,
            0, // 0 TTL
            PresenceKind::LocalNet,
        );
        receipt.expires_at_micros = 1; // Already expired

        cache.insert(&receipt).unwrap();
        assert_eq!(cache.len(), 0); // Should not be inserted
    }

    #[test]
    fn test_bucket_ref_conversion() {
        let receipt = PresenceReceipt::new(
            "beacon-123".into(),
            "device-456".into(),
            "bucket-789".into(),
            500,
            9000,
            3600,
            PresenceKind::RangeBoost,
        );

        let bucket_ref = receipt.to_bucket_ref();
        assert_eq!(bucket_ref.bucket_id, "bucket-789");
        assert_eq!(bucket_ref.kind, PresenceKind::RangeBoost);
        assert_eq!(bucket_ref.radius_meters, 500);
        assert_eq!(bucket_ref.confidence_bps, 9000);
    }
}
