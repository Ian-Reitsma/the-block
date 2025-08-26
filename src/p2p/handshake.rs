#[cfg(feature = "telemetry")]
use crate::telemetry;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum FeatureBit {
    StorageV1 = 1 << 0,
    ComputeMarketV1 = 1 << 1,
    GovV1 = 1 << 2,
    FeeRoutingV2 = 1 << 3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub network_id: [u8; 4],
    pub proto_version: u16,
    pub feature_bits: u32,
    pub agent: String,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAck {
    pub ok: bool,
    pub reason: Option<String>,
    pub features_accepted: u32,
    pub min_backoff_ms: u32,
}

pub struct HandshakeCfg {
    pub network_id: [u8; 4],
    pub min_proto: u16,
    pub required_features: u32,
    pub supported_features: u32,
}

#[derive(Clone)]
pub struct PeerInfo {
    pub agent: String,
    pub features: u32,
}

static PEERS: Lazy<Mutex<HashMap<String, PeerInfo>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn handle_handshake(peer_id: &str, hello: Hello, cfg: &HandshakeCfg) -> HelloAck {
    if hello.network_id != cfg.network_id {
        #[cfg(feature = "telemetry")]
        telemetry::P2P_HANDSHAKE_REJECT_TOTAL
            .with_label_values(&["bad_network"])
            .inc();
        return HelloAck {
            ok: false,
            reason: Some("bad_network".into()),
            features_accepted: 0,
            min_backoff_ms: 1000,
        };
    }
    if hello.proto_version < cfg.min_proto {
        #[cfg(feature = "telemetry")]
        telemetry::P2P_HANDSHAKE_REJECT_TOTAL
            .with_label_values(&["old_proto"])
            .inc();
        return HelloAck {
            ok: false,
            reason: Some("old_proto".into()),
            features_accepted: 0,
            min_backoff_ms: 1000,
        };
    }
    if hello.feature_bits & cfg.required_features != cfg.required_features {
        #[cfg(feature = "telemetry")]
        telemetry::P2P_HANDSHAKE_REJECT_TOTAL
            .with_label_values(&["missing_features"])
            .inc();
        return HelloAck {
            ok: false,
            reason: Some("missing_features".into()),
            features_accepted: 0,
            min_backoff_ms: 1000,
        };
    }
    let accepted = hello.feature_bits & cfg.supported_features;
    #[cfg(feature = "telemetry")]
    telemetry::P2P_HANDSHAKE_ACCEPT_TOTAL
        .with_label_values(&[&format!("{accepted:#x}")])
        .inc();
    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    peers.insert(
        peer_id.to_string(),
        PeerInfo {
            agent: hello.agent.clone(),
            features: accepted,
        },
    );
    HelloAck {
        ok: true,
        reason: None,
        features_accepted: accepted,
        min_backoff_ms: 0,
    }
}

pub fn list_peers() -> Vec<(String, PeerInfo)> {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    peers.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}
