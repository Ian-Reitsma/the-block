#![cfg(feature = "integration-tests")]
#![cfg(feature = "quic")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::binary;
use foundation_time::{Duration as TimeDuration, UtcDateTime};
use foundation_tls::{
    generate_self_signed_ed25519, sign_self_signed_ed25519, sign_with_ca_ed25519, RotationPolicy,
    SelfSignedCertParams,
};
use rand::rngs::OsRng;
use std::io::Read;
#[cfg(any(feature = "s2n-quic", feature = "inhouse"))]
use sys::tempfile::tempdir;
use the_block::gossip::relay::Relay;
#[cfg(any(feature = "s2n-quic", feature = "inhouse"))]
use the_block::net::transport_quic;
use the_block::net::{self, quic, Message, Payload, PeerSet, PROTOCOL_VERSION};
use the_block::p2p::handshake::{Hello, Transport};
#[cfg(feature = "telemetry")]
use the_block::telemetry::{QUIC_ENDPOINT_REUSE_TOTAL, QUIC_HANDSHAKE_FAIL_TOTAL};
use the_block::Blockchain;
#[cfg(any(feature = "s2n-quic", feature = "inhouse"))]
use transport::{Config as TransportConfig, ProviderKind};

#[cfg(feature = "telemetry")]
fn reset_counters() {
    QUIC_ENDPOINT_REUSE_TOTAL.reset();
    QUIC_HANDSHAKE_FAIL_TOTAL.reset();
}

#[cfg(all(feature = "quic", feature = "inhouse"))]
struct InhouseTransportGuard {
    _dir: sys::tempfile::TempDir,
    override_store: std::path::PathBuf,
    env_store: std::path::PathBuf,
    prev_net_key: Option<String>,
    prev_peer_store: Option<String>,
    prev_env_store: Option<String>,
}

#[cfg(all(feature = "quic", feature = "inhouse"))]
impl InhouseTransportGuard {
    fn install() -> Self {
        let dir = tempdir().expect("tempdir");
        let override_store = dir.path().join("override_store.json");
        let env_store = dir.path().join("legacy_store.json");
        let net_key = dir.path().join("net_key");
        let peer_store = dir.path().join("peer_store.json");
        let prev_net_key = std::env::var("TB_NET_KEY_PATH").ok();
        std::env::set_var("TB_NET_KEY_PATH", &net_key);
        let prev_peer_store = std::env::var("TB_PEER_CERT_CACHE_PATH").ok();
        std::env::set_var("TB_PEER_CERT_CACHE_PATH", &peer_store);
        let prev_env_store = std::env::var("TB_NET_CERT_STORE_PATH").ok();
        std::env::set_var("TB_NET_CERT_STORE_PATH", &env_store);

        let mut cfg = TransportConfig::default();
        cfg.provider = ProviderKind::Inhouse;
        cfg.certificate_cache = Some(override_store.clone());
        the_block::net::configure_transport(&cfg).expect("configure inhouse transport");

        Self {
            _dir: dir,
            override_store,
            env_store,
            prev_net_key,
            prev_peer_store,
            prev_env_store,
        }
    }

    fn legacy_store_path(&self) -> std::path::PathBuf {
        self.env_store.clone()
    }

    fn der_path(&self) -> std::path::PathBuf {
        self.override_store.with_extension("der")
    }
}

#[cfg(all(feature = "quic", feature = "inhouse"))]
impl Drop for InhouseTransportGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.prev_net_key {
            std::env::set_var("TB_NET_KEY_PATH", value);
        } else {
            std::env::remove_var("TB_NET_KEY_PATH");
        }
        if let Some(value) = &self.prev_peer_store {
            std::env::set_var("TB_PEER_CERT_CACHE_PATH", value);
        } else {
            std::env::remove_var("TB_PEER_CERT_CACHE_PATH");
        }
        if let Some(value) = &self.prev_env_store {
            std::env::set_var("TB_NET_CERT_STORE_PATH", value);
        } else {
            std::env::remove_var("TB_NET_CERT_STORE_PATH");
        }
        let _ = the_block::net::configure_transport(&TransportConfig::default());
    }
}

