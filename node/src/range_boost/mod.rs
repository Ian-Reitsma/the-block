use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

#[cfg(feature = "telemetry")]
use crate::telemetry::{MESH_PEER_CONNECTED_TOTAL, MESH_PEER_LATENCY_MS};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopProof {
    pub relay: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub payload: Vec<u8>,
    pub proofs: Vec<HopProof>,
}

pub struct RangeBoost {
    queue: VecDeque<Bundle>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshPeer {
    pub addr: String,
    pub latency_ms: u128,
}

static PEER_LATENCY: Lazy<Mutex<HashMap<String, u128>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static MESH_TASK_ACTIVE: AtomicBool = AtomicBool::new(false);
static RANGE_BOOST_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn mesh_active() -> bool {
    MESH_TASK_ACTIVE.load(Ordering::SeqCst)
}

pub fn set_enabled(v: bool) {
    RANGE_BOOST_ENABLED.store(v, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    RANGE_BOOST_ENABLED.load(Ordering::SeqCst)
}

fn record_latency(addr: String, latency: u128) {
    let mut guard = PEER_LATENCY.lock().unwrap();
    let is_new = !guard.contains_key(&addr);
    guard.insert(addr.clone(), latency);
    #[cfg(feature = "telemetry")]
    {
        if is_new {
            MESH_PEER_CONNECTED_TOTAL.with_label_values(&[&addr]).inc();
        }
        MESH_PEER_LATENCY_MS
            .with_label_values(&[&addr])
            .set(latency as i64);
    }
}

pub fn peer_latency(addr: &SocketAddr) -> Option<u128> {
    PEER_LATENCY
        .lock()
        .ok()
        .and_then(|map| map.get(&addr.to_string()).cloned())
}

pub fn best_peer() -> Option<MeshPeer> {
    let map = PEER_LATENCY.lock().ok()?;
    map.iter().min_by_key(|(_, l)| **l).map(|(a, l)| MeshPeer {
        addr: a.clone(),
        latency_ms: *l,
    })
}

pub fn peers() -> Vec<MeshPeer> {
    let map = PEER_LATENCY.lock().unwrap();
    let mut peers: Vec<MeshPeer> = map
        .iter()
        .map(|(a, l)| MeshPeer {
            addr: a.clone(),
            latency_ms: *l,
        })
        .collect();
    peers.sort_by_key(|p| p.latency_ms);
    peers
}

pub fn discover_peers() -> Vec<MeshPeer> {
    MESH_TASK_ACTIVE.store(true, Ordering::SeqCst);
    let peers_env = std::env::var("TB_MESH_STATIC_PEERS").unwrap_or_default();
    let mut peers = Vec::new();
    for addr in peers_env.split(',').filter(|a| !a.is_empty()) {
        if addr.starts_with("unix:") {
            #[cfg(unix)]
            {
                let path = &addr[5..];
                let start = Instant::now();
                if let Ok(mut stream) = UnixStream::connect(path) {
                    let _ = stream.write_all(&[0u8]);
                    let mut buf = [0u8; 1];
                    let _ = stream.read(&mut buf);
                    let latency = start.elapsed().as_millis();
                    record_latency(addr.to_string(), latency);
                    peers.push(MeshPeer {
                        addr: addr.to_string(),
                        latency_ms: latency,
                    });
                }
            }
        } else if let Ok(sock) = addr.parse::<SocketAddr>() {
            let start = Instant::now();
            if let Ok(mut stream) = TcpStream::connect_timeout(&sock, Duration::from_millis(100)) {
                let _ = stream.write_all(&[0u8]);
                let mut buf = [0u8; 1];
                let _ = stream.read(&mut buf);
                let latency = start.elapsed().as_millis();
                record_latency(sock.to_string(), latency);
                peers.push(MeshPeer {
                    addr: sock.to_string(),
                    latency_ms: latency,
                });
            }
        } else if addr.starts_with("bt:") {
            #[cfg(all(feature = "bluetooth", any(target_os = "linux", target_os = "macos")))]
            {
                let latency = 0;
                record_latency(addr.to_string(), latency);
                peers.push(MeshPeer {
                    addr: addr.to_string(),
                    latency_ms: latency,
                });
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        peers.extend(discover_wifi_peers());
        peers.extend(discover_bt_peers());
    }
    peers.sort_by_key(|p| p.latency_ms);
    MESH_TASK_ACTIVE.store(false, Ordering::SeqCst);
    peers
}

#[cfg(target_os = "linux")]
fn discover_bt_peers() -> Vec<MeshPeer> {
    let mut out = Vec::new();
    if let Ok(res) = Command::new("hcitool").arg("scan").output() {
        let text = String::from_utf8_lossy(&res.stdout);
        for line in text.lines().skip(1) {
            if let Some(addr) = line.split_whitespace().next() {
                let peer = format!("bt:{}", addr);
                record_latency(peer.clone(), 0);
                out.push(MeshPeer {
                    addr: peer,
                    latency_ms: 0,
                });
            }
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn discover_bt_peers() -> Vec<MeshPeer> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn discover_wifi_peers() -> Vec<MeshPeer> {
    let mut out = Vec::new();
    if let Ok(res) = Command::new("iwlist").arg("scan").output() {
        let text = String::from_utf8_lossy(&res.stdout);
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with("Cell") {
                if let Some(addr) = l.split_whitespace().nth(4) {
                    let peer = format!("wifi:{}", addr);
                    record_latency(peer.clone(), 0);
                    out.push(MeshPeer {
                        addr: peer,
                        latency_ms: 0,
                    });
                }
            }
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn discover_wifi_peers() -> Vec<MeshPeer> {
    Vec::new()
}

pub fn parse_discovery_packet(data: &[u8]) -> Option<MeshPeer> {
    let s = std::str::from_utf8(data).ok()?;
    let mut parts = s.split(',');
    let addr = parts.next()?.to_string();
    let latency_ms = parts.next()?.parse().ok()?;
    Some(MeshPeer { addr, latency_ms })
}

impl RangeBoost {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, payload: Vec<u8>) {
        self.queue.push_back(Bundle {
            payload,
            proofs: vec![],
        });
    }

    pub fn record_proof(&mut self, idx: usize, proof: HopProof) {
        if let Some(bundle) = self.queue.get_mut(idx) {
            bundle.proofs.push(proof);
        }
    }

    pub fn dequeue(&mut self) -> Option<Bundle> {
        self.queue.pop_front()
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_roundtrip() {
        let mut rb = RangeBoost::new();
        rb.enqueue(vec![1, 2, 3]);
        assert_eq!(rb.pending(), 1);
        rb.record_proof(0, HopProof { relay: "r1".into() });
        let b = rb.dequeue().unwrap();
        assert_eq!(b.payload, vec![1, 2, 3]);
        assert_eq!(b.proofs.len(), 1);
    }
}
