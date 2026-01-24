use super::erasure::{self, ErasureParams};
use super::fs::RentEscrow;
use super::placement::{NodeCatalog, NodeHandle};
use super::repair;
use super::types::{ChunkRef, ObjectManifest, ProviderChunkEntry, Redundancy, StoreReceipt};
use crate::compute_market::settlement::Settlement;
use crate::simple_db::{names, SimpleDb};
use crate::storage::manifest_binary::{decode_manifest, encode_manifest, encode_store_receipt};
use crate::storage::marketplace::SearchOptions;
use crate::storage::settings;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    MemoryComponent, STORAGE_CHUNK_SIZE_BYTES, STORAGE_FINAL_CHUNK_SIZE,
    STORAGE_INITIAL_CHUNK_SIZE, STORAGE_PROVIDER_LOSS_RATE, STORAGE_PROVIDER_RTT_MS,
    STORAGE_PUT_CHUNK_SECONDS, STORAGE_PUT_ETA_SECONDS, STORAGE_PUT_OBJECT_SECONDS,
    SUBSIDY_BYTES_TOTAL,
};
use crate::transaction::BlobTx;
use coding::{Compressor, EncryptError, Encryptor};
#[cfg(any(test, feature = "gateway-test-helpers"))]
use concurrency::Lazy;
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};
use rand::{rngs::OsRng, RngCore};
use std::cmp::Ordering;
use std::collections::BTreeMap;
#[cfg(any(test, feature = "gateway-test-helpers"))]
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
#[cfg(any(test, feature = "gateway"))]
use std::fs;
#[cfg(feature = "gateway")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
#[cfg(any(test, feature = "gateway-test-helpers"))]
use std::sync::Mutex;
use std::time::{Duration, Instant};

const VERSION: u16 = 1;
const FALLBACK_CHUNK: u32 = 1024 * 1024; // 1 MiB
const FALLBACK_CHUNK_LADDER: [u32; 5] = [
    256 * 1024,
    512 * 1024,
    1024 * 1024,
    2 * 1024 * 1024,
    4 * 1024 * 1024,
];
const TARGET_TIME_SECS: f64 = 3.0;
pub const LOSS_HI: f64 = 0.02; // 2%
const LOSS_LO: f64 = 0.002; // 0.2%
pub const RTT_HI_MS: f64 = 200.0;
const RTT_LO_MS: f64 = 80.0;
const QUOTA_BYTES_PER_CREDIT: u64 = 1024 * 1024; // 1 credit == 1 MiB logical quota

mod binary;
use self::binary::{decode_provider_profile, encode_provider_profile};

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ManifestSummary {
    pub manifest: String,
    pub total_len: u64,
    pub chunk_count: u32,
    pub erasure: String,
    pub compression: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub encryption: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub compression_level: Option<i32>,
    pub erasure_fallback: bool,
    pub compression_fallback: bool,
}

fn chunk_defaults() -> (u32, Vec<u32>) {
    let (default, ladder) = settings::chunk_defaults();
    let default = u32::try_from(default).unwrap_or(FALLBACK_CHUNK);
    let mut converted: Vec<u32> = ladder
        .into_iter()
        .filter_map(|step| u32::try_from(step).ok())
        .collect();
    if converted.is_empty() {
        converted.extend_from_slice(&FALLBACK_CHUNK_LADDER);
    } else {
        converted.sort_unstable();
        converted.dedup();
    }
    (default, converted)
}

fn default_chunk_size() -> u32 {
    chunk_defaults().0
}

fn chunk_ladder() -> Vec<u32> {
    chunk_defaults().1
}

fn clamp_to_ladder(bytes: f64, ladder: &[u32]) -> u32 {
    let mut chosen = ladder.first().copied().unwrap_or(FALLBACK_CHUNK);
    for &step in ladder {
        if step as f64 <= bytes {
            chosen = step;
        }
    }
    chosen
}

fn previous_chunk_step(current: usize, ladder: &[u32]) -> usize {
    if current <= 1 {
        return 0;
    }
    for step in ladder.iter().rev() {
        if (*step as usize) < current {
            return *step as usize;
        }
    }
    std::cmp::max(1, current / 2)
}

fn manifest_encryptor(manifest: &ObjectManifest) -> Result<Box<dyn Encryptor>, String> {
    let algorithms = settings::algorithms();
    let algorithm = manifest
        .encryption_alg
        .as_deref()
        .unwrap_or(algorithms.encryptor());
    if algorithm == algorithms.encryptor() {
        settings::encryptor(&manifest.content_key_enc).map_err(|e| e.to_string())
    } else {
        settings::encryptor_for_algorithm(algorithm, &manifest.content_key_enc)
            .map_err(|e| e.to_string())
    }
}

fn manifest_compressor(manifest: &ObjectManifest) -> Result<Arc<dyn Compressor>, String> {
    let algorithms = settings::algorithms();
    let name = manifest
        .compression_alg
        .as_deref()
        .unwrap_or(algorithms.compression());
    let level = manifest
        .compression_level
        .unwrap_or_else(|| settings::compression_level());
    if name == algorithms.compression() && level == settings::compression_level() {
        Ok(settings::compressor())
    } else {
        settings::compressor_for_algorithm(name, level).map_err(|e| e.to_string())
    }
}

fn manifest_erasure_params(manifest: &ObjectManifest) -> Result<ErasureParams, String> {
    match manifest.redundancy {
        Redundancy::ReedSolomon { data, parity } => {
            let algorithm = manifest
                .erasure_alg
                .clone()
                .unwrap_or_else(|| settings::algorithms().erasure().to_string());
            Ok(ErasureParams::new(
                algorithm,
                data as usize,
                parity as usize,
            ))
        }
        Redundancy::None => Err("no erasure parameters".into()),
    }
}

fn decrypt_chunk(encryptor: &dyn Encryptor, blob: &[u8]) -> Result<Vec<u8>, String> {
    let algorithm = encryptor.algorithm();
    if cfg!(not(feature = "telemetry")) {
        let _ = &algorithm;
    }
    match encryptor.decrypt(blob) {
        Ok(bytes) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("decrypt", algorithm, "ok");
            Ok(bytes)
        }
        Err(err) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("decrypt", algorithm, "err");
            Err(match err {
                EncryptError::InvalidCiphertext { .. } => "corrupt chunk".to_string(),
                _ => "decrypt fail".to_string(),
            })
        }
    }
}