fn sample_sk() -> SigningKey {
    SigningKey::from_bytes(&[1u8; 32])
}

fn random_serial_bytes() -> [u8; 16] {
    use rand::RngCore;

    let mut serial = [0u8; 16];
    OsRng::default().fill_bytes(&mut serial);
    serial[0] &= 0x7F;
    serial
}

fn generate_test_cert_der(name: &str) -> Vec<u8> {
    let now = UtcDateTime::now();
    let params = SelfSignedCertParams::builder()
        .subject_cn(name.to_string())
        .add_dns_name(name.to_string())
        .validity(now - TimeDuration::hours(1), now + TimeDuration::days(7))
        .serial(random_serial_bytes())
        .build()
        .expect("test cert params");
    generate_self_signed_ed25519(&params)
        .expect("test cert generation")
        .certificate
}

#[cfg(feature = "s2n-quic")]
struct S2nTransportGuard {
    _dir: sys::tempfile::TempDir,
}

#[cfg(feature = "s2n-quic")]
impl S2nTransportGuard {
    fn install() -> Self {
        let dir = tempdir().expect("tempdir");
        let cert_store = dir.path().join("cert_store.json");
        let peer_store = dir.path().join("peer_store.json");
        let net_key = dir.path().join("net_key");
        std::env::set_var("TB_NET_CERT_STORE_PATH", &cert_store);
        std::env::set_var("TB_PEER_CERT_CACHE_PATH", &peer_store);
        std::env::set_var("TB_NET_KEY_PATH", &net_key);
        let mut cfg = TransportConfig::default();
        cfg.provider = ProviderKind::S2nQuic;
        cfg.certificate_cache = Some(cert_store);
        the_block::net::configure_transport(&cfg).expect("configure transport");
        Self { _dir: dir }
    }
}

#[cfg(feature = "s2n-quic")]
impl Drop for S2nTransportGuard {
    fn drop(&mut self) {
        std::env::remove_var("TB_NET_CERT_STORE_PATH");
        std::env::remove_var("TB_PEER_CERT_CACHE_PATH");
        std::env::remove_var("TB_NET_KEY_PATH");
        let _ = the_block::net::configure_transport(&TransportConfig::default());
    }
}

