use super::erasure;
use super::placement::NodeCatalog;
use super::types::{ChunkRef, ObjectManifest, Redundancy, StoreReceipt};
use crate::compute_market::settlement::Settlement;
use crate::simple_db::SimpleDb;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_CHUNK_SIZE_BYTES, STORAGE_FINAL_CHUNK_SIZE, STORAGE_INITIAL_CHUNK_SIZE,
    STORAGE_PROVIDER_LOSS_RATE, STORAGE_PROVIDER_RTT_MS, STORAGE_PUT_CHUNK_SECONDS,
    STORAGE_PUT_ETA_SECONDS,
};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use credits::CreditError;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
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
}

impl StoragePipeline {
    pub fn open(path: &str) -> Self {
        if tokio::runtime::Handle::try_current().is_ok() {
            super::repair::spawn(path.to_string(), Duration::from_secs(60));
        }
        Self {
            db: SimpleDb::open(path),
        }
    }

    /// Logical quota in bytes derived from the provider's credit balance.
    /// Each credit grants one kilobyte of storage.
    pub fn logical_quota_bytes(provider: &str) -> u64 {
        Settlement::balance(provider) * 1024
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
    ) -> Result<StoreReceipt, String> {
        let kb = ((data.len() as u64) + 1023) / 1024;
        if let Err(e) = Settlement::spend(lane, "write_kb", kb) {
            return Err(match e {
                CreditError::Insufficient => "ERR_STORAGE_QUOTA_CREDITS".into(),
                _ => e.to_string(),
            });
        }
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

        let mut manifest = ObjectManifest {
            version: VERSION,
            total_len: data.len() as u64,
            chunk_len: chunk_len as u32,
            chunks,
            redundancy: Redundancy::ReedSolomon { data: 1, parity: 1 },
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
        let receipt = StoreReceipt {
            manifest_hash: man_hash,
            chunk_count: manifest.chunks.len() as u32,
            redundancy: Redundancy::ReedSolomon { data: 1, parity: 1 },
            lane: lane.to_string(),
        };
        let rec_bytes = bincode::serialize(&receipt).map_err(|e| e.to_string())?;
        self.db
            .try_insert(&format!("receipt/{}", hex::encode(man_hash)), rec_bytes)
            .map_err(|e| e.to_string())?;
        #[cfg(feature = "telemetry")]
        {
            STORAGE_FINAL_CHUNK_SIZE.set(profile.preferred_chunk as i64);
            if profile.bw_ewma > 0.0 {
                let eta = data.len() as f64 / profile.bw_ewma;
                STORAGE_PUT_ETA_SECONDS.set(eta as i64);
            }
        }
        Ok(receipt)
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
                for ch in manifest.chunks.iter() {
                    let blob = self
                        .db
                        .get(&format!("chunk/{}", hex::encode(ch.id)))
                        .ok_or("missing chunk")?;
                    if blob.len() < 12 {
                        return Err("corrupt chunk".into());
                    }
                    let (nonce_bytes, ct) = blob.split_at(12);
                    let nonce = Nonce::from_slice(nonce_bytes);
                    let plain = cipher
                        .decrypt(nonce, ct)
                        .map_err(|_| "decrypt fail".to_string())?;
                    out.extend_from_slice(&plain);
                }
            }
            Redundancy::ReedSolomon { data: d, parity: p } => {
                let step = (d + p) as usize;
                for group in manifest.chunks.chunks(step) {
                    let mut shards: Vec<Option<Vec<u8>>> = Vec::new();
                    for r in group {
                        let blob = self.db.get(&format!("chunk/{}", hex::encode(r.id)));
                        shards.push(blob);
                    }
                    let blob = erasure::reconstruct(shards)?;
                    if blob.len() < 12 {
                        return Err("corrupt chunk".into());
                    }
                    let (nonce_bytes, ct) = blob.split_at(12);
                    let nonce = Nonce::from_slice(nonce_bytes);
                    let plain = cipher
                        .decrypt(nonce, ct)
                        .map_err(|_| "decrypt fail".to_string())?;
                    out.extend_from_slice(&plain);
                }
            }
        }
        out.truncate(manifest.total_len as usize);
        Ok(out)
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
