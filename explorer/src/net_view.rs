use foundation_serialization::Serialize;

#[derive(Serialize)]
pub struct CertRecord {
    pub peer_id: String,
    pub fingerprint: String,
    pub updated_at: u64,
}

#[derive(Serialize)]
pub struct OverlayStatusRecord {
    pub backend: String,
    pub active_peers: usize,
    pub persisted_peers: usize,
    pub database_path: Option<String>,
}

pub fn list_peer_certs() -> Vec<CertRecord> {
    the_block::net::peer_cert_snapshot()
        .into_iter()
        .map(|entry| CertRecord {
            peer_id: the_block::net::overlay_peer_from_bytes(&entry.peer)
                .map(|peer| the_block::net::overlay_peer_to_base58(&peer))
                .unwrap_or_else(|_| crypto_suite::hex::encode(entry.peer)),
            fingerprint: crypto_suite::hex::encode(entry.fingerprint),
            updated_at: entry.updated_at,
        })
        .collect()
}

pub fn overlay_status() -> OverlayStatusRecord {
    let status = the_block::net::overlay_status();
    OverlayStatusRecord {
        backend: status.backend,
        active_peers: status.active_peers,
        persisted_peers: status.persisted_peers,
        database_path: status.database_path,
    }
}