#[testkit::tb_serial]
fn quic_handshake_roundtrip() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let listen_addr = server_ep.local_addr().unwrap();
        let (tx, rx) = runtime::sync::oneshot::channel();
        let ep = server_ep.clone();
        the_block::spawn(async move {
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
            .ensure_handle_for_label_values(&["unknown", "certificate"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        let conn = quic::connect(listen_addr, &cert).await.unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: 0,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Quic,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),
            quic_provider: None,
            quic_capabilities: Vec::new(),
        };
        let msg =
            Message::new(Payload::Handshake(hello.clone()), &sample_sk()).expect("sign message");
        let bytes = binary::encode(&msg).unwrap();
        quic::send(&conn, &bytes).await.unwrap();
        let recv = rx.await.unwrap();
        let parsed: Message = binary::decode(&recv).unwrap();
        assert!(matches!(parsed.body, Payload::Handshake(h) if h.transport == Transport::Quic));
        conn.close(0u32.into(), b"done");
        server_ep.wait_idle().await;
        #[cfg(feature = "telemetry")]
        assert_eq!(
            QUIC_HANDSHAKE_FAIL_TOTAL
                .ensure_handle_for_label_values(&["unknown", "certificate"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get(),
            before
        );
    });
}

#[testkit::tb_serial]
fn quic_gossip_roundtrip() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let listen_addr = server_ep.local_addr().unwrap();
        let (hs_tx, hs_rx) = runtime::sync::oneshot::channel();
        let (msg_tx, msg_rx) = runtime::sync::oneshot::channel();
        let ep = server_ep.clone();
        the_block::spawn(async move {
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
            .ensure_handle_for_label_values(&["unknown", "certificate"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        let conn = quic::connect(listen_addr, &cert).await.unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: 0,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Quic,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),
            quic_provider: None,
            quic_capabilities: Vec::new(),
        };
        let msg =
            Message::new(Payload::Handshake(hello.clone()), &sample_sk()).expect("sign message");
        quic::send(&conn, &binary::encode(&msg).unwrap())
            .await
            .unwrap();
        let recv = hs_rx.await.unwrap();
        let parsed: Message = binary::decode(&recv).unwrap();
        assert!(matches!(parsed.body, Payload::Handshake(h) if h.transport == Transport::Quic));
        let gossip = Message::new(Payload::Hello(Vec::new()), &sample_sk()).expect("sign message");
        quic::send(&conn, &binary::encode(&gossip).unwrap())
            .await
            .unwrap();
        let recv = msg_rx.await.unwrap();
        let parsed: Message = binary::decode(&recv).unwrap();
        assert!(matches!(parsed.body, Payload::Hello(peers) if peers.is_empty()));
        conn.close(0u32.into(), b"done");
        server_ep.wait_idle().await;
        #[cfg(feature = "telemetry")]
        assert_eq!(
            QUIC_HANDSHAKE_FAIL_TOTAL
                .ensure_handle_for_label_values(&["unknown", "certificate"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get(),
            before
        );
    });
}

#[testkit::tb_serial]
fn quic_disconnect() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let listen_addr = server_ep.local_addr().unwrap();
        let (close_tx, close_rx) = runtime::sync::oneshot::channel();
        let ep = server_ep.clone();
        the_block::spawn(async move {
            if let Some(conn) = ep.accept().await {
                let connection = conn.await.unwrap();
                let _ = quic::recv(&connection).await;
                connection.close(0u32.into(), b"server");
                connection.closed().await;
                close_tx.send(()).unwrap();
            }
        });
        let conn = quic::connect(listen_addr, &cert).await.unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: 0,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Quic,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sample_sk()).expect("sign message");
        quic::send(&conn, &binary::encode(&msg).unwrap())
            .await
            .unwrap();
        conn.close(0u32.into(), b"client");
        conn.closed().await;
        close_rx.await.unwrap();
        server_ep.wait_idle().await;
    });
}

#[testkit::tb_serial]
fn quic_fallback_to_tcp() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let cert = generate_test_cert_der("fallback");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = runtime::sync::oneshot::channel();
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sample_sk()).expect("sign message");
        let msg_clone = msg.clone();
        the_block::spawn_blocking(move || {
            let relay = relay;
            relay.broadcast(&msg_clone, &[(addr, Transport::Quic, Some(cert))]);
        })
        .await
        .unwrap();
        let recv = rx.await.unwrap();
        let parsed: Message = binary::decode(&recv).unwrap();
        assert!(matches!(parsed.body, Payload::Handshake(_)));
    });
}

#[cfg(feature = "s2n-quic")]
#[testkit::tb_serial]
fn s2n_quic_connect_roundtrip() {
    runtime::block_on(async {
        let _guard = S2nTransportGuard::install();
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = transport_quic::start_server(addr)
            .await
            .expect("start s2n server");
        let listen_addr = listener
            .as_s2n()
            .expect("s2n listener unavailable")
            .local_addr()
            .expect("local addr");
        let server = listener.into_s2n().expect("s2n listener unavailable");
        let (done_tx, done_rx) = runtime::sync::oneshot::channel();
        let accept = server.clone();
        the_block::spawn(async move {
            if let Some(connecting) = accept.accept().await {
                let _ = connecting.await;
            }
            let _ = done_tx.send(());
        });

        transport_quic::connect(listen_addr)
            .await
            .expect("connect to s2n server");

        done_rx.await.unwrap();
    });
}

