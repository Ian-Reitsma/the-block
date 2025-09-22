use super::erasure;
use super::fs::RentEscrow;
use super::placement::{NodeCatalog, NodeHandle};
use super::repair;
use super::types::{
    ChunkRef, ObjectManifest, ProviderChunkEntry, Redundancy, StoreReceipt,
    CHACHA20_POLY1305_NONCE_LEN,
};
use crate::compute_market::settlement::Settlement;
use crate::simple_db::SimpleDb;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    MemoryComponent, STORAGE_CHUNK_SIZE_BYTES, STORAGE_FINAL_CHUNK_SIZE,
    STORAGE_INITIAL_CHUNK_SIZE, STORAGE_PROVIDER_LOSS_RATE, STORAGE_PROVIDER_RTT_MS,
    STORAGE_PUT_CHUNK_SECONDS, STORAGE_PUT_ETA_SECONDS, STORAGE_PUT_OBJECT_SECONDS,
    SUBSIDY_BYTES_TOTAL,
};
use crate::transaction::BlobTx;
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

const VERSION: u16 = 1;
const DEFAULT_CHUNK: u32 = 1024 * 1024; // 1 MiB
const CHUNK_LADDER: [u32; 5] = [
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

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn send_chunk(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn probe(&self) -> Result<f64, String> {
        Ok(0.0)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProviderProfile {
    pub bw_ewma: f64,
    pub rtt_ewma: f64,
    pub loss_ewma: f64,
    pub preferred_chunk: u32,
    pub stable_chunks: u32,
    pub updated_at: u64,
    #[serde(default)]
    pub success_rate_ewma: f64,
    #[serde(default)]
    pub recent_failures: u32,
    #[serde(default)]
    pub total_chunks: u64,
    #[serde(default)]
    pub total_failures: u64,
    #[serde(default)]
    pub last_upload_bytes: u64,
    #[serde(default)]
    pub last_upload_secs: f64,
    #[serde(default)]
    pub maintenance: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProviderProfileSnapshot {
    pub provider: String,
    pub profile: ProviderProfile,
    pub quota_bytes: u64,
}

impl ProviderProfile {
    fn new() -> Self {
        Self {
            bw_ewma: 0.0,
            rtt_ewma: 0.0,
            loss_ewma: 0.0,
            preferred_chunk: DEFAULT_CHUNK,
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
            self.preferred_chunk = DEFAULT_CHUNK;
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
            if self.preferred_chunk > CHUNK_LADDER[0] {
                let idx = CHUNK_LADDER
                    .iter()
                    .position(|s| *s == self.preferred_chunk)
                    .unwrap_or(0);
                let downgraded_idx = idx.saturating_sub(1);
                self.preferred_chunk = CHUNK_LADDER[downgraded_idx];
            }
        }
        if let Ok(secs) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            self.updated_at = secs.as_secs();
        }
    }

    fn adjust_preferred_chunk(&mut self) {
        let mut desired = StoragePipeline::clamp_to_ladder(self.bw_ewma * TARGET_TIME_SECS);
        let current = self.preferred_chunk;
        let step_idx = CHUNK_LADDER.iter().position(|s| *s == current).unwrap_or(2);
        let desired_idx = CHUNK_LADDER
            .iter()
            .position(|s| *s == desired)
            .unwrap_or(step_idx);
        let diff = desired_idx as i32 - step_idx as i32;

        if self.loss_ewma > LOSS_HI || self.rtt_ewma > RTT_HI_MS {
            desired = CHUNK_LADDER[step_idx.saturating_sub(1)];
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
        if tokio::runtime::Handle::try_current().is_ok() {
            super::repair::spawn(path.to_string(), Duration::from_secs(60));
        }
        let repair_log_dir = PathBuf::from(path).join("repair_log");
        Self {
            db: SimpleDb::open(path),
            rent: RentEscrow::open(&format!("{path}/rent_escrow.db")),
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
        OsRng.fill_bytes(&mut blob_id);
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
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_else(ProviderProfile::new);
        profile.ensure_defaults();
        profile
    }

    fn save_profile(&mut self, provider: &str, profile: &ProviderProfile) {
        let key = Self::profile_key(provider);
        if let Ok(bytes) = bincode::serialize(profile) {
            let _ = self.db.try_insert(&key, bytes);
        }
    }

    pub fn get_profile(&self, provider: &str) -> Option<ProviderProfile> {
        let key = Self::profile_key(provider);
        self.db
            .get(&key)
            .and_then(|b| bincode::deserialize(&b).ok())
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

    fn clamp_to_ladder(bytes: f64) -> u32 {
        let mut chosen = CHUNK_LADDER[0];
        for step in CHUNK_LADDER.iter() {
            if *step as f64 <= bytes {
                chosen = *step;
            }
        }
        chosen
    }

    fn ewma(prev: f64, new: f64) -> f64 {
        if prev == 0.0 {
            new
        } else {
            prev * 0.8 + new * 0.2
        }
    }

    fn quota_for(provider: &str) -> u64 {
        let (ct, industrial) = Settlement::balance_split(provider);
        ct.saturating_add(industrial)
            .saturating_mul(QUOTA_BYTES_PER_CREDIT)
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
            remaining.min(DEFAULT_CHUNK as usize)
        } else {
            best
        }
    }

    fn previous_chunk_step(current: usize) -> usize {
        if current <= 1 {
            return 0;
        }
        if let Some(step) = CHUNK_LADDER
            .iter()
            .rev()
            .find(|&&step| (step as usize) < current)
        {
            *step as usize
        } else {
            std::cmp::max(1, current / 2)
        }
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
                            .try_insert(&format!("chunk/{}", hex::encode(id)), shard.clone())
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
                                .with_label_values(&[provider_id.as_str()])
                                .observe(rtt);
                            STORAGE_PROVIDER_LOSS_RATE
                                .with_label_values(&[provider_id.as_str()])
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
                            tracing::warn!(%err, provider = %provider_id, "storage shard send failed");
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

        let mut data_hasher = Hasher::new();
        data_hasher.update(data);
        let blob_root: [u8; 32] = data_hasher.finalize().into();
        let mut blob_id = [0u8; 32];
        OsRng.fill_bytes(&mut blob_id);
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key = Key::from_slice(&key_bytes);
        let cipher = ChaCha20Poly1305::new(key);

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
                .unwrap_or(DEFAULT_CHUNK);
            STORAGE_INITIAL_CHUNK_SIZE.set(initial as i64);
        }

        let mut chunks = Vec::new();
        let mut chunk_lens: Vec<u32> = Vec::new();
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
                let mut nonce_bytes = [0u8; 12];
                OsRng.fill_bytes(&mut nonce_bytes);
                let nonce = Nonce::from_slice(&nonce_bytes);
                #[cfg(feature = "telemetry")]
                let chunk_start = Instant::now();
                let ciphertext = cipher.encrypt(nonce, chunk).map_err(|e| e.to_string())?;
                let mut blob = nonce_bytes.to_vec();
                blob.extend_from_slice(&ciphertext);
                let shards = erasure::encode(&blob)?;

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
                            STORAGE_PUT_CHUNK_SECONDS.observe(chunk_start.elapsed().as_secs_f64());
                        }
                        chunk_lens.push(chunk.len() as u32);
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

        let (rs_data, rs_parity) = erasure::reed_solomon_counts();
        let rs_data_u8 = u8::try_from(rs_data).map_err(|_| "invalid data shard count")?;
        let rs_parity_u8 = u8::try_from(rs_parity).map_err(|_| "invalid parity shard count")?;

        let mut manifest = ObjectManifest {
            version: VERSION,
            total_len: data.len() as u64,
            chunk_len: chunk_lens.first().copied().unwrap_or(DEFAULT_CHUNK) as u32,
            chunks,
            redundancy: Redundancy::ReedSolomon {
                data: rs_data_u8,
                parity: rs_parity_u8,
            },
            content_key_enc: key_bytes.to_vec(),
            blake3: [0u8; 32],
            chunk_lens,
            provider_chunks: provider_chunk_index.values().cloned().collect(),
        };
        let mut h = Hasher::new();
        let manifest_bytes_temp = bincode::serialize(&manifest).map_err(|e| e.to_string())?;
        h.update(&manifest_bytes_temp);
        let man_hash = *h.finalize().as_bytes();
        manifest.blake3 = man_hash;
        let manifest_bytes = bincode::serialize(&manifest).map_err(|e| e.to_string())?;
        self.db
            .try_insert(
                &format!("manifest/{}", hex::encode(man_hash)),
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
        let rec_bytes = bincode::serialize(&receipt).map_err(|e| e.to_string())?;
        self.db
            .try_insert(&format!("receipt/{}", hex::encode(man_hash)), rec_bytes)
            .map_err(|e| e.to_string())?;
        self.rent.lock(&hex::encode(man_hash), lane, rent, 0);
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
            .with_label_values(&["storage"])
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
            crate::telemetry::sampled_observe(
                &STORAGE_PUT_OBJECT_SECONDS,
                telemetry_start.elapsed().as_secs_f64(),
            );
            crate::telemetry::update_memory_usage(MemoryComponent::Storage);
        }
        Ok((receipt, blob_tx))
    }

    pub fn get_object(&self, manifest_hash: &[u8; 32]) -> Result<Vec<u8>, String> {
        let key = format!("manifest/{}", hex::encode(manifest_hash));
        let manifest_bytes = self.db.get(&key).ok_or("missing manifest")?;
        let manifest: ObjectManifest =
            bincode::deserialize(&manifest_bytes).map_err(|e| e.to_string())?;
        let key_bytes = manifest.content_key_enc.clone();
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
        let mut out = Vec::with_capacity(manifest.total_len as usize);
        match manifest.redundancy {
            Redundancy::None => {
                for (idx, ch) in manifest.chunks.iter().enumerate() {
                    let blob = self
                        .db
                        .get(&format!("chunk/{}", hex::encode(ch.id)))
                        .ok_or("missing chunk")?;
                    if blob.len() < CHACHA20_POLY1305_NONCE_LEN {
                        return Err("corrupt chunk".into());
                    }
                    let (nonce_bytes, ct) = blob.split_at(CHACHA20_POLY1305_NONCE_LEN);
                    let nonce = Nonce::from_slice(nonce_bytes);
                    let mut plain = cipher
                        .decrypt(nonce, ct)
                        .map_err(|_| "decrypt fail".to_string())?;
                    let expected = manifest.chunk_plain_len(idx);
                    if plain.len() < expected {
                        return Err("corrupt chunk".into());
                    }
                    plain.truncate(expected);
                    out.extend_from_slice(&plain);
                }
            }
            Redundancy::ReedSolomon { .. } => {
                let shards_per_chunk = erasure::total_shards_per_chunk();
                if shards_per_chunk == 0 {
                    return Err("invalid shard layout".into());
                }
                if manifest.chunks.len() % shards_per_chunk != 0 {
                    return Err("corrupt manifest".into());
                }
                for (chunk_idx, group) in manifest.chunks.chunks(shards_per_chunk).enumerate() {
                    let mut shards = vec![None; shards_per_chunk];
                    for (slot, r) in group.iter().enumerate() {
                        let blob = self.db.get(&format!("chunk/{}", hex::encode(r.id)));
                        shards[slot] = blob;
                    }
                    let expected_cipher = manifest.chunk_cipher_len(chunk_idx);
                    let blob = erasure::reconstruct(shards, expected_cipher)?;
                    if blob.len() < CHACHA20_POLY1305_NONCE_LEN {
                        return Err("corrupt chunk".into());
                    }
                    let (nonce_bytes, ct) = blob.split_at(CHACHA20_POLY1305_NONCE_LEN);
                    let nonce = Nonce::from_slice(nonce_bytes);
                    let mut plain = cipher
                        .decrypt(nonce, ct)
                        .map_err(|_| "decrypt fail".to_string())?;
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
            .with_label_values(&["read"])
            .inc_by(out.len() as u64);
        Ok(out)
    }

    pub fn delete_object(&mut self, manifest_hash: &[u8; 32]) -> Result<u64, String> {
        let key = format!("manifest/{}", hex::encode(manifest_hash));
        let _ = self.db.remove(&key);
        let id = hex::encode(manifest_hash);
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
        let key = format!("manifest/{}", hex::encode(manifest_hash));
        self.db
            .get(&key)
            .and_then(|b| bincode::deserialize(&b).ok())
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
    use crate::storage::types::ObjectManifest;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tempfile::tempdir;

    static SETTLEMENT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct SettlementGuard {
        _lock: MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
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
        let key = format!("manifest/{}", hex::encode(hash));
        let bytes = {
            let db = pipeline.db();
            db.get(&key).expect("manifest present")
        };
        bincode::deserialize(&bytes).expect("manifest decode")
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
            let key = format!("chunk/{}", hex::encode(shard_id));
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
            let key = format!("chunk/{}", hex::encode(shard_id));
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
}

static L2_CAP_BYTES_PER_EPOCH: AtomicU64 = AtomicU64::new(33_554_432);
static BYTES_PER_SENDER_EPOCH_CAP: AtomicU64 = AtomicU64::new(16_777_216);

pub fn set_l2_cap_bytes_per_epoch(v: u64) {
    L2_CAP_BYTES_PER_EPOCH.store(v, AtomicOrdering::Relaxed);
}

pub fn set_bytes_per_sender_epoch_cap(v: u64) {
    BYTES_PER_SENDER_EPOCH_CAP.store(v, AtomicOrdering::Relaxed);
}
