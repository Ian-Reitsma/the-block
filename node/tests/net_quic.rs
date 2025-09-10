#![cfg(feature = "quic")]
use ed25519_dalek::SigningKey;
use serial_test::serial;
use std::io::Read;
use the_block::gossip::relay::Relay;
use the_block::net::{quic, Message, Payload, PROTOCOL_VERSION};
use the_block::p2p::handshake::{Hello, Transport};
#[cfg(feature = "telemetry")]
use the_block::telemetry::{
    QUIC_ENDPOINT_REUSE_TOTAL, QUIC_HANDSHAKE_FAIL_TOTAL,
};

#[cfg(feature = "telemetry")]
fn reset_counters() {
    QUIC_ENDPOINT_REUSE_TOTAL.reset();
    QUIC_HANDSHAKE_FAIL_TOTAL.reset();
}

fn sample_sk() -> SigningKey {
    SigningKey::from_bytes(&[1u8; 32])
}

#[tokio::test]
#[serial]
async fn quic_handshake_roundtrip() {
    #[cfg(feature = "telemetry")]
    reset_counters();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let ep = server_ep.clone();
    tokio::spawn(async move {
        if let Some(conn) = ep.accept().await {
            let connection = conn.await.unwrap();
            if let Some(bytes) = quic::recv(&connection).await {
                tx.send(bytes).unwrap();
            }
            connection.close(0u32.into(), b"done");
        }
    });
    #[cfg(feature = "telemetry")]
    let before = QUIC_HANDSHAKE_FAIL_TOTAL
        .with_label_values(&["certificate"])
        .get();
    let conn = quic::connect(listen_addr, cert).await.unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: 0,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Quic,
        quic_addr: None,
        quic_cert: None,
    };
    let msg = Message::new(Payload::Handshake(hello.clone()), &sample_sk());
    let bytes = bincode::serialize(&msg).unwrap();
    quic::send(&conn, &bytes).await.unwrap();
    let recv = rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Handshake(h) if h.transport == Transport::Quic));
    conn.close(0u32.into(), b"done");
    server_ep.wait_idle().await;
    #[cfg(feature = "telemetry")]
    assert_eq!(
        QUIC_HANDSHAKE_FAIL_TOTAL
            .with_label_values(&["certificate"])
            .get(),
        before
    );
}

#[tokio::test]
#[serial]
async fn quic_gossip_roundtrip() {
    #[cfg(feature = "telemetry")]
    reset_counters();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let (hs_tx, hs_rx) = tokio::sync::oneshot::channel();
    let (msg_tx, msg_rx) = tokio::sync::oneshot::channel();
    let ep = server_ep.clone();
    tokio::spawn(async move {
        if let Some(conn) = ep.accept().await {
            let connection = conn.await.unwrap();
            if let Some(bytes) = quic::recv(&connection).await {
                hs_tx.send(bytes).unwrap();
            }
            if let Some(bytes) = quic::recv(&connection).await {
                msg_tx.send(bytes).unwrap();
            }
            connection.close(0u32.into(), b"done");
        }
    });
    #[cfg(feature = "telemetry")]
    let before = QUIC_HANDSHAKE_FAIL_TOTAL
        .with_label_values(&["certificate"])
        .get();
    let conn = quic::connect(listen_addr, cert).await.unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: 0,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Quic,
        quic_addr: None,
        quic_cert: None,
    };
    let msg = Message::new(Payload::Handshake(hello.clone()), &sample_sk());
    quic::send(&conn, &bincode::serialize(&msg).unwrap())
        .await
        .unwrap();
    let recv = hs_rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Handshake(h) if h.transport == Transport::Quic));
    let gossip = Message::new(Payload::Hello(Vec::new()), &sample_sk());
    quic::send(&conn, &bincode::serialize(&gossip).unwrap())
        .await
        .unwrap();
    let recv = msg_rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Hello(peers) if peers.is_empty()));
    conn.close(0u32.into(), b"done");
    server_ep.wait_idle().await;
    #[cfg(feature = "telemetry")]
    assert_eq!(
        QUIC_HANDSHAKE_FAIL_TOTAL
            .with_label_values(&["certificate"])
            .get(),
        before
    );
}

#[tokio::test]
#[serial]
async fn quic_disconnect() {
    #[cfg(feature = "telemetry")]
    reset_counters();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let (close_tx, close_rx) = tokio::sync::oneshot::channel();
    let ep = server_ep.clone();
    tokio::spawn(async move {
        if let Some(conn) = ep.accept().await {
            let connection = conn.await.unwrap();
            let _ = quic::recv(&connection).await;
            connection.close(0u32.into(), b"server");
            connection.closed().await;
            close_tx.send(()).unwrap();
        }
    });
    let conn = quic::connect(listen_addr, cert).await.unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: 0,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Quic,
        quic_addr: None,
        quic_cert: None,
    };
    let msg = Message::new(Payload::Handshake(hello), &sample_sk());
    quic::send(&conn, &bincode::serialize(&msg).unwrap())
        .await
        .unwrap();
    conn.close(0u32.into(), b"client");
    conn.closed().await;
    close_rx.await.unwrap();
    server_ep.wait_idle().await;
}

#[tokio::test]
#[serial]
async fn quic_fallback_to_tcp() {
    #[cfg(feature = "telemetry")]
    reset_counters();
    use rcgen::generate_simple_self_signed;
    let cert = generate_simple_self_signed(["fallback".into()])
        .unwrap()
        .serialize_der()
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
        tx.send(buf).unwrap();
    });
    let relay = Relay::default();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: 0,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Quic,
        quic_addr: None,
        quic_cert: None,
    };
    let msg = Message::new(Payload::Handshake(hello), &sample_sk());
    let msg_clone = msg.clone();
    tokio::task::spawn_blocking(move || {
        let relay = relay;
        relay.broadcast(&msg_clone, &[(addr, Transport::Quic, Some(cert))]);
    })
    .await
    .unwrap();
    let recv = rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Handshake(_)));
}

#[tokio::test]
#[serial]
async fn quic_endpoint_reuse() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, cert) = quic::listen(addr).await.unwrap();
    let ep = server_ep.clone();
    tokio::spawn(async move {
        while let Some(conn) = ep.accept().await {
            let connection = conn.await.unwrap();
            connection.close(0u32.into(), b"done");
        }
    });
    let listen_addr = server_ep.local_addr().unwrap();
    let conn1 = quic::connect(listen_addr, cert.clone()).await.unwrap();
    conn1.close(0u32.into(), b"done");
    let conn2 = quic::connect(listen_addr, cert).await.unwrap();
    conn2.close(0u32.into(), b"done");
    server_ep.wait_idle().await;
    #[cfg(feature = "telemetry")]
    assert_eq!(QUIC_ENDPOINT_REUSE_TOTAL.get(), 0);
}

#[tokio::test]
#[serial]
async fn quic_handshake_failure_metric() {
    #[cfg(feature = "telemetry")]
    reset_counters();
    use rcgen::generate_simple_self_signed;
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, _cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let bad = generate_simple_self_signed(["bad".into()]).unwrap();
    let bad_cert = rustls::Certificate(bad.serialize_der().unwrap());
    let before = the_block::telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
        .with_label_values(&["certificate"])
        .get();
    let res = quic::connect(listen_addr, bad_cert).await;
    assert!(res.is_err());
    server_ep.wait_idle().await;
    #[cfg(feature = "telemetry")]
    assert!(the_block::telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
        .with_label_values(&["certificate"])
        .get()
        >= before + 1);
}