#[cfg(feature = "s2n-quic")]
#[testkit::tb_serial]
fn s2n_ca_signed_certificate_verification() {
    let anchor = UtcDateTime::from_unix_timestamp(0).unwrap();
    let policy =
        RotationPolicy::new(anchor, TimeDuration::days(7), TimeDuration::hours(1)).expect("policy");
    let mut rng = OsRng::default();
    let ca_plan = policy.plan(0, b"s2n-ca").expect("plan");
    let ca_params = SelfSignedCertParams::builder()
        .subject_cn("s2n-ca")
        .ca(true)
        .apply_rotation_plan(&ca_plan)
        .build()
        .expect("ca params");
    let ca_key = SigningKey::generate(&mut rng);
    let _ = sign_self_signed_ed25519(&ca_key, &ca_params).expect("ca cert");

    let leaf_key = SigningKey::generate(&mut rng);
    let leaf_ctx = leaf_key.verifying_key().to_bytes();
    let leaf_plan = policy.plan(1, &leaf_ctx).expect("leaf plan");
    let leaf_params = SelfSignedCertParams::builder()
        .subject_cn("s2n-leaf")
        .apply_rotation_plan(&leaf_plan)
        .build()
        .expect("leaf params");
    let leaf_cert = sign_with_ca_ed25519(&ca_key, ca_params.subject_cn(), &leaf_key, &leaf_params)
        .expect("leaf cert");
    let pubkey = leaf_key.verifying_key().to_bytes();
    let fingerprint =
        transport::verify_remote_certificate(&pubkey, &leaf_cert).expect("verify certificate");
    assert!(fingerprint.iter().any(|byte| *byte != 0));
}

#[testkit::tb_serial]
fn quic_endpoint_reuse() {
    runtime::block_on(async {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let ep = server_ep.clone();
        the_block::spawn(async move {
            while let Some(conn) = ep.accept().await {
                let connection = conn.await.unwrap();
                connection.close(0u32.into(), b"done");
            }
        });
        let listen_addr = server_ep.local_addr().unwrap();
        let conn1 = quic::connect(listen_addr, &cert).await.unwrap();
        conn1.close(0u32.into(), b"done");
        let conn2 = quic::connect(listen_addr, &cert).await.unwrap();
        conn2.close(0u32.into(), b"done");
        server_ep.wait_idle().await;
        #[cfg(feature = "telemetry")]
        assert_eq!(QUIC_ENDPOINT_REUSE_TOTAL.get(), 0);
    });
}

#[cfg(all(feature = "quic", feature = "inhouse"))]
#[testkit::tb_serial]
fn inhouse_quic_roundtrip_and_persistence() {
    let guard = InhouseTransportGuard::install();
    runtime::block_on(async {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = transport_quic::start_server(addr)
            .await
            .expect("start inhouse server");
        let endpoint = listener.as_inhouse().expect("inhouse listener unavailable");
        let listen_addr = endpoint.local_addr();
        let registry = the_block::net::transport_registry().expect("transport registry");
        let adapter = registry.inhouse().expect("inhouse adapter");
        let advert =
            the_block::net::transport_quic::current_advertisement().expect("inhouse advertisement");
        assert_eq!(
            the_block::net::transport_quic::provider_id(),
            Some(transport::INHOUSE_PROVIDER_ID)
        );

        let certificate = adapter.certificate_from_der(advert.cert.clone());
        let connection = adapter
            .connect(listen_addr, &certificate)
            .await
            .expect("connect inhouse");
        let payload = b"first-party-quic".to_vec();
        adapter
            .send(&connection, &payload)
            .await
            .expect("send payload");
        let ack = adapter.recv(&connection).await.expect("receive ack");
        assert_eq!(ack, payload);

        drop(connection);
        drop(listener);

        let restart = transport_quic::start_server("127.0.0.1:0".parse().unwrap())
            .await
            .expect("restart inhouse server");
        let reused = the_block::net::transport_quic::current_advertisement()
            .expect("advertisement after restart");
        assert_eq!(reused.fingerprint, advert.fingerprint);
        drop(restart);
    });

    let der_path = guard.der_path();
    assert!(der_path.exists(), "persisted der missing");
    let legacy_der = guard.legacy_store_path().with_extension("der");
    assert!(
        !legacy_der.exists(),
        "legacy env-based path should remain unused"
    );
    let stored = std::fs::read(&der_path).expect("read persisted certificate");
    let snapshot =
        the_block::net::transport_quic::current_advertisement().expect("snapshot advertisement");
    assert_eq!(stored, snapshot.cert.as_ref());
}

