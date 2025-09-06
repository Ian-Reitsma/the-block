#![cfg(feature = "quic")]
use ed25519_dalek::SigningKey;
use futures::StreamExt;
use std::io::Read;
use the_block::gossip::relay::Relay;
use the_block::net::{quic, Message, Payload, PROTOCOL_VERSION};
use the_block::p2p::handshake::{Hello, Transport};
#[cfg(feature = "telemetry")]
use the_block::telemetry::QUIC_ENDPOINT_REUSE_TOTAL;

fn sample_sk() -> SigningKey {
    SigningKey::from_bytes(&[1u8; 32])
}

#[tokio::test]
async fn quic_handshake_roundtrip() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, mut incoming, cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if let Some(conn) = incoming.next().await {
            let quinn::NewConnection {
                connection,
                mut uni_streams,
                ..
            } = conn.await.unwrap();
            if let Some(stream) = uni_streams.next().await {
                let mut s = stream.unwrap();
                let mut buf = Vec::new();
                s.read_to_end(&mut buf).await.unwrap();
                tx.send(buf).unwrap();
            }
            connection.close(0u32.into(), b"done");
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
    let msg = Message::new(Payload::Handshake(hello.clone()), &sample_sk());
    let bytes = bincode::serialize(&msg).unwrap();
    quic::send(&conn, &bytes).await.unwrap();
    let recv = rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Handshake(h) if h.transport == Transport::Quic));
    conn.close(0u32.into(), b"done");
    server_ep.wait_idle().await;
}

#[tokio::test]
async fn quic_gossip_roundtrip() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, mut incoming, cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let (hs_tx, hs_rx) = tokio::sync::oneshot::channel();
    let (msg_tx, msg_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if let Some(conn) = incoming.next().await {
            let quinn::NewConnection {
                connection,
                mut uni_streams,
                ..
            } = conn.await.unwrap();
            if let Some(bytes) = quic::recv(&mut uni_streams).await {
                hs_tx.send(bytes).unwrap();
            }
            if let Some(bytes) = quic::recv(&mut uni_streams).await {
                msg_tx.send(bytes).unwrap();
            }
            connection.close(0u32.into(), b"done");
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
    let msg = Message::new(Payload::Handshake(hello.clone()), &sample_sk());
    quic::send(&conn, &bincode::serialize(&msg).unwrap()).await.unwrap();
    let recv = hs_rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Handshake(h) if h.transport == Transport::Quic));
    let gossip = Message::new(Payload::Hello(Vec::new()), &sample_sk());
    quic::send(&conn, &bincode::serialize(&gossip).unwrap()).await.unwrap();
    let recv = msg_rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Hello(peers) if peers.is_empty()));
    conn.close(0u32.into(), b"done");
    server_ep.wait_idle().await;
}

#[tokio::test]
async fn quic_disconnect() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, mut incoming, cert) = quic::listen(addr).await.unwrap();
    let listen_addr = server_ep.local_addr().unwrap();
    let (close_tx, close_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if let Some(conn) = incoming.next().await {
            let quinn::NewConnection {
                connection,
                mut uni_streams,
                ..
            } = conn.await.unwrap();
            let _ = quic::recv(&mut uni_streams).await;
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
    quic::send(&conn, &bincode::serialize(&msg).unwrap()).await.unwrap();
    conn.close(0u32.into(), b"client");
    conn.closed().await;
    close_rx.await.unwrap();
    server_ep.wait_idle().await;
}

#[tokio::test]
async fn quic_fallback_to_tcp() {
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
    relay.broadcast(&msg, &[(addr, Transport::Quic, Some(cert))]);
    let recv = rx.await.unwrap();
    let parsed: Message = bincode::deserialize(&recv).unwrap();
    assert!(matches!(parsed.body, Payload::Handshake(_)));
}

#[tokio::test]
async fn quic_endpoint_reuse() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (server_ep, mut incoming, cert) = quic::listen(addr).await.unwrap();
    tokio::spawn(async move {
        while let Some(conn) = incoming.next().await {
            let c = conn.await.unwrap();
            c.connection.close(0u32.into(), b"done");
        }
    });
    let listen_addr = server_ep.local_addr().unwrap();
    let conn1 = quic::connect(listen_addr, cert.clone()).await.unwrap();
    conn1.close(0u32.into(), b"done");
    let conn2 = quic::connect(listen_addr, cert).await.unwrap();
    conn2.close(0u32.into(), b"done");
    server_ep.wait_idle().await;
    #[cfg(feature = "telemetry")]
    assert!(QUIC_ENDPOINT_REUSE_TOTAL.get() >= 1);
}
