use blake3::hash;

#[cfg(feature = "telemetry")]
use crate::telemetry::DID_ANCHOR_TOTAL;

/// Anchor a DID document and return its hash.
pub fn anchor(doc: &str) -> [u8; 32] {
    let h = hash(doc.as_bytes()).into();
    #[cfg(feature = "telemetry")]
    DID_ANCHOR_TOTAL.inc();
    h
}

/// Resolve a DID document hash.
///
/// This is a placeholder until full registry integration is implemented.
pub fn resolve(_hash: &[u8; 32]) -> Option<String> {
    None
}
