use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use hex;

#[derive(serde::Serialize)]
struct PersistedPeer {
    id: String,
    address: String,
    last_seen: u64,
}

#[derive(serde::Serialize)]
struct PersistedPeers {
    peers: Vec<PersistedPeer>,
}

fn default_old_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".the_block")
        .join("overlay_peers.json")
}

fn default_new_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".the_block")
        .join("overlay")
        .join("peers.json")
}

fn canonical_peer_id(raw: &str) -> String {
    if let Ok(peer) = the_block::net::overlay_peer_from_base58(raw) {
        return the_block::net::overlay_peer_to_base58(&peer);
    }
    if let Ok(bytes) = hex::decode(raw) {
        if let Ok(peer) = the_block::net::overlay_peer_from_bytes(&bytes) {
            return the_block::net::overlay_peer_to_base58(&peer);
        }
    }
    raw.trim().to_string()
}

fn parse_pairs(bytes: &[u8]) -> Vec<(String, String)> {
    if bytes.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        return extract_pairs(&value);
    }
    let mut pairs = Vec::new();
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(line).trim().to_string();
        if text.is_empty() {
            continue;
        }
        let mut parts = text.split_whitespace();
        let first = parts.next().unwrap();
        if let Some(second) = parts.next() {
            pairs.push((first.to_string(), second.to_string()));
        } else {
            pairs.push((text.clone(), String::new()));
        }
    }
    pairs
}

fn extract_pairs(value: &Value) -> Vec<(String, String)> {
    match value {
        Value::Array(items) => items.iter().flat_map(extract_pairs).collect(),
        Value::Object(map) => {
            if let (Some(peer), Some(addr)) = (map.get("peer"), map.get("address")) {
                let id = peer.as_str().unwrap_or_default().to_string();
                let address = addr.as_str().unwrap_or_default().to_string();
                return vec![(id, address)];
            }
            if let (Some(id), Some(addr)) = (map.get("id"), map.get("address")) {
                let id = id.as_str().unwrap_or_default().to_string();
                let address = addr.as_str().unwrap_or_default().to_string();
                return vec![(id, address)];
            }
            if let Some(peers) = map.get("peers") {
                return extract_pairs(peers);
            }
            Vec::new()
        }
        Value::String(text) => {
            let mut parts = text.split_whitespace();
            let first = parts.next().unwrap_or("").to_string();
            let second = parts.next().unwrap_or("").to_string();
            if second.is_empty() {
                vec![(text.clone(), String::new())]
            } else {
                vec![(first, second)]
            }
        }
        _ => Vec::new(),
    }
}

fn main() {
    let old_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_old_path);
    let new_path = env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(default_new_path);

    let bytes = fs::read(&old_path).unwrap_or_default();
    let pairs = parse_pairs(&bytes);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let peers: Vec<PersistedPeer> = pairs
        .into_iter()
        .map(|(id, address)| PersistedPeer {
            id: canonical_peer_id(&id),
            address: address.trim().to_string(),
            last_seen: now,
        })
        .collect();

    if let Some(parent) = new_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let payload = PersistedPeers { peers };
    let json = serde_json::to_vec_pretty(&payload).expect("serialize overlay peers");
    fs::write(&new_path, json).expect("write migrated overlay store");
}
