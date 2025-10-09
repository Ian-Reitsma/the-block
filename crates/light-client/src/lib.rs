#![cfg_attr(
    not(any(feature = "android-probe", feature = "ios-probe")),
    forbid(unsafe_code)
)]

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::time::{Duration, UNIX_EPOCH};

use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use flate2::{write::GzEncoder, Compression};
use foundation_serialization::{Deserialize, Serialize};

mod config;
mod device;
pub mod proof_tracker;
mod state_stream;
pub use config::{config_path, load_user_config, save_user_config, LightClientConfig};
pub use device::{
    default_probe, DeviceFallback, DeviceStatus, DeviceStatusFreshness, DeviceStatusProbe,
    DeviceStatusSnapshot, DeviceStatusWatcher, DynDeviceStatusProbe, ProbeError,
};
#[cfg(feature = "telemetry")]
pub use device::{DEVICE_TELEMETRY_REGISTRY, LIGHT_CLIENT_DEVICE_STATUS};
pub use state_stream::{
    account_state_value, AccountChunk, GapCallback, StateChunk, StateStream, StateStreamBuilder,
    StateStreamError,
};
#[cfg(feature = "telemetry")]
pub use state_stream::{
    LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES, LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES,
    LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL, STATE_STREAM_TELEMETRY_REGISTRY,
};

/// Options controlling background synchronization.
#[derive(Clone, Copy, Debug)]
pub struct SyncOptions {
    pub wifi_only: bool,
    pub require_charging: bool,
    pub min_battery: f32,
    pub batch_size: usize,
    pub poll_interval: Duration,
    pub stale_after: Duration,
    pub fallback: DeviceFallback,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            wifi_only: true,
            require_charging: true,
            min_battery: 0.5,
            batch_size: 32,
            poll_interval: Duration::from_secs(1),
            stale_after: Duration::from_secs(30),
            fallback: DeviceFallback::default(),
        }
    }
}

impl SyncOptions {
    fn normalized(mut self) -> Self {
        if !self.min_battery.is_finite() {
            self.min_battery = 0.0;
        }
        self.min_battery = self.min_battery.clamp(0.0, 1.0);
        if self.batch_size == 0 {
            self.batch_size = 1;
        }
        self
    }

    pub fn apply_config(mut self, cfg: &LightClientConfig) -> Self {
        if cfg.ignore_charging_requirement {
            self.require_charging = false;
        }
        if let Some(wifi) = cfg.wifi_only_override {
            self.wifi_only = wifi;
        }
        if let Some(min) = cfg.min_battery_override {
            self.min_battery = min;
        }
        if let Some(fallback) = cfg.fallback_override {
            self.fallback = fallback;
        }
        self.normalized()
    }

    pub fn gating_reason(&self, status: &DeviceStatus) -> Option<GatingReason> {
        if self.wifi_only && !status.on_wifi {
            return Some(GatingReason::WifiUnavailable);
        }
        if self.require_charging && !status.is_charging {
            return Some(GatingReason::RequiresCharging);
        }
        if status.battery_level < self.min_battery {
            return Some(GatingReason::BatteryTooLow);
        }
        None
    }
}

/// Block header for light-client verification.
#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Header {
    pub height: u64,
    pub prev_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub checkpoint_hash: [u8; 32],
    /// Optional validator verifying key for PoS checkpoints.
    pub validator_key: Option<[u8; 32]>,
    /// Signature over `checkpoint_hash` when validator key is present.
    pub checkpoint_sig: Option<Vec<u8>>,
    pub nonce: u64,
    pub difficulty: u64,
    pub timestamp_millis: u64,
    pub l2_roots: Vec<[u8; 32]>,
    pub l2_sizes: Vec<u32>,
    pub vdf_commit: [u8; 32],
    pub vdf_output: [u8; 32],
    pub vdf_proof: Vec<u8>,
}

impl Header {
    pub fn hash(&self) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.prev_hash);
        h.update(&self.merkle_root);
        h.update(&self.checkpoint_hash);
        h.update(&self.nonce.to_le_bytes());
        h.update(&self.timestamp_millis.to_le_bytes());
        h.update(&(self.l2_roots.len() as u32).to_le_bytes());
        for r in &self.l2_roots {
            h.update(r);
        }
        h.update(&(self.l2_sizes.len() as u32).to_le_bytes());
        for s in &self.l2_sizes {
            h.update(&s.to_le_bytes());
        }
        h.update(&self.vdf_commit);
        h.update(&self.vdf_output);
        h.update(&(self.vdf_proof.len() as u32).to_le_bytes());
        h.update(&self.vdf_proof);
        h.finalize().into()
    }
}

