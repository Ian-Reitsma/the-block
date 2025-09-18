use super::erasure;
use super::fs::RentEscrow;
use super::placement::NodeCatalog;
use super::types::{
    ChunkRef, ObjectManifest, Redundancy, StoreReceipt, CHACHA20_POLY1305_NONCE_LEN,
};
use crate::compute_market::settlement::Settlement;
use crate::simple_db::SimpleDb;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_CHUNK_SIZE_BYTES, STORAGE_FINAL_CHUNK_SIZE, STORAGE_INITIAL_CHUNK_SIZE,
    STORAGE_PROVIDER_LOSS_RATE, STORAGE_PROVIDER_RTT_MS, STORAGE_PUT_CHUNK_SECONDS,
    STORAGE_PUT_ETA_SECONDS, SUBSIDY_BYTES_TOTAL,
};
use crate::transaction::BlobTx;
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::sync::atomic::{AtomicU64, Ordering};
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
        }
    }
}

pub struct StoragePipeline {
    db: SimpleDb,
    rent: RentEscrow,
    rent_rate: i64,
}

impl StoragePipeline {
    pub fn open(path: &str) -> Self {
        if tokio::runtime::Handle::try_current().is_ok() {
            super::repair::spawn(path.to_string(), Duration::from_secs(60));
        }
        Self {
            db: SimpleDb::open(path),
            rent: RentEscrow::open(&format!("{path}/rent_escrow.db")),
            rent_rate: 0,
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
    /// Placeholder implementation until stake-backed quotas are implemented.
    pub fn logical_quota_bytes(_provider: &str) -> u64 {
        u64::MAX
    }

    fn profile_key(provider: &str) -> String {
        format!("provider_profiles/{}", provider)
    }

    fn load_profile(&self, provider: &str) -> ProviderProfile {
        let key = Self::profile_key(provider);
        self.db
            .get(&key)
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_else(ProviderProfile::new)
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

    pub fn put_object(
        &mut self,
        data: &[u8],
        lane: &str,
        catalog: &NodeCatalog,
    ) -> Result<(StoreReceipt, BlobTx), String> {
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
        let providers = catalog.healthy_nodes();
        if providers.is_empty() {
            return Err("no providers".into());
        }
        let mut profile = self.load_profile(providers[0].id());
        let chunk_len = profile.preferred_chunk as usize;
        #[cfg(feature = "telemetry")]
        STORAGE_INITIAL_CHUNK_SIZE.set(chunk_len as i64);

        let mut chunks = Vec::new();
        let mut offset = 0;
        let mut provider_idx = 0;
        while offset < data.len() {
            let end = (offset + chunk_len).min(data.len());
            let chunk = &data[offset..end];
            let mut nonce = [0u8; 12];
            OsRng.fill_bytes(&mut nonce);
            let nonce = Nonce::from_slice(&nonce);
            let start = Instant::now();
            let ciphertext = cipher.encrypt(nonce, chunk).map_err(|e| e.to_string())?;
            let mut blob = nonce.to_vec();
            blob.extend_from_slice(&ciphertext);
            let shards = erasure::encode(&blob)?;
            for (idx, shard) in shards.into_iter().enumerate() {
                let prov = &providers[provider_idx % providers.len()];
                prov.send_chunk(&shard)?;
                provider_idx += 1;
                let mut h = Hasher::new();
                h.update(&[idx as u8]);
                h.update(&shard);
                let id = *h.finalize().as_bytes();
                self.db
                    .try_insert(&format!("chunk/{}", hex::encode(id)), shard.clone())
                    .map_err(|e| e.to_string())?;
                chunks.push(ChunkRef {
                    id,
                    nodes: vec![prov.id().into()],
                });
            }
            let dur = start.elapsed();
            let throughput = chunk.len() as f64 / dur.as_secs_f64();
            profile.bw_ewma = Self::ewma(profile.bw_ewma, throughput);
            let (rtt, loss) = catalog.stats(providers[0].id());
            profile.rtt_ewma = Self::ewma(profile.rtt_ewma, rtt);
            profile.loss_ewma = Self::ewma(profile.loss_ewma, loss);
            profile.stable_chunks += 1;
            #[cfg(feature = "telemetry")]
            {
                STORAGE_CHUNK_SIZE_BYTES.observe(chunk.len() as f64);
                STORAGE_PUT_CHUNK_SECONDS.observe(dur.as_secs_f64());
                STORAGE_PROVIDER_RTT_MS.observe(rtt);
                STORAGE_PROVIDER_LOSS_RATE.observe(loss);
            }
            offset = end;
        }

        // Decide next chunk size using hysteresis
        let mut desired = Self::clamp_to_ladder(profile.bw_ewma * TARGET_TIME_SECS);
        let current = profile.preferred_chunk;
        let step_idx = CHUNK_LADDER.iter().position(|s| *s == current).unwrap_or(2);
        let desired_idx = CHUNK_LADDER
            .iter()
            .position(|s| *s == desired)
            .unwrap_or(step_idx);
        let diff = desired_idx as i32 - step_idx as i32;

        if profile.loss_ewma > LOSS_HI || profile.rtt_ewma > RTT_HI_MS {
            desired = CHUNK_LADDER[step_idx.saturating_sub(1)]
        } else if profile.loss_ewma < LOSS_LO && profile.rtt_ewma < RTT_LO_MS {
            // allow desired as computed
        } else {
            desired = current;
        }

        if desired != current && profile.stable_chunks >= 3 {
            if (diff.abs() as usize) >= 1 {
                profile.preferred_chunk = desired;
                profile.stable_chunks = 0;
            }
        }

        if let Ok(secs) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            profile.updated_at = secs.as_secs();
        }
        self.save_profile(providers[0].id(), &profile);

        let (rs_data, rs_parity) = erasure::reed_solomon_counts();
        let rs_data_u8 = u8::try_from(rs_data).map_err(|_| "invalid data shard count")?;
        let rs_parity_u8 = u8::try_from(rs_parity).map_err(|_| "invalid parity shard count")?;

        let mut manifest = ObjectManifest {
            version: VERSION,
            total_len: data.len() as u64,
            chunk_len: chunk_len as u32,
            chunks,
            redundancy: Redundancy::ReedSolomon {
                data: rs_data_u8,
                parity: rs_parity_u8,
            },
            content_key_enc: key_bytes.to_vec(),
            blake3: [0u8; 32],
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
            STORAGE_FINAL_CHUNK_SIZE.set(profile.preferred_chunk as i64);
            if profile.bw_ewma > 0.0 {
                let eta = data.len() as f64 / profile.bw_ewma;
                STORAGE_PUT_ETA_SECONDS.set(eta as i64);
            }
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
    use crate::storage::repair;
    use crate::storage::types::ObjectManifest;
    use tempfile::tempdir;

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
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("pipeline-db");
        let path_str = path.to_str().expect("path str");
        let mut pipeline = StoragePipeline::open(path_str);
        let catalog = catalog_with_stub();

        let data = sample_blob(1_200_000);
        let (receipt, _) = pipeline
            .put_object(&data, "lane", &catalog)
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
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("repair-db");
        let path_str = path.to_str().expect("path str");
        let mut pipeline = StoragePipeline::open(path_str);
        let catalog = catalog_with_stub();

        let data = sample_blob(1_000_000);
        let (receipt, _) = pipeline
            .put_object(&data, "lane", &catalog)
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

        repair::run_once(pipeline.db_mut()).expect("repair");

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
    L2_CAP_BYTES_PER_EPOCH.store(v, Ordering::Relaxed);
}

pub fn set_bytes_per_sender_epoch_cap(v: u64) {
    BYTES_PER_SENDER_EPOCH_CAP.store(v, Ordering::Relaxed);
}