#[testkit::tb_serial]
fn quic_ca_signed_chain() {
    runtime::block_on(async {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let anchor = UtcDateTime::from_unix_timestamp(0).unwrap();
        let policy = RotationPolicy::new(anchor, TimeDuration::days(7), TimeDuration::hours(1))
            .expect("policy");
        let mut rng = rand::rngs::OsRng::default();
        let ca_plan = policy.plan(0, b"quinn-ca").expect("plan");
        let ca_params = SelfSignedCertParams::builder()
            .subject_cn("quinn-ca")
            .ca(true)
            .apply_rotation_plan(&ca_plan)
            .build()
            .expect("build ca params");
        let ca_key = SigningKey::generate(&mut rng);
        let ca_cert = sign_self_signed_ed25519(&ca_key, &ca_params).expect("ca cert");

        let leaf_key = SigningKey::generate(&mut rng);
        let leaf_ctx = leaf_key.verifying_key().to_bytes();
        let leaf_plan = policy.plan(1, &leaf_ctx).expect("leaf plan");
        let leaf_params = SelfSignedCertParams::builder()
            .subject_cn("quinn-leaf")
            .add_dns_name("quinn.local")
            .apply_rotation_plan(&leaf_plan)
            .build()
            .expect("leaf params");
        let leaf_cert =
            sign_with_ca_ed25519(&ca_key, ca_params.subject_cn(), &leaf_key, &leaf_params)
                .expect("leaf cert");
        let key_der = concurrency::Bytes::from(
            leaf_key
                .to_pkcs8_der()
                .expect("leaf key pkcs8")
                .as_bytes()
                .to_vec(),
        );
        let chain = vec![
            concurrency::Bytes::from(leaf_cert.clone()),
            concurrency::Bytes::from(ca_cert.clone()),
        ];
        let listener = quic::listen_with_chain(addr, &chain, key_der)
            .await
            .unwrap();
        let listen_addr = listener
            .as_quinn()
            .expect("quinn listener unavailable")
            .local_addr()
            .unwrap();
        let server_ep = listener.into_quinn().expect("quinn listener unavailable");
        let (done_tx, done_rx) = runtime::sync::oneshot::channel();
        let accept_ep = server_ep.clone();
        the_block::spawn(async move {
            if let Some(conn) = accept_ep.accept().await {
                let connection = conn.await.unwrap();
                let _ = done_tx.send(());
                connection.close(0u32.into(), b"done");
            }
        });
        let ca_handle = quic::certificate_from_der(concurrency::Bytes::from(ca_cert)).unwrap();
        let conn = quic::connect(listen_addr, &ca_handle).await.unwrap();
        conn.close(0u32.into(), b"done");
        done_rx.await.unwrap();
        server_ep.wait_idle().await;
    });
}

#[cfg(feature = "telemetry")]
#[testkit::tb_serial]
fn quic_handshake_failure_metric() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, _cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let listen_addr = server_ep.local_addr().unwrap();
        #[cfg(feature = "telemetry")]
        let before = the_block::telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
            .ensure_handle_for_label_values(&["unknown", "certificate"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        let bad_cert =
            quic::certificate_from_der(concurrency::Bytes::from(generate_test_cert_der("bad")))
                .unwrap();
        let res = quic::connect(listen_addr, &bad_cert).await;
        assert!(res.is_err());
        server_ep.wait_idle().await;
        #[cfg(feature = "telemetry")]
        {
            assert!(
                the_block::telemetry::QUIC_HANDSHAKE_FAIL_TOTAL
                    .ensure_handle_for_label_values(&["unknown", "certificate"])
                    .expect(telemetry::LABEL_REGISTRATION_ERR)
                    .get()
                    >= before + 1
            );
        }
    });
}