/// Light client maintaining a header chain and trusted checkpoints.
pub struct LightClient {
    pub chain: Vec<Header>,
    checkpoints: HashMap<u64, [u8; 32]>,
}

impl LightClient {
    pub fn new(genesis: Header) -> Self {
        Self {
            chain: vec![genesis],
            checkpoints: HashMap::new(),
        }
    }

    pub fn add_checkpoint(&mut self, height: u64, hash: [u8; 32]) {
        self.checkpoints.insert(height, hash);
    }

    pub fn tip_height(&self) -> u64 {
        self.chain.last().map(|h| h.height).unwrap_or(0)
    }

    pub fn verify_and_append(&mut self, h: Header) -> Result<(), ()> {
        let last = self.chain.last().ok_or(())?;
        if !verify_pow(last, &h) {
            return Err(());
        }
        if !verify_checkpoint(&h, &self.checkpoints) {
            return Err(());
        }
        self.chain.push(h);
        Ok(())
    }
}

/// Verify PoW linkage and difficulty between two headers.
pub fn verify_pow(prev: &Header, h: &Header) -> bool {
    if h.prev_hash != prev.hash() {
        return false;
    }
    let hash = h.hash();
    let value = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
    let target = u64::MAX / h.difficulty.max(1);
    value <= target
}

