#![deny(warnings)]

use crate::net::peer::HandshakeError;
#[cfg(feature = "telemetry")]
use crate::telemetry;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
#[cfg(feature = "telemetry")]
use tracing::warn;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedCert {
    pub fingerprint: [u8; 32],
    pub previous: Vec<[u8; 32]>,
}

#[cfg(feature = "quic")]
use crate::net::transport_quic;

#[cfg(feature = "quic")]
pub fn validate_quic_certificate(
    peer_key: &[u8; 32],
    hello: &Hello,
) -> Result<Option<ValidatedCert>, HandshakeError> {
    if let Some(cert) = hello.quic_cert.as_ref() {
        let fingerprint =
            transport_quic::verify_remote_certificate(peer_key, cert).map_err(|err| {
                #[cfg(feature = "telemetry")]
                {
                    let peer = hex::encode(peer_key);
                    telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[&peer, "certificate"])
                        .inc();
                    warn!(
                        target: "p2p",
                        peer = %hex::encode(peer_key),
                        error = %err,
                        "QUIC certificate validation failed"
                    );
                }
                HandshakeError::Certificate
            })?;
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

#[cfg(all(test, feature = "quic"))]
mod tests {
    use super::*;
    use rcgen::{Certificate, CertificateParams, KeyPair, PKCS_ED25519};

    fn sample_quic_certificate() -> ([u8; 32], Vec<u8>, [u8; 32]) {
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

    fn base_hello(cert: Vec<u8>, fingerprint: Option<Vec<u8>>) -> Hello {
        Hello {
            network_id: [0u8; 4],
            proto_version: SUPPORTED_VERSION,
            feature_bits: 0,
            agent: "test-node".into(),
            nonce: 0,
            transport: Transport::Quic,
            quic_addr: None,
            quic_cert: Some(cert),
            quic_fingerprint: fingerprint,
            quic_fingerprint_previous: Vec::new(),
        }
    }

    #[test]
    fn rejects_certificate_with_peer_key_mismatch() {
        let (peer_key, cert, fingerprint) = sample_quic_certificate();
        let mut wrong_key = peer_key;
        wrong_key[0] ^= 0x01;
        let hello = base_hello(cert, Some(fingerprint.to_vec()));
        let err = validate_quic_certificate(&wrong_key, &hello).unwrap_err();
        assert_eq!(err, HandshakeError::Certificate);
    }

    #[test]
    fn rejects_certificate_with_unexpected_fingerprint() {
        let (peer_key, cert, fingerprint) = sample_quic_certificate();
        let mut mismatched = fingerprint.to_vec();
        mismatched[0] ^= 0x01;
        let hello = base_hello(cert, Some(mismatched));
        let err = validate_quic_certificate(&peer_key, &hello).unwrap_err();
        assert_eq!(err, HandshakeError::Certificate);
    }
}