#[testkit::tb_serial]
fn quic_handshake_timeout() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let cert = generate_test_cert_der("timeout");
        let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        let cert = quic::certificate_from_der(concurrency::Bytes::from(cert)).unwrap();
        #[cfg(feature = "telemetry")]
        let before = QUIC_HANDSHAKE_FAIL_TOTAL
            .ensure_handle_for_label_values(&["unknown", "timeout"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        let res = quic::connect(addr, &cert).await;
        assert!(res.is_err());
        #[cfg(feature = "telemetry")]
        {
            assert!(
                QUIC_HANDSHAKE_FAIL_TOTAL
                    .ensure_handle_for_label_values(&["unknown", "timeout"])
                    .expect(telemetry::LABEL_REGISTRATION_ERR)
                    .get()
                    >= before + 1
            );
        }
    });
}

#[testkit::tb_serial]
fn quic_version_mismatch() {
    runtime::block_on(async {
        #[cfg(feature = "telemetry")]
        reset_counters();
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let listen_addr = server_ep.local_addr().unwrap();
        let (tx, rx) = runtime::sync::oneshot::channel();
        let ep = server_ep.clone();
        the_block::spawn(async move {
            if let Some(conn) = ep.accept().await {
                let connection = conn.await.unwrap();
                if let Some(bytes) = quic::recv(&connection).await {
                    tx.send(bytes).unwrap();
                }
                connection.close(0u32.into(), b"done");
            }
        });
        let conn = quic::connect(listen_addr, &cert).await.unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION - 1,
            feature_bits: 0,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Quic,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg =
            Message::new(Payload::Handshake(hello.clone()), &sample_sk()).expect("sign message");
        quic::send(&conn, &binary::encode(&msg).unwrap())
            .await
            .unwrap();
        let recv = rx.await.unwrap();
        let parsed: Message = binary::decode(&recv).unwrap();
        let peers = PeerSet::new(Vec::new());
        let chain = std::sync::Arc::new(std::sync::Mutex::new(Blockchain::default()));
        net::set_track_handshake_fail(true);
        peers.handle_message(parsed, Some(listen_addr), &chain);
        #[cfg(feature = "telemetry")]
        assert!(
            the_block::telemetry::HANDSHAKE_FAIL_TOTAL
                .ensure_handle_for_label_values(&["protocol"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get()
                >= 1
        );
        net::set_track_handshake_fail(false);
        conn.close(0u32.into(), b"done");
        server_ep.wait_idle().await;
    });
}

#[testkit::tb_serial]
fn quic_packet_loss_env() {
    runtime::block_on(async {
        std::env::set_var("TB_QUIC_PACKET_LOSS", "1.0");
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = quic::listen(addr).await.unwrap();
        let server_ep = server_ep.into_quinn().expect("quinn listener unavailable");
        let listen_addr = server_ep.local_addr().unwrap();
        let (tx, rx) = runtime::sync::oneshot::channel();
        let ep = server_ep.clone();
        the_block::spawn(async move {
            if let Some(conn) = ep.accept().await {
                let connection = conn.await.unwrap();
                let res = the_block::timeout(
                    std::time::Duration::from_millis(200),
                    quic::recv(&connection),
                )
                .await;
                tx.send(res.is_err()).unwrap();
                connection.close(0u32.into(), b"done");
            }
        });
        let conn = quic::connect(listen_addr, &cert).await.unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: 0,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Quic,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sample_sk()).expect("sign message");
        quic::send(&conn, &binary::encode(&msg).unwrap())
            .await
            .unwrap();
        assert!(
            rx.await.unwrap(),
            "expected receive timeout due to packet loss"
        );
        std::env::remove_var("TB_QUIC_PACKET_LOSS");
        conn.close(0u32.into(), b"done");
        server_ep.wait_idle().await;
    });
}
