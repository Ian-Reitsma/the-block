use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use base64_fp::decode_standard;
use crypto_suite::hashing::blake3;
use foundation_time::UtcDateTime;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrustAnchorError {
    #[error("trust anchor input did not contain any certificates")]
    Empty,
    #[error("invalid PEM boundaries in trust anchor input")]
    InvalidPem,
    #[error("failed to decode base64 trust anchor: {0}")]
    Base64(#[from] base64_fp::Error),
    #[error("io error while loading trust anchors: {0}")]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug)]
pub struct TrustAnchor {
    der: Arc<Vec<u8>>,
    fingerprint: [u8; 32],
    subject_cn: Option<String>,
}

impl TrustAnchor {
    pub fn from_der(der: Vec<u8>) -> Self {
        let fingerprint = fingerprint(&der);
        let subject_cn = extract_common_name(&der);
        Self {
            der: Arc::new(der),
            fingerprint,
            subject_cn,
        }
    }

    pub fn der(&self) -> &[u8] {
        &self.der
    }

    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn subject_cn(&self) -> Option<&str> {
        self.subject_cn.as_deref()
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrustAnchorStore {
    anchors: Arc<Vec<TrustAnchor>>,
}

impl TrustAnchorStore {
    pub fn empty() -> Self {
        Self {
            anchors: Arc::new(Vec::new()),
        }
    }

    pub fn from_pem_str(input: &str) -> Result<Self, TrustAnchorError> {
        let ders = parse_pem_blocks(input)?;
        if ders.is_empty() {
            return Err(TrustAnchorError::Empty);
        }
        let anchors = ders.into_iter().map(TrustAnchor::from_der).collect();
        Ok(Self {
            anchors: Arc::new(anchors),
        })
    }

    pub fn from_pem_file(path: &Path) -> Result<Self, TrustAnchorError> {
        let contents = fs::read_to_string(path)?;
        Self::from_pem_str(&contents)
    }

    pub fn from_der_blobs(blobs: Vec<Vec<u8>>) -> Result<Self, TrustAnchorError> {
        if blobs.is_empty() {
            return Err(TrustAnchorError::Empty);
        }
        let anchors = blobs.into_iter().map(TrustAnchor::from_der).collect();
        Ok(Self {
            anchors: Arc::new(anchors),
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &TrustAnchor> {
        self.anchors.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    pub fn fingerprints(&self) -> Vec<[u8; 32]> {
        self.anchors
            .iter()
            .map(|anchor| *anchor.fingerprint())
            .collect()
    }
}

fn parse_pem_blocks(input: &str) -> Result<Vec<Vec<u8>>, TrustAnchorError> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let mut blocks = Vec::new();
    let mut collecting = false;
    let mut buffer = String::new();
    for line in input.lines() {
        let line = line.trim();
        if line.starts_with(BEGIN) {
            if collecting {
                return Err(TrustAnchorError::InvalidPem);
            }
            collecting = true;
            buffer.clear();
            continue;
        }
        if line.starts_with(END) {
            if !collecting {
                return Err(TrustAnchorError::InvalidPem);
            }
            collecting = false;
            let der = decode_standard(&buffer)?;
            blocks.push(der);
            buffer.clear();
            continue;
        }
        if collecting {
            buffer.push_str(line);
        }
    }
    if collecting {
        return Err(TrustAnchorError::InvalidPem);
    }
    Ok(blocks)
}

fn fingerprint(data: &[u8]) -> [u8; 32] {
    let digest = blake3::hash(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_bytes());
    out
}

fn extract_common_name(der: &[u8]) -> Option<String> {
    let needle = [0x06, 0x03, 0x55, 0x04, 0x03];
    let mut idx = 0usize;
    while let Some(pos) = find_slice(der, &needle, idx) {
        let mut cursor = pos + needle.len();
        if cursor >= der.len() {
            return None;
        }
        let tag = der[cursor];
        cursor += 1;
        if cursor >= der.len() {
            return None;
        }
        let (len, consumed) = match decode_length(&der[cursor..]) {
            Some(value) => value,
            None => return None,
        };
        cursor += consumed;
        let end = cursor.saturating_add(len);
        if end > der.len() {
            return None;
        }
        let slice = &der[cursor..end];
        if tag == 0x0c || tag == 0x13 || tag == 0x16 {
            if let Ok(value) = std::str::from_utf8(slice) {
                return Some(value.to_string());
            }
        }
        idx = end;
    }
    None
}

fn find_slice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let mut idx = start;
    while idx + needle.len() <= haystack.len() {
        if &haystack[idx..idx + needle.len()] == needle {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

fn decode_length(input: &[u8]) -> Option<(usize, usize)> {
    if input.is_empty() {
        return None;
    }
    let first = input[0];
    if first & 0x80 == 0 {
        return Some((first as usize, 1));
    }
    let bytes = (first & 0x7f) as usize;
    if bytes == 0 || bytes > 4 || input.len() < bytes + 1 {
        return None;
    }
    let mut value = 0usize;
    for &byte in &input[1..=bytes] {
        value = (value << 8) | (byte as usize);
    }
    Some((value, bytes + 1))
}

#[derive(Clone, Debug)]
pub struct OcspResponse {
    der: Arc<Vec<u8>>,
    produced_at: UtcDateTime,
}

impl OcspResponse {
    pub fn new(der: Vec<u8>, produced_at: UtcDateTime) -> Result<Self, OcspError> {
        if der.is_empty() {
            return Err(OcspError::Empty);
        }
        Ok(Self {
            der: Arc::new(der),
            produced_at,
        })
    }

    pub fn der(&self) -> &[u8] {
        &self.der
    }

    pub fn produced_at(&self) -> UtcDateTime {
        self.produced_at
    }
}

#[derive(Debug, Error)]
pub enum OcspError {
    #[error("ocsp response must not be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_parser_rejects_mismatched_markers() {
        let pem = "-----BEGIN CERTIFICATE-----\nZm9v\n";
        let err = parse_pem_blocks(pem).unwrap_err();
        assert!(matches!(err, TrustAnchorError::InvalidPem));
    }

    #[test]
    fn trust_anchor_store_builds_from_pem() {
        let cert = vec![1u8; 64];
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            base64_fp::encode_standard(&cert)
        );
        let store = TrustAnchorStore::from_pem_str(&pem).expect("store");
        assert_eq!(store.len(), 1);
        assert!(!store.fingerprints().is_empty());
    }
}