fn decompress_chunk(compressor: &dyn Compressor, data: Vec<u8>) -> Result<Vec<u8>, String> {
    let algorithm = compressor.algorithm();
    if cfg!(not(feature = "telemetry")) {
        let _ = &algorithm;
    }
    match compressor.decompress(&data) {
        Ok(bytes) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("decompress", algorithm, "ok");
            Ok(bytes)
        }
        Err(err) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("decompress", algorithm, "err");
            Err(err.to_string())
        }
    }
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn send_chunk(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn probe(&self) -> Result<f64, String> {
        Ok(0.0)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderProfile {
    pub bw_ewma: f64,
    pub rtt_ewma: f64,
    pub loss_ewma: f64,
    pub preferred_chunk: u32,
    pub stable_chunks: u32,
    pub updated_at: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub success_rate_ewma: f64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub recent_failures: u32,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub total_chunks: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub total_failures: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub last_upload_bytes: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub last_upload_secs: f64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub maintenance: bool,
}

#[derive(Clone, Debug)]
pub struct ProviderProfileSnapshot {
    pub provider: String,
    pub profile: ProviderProfile,
    pub quota_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineEngineSummary {
    pub pipeline: String,
    pub rent_escrow: String,
}

impl ProviderProfile {
    fn new() -> Self {
        Self {
            bw_ewma: 0.0,
            rtt_ewma: 0.0,
            loss_ewma: 0.0,
            preferred_chunk: default_chunk_size(),
            stable_chunks: 0,
            updated_at: 0,
            success_rate_ewma: 0.0,
            recent_failures: 0,
            total_chunks: 0,
            total_failures: 0,
            last_upload_bytes: 0,
            last_upload_secs: 0.0,
            maintenance: false,
        }
    }

    fn ensure_defaults(&mut self) {
        if self.preferred_chunk == 0 {
            self.preferred_chunk = default_chunk_size();
        }
    }

    fn record_chunk(
        &mut self,
        chunk_bytes: usize,
        throughput: f64,
        rtt: f64,
        loss: f64,
        success: bool,
    ) {
        self.ensure_defaults();
        self.total_chunks = self.total_chunks.saturating_add(1);
        self.last_upload_bytes = chunk_bytes as u64;
        self.last_upload_secs = if throughput > 0.0 {
            chunk_bytes as f64 / throughput
        } else {
            0.0
        };
        if success {
            self.bw_ewma = StoragePipeline::ewma(self.bw_ewma, throughput);
            self.rtt_ewma = StoragePipeline::ewma(self.rtt_ewma, rtt);
            self.loss_ewma = StoragePipeline::ewma(self.loss_ewma, loss);
            self.success_rate_ewma = StoragePipeline::ewma(self.success_rate_ewma, 1.0);
            self.recent_failures = 0;
            self.stable_chunks = self.stable_chunks.saturating_add(1);
            self.adjust_preferred_chunk();
        } else {
            self.total_failures = self.total_failures.saturating_add(1);
            self.success_rate_ewma = StoragePipeline::ewma(self.success_rate_ewma, 0.0);
            self.recent_failures = self.recent_failures.saturating_add(1);
            self.stable_chunks = 0;
            let ladder = chunk_ladder();
            if let Some(first) = ladder.first() {
                if self.preferred_chunk > *first {
                    let idx = ladder
                        .iter()
                        .position(|s| *s == self.preferred_chunk)
                        .unwrap_or(0);
                    let downgraded_idx = idx.saturating_sub(1);
                    self.preferred_chunk = ladder[downgraded_idx];
                }
            }
        }
        if let Ok(secs) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            self.updated_at = secs.as_secs();
        }
    }

    fn adjust_preferred_chunk(&mut self) {
        let ladder = chunk_ladder();
        let mut desired = clamp_to_ladder(self.bw_ewma * TARGET_TIME_SECS, &ladder);
        let current = self.preferred_chunk;
        let step_idx = ladder.iter().position(|s| *s == current).unwrap_or(0);
        let desired_idx = ladder
            .iter()
            .position(|s| *s == desired)
            .unwrap_or(step_idx);
        let diff = desired_idx as i32 - step_idx as i32;

        if self.loss_ewma > LOSS_HI || self.rtt_ewma > RTT_HI_MS {
            desired = ladder[step_idx.saturating_sub(1)];
        } else if self.loss_ewma < LOSS_LO && self.rtt_ewma < RTT_LO_MS {
            // allow desired as computed
        } else {
            desired = current;
        }

        if desired != current && self.stable_chunks >= 3 {
            if (diff.abs() as usize) >= 1 {
                self.preferred_chunk = desired;
                self.stable_chunks = 0;
            }
        }
    }
}

struct ProviderState {
    handle: NodeHandle,
    profile: ProviderProfile,
    quota_bytes: u64,
    used_bytes: u64,
}

impl ProviderState {
    fn available_bytes(&self) -> u64 {
        if self.quota_bytes == 0 {
            u64::MAX
        } else {
            self.quota_bytes.saturating_sub(self.used_bytes)
        }
    }

    fn has_capacity(&self, bytes: usize) -> bool {
        self.quota_bytes == 0 || self.available_bytes() >= bytes as u64
    }

    fn score(&self) -> f64 {
        let loss = self.handle.loss.max(0.0).min(0.5);
        let rtt = self.handle.rtt.max(1.0);
        let success = self.profile.success_rate_ewma.max(0.1);
        (1.0 - loss).max(0.05) * success / rtt
    }

    fn id(&self) -> &str {
        &self.handle.id
    }
}

enum DispatchError {
    InsufficientCapacity,
}

pub struct StoragePipeline {
    db: SimpleDb,
    rent: RentEscrow,
    rent_rate: i64,
    repair_log_dir: PathBuf,
}

impl StoragePipeline {
    pub fn open(path: &str) -> Self {
        Self::open_with_factory(path, &SimpleDb::open_named)
    }

    pub fn open_with_factory<F>(path: &str, factory: &F) -> Self
    where
        F: Fn(&str, &str) -> SimpleDb,
    {
        super::repair::spawn(path.to_string(), Duration::from_secs(60));
        let repair_log_dir = PathBuf::from(path).join("repair_log");
        Self {
            db: factory(names::STORAGE_PIPELINE, path),
            rent: RentEscrow::open_with_factory(&format!("{path}/rent_escrow.db"), factory),
            rent_rate: 0,
            repair_log_dir,
        }
    }

    /// Build a [`BlobTx`] for raw data, hashing with BLAKE3 and assigning a
    /// unique `blob_id`. The transaction targets fractal layer 1 (L2) by
    /// default.
    pub fn build_blob_tx(owner: &str, data: &[u8], expiry: Option<u64>) -> BlobTx {
        let mut hasher = Hasher::new();
        hasher.update(data);
        let root: [u8; 32] = hasher.finalize().into();
        let mut blob_id = [0u8; 32];
        OsRng::default().fill_bytes(&mut blob_id);
        BlobTx {
            owner: owner.to_string(),
            blob_id,
            blob_root: root,
            blob_size: data.len() as u64,
            fractal_lvl: 1,
            expiry,
        }
    }

    pub fn set_rent_rate(&mut self, rate: i64) {
        self.rent_rate = rate;
    }

    /// Logical quota in bytes derived from the provider's stake balance.
    pub fn logical_quota_bytes(provider: &str) -> u64 {
        Self::quota_for(provider)
    }

    fn profile_key(provider: &str) -> String {
        format!("provider_profiles/{}", provider)
    }

    fn load_profile(&self, provider: &str) -> ProviderProfile {
        let key = Self::profile_key(provider);
        let mut profile = self
            .db
            .get(&key)
            .and_then(|b| decode_provider_profile(&b).ok())
            .unwrap_or_else(ProviderProfile::new);
        profile.ensure_defaults();
        profile
    }

    fn save_profile(&mut self, provider: &str, profile: &ProviderProfile) {
        let key = Self::profile_key(provider);
        if let Ok(bytes) = encode_provider_profile(profile) {
            let _ = self.db.try_insert(&key, bytes);
        }
    }

    pub fn get_profile(&self, provider: &str) -> Option<ProviderProfile> {
        let key = Self::profile_key(provider);
        self.db
            .get(&key)
            .and_then(|b| decode_provider_profile(&b).ok())
    }

    pub fn provider_profile_snapshots(&self) -> Vec<ProviderProfileSnapshot> {
        self.db
            .keys_with_prefix("provider_profiles/")
            .into_iter()
            .filter_map(|key| {
                let provider = key.strip_prefix("provider_profiles/")?.to_string();
                let profile = self.get_profile(&provider)?;
                Some(ProviderProfileSnapshot {
                    quota_bytes: Self::logical_quota_bytes(&provider),
                    provider,
                    profile,
                })
            })
            .collect()
    }

    /// Build marketplace search options backed by the provider snapshots.
    pub fn marketplace_search_options(&self, object_size: u64, shares: u16) -> SearchOptions {
        let ready = self.provider_profile_snapshots().len();
        let limit = ready.max(5).min(100);
        SearchOptions {
            object_size,
            shares: shares.max(1),
            region: env::var("TB_GATEWAY_REGION").ok(),
            limit,
            max_price_per_block: None,
            min_success_rate_ppm: Some(900_000),
        }
    }

    pub fn set_provider_maintenance(
        &mut self,
        provider: &str,
        maintenance: bool,
    ) -> Result<(), String> {
        let mut profile = self.load_profile(provider);
        profile.maintenance = maintenance;
        self.save_profile(provider, &profile);
        Ok(())
    }

    pub fn engine_summary(&self) -> PipelineEngineSummary {
        PipelineEngineSummary {
            pipeline: self.db.backend_name().to_string(),
            rent_escrow: self.rent.engine_label().to_string(),
        }
    }

    fn ewma(prev: f64, new: f64) -> f64 {
        if prev == 0.0 {
            new
        } else {
            prev * 0.8 + new * 0.2
        }
    }

    fn quota_for(provider: &str) -> u64 {
        Settlement::balance(provider).saturating_mul(QUOTA_BYTES_PER_CREDIT)
    }

    fn select_chunk_len(states: &[ProviderState], remaining: usize) -> usize {
        if remaining == 0 {
            return 0;
        }
        let mut best = 0usize;
        let mut best_score = f64::MIN;
        for state in states {
            let preferred = state.profile.preferred_chunk as usize;
            if preferred == 0 {
                continue;
            }
            let candidate = preferred.min(remaining);
            if candidate == 0 || !state.has_capacity(candidate) {
                continue;
            }
            let score = state.score();
            if score > best_score {
                best_score = score;
                best = candidate;
            }
        }
        if best == 0 {
            remaining.min(default_chunk_size() as usize)
        } else {
            best
        }
    }

    fn previous_chunk_step(current: usize) -> usize {
        previous_chunk_step(current, &chunk_ladder())
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_shards(
        &mut self,
        provider_states: &mut [ProviderState],
        catalog: &mut NodeCatalog,
        shards: Vec<Vec<u8>>,
        chunk_idx: usize,
        chunk_plain_len: usize,
        chunks: &mut Vec<ChunkRef>,
        provider_chunk_index: &mut BTreeMap<String, ProviderChunkEntry>,
        provider_keys: &mut BTreeMap<String, Vec<u8>>,
        key_bytes: &[u8; 32],
    ) -> Result<(), DispatchError> {
        let mut provider_order: Vec<usize> = (0..provider_states.len()).collect();
        provider_order.sort_by(|a, b| {
            provider_states[*b]
                .score()
                .partial_cmp(&provider_states[*a].score())
                .unwrap_or(Ordering::Equal)
        });

        for (idx, shard) in shards.into_iter().enumerate() {
            let mut assigned = None;
            for &pi in &provider_order {
                if !provider_states[pi].has_capacity(shard.len()) {
                    continue;
                }
                let provider_id = provider_states[pi].id().to_string();
                let start = Instant::now();
                match provider_states[pi].handle.provider.send_chunk(&shard) {
                    Ok(()) => {
                        let duration = start.elapsed();
                        let throughput = if duration.as_secs_f64() > 0.0 {
                            shard.len() as f64 / duration.as_secs_f64()
                        } else {
                            shard.len() as f64
                        };
                        provider_states[pi].used_bytes = provider_states[pi]
                            .used_bytes
                            .saturating_add(shard.len() as u64);
                        let rtt = provider_states[pi].handle.rtt;
                        let loss = provider_states[pi].handle.loss;
                        provider_states[pi].profile.record_chunk(
                            shard.len(),
                            throughput,
                            rtt,
                            loss,
                            true,
                        );
                        catalog.record_chunk_result(&provider_id, shard.len(), duration, true);
                        let entry = provider_keys.entry(provider_id.clone()).or_insert_with(|| {
                            let mut keyed = Hasher::new_keyed(key_bytes);
                            keyed.update(provider_id.as_bytes());
                            keyed.finalize().as_bytes().to_vec()
                        });
                        let provider_key = entry.clone();

                        let mut h = Hasher::new();
                        h.update(&[idx as u8]);
                        h.update(&shard);
                        let id = *h.finalize().as_bytes();
                        if self
                            .db
                            .try_insert(
                                &format!("chunk/{}", crypto_suite::hex::encode(id)),
                                shard.clone(),
                            )
                            .is_err()
                        {
                            return Err(DispatchError::InsufficientCapacity);
                        }

                        let mut chunk_ref = ChunkRef {
                            id,
                            nodes: vec![provider_id.clone()],
                            provider_chunks: Vec::new(),
                        };
                        chunk_ref.provider_chunks.push(ProviderChunkEntry {
                            provider: provider_id.clone(),
                            chunk_indices: vec![chunk_idx as u32],
                            chunk_lens: vec![chunk_plain_len as u32],
                            encryption_key: provider_key.clone(),
                        });
                        chunks.push(chunk_ref);

                        let entry = provider_chunk_index
                            .entry(provider_id.clone())
                            .or_insert_with(|| ProviderChunkEntry {
                                provider: provider_id.clone(),
                                ..Default::default()
                            });
                        if entry.chunk_indices.last().copied().unwrap_or(u32::MAX)
                            != chunk_idx as u32
                        {
                            entry.chunk_indices.push(chunk_idx as u32);
                            entry.chunk_lens.push(chunk_plain_len as u32);
                        }
                        if entry.encryption_key.is_empty() {
                            entry.encryption_key = provider_key.clone();
                        }

                        #[cfg(feature = "telemetry")]
                        {
                            STORAGE_PROVIDER_RTT_MS
                                .ensure_handle_for_label_values(&[provider_id.as_str()])
                                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                                .observe(rtt);
                            STORAGE_PROVIDER_LOSS_RATE
                                .ensure_handle_for_label_values(&[provider_id.as_str()])
                                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                                .observe(loss);
                        }
                        assigned = Some(());
                        break;
                    }
                    Err(err) => {
                        provider_states[pi].profile.record_chunk(
                            shard.len(),
                            0.0,
                            provider_states[pi].handle.rtt,
                            1.0,
                            false,
                        );
                        catalog.record_chunk_result(
                            &provider_id,
                            shard.len(),
                            start.elapsed(),
                            false,
                        );
                        #[cfg(feature = "telemetry")]
                        {
                            diagnostics::tracing::warn!(%err, provider = %provider_id, "storage shard send failed");
                        }
                        #[cfg(not(feature = "telemetry"))]
                        {
                            let _ = err;
                        }
                        continue;
                    }
                }
            }
            if assigned.is_none() {
                return Err(DispatchError::InsufficientCapacity);
            }
        }
        Ok(())
    }

    pub fn put_object(
        &mut self,
        data: &[u8],
        lane: &str,
        catalog: &mut NodeCatalog,
    ) -> Result<(StoreReceipt, BlobTx), String> {
        #[cfg(feature = "telemetry")]
        let telemetry_start = Instant::now();
        let rent = (self.rent_rate as u64).saturating_mul(data.len() as u64);
        if Settlement::spend(lane, "rent", rent).is_err() {
            return Err("ERR_RENT_ESCROW_INSUFFICIENT".into());
        }

        let algorithms = settings::algorithms();
        let compressor = settings::compressor();
        let compression_level = settings::compression_level();
        let erasure_params = erasure::default_params();

        let mut data_hasher = Hasher::new();
        data_hasher.update(data);
        let blob_root: [u8; 32] = data_hasher.finalize().into();
        let mut blob_id = [0u8; 32];
        OsRng::default().fill_bytes(&mut blob_id);
        let mut key_bytes = [0u8; 32];
        OsRng::default().fill_bytes(&mut key_bytes);
        let encryptor = settings::encryptor(&key_bytes).map_err(|e| e.to_string())?;

        let handles = catalog.ranked_nodes();
        let mut provider_states: Vec<ProviderState> = handles
            .into_iter()
            .filter(|h| !h.maintenance)
            .filter_map(|handle| {
                let profile = self.load_profile(&handle.id);
                if profile.maintenance {
                    return None;
                }
                Some(ProviderState {
                    quota_bytes: Self::quota_for(&handle.id),
                    used_bytes: 0,
                    profile,
                    handle,
                })
            })
            .collect();

        if provider_states.is_empty() {
            return Err("no providers".into());
        }

        #[cfg(feature = "telemetry")]
        {
            let initial = provider_states
                .first()
                .map(|p| p.profile.preferred_chunk)
                .unwrap_or(default_chunk_size());
            STORAGE_INITIAL_CHUNK_SIZE.set(initial as i64);
        }

        let mut chunks = Vec::new();
        let mut chunk_lens: Vec<u32> = Vec::new();
        let mut compressed_lens: Vec<u32> = Vec::new();
        let mut cipher_lens: Vec<u32> = Vec::new();
        let mut provider_chunk_index: BTreeMap<String, ProviderChunkEntry> = BTreeMap::new();
        let mut provider_keys: BTreeMap<String, Vec<u8>> = BTreeMap::new();

        let mut offset = 0usize;
        while offset < data.len() {
            let remaining = data.len() - offset;
            let mut desired = Self::select_chunk_len(&provider_states, remaining);
            if desired == 0 {
                desired = remaining;
            }
            let mut dispatched = false;

            while desired > 0 && !dispatched {
                let end = offset + desired.min(remaining);
                let chunk = &data[offset..end];
                #[cfg(feature = "telemetry")]
                let chunk_start = Instant::now();
                let compression_algo = compressor.algorithm();
                if cfg!(not(feature = "telemetry")) {
                    let _ = &compression_algo;
                }
                let compressed = match compressor.compress(chunk) {
                    Ok(data) => {
                        #[cfg(feature = "telemetry")]
                        {
                            crate::telemetry::record_coding_result(
                                "compress",
                                compression_algo,
                                "ok",
                            );
                            if !chunk.is_empty() {
                                crate::telemetry::record_compression_ratio(
                                    compression_algo,
                                    data.len() as f64 / chunk.len() as f64,
                                );
                            }
                        }
                        data
                    }
                    Err(err) => {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::record_coding_result("compress", compression_algo, "err");
                        return Err(err.to_string());
                    }
                };
                let encrypt_algo = algorithms.encryptor();
                if cfg!(not(feature = "telemetry")) {
                    let _ = &encrypt_algo;
                }
                let blob = match encryptor.encrypt(&compressed) {
                    Ok(ciphertext) => {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::record_coding_result("encrypt", encrypt_algo, "ok");
                        ciphertext
                    }
                    Err(err) => {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::record_coding_result("encrypt", encrypt_algo, "err");
                        return Err(err.to_string());
                    }
                };
                let shards = erasure::encode_with_params(&blob, &erasure_params)?;

                match self.dispatch_shards(
                    provider_states.as_mut_slice(),
                    catalog,
                    shards,
                    chunk_lens.len(),
                    chunk.len(),
                    &mut chunks,
                    &mut provider_chunk_index,
                    &mut provider_keys,
                    &key_bytes,
                ) {
                    Ok(()) => {
                        #[cfg(feature = "telemetry")]
                        {
                            STORAGE_CHUNK_SIZE_BYTES.observe(chunk.len() as f64);
                            crate::telemetry::sampled_observe_vec(
                                &STORAGE_PUT_CHUNK_SECONDS,
                                &[algorithms.erasure(), algorithms.compression()],
                                chunk_start.elapsed().as_secs_f64(),
                            );
                        }
                        chunk_lens.push(u32::try_from(chunk.len()).map_err(|_| "chunk too large")?);
                        compressed_lens.push(
                            u32::try_from(compressed.len())
                                .map_err(|_| "compressed chunk too large")?,
                        );
                        cipher_lens
                            .push(u32::try_from(blob.len()).map_err(|_| "cipher chunk too large")?);
                        offset = end;
                        dispatched = true;
                    }
                    Err(DispatchError::InsufficientCapacity) => {
                        desired = Self::previous_chunk_step(desired);
                    }
                }
            }

            if !dispatched {
                return Err("storage provider capacity exhausted".into());
            }
        }

        for state in &provider_states {
            self.save_profile(state.id(), &state.profile);
        }

        let rs_data = erasure_params.data_shards;
        let rs_parity = erasure_params.parity_shards;
        let rs_data_u8 = u8::try_from(rs_data).map_err(|_| "invalid data shard count")?;
        let rs_parity_u8 = u8::try_from(rs_parity).map_err(|_| "invalid parity shard count")?;

        let mut manifest = ObjectManifest {
            version: VERSION,
            total_len: data.len() as u64,
            chunk_len: chunk_lens.first().copied().unwrap_or(default_chunk_size()) as u32,
            chunks,
            redundancy: Redundancy::ReedSolomon {
                data: rs_data_u8,
                parity: rs_parity_u8,
            },
            content_key_enc: key_bytes.to_vec(),
            blake3: [0u8; 32],
            chunk_lens,
            chunk_compressed_lens: compressed_lens,
            chunk_cipher_lens: cipher_lens,
            compression_alg: Some(algorithms.compression().to_string()),
            compression_level: Some(compression_level),
            encryption_alg: Some(algorithms.encryptor().to_string()),
            erasure_alg: Some(erasure_params.algorithm.clone()),
            provider_chunks: provider_chunk_index.values().cloned().collect(),
        };
        let mut h = Hasher::new();
        let manifest_bytes_temp = encode_manifest(&manifest).map_err(|e| e.to_string())?;
        h.update(&manifest_bytes_temp);
        let man_hash = *h.finalize().as_bytes();
        manifest.blake3 = man_hash;
        let manifest_bytes = encode_manifest(&manifest).map_err(|e| e.to_string())?;
        self.db
            .try_insert(
                &format!("manifest/{}", crypto_suite::hex::encode(man_hash)),
                manifest_bytes,
            )
            .map_err(|e| e.to_string())?;
        let chunk_count = u32::try_from(manifest.chunk_count())
            .map_err(|_| "chunk count overflow".to_string())?;
        let receipt = StoreReceipt {
            manifest_hash: man_hash,
            chunk_count,
            redundancy: Redundancy::ReedSolomon {
                data: rs_data_u8,
                parity: rs_parity_u8,
            },
            lane: lane.to_string(),
        };
        let rec_bytes = encode_store_receipt(&receipt).map_err(|e| e.to_string())?;
        self.db
            .try_insert(
                &format!("receipt/{}", crypto_suite::hex::encode(man_hash)),
                rec_bytes,
            )
            .map_err(|e| e.to_string())?;
        self.rent
            .lock(&crypto_suite::hex::encode(man_hash), lane, rent, 0);
        let blob_tx = BlobTx {
            owner: lane.to_string(),
            blob_id,
            blob_root,
            blob_size: data.len() as u64,
            fractal_lvl: 1,
            expiry: None,
        };
        #[cfg(feature = "telemetry")]
        SUBSIDY_BYTES_TOTAL
            .ensure_handle_for_label_values(&["storage"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .inc_by(data.len() as u64);
        #[cfg(feature = "telemetry")]
        {
            if let Some(final_pref) = provider_states.first() {
                STORAGE_FINAL_CHUNK_SIZE.set(final_pref.profile.preferred_chunk as i64);
            }
            let total_bw: f64 = provider_states
                .iter()
                .map(|s| s.profile.bw_ewma)
                .filter(|bw| *bw > 0.0)
                .sum();
            if total_bw > 0.0 {
                let eta = data.len() as f64 / total_bw;
                STORAGE_PUT_ETA_SECONDS.set(eta as i64);
            }
        }
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::sampled_observe_vec(
                &STORAGE_PUT_OBJECT_SECONDS,
                &[algorithms.erasure(), algorithms.compression()],
                telemetry_start.elapsed().as_secs_f64(),
            );
            crate::telemetry::update_memory_usage(MemoryComponent::Storage);
        }
        Ok((receipt, blob_tx))
    }

    pub fn get_object(&self, manifest_hash: &[u8; 32]) -> Result<Vec<u8>, String> {
        let key = format!("manifest/{}", crypto_suite::hex::encode(manifest_hash));
        let manifest_bytes = self.db.get(&key).ok_or("missing manifest")?;
        let manifest = decode_manifest(&manifest_bytes).map_err(|e| e.to_string())?;
        let encryptor = manifest_encryptor(&manifest)?;
        let compressor = manifest_compressor(&manifest)?;
        let mut out = Vec::with_capacity(manifest.total_len as usize);
        match manifest.redundancy {
            Redundancy::None => {
                for (idx, ch) in manifest.chunks.iter().enumerate() {
                    let blob = self
                        .db
                        .get(&format!("chunk/{}", crypto_suite::hex::encode(ch.id)))
                        .ok_or("missing chunk")?;
                    let decrypted = decrypt_chunk(&*encryptor, &blob)?;
                    let mut plain = decompress_chunk(&*compressor, decrypted)?;
                    let expected = manifest.chunk_plain_len(idx);
                    if plain.len() < expected {
                        return Err("corrupt chunk".into());
                    }
                    plain.truncate(expected);
                    out.extend_from_slice(&plain);
                }
            }
            Redundancy::ReedSolomon { .. } => {
                let params = manifest_erasure_params(&manifest)?;
                let shards_per_chunk = erasure::total_shards_for_params(&params);
                if shards_per_chunk == 0 {
                    return Err("invalid shard layout".into());
                }
                if manifest.chunks.len() % shards_per_chunk != 0 {
                    return Err("corrupt manifest".into());
                }
                for (chunk_idx, group) in manifest.chunks.chunks(shards_per_chunk).enumerate() {
                    let mut shards = vec![None; shards_per_chunk];
                    for (slot, r) in group.iter().enumerate() {
                        let blob = self
                            .db
                            .get(&format!("chunk/{}", crypto_suite::hex::encode(r.id)));
                        shards[slot] = blob;
                    }
                    let expected_cipher = manifest.chunk_cipher_len(chunk_idx);
                    let blob = erasure::reconstruct_with_params(shards, expected_cipher, &params)?;
                    let decrypted = decrypt_chunk(&*encryptor, &blob)?;
                    let mut plain = decompress_chunk(&*compressor, decrypted)?;
                    let expected = manifest.chunk_plain_len(chunk_idx);
                    if plain.len() < expected {
                        return Err("corrupt chunk".into());
                    }
                    plain.truncate(expected);
                    out.extend_from_slice(&plain);
                }
            }
        }
        out.truncate(manifest.total_len as usize);
        #[cfg(feature = "telemetry")]
        SUBSIDY_BYTES_TOTAL
            .ensure_handle_for_label_values(&["read"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .inc_by(out.len() as u64);
        Ok(out)
    }

    pub fn manifest_summaries(&self, limit: usize) -> Vec<ManifestSummary> {
        let defaults = settings::algorithms();
        let mut entries = Vec::new();
        let keys = self.db.keys_with_prefix("manifest/");
        for key in keys {
            if limit > 0 && entries.len() >= limit {
                break;
            }
            let Some(hex_hash) = key.strip_prefix("manifest/") else {
                continue;
            };
            let Ok(raw_hash) = crypto_suite::hex::decode(hex_hash) else {
                continue;
            };
            if raw_hash.len() != 32 {
                continue;
            }
            let Some(bytes) = self.db.get(&key) else {
                continue;
            };
            let Ok(manifest) = decode_manifest(&bytes) else {
                continue;
            };
            let Ok(chunk_count) = u32::try_from(manifest.chunk_count()) else {
                continue;
            };
            let erasure = manifest
                .erasure_alg
                .clone()
                .unwrap_or_else(|| defaults.erasure().to_string());
            let compression = manifest
                .compression_alg
                .clone()
                .unwrap_or_else(|| defaults.compression().to_string());
            entries.push(ManifestSummary {
                manifest: crypto_suite::hex::encode(&raw_hash),
                total_len: manifest.total_len,
                chunk_count,
                erasure: erasure.clone(),
                compression: compression.clone(),
                encryption: manifest.encryption_alg.clone(),
                compression_level: manifest.compression_level,
                erasure_fallback: manifest_uses_fallback_erasure(&erasure),
                compression_fallback: manifest_uses_fallback_compression(&compression),
            });
        }
        entries
    }

    pub fn delete_object(&mut self, manifest_hash: &[u8; 32]) -> Result<u64, String> {
        let key = format!("manifest/{}", crypto_suite::hex::encode(manifest_hash));
        let _ = self.db.remove(&key);
        let id = crypto_suite::hex::encode(manifest_hash);
        if let Some((depositor, refund, _burn)) = self.rent.release(&id) {
            Settlement::accrue(&depositor, "rent_refund", refund);
            Ok(refund)
        } else {
            Ok(0)
        }
    }

    pub fn process_expired(&mut self, now: u64) {
        for (depositor, refund, _burn) in self.rent.purge_expired(now) {
            Settlement::accrue(&depositor, "rent_refund", refund);
        }
    }

    pub fn get_manifest(&self, manifest_hash: &[u8; 32]) -> Option<ObjectManifest> {
        let key = format!("manifest/{}", crypto_suite::hex::encode(manifest_hash));
        self.db.get(&key).and_then(|b| decode_manifest(&b).ok())
    }

    pub fn db_mut(&mut self) -> &mut SimpleDb {
        &mut self.db
    }

    pub fn repair_log(&self) -> repair::RepairLog {
        repair::RepairLog::new(self.repair_log_dir.clone())
    }

    pub fn repair_log_dir(&self) -> PathBuf {
        self.repair_log_dir.clone()
    }
}

fn manifest_uses_fallback_erasure(algorithm: &str) -> bool {
    algorithm.eq_ignore_ascii_case("xor")
}

fn manifest_uses_fallback_compression(algorithm: &str) -> bool {
    algorithm.eq_ignore_ascii_case("rle")
}

#[cfg(test)]
impl StoragePipeline {
    pub(crate) fn db(&self) -> &SimpleDb {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{erasure, NodeCatalog};
    use crate::compute_market::settlement::{SettleMode, Settlement};
    use crate::storage::repair;
    use crate::storage::types::{ChunkRef, ObjectManifest, ProviderChunkEntry};
    use crypto_suite::hashing::blake3;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use sys::tempfile::tempdir;

    static SETTLEMENT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct SettlementGuard {
        _lock: MutexGuard<'static, ()>,
        _dir: sys::tempfile::TempDir,
    }

    impl SettlementGuard {
        fn new() -> Self {
            let lock = SETTLEMENT_TEST_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = tempdir().expect("settlement tempdir");
            let path = dir.path().join("settlement");
            let path_str = path.to_str().expect("settlement path str");
            Settlement::init(path_str, SettleMode::DryRun);
            Self {
                _lock: lock,
                _dir: dir,
            }
        }

        fn prefund_lane(&self, lane: &str, amount: u64) {
            Settlement::accrue(lane, "test_prefund", amount);
        }
    }

    impl Drop for SettlementGuard {
        fn drop(&mut self) {
            Settlement::shutdown();
        }
    }

    struct StubProvider {
        id: String,
    }

    impl Provider for StubProvider {
        fn id(&self) -> &str {
            &self.id
        }
    }

    fn catalog_with_stub() -> NodeCatalog {
        let mut catalog = NodeCatalog::new();
        catalog.register(StubProvider {
            id: "provider-1".to_string(),
        });
        catalog
    }

    fn load_manifest(pipeline: &StoragePipeline, hash: &[u8; 32]) -> ObjectManifest {
        let key = format!("manifest/{}", crypto_suite::hex::encode(hash));
        let bytes = {
            let db = pipeline.db();
            db.get(&key).expect("manifest present")
        };
        decode_manifest(&bytes).expect("manifest decode")
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(prev) = &self.previous {
                std::env::set_var(self.key, prev);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn sample_blob(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn get_object_round_trips_with_missing_shards() {
        let settlement = SettlementGuard::new();
        settlement.prefund_lane("lane", 1_000_000);
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("pipeline-db");
        let path_str = path.to_str().expect("path str");
        let mut pipeline = StoragePipeline::open(path_str);
        let mut catalog = catalog_with_stub();

        let data = sample_blob(1_200_000);
        let (receipt, _) = pipeline
            .put_object(&data, "lane", &mut catalog)
            .expect("store object");

        let manifest = load_manifest(&pipeline, &receipt.manifest_hash);
        let shards_per_chunk = erasure::total_shards_per_chunk();
        assert!(manifest.chunks.len() >= shards_per_chunk);
        let first_chunk = &manifest.chunks[..shards_per_chunk];
        for idx in [0usize, 3, 17] {
            let shard_id = first_chunk[idx].id;
            let key = format!("chunk/{}", crypto_suite::hex::encode(shard_id));
            pipeline.db_mut().remove(&key);
            assert!(pipeline.db().get(&key).is_none());
        }

        let restored = pipeline
            .get_object(&receipt.manifest_hash)
            .expect("reconstruct");
        assert_eq!(restored, data);
    }

    #[test]
    fn repair_rebuilds_missing_shards() {
        let settlement = SettlementGuard::new();
        settlement.prefund_lane("lane", 1_000_000);
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("repair-db");
        let path_str = path.to_str().expect("path str");
        let mut pipeline = StoragePipeline::open(path_str);
        let mut catalog = catalog_with_stub();

        let data = sample_blob(1_000_000);
        let (receipt, _) = pipeline
            .put_object(&data, "lane", &mut catalog)
            .expect("store object");

        let manifest = load_manifest(&pipeline, &receipt.manifest_hash);
        let shards_per_chunk = erasure::total_shards_per_chunk();
        assert!(manifest.chunks.len() >= shards_per_chunk);
        let first_chunk = &manifest.chunks[..shards_per_chunk];
        let mut removed_keys = Vec::new();
        for idx in [0usize, 2, 5, 21] {
            let shard_id = first_chunk[idx].id;
            let key = format!("chunk/{}", crypto_suite::hex::encode(shard_id));
            pipeline.db_mut().remove(&key);
            removed_keys.push(key);
        }

        let log = pipeline.repair_log();
        repair::run_once(pipeline.db_mut(), &log, repair::RepairRequest::default())
            .expect("repair");

        for key in &removed_keys {
            assert!(pipeline.db().get(key).is_some());
        }

        let restored = pipeline
            .get_object(&receipt.manifest_hash)
            .expect("reconstruct");
        assert_eq!(restored, data);
    }

    #[test]
    fn normalize_providers_sorts_and_dedups() {
        let providers = vec![
            "gateway-ldn-01".to_string(),
            "".to_string(),
            "gateway-nyc-01".to_string(),
            "gateway-ldn-01".to_string(),
            "gateway-sfo-01".to_string(),
        ];
        let normalized = super::normalize_providers(providers);
        assert_eq!(
            normalized,
            vec![
                "gateway-ldn-01".to_string(),
                "gateway-nyc-01".to_string(),
                "gateway-sfo-01".to_string(),
            ]
        );
    }

    #[test]
    fn select_provider_is_deterministic() {
        let providers = vec![
            "gateway-ams-01".to_string(),
            "gateway-nyc-01".to_string(),
            "gateway-sfo-01".to_string(),
        ];
        let manifest = [0xAAu8; 32];
        let first_path: [u8; 32] = blake3::hash(b"/first").into();
        let second_path: [u8; 32] = blake3::hash(b"/second").into();

        let first =
            super::select_provider(&providers, &manifest, &first_path).expect("provider selected");
        let repeat = super::select_provider(&providers, &manifest, &first_path)
            .expect("repeat provider selected");
        assert_eq!(first, repeat);

        let second =
            super::select_provider(&providers, &manifest, &second_path).expect("provider selected");
        assert!(providers.contains(&second));

        let single = super::select_provider(&providers[..1], &manifest, &first_path)
            .expect("single provider");
        assert_eq!(single, providers[0]);

        assert!(super::select_provider(&[], &manifest, &first_path).is_none());
    }

    #[test]
    fn provider_for_manifest_prefers_override_then_manifest() {
        let dir = tempdir().expect("pipeline tempdir");
        let db_path = dir.path().join("pipeline-db");
        let path_str = db_path.to_str().expect("db path str");
        let _guard = EnvGuard::set("TB_STORAGE_PIPELINE_DIR", path_str);

        fs::create_dir_all(&db_path).expect("pipeline directory");

        let manifest_hash = [0xBCu8; 32];
        let mut manifest = ObjectManifest {
            version: 1,
            total_len: 1024,
            chunk_len: 1024,
            chunks: vec![ChunkRef {
                id: [0x11u8; 32],
                nodes: vec!["gateway-nyc-01".to_string()],
                provider_chunks: Vec::new(),
            }],
            redundancy: Redundancy::None,
            content_key_enc: Vec::new(),
            blake3: manifest_hash,
            chunk_lens: vec![1024],
            chunk_compressed_lens: vec![1024],
            chunk_cipher_lens: vec![1040],
            compression_alg: None,
            compression_level: None,
            encryption_alg: None,
            erasure_alg: None,
            provider_chunks: vec![ProviderChunkEntry {
                provider: "gateway-sfo-01".to_string(),
                chunk_indices: vec![0],
                chunk_lens: vec![1024],
                encryption_key: Vec::new(),
            }],
        };

        let encoded = encode_manifest(&manifest).expect("manifest encode");
        let mut db = SimpleDb::open_named(names::STORAGE_PIPELINE, path_str);
        let key = format!("manifest/{}", crypto_suite::hex::encode(manifest_hash));
        assert!(db.insert(&key, encoded).is_none(), "manifest stored");

        let path_hash: [u8; 32] = blake3::hash(b"/provider/test").into();
        let selected =
            super::provider_for_manifest(&manifest_hash, &path_hash).expect("manifest provider");
        assert_eq!(selected, "gateway-sfo-01");

        super::override_manifest_providers_for_test(
            manifest_hash,
            vec!["gateway-ams-01".to_string()],
        );
        let override_selected =
            super::provider_for_manifest(&manifest_hash, &path_hash).expect("override provider");
        assert_eq!(override_selected, "gateway-ams-01");

        super::clear_test_manifest_providers();

        manifest.provider_chunks = Vec::new();
        manifest.chunks[0].provider_chunks = vec![ProviderChunkEntry {
            provider: "gateway-ldn-01".to_string(),
            chunk_indices: vec![0],
            chunk_lens: vec![1024],
            encryption_key: Vec::new(),
        }];
        let encoded = encode_manifest(&manifest).expect("manifest encode");
        db.insert(&key, encoded).expect("manifest updated");

        let fallback =
            super::provider_for_manifest(&manifest_hash, &path_hash).expect("fallback provider");
        assert_eq!(fallback, "gateway-ldn-01");
    }
}

static L2_CAP_BYTES_PER_EPOCH: AtomicU64 = AtomicU64::new(33_554_432);
static BYTES_PER_SENDER_EPOCH_CAP: AtomicU64 = AtomicU64::new(16_777_216);

pub fn set_l2_cap_bytes_per_epoch(v: u64) {
    L2_CAP_BYTES_PER_EPOCH.store(v, AtomicOrdering::Relaxed);
}

pub fn set_bytes_per_sender_epoch_cap(v: u64) {
    BYTES_PER_SENDER_EPOCH_CAP.store(v, AtomicOrdering::Relaxed);
}

pub fn l2_cap_bytes_per_epoch() -> u64 {
    L2_CAP_BYTES_PER_EPOCH.load(AtomicOrdering::Relaxed)
}

pub fn bytes_per_sender_epoch_cap() -> u64 {
    BYTES_PER_SENDER_EPOCH_CAP.load(AtomicOrdering::Relaxed)
}

fn pipeline_dir() -> String {
    env::var("TB_STORAGE_PIPELINE_DIR").unwrap_or_else(|_| "blobstore".to_string())
}

fn load_manifest_from_store(hash: &[u8; 32]) -> Option<ObjectManifest> {
    let path = pipeline_dir();
    let db = SimpleDb::open_named(names::STORAGE_PIPELINE, &path);
    let key = format!("manifest/{}", crypto_suite::hex::encode(hash));
    let bytes = db.get(&key)?;
    decode_manifest(&bytes).ok()
}

fn normalize_providers(mut providers: Vec<String>) -> Vec<String> {
    providers.retain(|id| !id.is_empty());
    providers.sort();
    providers.dedup();
    providers
}

fn providers_from_manifest(manifest: &ObjectManifest) -> Vec<String> {
    let mut providers: Vec<String> = manifest
        .provider_chunks
        .iter()
        .map(|entry| entry.provider.clone())
        .collect();
    if providers.is_empty() {
        for chunk in &manifest.chunks {
            providers.extend(chunk.nodes.iter().cloned());
            providers.extend(
                chunk
                    .provider_chunks
                    .iter()
                    .map(|entry| entry.provider.clone()),
            );
        }
    }
    normalize_providers(providers)
}

fn select_provider(
    providers: &[String],
    manifest_hash: &[u8; 32],
    path_hash: &[u8; 32],
) -> Option<String> {
    if providers.is_empty() {
        return None;
    }
    if providers.len() == 1 {
        return Some(providers[0].clone());
    }
    let mut hasher = Hasher::new();
    hasher.update(manifest_hash);
    hasher.update(path_hash);
    let digest = hasher.finalize();
    let mut idx_bytes = [0u8; 8];
    idx_bytes.copy_from_slice(&digest.as_bytes()[..8]);
    let idx = (u64::from_le_bytes(idx_bytes) as usize) % providers.len();
    providers.get(idx).cloned()
}

pub fn provider_for_manifest(manifest: &[u8; 32], path_hash: &[u8; 32]) -> Option<String> {
    #[cfg(any(test, feature = "gateway-test-helpers"))]
    {
        if let Some(providers) = TEST_MANIFEST_PROVIDERS
            .lock()
            .unwrap()
            .get(manifest)
            .cloned()
        {
            let normalized = normalize_providers(providers);
            return select_provider(&normalized, manifest, path_hash);
        }
    }

    let manifest = load_manifest_from_store(manifest)?;
    let providers = providers_from_manifest(&manifest);
    let manifest_hash = manifest.blake3;
    select_provider(&providers, &manifest_hash, path_hash)
}

#[cfg(feature = "gateway")]
fn sanitize_component(value: &str) -> Option<&str> {
    if value.is_empty() || value == "." || value == ".." {
        return None;
    }
    if value.contains('/') || value.chars().any(|c| c == '\\') {
        return None;
    }
    Some(value)
}

#[cfg(feature = "gateway")]
fn sanitized_path(root: &Path, domain: &str, path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::from(root);
    out.push(sanitize_component(domain)?);
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || segment.chars().any(|c| c == '\\') {
            continue;
        }
        out.push(segment);
    }
    Some(out)
}

#[cfg(feature = "gateway")]
pub fn fetch_blob(domain: &str, path: &str) -> Option<Vec<u8>> {
    #[cfg(any(test, feature = "gateway-test-helpers"))]
    {
        if let Some(bytes) = TEST_GATEWAY_BLOBS
            .lock()
            .unwrap()
            .get(&(domain.to_string(), path.to_string()))
            .cloned()
        {
            return Some(bytes);
        }
    }

    let root = PathBuf::from(pipeline_dir()).join("gateway").join("static");
    let resolved = sanitized_path(&root, domain, path)?;
    fs::read(resolved).ok()
}

#[cfg(feature = "gateway")]
pub fn fetch_wasm(domain: &str) -> Option<Vec<u8>> {
    #[cfg(any(test, feature = "gateway-test-helpers"))]
    {
        if let Some(bytes) = TEST_GATEWAY_WASM.lock().unwrap().get(domain).cloned() {
            return Some(bytes);
        }
    }

    let sanitized = sanitize_component(domain)?;
    let mut path = PathBuf::from(pipeline_dir());
    path.push("gateway");
    path.push("wasm");
    path.push(sanitized);
    path.set_extension("wasm");
    fs::read(path).ok()
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
static TEST_GATEWAY_BLOBS: Lazy<Mutex<HashMap<(String, String), Vec<u8>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(any(test, feature = "gateway-test-helpers"))]
static TEST_GATEWAY_WASM: Lazy<Mutex<HashMap<String, Vec<u8>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(any(test, feature = "gateway-test-helpers"))]
pub fn override_static_blob_for_test(domain: &str, path: &str, data: Vec<u8>) {
    TEST_GATEWAY_BLOBS
        .lock()
        .unwrap()
        .insert((domain.to_string(), path.to_string()), data);
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
pub fn clear_test_static_blobs() {
    TEST_GATEWAY_BLOBS.lock().unwrap().clear();
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
pub fn override_wasm_for_test(domain: &str, data: Vec<u8>) {
    TEST_GATEWAY_WASM
        .lock()
        .unwrap()
        .insert(domain.to_string(), data);
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
pub fn clear_test_wasm() {
    TEST_GATEWAY_WASM.lock().unwrap().clear();
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
static TEST_MANIFEST_PROVIDERS: Lazy<Mutex<HashMap<[u8; 32], Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(any(test, feature = "gateway-test-helpers"))]
pub fn override_manifest_providers_for_test(manifest: [u8; 32], providers: Vec<String>) {
    let normalized = normalize_providers(providers);
    TEST_MANIFEST_PROVIDERS
        .lock()
        .unwrap()
        .insert(manifest, normalized);
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
pub fn clear_test_manifest_providers() {
    TEST_MANIFEST_PROVIDERS.lock().unwrap().clear();
}

/// RAII guard to automatically clear pipeline test state on drop.
/// Ensures test isolation by cleaning up even if a test panics.
#[cfg(any(test, feature = "gateway-test-helpers"))]
pub struct PipelineTestGuard;

#[cfg(any(test, feature = "gateway-test-helpers"))]
impl Drop for PipelineTestGuard {
    fn drop(&mut self) {
        clear_test_manifest_providers();
        #[cfg(feature = "gateway")]
        {
            clear_test_static_blobs();
            clear_test_wasm();
        }
    }
}

#[cfg(any(test, feature = "gateway-test-helpers"))]
impl PipelineTestGuard {
    /// Create a new guard that will clean up test state on drop.
    pub fn new() -> Self {
        // Clear any existing state before starting the test
        Self::cleanup();
        PipelineTestGuard
    }

    fn cleanup() {
        clear_test_manifest_providers();
        #[cfg(feature = "gateway")]
        {
            clear_test_static_blobs();
            clear_test_wasm();
        }
    }
}
