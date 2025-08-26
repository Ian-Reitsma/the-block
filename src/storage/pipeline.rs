use super::types::{ChunkRef, ObjectManifest, Redundancy, StoreReceipt};
use crate::simple_db::SimpleDb;
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::{rngs::OsRng, RngCore};

const VERSION: u16 = 1;
const DEFAULT_CHUNK: usize = 1024 * 1024; // 1 MiB

pub struct StoragePipeline {
    db: SimpleDb,
}

impl StoragePipeline {
    pub fn open(path: &str) -> Self {
        Self {
            db: SimpleDb::open(path),
        }
    }

    pub fn put_object(&mut self, data: &[u8], lane: &str) -> Result<StoreReceipt, String> {
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key = Key::from_slice(&key_bytes);
        let cipher = ChaCha20Poly1305::new(key);

        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + DEFAULT_CHUNK).min(data.len());
            let chunk = &data[offset..end];
            let mut nonce = [0u8; 12];
            OsRng.fill_bytes(&mut nonce);
            let nonce = Nonce::from_slice(&nonce);
            let ciphertext = cipher.encrypt(nonce, chunk).map_err(|e| e.to_string())?;
            let mut blob = nonce.to_vec();
            blob.extend_from_slice(&ciphertext);
            let mut h = Hasher::new();
            h.update(&blob);
            let id = *h.finalize().as_bytes();
            self.db
                .insert(&format!("chunk/{}", hex::encode(id)), blob.clone());
            chunks.push(ChunkRef {
                id,
                nodes: vec!["local".into()],
            });
            offset = end;
        }
        let mut manifest = ObjectManifest {
            version: VERSION,
            total_len: data.len() as u64,
            chunk_len: DEFAULT_CHUNK as u32,
            chunks,
            redundancy: Redundancy::None,
            content_key_enc: key_bytes.to_vec(),
            blake3: [0u8; 32],
        };
        let mut h = Hasher::new();
        let manifest_bytes_temp = bincode::serialize(&manifest).map_err(|e| e.to_string())?;
        h.update(&manifest_bytes_temp);
        let man_hash = *h.finalize().as_bytes();
        manifest.blake3 = man_hash;
        let manifest_bytes = bincode::serialize(&manifest).map_err(|e| e.to_string())?;
        self.db.insert(
            &format!("manifest/{}", hex::encode(man_hash)),
            manifest_bytes,
        );
        let receipt = StoreReceipt {
            manifest_hash: man_hash,
            chunk_count: manifest.chunks.len() as u32,
            redundancy: Redundancy::None,
            lane: lane.to_string(),
        };
        let rec_bytes = bincode::serialize(&receipt).map_err(|e| e.to_string())?;
        self.db
            .insert(&format!("receipt/{}", hex::encode(man_hash)), rec_bytes);
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
        Ok(out)
    }
}
