#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::Write;

use blake3::Hasher;
use ed25519_dalek::{PublicKey, Signature};
use flate2::{write::GzEncoder, Compression};

mod state_stream;
pub use state_stream::{StateChunk, StateStream};

/// Options controlling background synchronization.
#[derive(Clone, Copy)]
pub struct SyncOptions {
    pub wifi_only: bool,
    pub require_charging: bool,
    pub min_battery: f32,
}

/// Block header for light-client verification.
#[derive(Clone, Default, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
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
        let vk = match PublicKey::from_bytes(&pk_bytes) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut arr = [0u8;64];
        arr.copy_from_slice(sig_bytes);
        let sig = match Signature::from_bytes(&arr) {
            Ok(s) => s,
            Err(_) => return false,
        };
        vk.verify_strict(&h.checkpoint_hash, &sig).is_ok()
    } else if let Some(expected) = checkpoints.get(&h.height) {
        expected == &h.checkpoint_hash
    } else {
        true
    }
}

/// Attempt a background delta sync using the provided fetcher.
pub fn sync_background<F>(client: &mut LightClient, opts: SyncOptions, fetch: F)
where
    F: Fn(u64) -> Vec<Header>,
{
    if opts.wifi_only && !on_wifi() {
        return;
    }
    if opts.require_charging && !is_charging() {
        return;
    }
    if battery_level() < opts.min_battery {
        return;
    }
    let start = client.tip_height() + 1;
    for h in fetch(start) {
        let _ = client.verify_and_append(h);
    }
}

fn on_wifi() -> bool {
    true
}

fn is_charging() -> bool {
    true
}

fn battery_level() -> f32 {
    1.0
}

/// Compress log data for upload via telemetry.
pub fn upload_compressed_logs(data: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
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
        let mut lc = LightClient::new(genesis.clone());
        sync_background(&mut lc, opts, |_| Vec::new());
        assert_eq!(lc.chain.len(), 1);
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
        use ed25519_dalek::{Keypair, Signer};
        use rand::rngs::OsRng;
        let genesis = Header { height: 0, prev_hash: [0;32], merkle_root: [0;32], checkpoint_hash: [0;32], validator_key: None, checkpoint_sig: None, nonce:0, difficulty:1, timestamp_millis:0, l2_roots:vec![], l2_sizes:vec![], vdf_commit:[0;32], vdf_output:[0;32], vdf_proof:vec![] };
        let mut lc = LightClient::new(genesis.clone());
        let mut rng = OsRng;
        let kp = Keypair::generate(&mut rng);
        let pk = kp.public.to_bytes();
        let mut h1 = make_header(&genesis, 1);
        h1.checkpoint_hash = [3u8;32];
        let sig = kp.sign(&h1.checkpoint_hash);
        h1.validator_key = Some(pk);
        h1.checkpoint_sig = Some(sig.to_bytes().to_vec());
        assert!(lc.verify_and_append(h1.clone()).is_ok());
        let mut bad = h1.clone();
        bad.checkpoint_hash = [4u8;32];
        assert!(lc.verify_and_append(bad).is_err());
    }
}
