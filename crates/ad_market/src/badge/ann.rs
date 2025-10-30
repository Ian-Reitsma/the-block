use crypto_suite::encryption::symmetric::{decrypt_aes256_cbc, encrypt_aes256_cbc};
use crypto_suite::hashing::blake3;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashSet;

const AES_KEY_LEN: usize = 32;
const AES_BLOCK_LEN: usize = 16;
const HASH_LEN: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WalletAnnIndexSnapshot {
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub fingerprint: Vec<u8>,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub bucket_hashes: Vec<u8>,
    pub dimensions: u16,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub entropy_salt: Vec<u8>,
}

impl WalletAnnIndexSnapshot {
    pub fn new(
        fingerprint: [u8; HASH_LEN],
        bucket_hashes: Vec<[u8; HASH_LEN]>,
        dimensions: u16,
    ) -> Self {
        let mut flattened = Vec::with_capacity(bucket_hashes.len() * HASH_LEN);
        for hash in bucket_hashes {
            flattened.extend_from_slice(&hash);
        }
        Self {
            fingerprint: fingerprint.to_vec(),
            bucket_hashes: flattened,
            dimensions,
            entropy_salt: Vec::new(),
        }
    }

    pub fn bucket_iter(&self) -> impl Iterator<Item = &[u8]> {
        self.bucket_hashes.chunks(HASH_LEN)
    }

    pub fn bucket_count(&self) -> usize {
        self.bucket_hashes.len() / HASH_LEN
    }

    pub fn with_entropy_salt(mut self, salt: impl Into<Vec<u8>>) -> Self {
        self.entropy_salt = salt.into();
        self
    }

    pub fn entropy_salt(&self) -> &[u8] {
        &self.entropy_salt
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct EncryptedAnnProof {
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub ciphertext: Vec<u8>,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub iv: Vec<u8>,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub neighbor_fingerprint: Vec<u8>,
    pub distance_ppm: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SoftIntentReceipt {
    pub proof: EncryptedAnnProof,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub index_fingerprint: Vec<u8>,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub wallet_entropy: Vec<u8>,
}

pub fn build_proof(
    snapshot: &WalletAnnIndexSnapshot,
    badges: &[String],
) -> Option<SoftIntentReceipt> {
    build_proof_with_entropy(snapshot, badges, None)
}

pub fn build_proof_with_entropy(
    snapshot: &WalletAnnIndexSnapshot,
    badges: &[String],
    wallet_entropy: Option<&[u8]>,
) -> Option<SoftIntentReceipt> {
    if snapshot.bucket_count() == 0 || snapshot.fingerprint.len() != HASH_LEN {
        return None;
    }
    let query_hash = hash_badges(badges);
    let neighbor = nearest_neighbor(snapshot, &query_hash)?;
    let wallet_entropy = wallet_entropy
        .map(|bytes| bytes.to_vec())
        .unwrap_or_default();
    let key = derive_key(snapshot, &wallet_entropy);
    let iv = derive_iv(snapshot, &wallet_entropy, &query_hash);
    let ciphertext = encrypt_aes256_cbc(&key, &iv, query_hash.as_slice());
    let distance_ppm = compute_distance_ppm(&query_hash, neighbor);
    Some(SoftIntentReceipt {
        proof: EncryptedAnnProof {
            ciphertext,
            iv: iv.to_vec(),
            neighbor_fingerprint: neighbor.to_vec(),
            distance_ppm,
        },
        index_fingerprint: snapshot.fingerprint.clone(),
        wallet_entropy,
    })
}

pub fn verify_receipt(
    snapshot: &WalletAnnIndexSnapshot,
    receipt: &SoftIntentReceipt,
    badges: &[String],
) -> bool {
    if snapshot.fingerprint != receipt.index_fingerprint {
        return false;
    }
    let proof = &receipt.proof;
    if proof.ciphertext.is_empty()
        || proof.iv.len() != AES_BLOCK_LEN
        || proof.neighbor_fingerprint.len() != HASH_LEN
    {
        return false;
    }
    let query_hash = hash_badges(badges);
    let key = derive_key(snapshot, &receipt.wallet_entropy);
    let expected_iv = derive_iv(snapshot, &receipt.wallet_entropy, &query_hash);
    if proof.iv.as_slice() != expected_iv.as_slice() {
        return false;
    }
    let decrypted = match decrypt_aes256_cbc(&key, &expected_iv, &proof.ciphertext) {
        Ok(plaintext) => plaintext,
        Err(_) => return false,
    };
    if decrypted.as_slice() != query_hash.as_slice() {
        return false;
    }
    if !snapshot
        .bucket_iter()
        .any(|bucket| bucket == proof.neighbor_fingerprint.as_slice())
    {
        return false;
    }
    compute_distance_ppm(&query_hash, proof.neighbor_fingerprint.as_slice()) == proof.distance_ppm
}

pub fn hash_badges(badges: &[String]) -> [u8; HASH_LEN] {
    let mut hasher = blake3::Hasher::new();
    let mut dedup: HashSet<&str> = HashSet::with_capacity(badges.len());
    for badge in badges {
        dedup.insert(badge.as_str());
    }
    let mut sorted: Vec<&str> = dedup.into_iter().collect();
    sorted.sort_unstable();
    for badge in sorted {
        hasher.update(badge.as_bytes());
        hasher.update(&[0xff]);
    }
    hasher.finalize().into()
}

fn nearest_neighbor<'a>(
    snapshot: &'a WalletAnnIndexSnapshot,
    query: &[u8; HASH_LEN],
) -> Option<&'a [u8]> {
    let mut best: Option<(&[u8], u32)> = None;
    for bucket in snapshot.bucket_iter() {
        let distance = hamming_distance(query, bucket) as u32;
        match best {
            Some((_, current)) if distance >= current => continue,
            _ => best = Some((bucket, distance)),
        }
    }
    best.map(|(bucket, _)| bucket)
}

fn compute_distance_ppm(lhs: &[u8; HASH_LEN], rhs: &[u8]) -> u32 {
    let distance = hamming_distance(lhs, rhs) as f64;
    ((distance / (HASH_LEN as f64 * 8.0)) * 1_000_000.0).round() as u32
}

fn hamming_distance(lhs: &[u8; HASH_LEN], rhs: &[u8]) -> usize {
    lhs.iter()
        .zip(rhs.iter())
        .map(|(a, b)| (a ^ b).count_ones() as usize)
        .sum()
}

fn derive_key(snapshot: &WalletAnnIndexSnapshot, wallet_entropy: &[u8]) -> [u8; AES_KEY_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&snapshot.fingerprint);
    if !snapshot.entropy_salt.is_empty() {
        hasher.update(&snapshot.entropy_salt);
    }
    if !wallet_entropy.is_empty() {
        hasher.update(wallet_entropy);
    }
    hasher.update(b"ann-key");
    let digest = hasher.finalize();
    let mut key = [0u8; AES_KEY_LEN];
    key.copy_from_slice(&digest.as_bytes()[..AES_KEY_LEN]);
    key
}

