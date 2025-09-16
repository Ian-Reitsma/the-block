#![cfg(feature = "telemetry")]
use the_block::p2p::handshake::{
    handle_handshake, FeatureBit, HandshakeCfg, Hello, Transport, SUPPORTED_VERSION,
};
use the_block::telemetry;

fn cfg() -> HandshakeCfg {
    HandshakeCfg {
        network_id: [1, 2, 3, 4],
        min_proto: 1,
        required_features: FeatureBit::StorageV1 as u32,
        supported_features: (FeatureBit::StorageV1 as u32) | (FeatureBit::ComputeMarketV1 as u32),
    }
}

#[test]
fn reject_mismatched_network() {
    telemetry::P2P_HANDSHAKE_REJECT_TOTAL
        .with_label_values(&["bad_network"])
        .reset();
    let hello = Hello {
        network_id: [9, 9, 9, 9],
        proto_version: 1,
        feature_bits: FeatureBit::StorageV1 as u32,
        agent: "a".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let ack = handle_handshake("peer1", hello, &cfg());
    assert!(!ack.ok);
    assert_eq!(ack.reason.as_deref(), Some("bad_network"));
    assert_eq!(ack.supported_version, SUPPORTED_VERSION);
    assert_eq!(
        telemetry::P2P_HANDSHAKE_REJECT_TOTAL
            .with_label_values(&["bad_network"])
            .get(),
        1
    );
}

#[test]
fn reject_old_proto() {
    telemetry::P2P_HANDSHAKE_REJECT_TOTAL
        .with_label_values(&["old_proto"])
        .reset();
    let hello = Hello {
        network_id: [1, 2, 3, 4],
        proto_version: 0,
        feature_bits: FeatureBit::StorageV1 as u32,
        agent: "a".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let ack = handle_handshake("peer2", hello, &cfg());
    assert!(!ack.ok);
    assert_eq!(ack.reason.as_deref(), Some("old_proto"));
    assert_eq!(ack.supported_version, SUPPORTED_VERSION);
    assert_eq!(
        telemetry::P2P_HANDSHAKE_REJECT_TOTAL
            .with_label_values(&["old_proto"])
            .get(),
        1
    );
}

#[test]
fn accept_and_record() {
    telemetry::P2P_HANDSHAKE_ACCEPT_TOTAL
        .with_label_values(&["0x3"])
        .reset();
    let hello = Hello {
        network_id: [1, 2, 3, 4],
        proto_version: 1,
        feature_bits: (FeatureBit::StorageV1 as u32) | (FeatureBit::ComputeMarketV1 as u32),
        agent: "blockd/0.1".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let ack = handle_handshake("peer3", hello, &cfg());
    assert!(ack.ok);
    assert_eq!(ack.features_accepted, 0x3);
    assert_eq!(ack.supported_version, SUPPORTED_VERSION);
    assert_eq!(
        telemetry::P2P_HANDSHAKE_ACCEPT_TOTAL
            .with_label_values(&["0x3"])
            .get(),
        1
    );
    assert!(the_block::p2p::handshake::list_peers()
        .iter()
        .any(|(id, info)| id == "peer3"
            && info.agent.contains("blockd")
            && info.transport == Transport::Tcp));
}
