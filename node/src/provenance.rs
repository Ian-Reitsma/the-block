use blake3::hash;
use ed25519_dalek::{Signature, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use hex;
use once_cell::sync::Lazy;
use std::convert::TryInto;
use std::fs;
use std::path::Path;
use std::sync::{RwLock, RwLockReadGuard};

#[cfg(feature = "telemetry")]
use crate::telemetry::{BUILD_PROVENANCE_INVALID_TOTAL, BUILD_PROVENANCE_VALID_TOTAL};

const RELEASE_SIGNERS_ENV: &str = "TB_RELEASE_SIGNERS";
const RELEASE_SIGNERS_FILE_ENV: &str = "TB_RELEASE_SIGNERS_FILE";
const DEFAULT_RELEASE_SIGNERS_PATH: &str = "config/release_signers.txt";

static RELEASE_SIGNERS: Lazy<RwLock<Vec<VerifyingKey>>> =
    Lazy::new(|| RwLock::new(load_release_signers()));

/// Verify the hash of the current executable against the embedded build hash.
pub fn verify_self() -> bool {
    let expected = env!("BUILD_BIN_HASH");
    if let Ok(path) = std::env::current_exe() {
        let ok = verify_file(&path, expected);
        if ok {
            #[cfg(feature = "telemetry")]
            BUILD_PROVENANCE_VALID_TOTAL.inc();
        } else {
            #[cfg(feature = "telemetry")]
            BUILD_PROVENANCE_INVALID_TOTAL.inc();
        }
        ok
    } else {
        #[cfg(feature = "telemetry")]
        BUILD_PROVENANCE_INVALID_TOTAL.inc();
        false
    }
}

/// Verify that `path` hashes to `expected` (hex).
pub fn verify_file(path: &Path, expected: &str) -> bool {
    match fs::read(path) {
        Ok(bytes) => hash(&bytes).to_hex().to_string() == expected,
        Err(_) => false,
    }
}

fn load_release_signers() -> Vec<VerifyingKey> {
    if let Ok(env) = std::env::var(RELEASE_SIGNERS_ENV) {
        if let Some(signers) = parse_signer_list(&env) {
            return signers;
        }
    }
    if let Ok(path) = std::env::var(RELEASE_SIGNERS_FILE_ENV) {
        if let Some(signers) = load_signers_from_path(Path::new(&path)) {
            return signers;
        }
    }
    load_signers_from_path(Path::new(DEFAULT_RELEASE_SIGNERS_PATH)).unwrap_or_default()
}

fn load_signers_from_path(path: &Path) -> Option<Vec<VerifyingKey>> {
    let contents = fs::read_to_string(path).ok()?;
    parse_signer_list(&contents)
}

fn parse_signer_list(input: &str) -> Option<Vec<VerifyingKey>> {
    let mut out = Vec::new();
    for token in input
        .split(|c| c == ',' || c == '\n' || c == '\r')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if let Ok(bytes) = hex::decode(token) {
            if bytes.len() == PUBLIC_KEY_LENGTH {
                if let Ok(arr) = bytes.as_slice().try_into() {
                    if let Ok(vk) = VerifyingKey::from_bytes(&arr) {
                        out.push(vk);
                    }
                }
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn decode_signature(sig_hex: &str) -> Option<Signature> {
    let bytes = hex::decode(sig_hex).ok()?;
    if bytes.len() != SIGNATURE_LENGTH {
        return None;
    }
    let mut arr = [0u8; SIGNATURE_LENGTH];
    arr.copy_from_slice(&bytes);
    Some(Signature::from_bytes(&arr))
}

/// Refresh the cached release signer list from the configured sources.
pub fn refresh_release_signers() {
    let mut guard = RELEASE_SIGNERS.write().expect("release signer lock");
    *guard = load_release_signers();
}

fn signer_list() -> RwLockReadGuard<'static, Vec<VerifyingKey>> {
    RELEASE_SIGNERS.read().expect("release signer lock")
}

/// Returns true when provenance signatures are required for release proposals.
pub fn release_signature_required() -> bool {
    !signer_list().is_empty()
}

/// Verify a release proposal signature against the configured signers.
pub fn verify_release_signature(build_hash: &str, signature_hex: &str) -> bool {
    let normalized = build_hash.trim().to_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let signature = match decode_signature(signature_hex) {
        Some(sig) => sig,
        None => return false,
    };
    let message = format!("release:{normalized}");
    let signers = signer_list();
    if signers.is_empty() {
        return true;
    }
    signers
        .iter()
        .any(|vk| vk.verify(message.as_bytes(), &signature).is_ok())
}

/// Return the configured release signer keys.
pub fn release_signer_keys() -> Vec<VerifyingKey> {
    signer_list().iter().cloned().collect()
}

/// Return the configured release signer keys as lowercase hex.
pub fn release_signer_hexes() -> Vec<String> {
    signer_list()
        .iter()
        .map(|vk| hex::encode(vk.to_bytes()))
        .collect()
}

/// Parse a verifying key from a lowercase hex string.
pub fn parse_signer_hex(input: &str) -> Option<VerifyingKey> {
    let bytes = hex::decode(input).ok()?;
    if bytes.len() != PUBLIC_KEY_LENGTH {
        return None;
    }
    let mut arr = [0u8; PUBLIC_KEY_LENGTH];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr).ok()
}

/// Verify a release attestation for a specific signer key.
pub fn verify_release_attestation(
    build_hash: &str,
    signer: &VerifyingKey,
    signature_hex: &str,
) -> bool {
    let normalized = build_hash.trim().to_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let signature = match decode_signature(signature_hex) {
        Some(sig) => sig,
        None => return false,
    };
    let message = format!("release:{normalized}");
    signer.verify(message.as_bytes(), &signature).is_ok()
}

/// Verify an artifact download matches a trusted release signature.
pub fn verify_artifact_signature(bytes: &[u8], signature_hex: &str) -> bool {
    let hash_hex = hash(bytes).to_hex().to_string();
    verify_release_signature(&hash_hex, signature_hex)
}

/// Verify an artifact attestation for a specific signer key.
pub fn verify_artifact_attestation(
    bytes: &[u8],
    signer: &VerifyingKey,
    signature_hex: &str,
) -> bool {
    let hash_hex = hash(bytes).to_hex().to_string();
    verify_release_attestation(&hash_hex, signer, signature_hex)
}