fn derive_iv(
    snapshot: &WalletAnnIndexSnapshot,
    wallet_entropy: &[u8],
    query_hash: &[u8; HASH_LEN],
) -> [u8; AES_BLOCK_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&snapshot.fingerprint);
    if !snapshot.entropy_salt.is_empty() {
        hasher.update(&snapshot.entropy_salt);
    }
    if !wallet_entropy.is_empty() {
        hasher.update(wallet_entropy);
    }
    hasher.update(query_hash);
    let digest = hasher.finalize();
    let mut iv = [0u8; AES_BLOCK_LEN];
    iv.copy_from_slice(&digest.as_bytes()[..AES_BLOCK_LEN]);
    iv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> WalletAnnIndexSnapshot {
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let query = hash_badges(&badges);
        WalletAnnIndexSnapshot::new(
            [0xAA; HASH_LEN],
            vec![query, [0x11; HASH_LEN], [0x22; HASH_LEN]],
            16,
        )
    }

    #[test]
    fn build_and_verify_round_trip() {
        let snapshot = sample_snapshot();
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let receipt = build_proof(&snapshot, &badges).expect("soft intent receipt");
        assert!(verify_receipt(&snapshot, &receipt, &badges));
    }

    #[test]
    fn verify_rejects_tampered_proof() {
        let snapshot = sample_snapshot();
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let mut receipt = build_proof(&snapshot, &badges).expect("soft intent receipt");
        receipt.proof.ciphertext[0] ^= 0xFF;
        assert!(!verify_receipt(&snapshot, &receipt, &badges));
    }

    #[test]
    fn entropy_salt_changes_ciphertext() {
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let base = sample_snapshot();
        let salted = base.clone().with_entropy_salt(vec![0x55; HASH_LEN]);
        let unsalted_receipt = build_proof(&base, &badges).expect("unsalted proof");
        let salted_receipt = build_proof(&salted, &badges).expect("salted proof");
        assert_ne!(
            unsalted_receipt.proof.ciphertext,
            salted_receipt.proof.ciphertext
        );
        assert!(verify_receipt(&base, &unsalted_receipt, &badges));
        assert!(verify_receipt(&salted, &salted_receipt, &badges));
    }

    #[test]
    fn wallet_entropy_influences_ciphertext_and_iv() {
        let snapshot = sample_snapshot();
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let entropy_a = [0xABu8; 32];
        let entropy_b = [0xCDu8; 32];
        let receipt_a = build_proof_with_entropy(&snapshot, &badges, Some(&entropy_a))
            .expect("receipt with entropy");
        let receipt_b = build_proof_with_entropy(&snapshot, &badges, Some(&entropy_b))
            .expect("receipt with alternate entropy");
        assert_ne!(receipt_a.proof.ciphertext, receipt_b.proof.ciphertext);
        assert_ne!(receipt_a.proof.iv, receipt_b.proof.iv);
        assert_eq!(receipt_a.wallet_entropy, entropy_a);
        assert_eq!(receipt_b.wallet_entropy, entropy_b);
        assert!(verify_receipt(&snapshot, &receipt_a, &badges));
        assert!(verify_receipt(&snapshot, &receipt_b, &badges));

        let mut tampered = receipt_a.clone();
        tampered.wallet_entropy = entropy_b.to_vec();
        assert!(!verify_receipt(&snapshot, &tampered, &badges));
    }
}
