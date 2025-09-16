use crate::governance::{self, ReleaseAttestation};
use crate::provenance;
use blake3::hash;
use reqwest::blocking::Client;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("release hash {hash} not authorized: {reason}")]
    Unauthorized { hash: String, reason: String },
    #[error("release source URL not configured")]
    MissingSource,
    #[error("network error: {0}")]
    Network(String),
    #[error("hash mismatch: expected {expected}, found {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("signature invalid for release hash {0}")]
    SignatureInvalid(String),
    #[error("rollback unavailable: {0}")]
    RollbackUnavailable(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct DownloadedRelease {
    pub hash: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl DownloadedRelease {
    pub fn persist_to(&self, dest: &Path) -> Result<(), UpdateError> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, &self.bytes)?;
        Ok(())
    }
}

pub fn fetch_release(
    hash: &str,
    attestations: &[ReleaseAttestation],
    destination: Option<&Path>,
) -> Result<DownloadedRelease, UpdateError> {
    let normalized = hash.trim().to_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::HashMismatch {
            expected: normalized.clone(),
            actual: hash.to_string(),
        });
    }
    if provenance::release_signature_required() && attestations.is_empty() {
        return Err(UpdateError::SignatureInvalid(normalized.clone()));
    }
    let base = std::env::var("TB_RELEASE_SOURCE_URL").map_err(|_| UpdateError::MissingSource)?;
    let url = format!("{}/{normalized}.bin", base.trim_end_matches('/'));
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    let response = client
        .get(&url)
        .send()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if !response.status().is_success() {
        return Err(UpdateError::Network(format!(
            "{} returned {}",
            url,
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .to_vec();
    let actual = hash(&bytes).to_hex().to_string();
    if actual != normalized {
        return Err(UpdateError::HashMismatch {
            expected: normalized,
            actual,
        });
    }
    if !attestations.is_empty() {
        let configured = provenance::release_signer_keys();
        let configured_lookup: HashSet<[u8; 32]> =
            configured.iter().map(|vk| vk.to_bytes()).collect();
        let mut seen: HashSet<[u8; 32]> = HashSet::new();
        let mut valid = 0usize;
        for att in attestations {
            let Some(vk) = provenance::parse_signer_hex(&att.signer) else {
                return Err(UpdateError::SignatureInvalid(att.signer.clone()));
            };
            let signer_bytes = vk.to_bytes();
            if !configured_lookup.is_empty() && !configured_lookup.contains(&signer_bytes) {
                return Err(UpdateError::SignatureInvalid(att.signer.clone()));
            }
            if provenance::verify_artifact_attestation(&bytes, &vk, &att.signature) {
                if seen.insert(signer_bytes) {
                    valid += 1;
                }
            } else {
                return Err(UpdateError::SignatureInvalid(att.signer.clone()));
            }
        }
        if provenance::release_signature_required() && valid == 0 {
            return Err(UpdateError::SignatureInvalid("none".into()));
        }
    }
    let mut file = NamedTempFile::new()?;
    file.write_all(&bytes)?;
    let path = if let Some(dest) = destination {
        file.persist(dest)?;
        dest.to_path_buf()
    } else {
        let dest = std::env::temp_dir().join(format!("the-block-{}.bin", normalized));
        file.persist(&dest)?;
        dest
    };
    Ok(DownloadedRelease {
        hash: normalized,
        path,
        bytes,
    })
}

pub fn install_release(hash: &str) -> Result<(), UpdateError> {
    governance::ensure_release_authorized(hash).map_err(|reason| UpdateError::Unauthorized {
        hash: hash.to_string(),
        reason,
    })
}

pub fn rollback_failed_startup() -> Result<(), UpdateError> {
    let backup = std::env::var("TB_PREVIOUS_BINARY")
        .map_err(|_| UpdateError::RollbackUnavailable("TB_PREVIOUS_BINARY not set".into()))?;
    let backup_path = PathBuf::from(&backup);
    if !backup_path.exists() {
        return Err(UpdateError::RollbackUnavailable(format!(
            "backup binary {backup} missing"
        )));
    }
    let current = std::env::current_exe()?;
    fs::copy(&backup_path, &current)?;
    Ok(())
}
