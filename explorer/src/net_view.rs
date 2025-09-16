use hex;
use serde::Serialize;

#[derive(Serialize)]
pub struct CertRecord {
    pub peer_id: String,
    pub fingerprint: String,
    pub updated_at: u64,
}

pub fn list_peer_certs() -> Vec<CertRecord> {
    the_block::net::peer_cert_snapshot()
        .into_iter()
        .map(|entry| CertRecord {
            peer_id: hex::encode(entry.peer),
            fingerprint: hex::encode(entry.fingerprint),
            updated_at: entry.updated_at,
        })
        .collect()
}
