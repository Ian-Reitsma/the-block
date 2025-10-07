use crate::governance::{self, ReleaseAttestation};
use crate::provenance;
use crypto_suite::hashing::blake3;
use httpd::{BlockingClient, Method};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use sys::tempfile::NamedTempFile;

#[derive(Debug)]
pub enum UpdateError {
    Unauthorized { hash: String, reason: String },
    MissingSource,
    Network(String),
    HashMismatch { expected: String, actual: String },
    SignatureInvalid(String),
    RollbackUnavailable(String),
    Io(std::io::Error),
}

fn sys_temp_err(err: sys::error::SysError) -> UpdateError {
    UpdateError::Io(io::Error::new(io::ErrorKind::Other, err))
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::Unauthorized { hash, reason } => {
                write!(f, "release hash {hash} not authorized: {reason}")
            }
            UpdateError::MissingSource => write!(f, "release source URL not configured"),
            UpdateError::Network(err) => write!(f, "network error: {err}"),
            UpdateError::HashMismatch { expected, actual } => {
                write!(f, "hash mismatch: expected {expected}, found {actual}")
            }
            UpdateError::SignatureInvalid(hash) => {
                write!(f, "signature invalid for release hash {hash}")
            }
            UpdateError::RollbackUnavailable(reason) => {
                write!(f, "rollback unavailable: {reason}")
            }
            UpdateError::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl From<std::io::Error> for UpdateError {
    fn from(value: std::io::Error) -> Self {
        UpdateError::Io(value)
    }
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
    let response = BlockingClient::default()
        .request(Method::Get, &url)
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if !response.status().is_success() {
        return Err(UpdateError::Network(format!(
            "{} returned {}",
            url,
            response.status().as_u16()
        )));
    }
    let bytes = response.into_body();
    let actual = blake3::hash(&bytes).to_hex().to_string();
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
    let mut file = NamedTempFile::new().map_err(sys_temp_err)?;
    file.write_all(&bytes)?;
    let dest_path = destination
        .map(|dest| dest.to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("the-block-{}.bin", normalized)));
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let path = persist_temp_file(file, &dest_path)?;
    Ok(DownloadedRelease {
        hash: normalized,
        path,
        bytes,
    })
}

fn persist_temp_file(file: NamedTempFile, dest: &Path) -> Result<PathBuf, UpdateError> {
    if let Some(parent) = dest.parent() {
        if let Ok(metadata) = fs::metadata(parent) {
            let read_only = {
                #[cfg(unix)]
                {
                    metadata.permissions().mode() & 0o200 == 0
                }
                #[cfg(not(unix))]
                {
                    metadata.permissions().readonly()
                }
            };
            if read_only {
                return Err(UpdateError::Io(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "cannot persist release into read-only directory {}",
                        parent.display()
                    ),
                )));
            }
        }
    }
    file.persist(dest)
        .map_err(|err| UpdateError::Io(err.error))?;
    Ok(dest.to_path_buf())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn persist_temp_file_converts_permission_error() {
        let dir = sys::tempfile::tempdir().unwrap();
        let readonly = dir.path().join("readonly");
        fs::create_dir(&readonly).unwrap();
        let mut perms = fs::metadata(&readonly).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&readonly, perms).unwrap();
        let file = NamedTempFile::new().unwrap();
        let result = persist_temp_file(file, &readonly.join("release.bin"));
        match result {
            Err(UpdateError::Io(err)) => {
                assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
            }
            other => panic!("unexpected persist result: {other:?}"),
        }
    }
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