/// Verify PoS checkpoints either via trusted hash or validator signature.
pub fn verify_checkpoint(h: &Header, checkpoints: &HashMap<u64, [u8; 32]>) -> bool {
    if let (Some(pk_bytes), Some(sig_bytes)) = (h.validator_key, h.checkpoint_sig.as_ref()) {
        if sig_bytes.len() != 64 {
            return false;
        }
        let vk = match VerifyingKey::from_bytes(&pk_bytes) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut arr = [0u8; 64];
        arr.copy_from_slice(sig_bytes);
        let sig = Signature::from_bytes(&arr);
        vk.verify_strict(&h.checkpoint_hash, &sig).is_ok()
    } else if let Some(expected) = checkpoints.get(&h.height) {
        expected == &h.checkpoint_hash
    } else {
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum GatingReason {
    WifiUnavailable,
    RequiresCharging,
    BatteryTooLow,
}

impl GatingReason {
    pub fn as_str(self) -> &'static str {
        match self {
            GatingReason::WifiUnavailable => "wifi_unavailable",
            GatingReason::RequiresCharging => "requires_charging",
            GatingReason::BatteryTooLow => "battery_too_low",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SyncOutcome {
    pub appended: usize,
    pub status: DeviceStatusSnapshot,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub gating: Option<GatingReason>,
}

/// Attempt a background delta sync using the provided fetcher.
pub async fn sync_background<F, Fut>(
    client: &mut LightClient,
    opts: SyncOptions,
    fetch: F,
) -> Result<SyncOutcome, ProbeError>
where
    F: FnMut(u64, usize) -> Fut,
    Fut: Future<Output = Vec<Header>>,
{
    let opts = opts.normalized();
    let probe = default_probe()?;
    let watcher = DeviceStatusWatcher::new(probe, opts.fallback, opts.stale_after);
    Ok(sync_background_with_probe(client, opts, &watcher, fetch).await)
}

/// Attempt a background sync using an explicit device status watcher.
pub async fn sync_background_with_probe<F, Fut>(
    client: &mut LightClient,
    opts: SyncOptions,
    watcher: &DeviceStatusWatcher,
    mut fetch: F,
) -> SyncOutcome
where
    F: FnMut(u64, usize) -> Fut,
    Fut: Future<Output = Vec<Header>>,
{
    let opts = opts.normalized();
    let mut appended = 0usize;
    loop {
        let snapshot = watcher.poll().await;
        if let Some(reason) = opts.gating_reason(&snapshot.status) {
            return SyncOutcome {
                appended,
                status: snapshot,
                gating: Some(reason),
            };
        }
        let start = client.tip_height() + 1;
        let batch = fetch(start, opts.batch_size).await;
        if batch.is_empty() {
            return SyncOutcome {
                appended,
                status: snapshot,
                gating: None,
            };
        }
        for header in batch.into_iter().take(opts.batch_size) {
            if client.verify_and_append(header).is_ok() {
                appended += 1;
            }
        }
        if opts.poll_interval > Duration::from_secs(0) {
            runtime::sleep(opts.poll_interval).await;
        } else {
            runtime::yield_now().await;
        }
    }
}

/// Compress log data for upload via telemetry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AnnotatedLogBundle {
    pub compression: &'static str,
    pub payload: Vec<u8>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub device_status: Option<DeviceStatusEnvelope>,
}

impl AnnotatedLogBundle {
    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DeviceStatusEnvelope {
    pub on_wifi: bool,
    pub is_charging: bool,
    pub battery_level: f32,
    pub freshness: DeviceStatusFreshness,
    pub observed_at_millis: u128,
    pub stale_for_millis: u64,
}

/// Compress log data for upload via telemetry.
pub fn upload_compressed_logs(
    data: &[u8],
    status: Option<&DeviceStatusSnapshot>,
) -> AnnotatedLogBundle {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    let payload = enc.finish().unwrap_or_default();
    let device_status = status.map(|snapshot| {
        let observed = snapshot
            .observed_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let stale = snapshot.stale_for.as_millis();
        DeviceStatusEnvelope {
            on_wifi: snapshot.status.on_wifi,
            is_charging: snapshot.status.is_charging,
            battery_level: snapshot.status.battery_level,
            freshness: snapshot.freshness,
            observed_at_millis: observed,
            stale_for_millis: stale.min(u128::from(u64::MAX)) as u64,
        }
    });
    AnnotatedLogBundle {
        compression: "gzip",
        payload,
        device_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_header(prev: &Header, height: u64) -> Header {
        let mut h = Header {
            height,
            prev_hash: prev.hash(),
            merkle_root: [0u8; 32],
            checkpoint_hash: [0u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: Vec::new(),
            l2_sizes: Vec::new(),
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: Vec::new(),
        };
        loop {
            let hash = h.hash();
            let v = u64::from_le_bytes(hash[..8].try_into().unwrap());
            if v <= u64::MAX / h.difficulty {
                break;
            }
            h.nonce = h.nonce.wrapping_add(1);
        }
        h
    }

    #[test]
    fn respects_thresholds() {
        let opts = SyncOptions {
            wifi_only: true,
            require_charging: true,
            min_battery: 0.5,
            ..SyncOptions::default()
        };
        let genesis = Header {
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            checkpoint_hash: [0u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: Vec::new(),
            l2_sizes: Vec::new(),
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: Vec::new(),
        };
        runtime::block_on(async move {
            let mut lc = LightClient::new(genesis.clone());
            let _ = sync_background(&mut lc, opts, |_start, _batch| async { Vec::new() }).await;
            assert_eq!(lc.chain.len(), 1);
        });
    }

    #[test]
    fn verifies_pow_and_checkpoint() {
        let genesis = Header {
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            checkpoint_hash: [1u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: Vec::new(),
            l2_sizes: Vec::new(),
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: Vec::new(),
        };
        let mut lc = LightClient::new(genesis.clone());
        lc.add_checkpoint(1, [2u8; 32]);
        let mut h1 = make_header(&genesis, 1);
        h1.checkpoint_hash = [2u8; 32];
        assert!(lc.verify_and_append(h1.clone()).is_ok());
        // tamper with PoW
        let mut bad = h1.clone();
        bad.nonce = 1;
        assert!(lc.verify_and_append(bad).is_err());
    }

    #[test]
    fn verifies_pos_signature() {
        use crypto_suite::signatures::ed25519::SigningKey;
        use rand::rngs::OsRng;
        let genesis = Header {
            height: 0,
            prev_hash: [0; 32],
            merkle_root: [0; 32],
            checkpoint_hash: [0; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: vec![],
            l2_sizes: vec![],
            vdf_commit: [0; 32],
            vdf_output: [0; 32],
            vdf_proof: vec![],
        };
        let mut lc = LightClient::new(genesis.clone());
        let mut rng = OsRng::default();
        let sk = SigningKey::generate(&mut rng);
        let pk = sk.verifying_key().to_bytes();
        let mut h1 = make_header(&genesis, 1);
        h1.checkpoint_hash = [3u8; 32];
        let sig = sk.sign(&h1.checkpoint_hash);
        h1.validator_key = Some(pk);
        h1.checkpoint_sig = Some(sig.to_bytes().to_vec());
        assert!(lc.verify_and_append(h1.clone()).is_ok());
        let mut bad = h1.clone();
        bad.checkpoint_hash = [4u8; 32];
        assert!(lc.verify_and_append(bad).is_err());
    }
}
