#![deny(warnings)]

use crate::net::peer::HandshakeError;
#[cfg(feature = "quic")]
use crate::net::transport_quic;
#[cfg(feature = "telemetry")]
use crate::telemetry;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
#[cfg(all(feature = "telemetry", feature = "quic"))]
use tracing::warn;
#[cfg(feature = "quic")]
use transport;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum FeatureBit {
    StorageV1 = 1 << 0,
    ComputeMarketV1 = 1 << 1,
    GovV1 = 1 << 2,
    FeeRoutingV2 = 1 << 3,
    QuicTransport = 1 << 4,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Quic,
}

pub const SUPPORTED_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub network_id: [u8; 4],
    pub proto_version: u16,
    pub feature_bits: u32,
    pub agent: String,
    pub nonce: u64,
    pub transport: Transport,
    #[serde(default)]
    pub quic_addr: Option<std::net::SocketAddr>,
    #[serde(default)]
    pub quic_cert: Option<Vec<u8>>,
    #[serde(default)]
    pub quic_fingerprint: Option<Vec<u8>>,
    #[serde(default)]
    pub quic_fingerprint_previous: Vec<Vec<u8>>,
    #[serde(default)]
    pub quic_provider: Option<String>,
    #[serde(default)]
    pub quic_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAck {
    pub ok: bool,
    pub reason: Option<String>,
    pub features_accepted: u32,
    pub min_backoff_ms: u32,
    pub supported_version: u16,
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
    pub transport: Transport,
    pub quic_addr: Option<std::net::SocketAddr>,
    pub quic_cert: Option<Vec<u8>>,
    pub quic_provider: Option<String>,
    pub quic_capabilities: Vec<String>,
    pub quic_fingerprint: Option<Vec<u8>>,
    pub quic_fingerprint_previous: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedCert {
    pub provider: String,
    pub fingerprint: [u8; 32],
    pub previous: Vec<[u8; 32]>,
}

#[cfg(feature = "quic")]
fn infer_quic_provider_kind(hello: &Hello) -> Option<transport::ProviderKind> {
    if let Some(provider) = hello.quic_provider.as_deref() {
        if let Some(kind) = transport::provider_kind_from_id(provider) {
            return Some(kind);
        }
    }

    let advertises_rotation = hello
        .quic_capabilities
        .iter()
        .any(|cap| cap.eq_ignore_ascii_case("certificate_rotation"));

    if advertises_rotation || hello.quic_cert.is_some() {
        #[cfg(feature = "s2n-quic")]
        {
            return Some(transport::ProviderKind::S2nQuic);
        }

        #[cfg(not(feature = "s2n-quic"))]
        {
            return None;
        }
    }

    if let Some(registry) = crate::net::transport_registry() {
        Some(registry.kind())
    } else {
        Some(transport::ProviderKind::Quinn)
    }
}

#[cfg(feature = "quic")]
fn canonical_provider_id(hello: &Hello, fallback: transport::ProviderKind) -> String {
    hello
        .quic_provider
        .as_ref()
        .and_then(|id| transport::provider_kind_from_id(id).map(|kind| kind.id().to_string()))
        .unwrap_or_else(|| fallback.id().to_string())
}

#[cfg(feature = "quic")]
fn verify_certificate_with_provider(
    kind: transport::ProviderKind,
    provider_id: &str,
    peer_key: &[u8; 32],
    cert: &[u8],
) -> anyhow::Result<[u8; 32]> {
    if let transport::ProviderKind::S2nQuic = kind {
        if let Some(registry) = crate::net::transport_registry() {
            if let Some(adapter) = registry.s2n() {
                return adapter.verify_remote_certificate(peer_key, cert);
            }
        }
    }
    transport::verify_remote_certificate_for(provider_id, peer_key, cert)
}

#[cfg(feature = "quic")]
pub fn validate_quic_certificate(
    peer_key: &[u8; 32],
    hello: &Hello,
) -> Result<Option<ValidatedCert>, HandshakeError> {
    if let Some(cert) = hello.quic_cert.as_ref() {
        let provider_kind = infer_quic_provider_kind(hello).ok_or_else(|| {
            #[cfg(feature = "telemetry")]
            {
                let peer = hex::encode(peer_key);
                telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
                    .with_label_values(&[&peer, "certificate"])
                    .inc();
                warn!(
                    target: "p2p",
                    peer = %hex::encode(peer_key),
                    "QUIC certificate presented without a supported provider"
                );
            }
            HandshakeError::Certificate
        })?;
        let provider_id = canonical_provider_id(hello, provider_kind);
        let fingerprint =
            verify_certificate_with_provider(provider_kind, &provider_id, peer_key, cert).map_err(
                |err| {
                    #[cfg(feature = "telemetry")]
                    {
                        let peer = hex::encode(peer_key);
                        telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
                            .with_label_values(&[&peer, "certificate"])
                            .inc();
                        warn!(
                            target: "p2p",
                            peer = %hex::encode(peer_key),
                            provider = provider_id.as_str(),
                            error = %err,
                            "QUIC certificate validation failed"
                        );
                    }
                    #[cfg(not(feature = "telemetry"))]
                    let _ = err;
                    HandshakeError::Certificate
                },
            )?;
        if let Some(expected) = hello.quic_fingerprint.as_ref() {
            if expected.len() != 32 || expected.as_slice() != fingerprint.as_slice() {
                #[cfg(feature = "telemetry")]
                {
                    let peer = hex::encode(peer_key);
                    telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[&peer, "fingerprint_mismatch"])
                        .inc();
                    warn!(
                        target: "p2p",
                        peer = %hex::encode(peer_key),
                        expected = %hex::encode(expected),
                        actual = %hex::encode(fingerprint),
                        "QUIC fingerprint mismatch"
                    );
                }
                return Err(HandshakeError::Certificate);
            }
        }
        let mut previous = Vec::new();
        for fp in &hello.quic_fingerprint_previous {
            if fp.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(fp);
                previous.push(arr);
            }
        }
        Ok(Some(ValidatedCert {
            provider: provider_id,
            fingerprint,
            previous,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(not(feature = "quic"))]
pub fn validate_quic_certificate(
    _peer_key: &[u8; 32],
    hello: &Hello,
) -> Result<Option<ValidatedCert>, HandshakeError> {
    if hello.quic_cert.is_some() {
        Ok(Some(ValidatedCert {
            provider: String::new(),
            fingerprint: [0u8; 32],
            previous: Vec::new(),
        }))
    } else {
        Ok(None)
    }
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
            supported_version: SUPPORTED_VERSION,
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
            supported_version: SUPPORTED_VERSION,
        };
    }
    if hello.proto_version > SUPPORTED_VERSION {
        #[cfg(feature = "telemetry")]
        telemetry::P2P_HANDSHAKE_REJECT_TOTAL
            .with_label_values(&["new_proto"])
            .inc();
        return HelloAck {
            ok: false,
            reason: Some("new_proto".into()),
            features_accepted: 0,
            min_backoff_ms: 1000,
            supported_version: SUPPORTED_VERSION,
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
            supported_version: SUPPORTED_VERSION,
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
            transport: hello.transport,
            quic_addr: hello.quic_addr,
            quic_cert: hello.quic_cert.clone(),
            quic_provider: hello.quic_provider.clone(),
            quic_capabilities: hello.quic_capabilities.clone(),
            quic_fingerprint: hello.quic_fingerprint.clone(),
            quic_fingerprint_previous: hello.quic_fingerprint_previous.clone(),
        },
    );
    HelloAck {
        ok: true,
        reason: None,
        features_accepted: accepted,
        min_backoff_ms: 0,
        supported_version: SUPPORTED_VERSION,
    }
}

pub fn list_peers() -> Vec<(String, PeerInfo)> {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    peers.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

pub fn peer_provider(peer: &[u8; 32]) -> Option<String> {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let key = hex::encode(peer);
    peers.get(&key).and_then(|info| info.quic_provider.clone())
}

pub fn peer_capabilities(peer: &[u8; 32]) -> Vec<String> {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let key = hex::encode(peer);
    peers
        .get(&key)
        .map(|info| info.quic_capabilities.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> HandshakeCfg {
        HandshakeCfg {
            network_id: [0u8; 4],
            min_proto: SUPPORTED_VERSION,
            required_features: FeatureBit::StorageV1 as u32,
            supported_features: (FeatureBit::StorageV1 as u32)
                | (FeatureBit::ComputeMarketV1 as u32),
        }
    }

    fn base_hello() -> Hello {
        Hello {
            network_id: [0u8; 4],
            proto_version: SUPPORTED_VERSION,
            feature_bits: FeatureBit::StorageV1 as u32,
            agent: "test-node".into(),
            nonce: 0,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),
            quic_provider: None,
            quic_capabilities: Vec::new(),
        }
    }

    #[test]
    fn missing_features_rejection_reports_supported_version() {
        let cfg = base_cfg();
        let mut hello = base_hello();
        hello.feature_bits = 0;
        let ack = handle_handshake("peer", hello, &cfg);
        assert!(!ack.ok);
        assert_eq!(ack.reason.as_deref(), Some("missing_features"));
        assert_eq!(ack.supported_version, SUPPORTED_VERSION);
    }

    #[test]
    fn network_mismatch_is_rejected() {
        let cfg = base_cfg();
        let mut hello = base_hello();
        hello.network_id = [1, 2, 3, 4];
        let ack = handle_handshake("peer", hello, &cfg);
        assert!(!ack.ok);
        assert_eq!(ack.reason.as_deref(), Some("bad_network"));
        assert_eq!(ack.supported_version, SUPPORTED_VERSION);
    }

    #[cfg(feature = "quic")]
    fn quic_hello(cert: Vec<u8>, fingerprint: Option<Vec<u8>>) -> Hello {
        let mut hello = base_hello();
        hello.transport = Transport::Quic;
        hello.quic_cert = Some(cert);
        hello.quic_fingerprint = fingerprint;
        hello.quic_provider = Some(transport::ProviderKind::S2nQuic.id().to_string());
        hello.quic_capabilities = vec!["certificate_rotation".into()];
        hello
    }

    #[cfg(feature = "quic")]
    fn sample_quic_certificate() -> ([u8; 32], Vec<u8>, [u8; 32]) {
        use rcgen::{Certificate, CertificateParams, KeyPair, PKCS_ED25519};

        let key_pair = KeyPair::generate_for(&PKCS_ED25519).expect("ed25519 keypair");
        let pk_raw = key_pair.public_key_raw();
        let mut peer_key = [0u8; 32];
        peer_key.copy_from_slice(&pk_raw);
        let mut params =
            CertificateParams::new(vec!["the-block.test".into()]).expect("certificate params");
        params.alg = &PKCS_ED25519;
        params.key_pair = Some(key_pair);
        let cert = Certificate::from_params(params).expect("certificate");
        let cert_der = cert.serialize_der().expect("serialize cert");
        let fingerprint = transport_quic::fingerprint(&cert_der);
        (peer_key, cert_der, fingerprint)
    }

    #[cfg(feature = "quic")]
    #[test]
    fn rejects_certificate_with_peer_key_mismatch() {
        let (peer_key, cert, fingerprint) = sample_quic_certificate();
        let mut wrong_key = peer_key;
        wrong_key[0] ^= 0x01;
        let hello = quic_hello(cert, Some(fingerprint.to_vec()));
        let err = validate_quic_certificate(&wrong_key, &hello).unwrap_err();
        assert_eq!(err, HandshakeError::Certificate);
    }

    #[cfg(feature = "quic")]
    #[test]
    fn rejects_certificate_with_unexpected_fingerprint() {
        let (peer_key, cert, fingerprint) = sample_quic_certificate();
        let mut mismatched = fingerprint.to_vec();
        mismatched[0] ^= 0x01;
        let hello = quic_hello(cert, Some(mismatched));
        let err = validate_quic_certificate(&peer_key, &hello).unwrap_err();
        assert_eq!(err, HandshakeError::Certificate);
    }

    #[cfg(feature = "quic")]
    #[test]
    fn infers_quinn_provider_when_metadata_missing() {
        let mut hello = base_hello();
        hello.transport = Transport::Quic;
        assert_eq!(
            super::infer_quic_provider_kind(&hello),
            Some(transport::ProviderKind::Quinn)
        );
    }

    #[cfg(all(feature = "quic", feature = "s2n-quic"))]
    #[test]
    fn infers_s2n_from_rotation_capability() {
        let mut hello = base_hello();
        hello.transport = Transport::Quic;
        hello.quic_capabilities = vec!["CERTIFICATE_ROTATION".into()];
        assert_eq!(
            super::infer_quic_provider_kind(&hello),
            Some(transport::ProviderKind::S2nQuic)
        );
    }
}
