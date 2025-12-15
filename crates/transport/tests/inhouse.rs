#![cfg(feature = "inhouse")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard, OnceLock,
};
use std::time::Duration;

use concurrency::Bytes;

use sys::tempfile::tempdir;
use transport::{
    available_providers, inhouse_certificate_store, CertificateHandle, CertificateStore, Config,
    DefaultFactory, ProviderCapability, ProviderKind, RetryPolicy, TransportCallbacks,
    TransportFactory,
};

fn test_config() -> Config {
    Config {
        provider: ProviderKind::Inhouse,
        certificate_cache: None,
        retry: RetryPolicy {
            attempts: 3,
            backoff: Duration::from_millis(10),
        },
        handshake_timeout: Duration::from_millis(750),
        tls: Default::default(),
    }
}

fn transport_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[test]
fn handshake_success_roundtrip() {
    let _guard = transport_test_guard();
    runtime::block_on(async {
        let cfg = test_config();
        let factory = DefaultFactory::default();
        let registry = factory
            .create(&cfg, &TransportCallbacks::default())
            .expect("registry");
        let adapter = registry.inhouse().expect("inhouse adapter");

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let (listener, cert) = adapter.listen(addr).await.expect("listen");
        let listen_addr = listener
            .as_inhouse()
            .expect("inhouse listener")
            .local_addr();

        let conn = adapter
            .connect(listen_addr, &cert)
            .await
            .expect("handshake succeeds");
        adapter.send(&conn, b"hello inhouse").await.expect("send");
        let payload = runtime::timeout(Duration::from_secs(5), adapter.recv(&conn))
            .await
            .expect("timed out waiting for application ack")
            .expect("connection closed before ack");
        assert_eq!(payload, b"hello inhouse");

        let stats = adapter.connection_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].0, listen_addr);
        assert!(stats[0].1.deliveries >= 1);

        // Prevent the listener from being dropped to avoid cancelling
        // the server loop via EndpointInner::drop
        std::mem::forget(listener);
    });
}

#[test]
fn handshake_rejects_mismatched_certificate() {
    let _guard = transport_test_guard();
    runtime::block_on(async {
        let cfg = test_config();
        let factory = DefaultFactory::default();
        let failures = Arc::new(Mutex::new(Vec::new()));
        let mut callbacks = TransportCallbacks::default();
        {
            let failures = Arc::clone(&failures);
            callbacks.inhouse.handshake_failure = Some(Arc::new(move |_addr, reason| {
                failures.lock().unwrap().push(reason.to_owned());
            }));
        }
        let registry = factory.create(&cfg, &callbacks).expect("registry");
        let adapter = registry.inhouse().expect("inhouse adapter");

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let (listener, _cert) = adapter.listen(addr).await.expect("listen");
        let listen_addr = listener
            .as_inhouse()
            .expect("inhouse listener")
            .local_addr();

        // Derive an unrelated certificate to trigger the mismatch path.
        let bogus = adapter.certificate_from_der(Bytes::from(vec![1, 2, 3, 4]));
        let err = adapter
            .connect(listen_addr, &bogus)
            .await
            .err()
            .expect("handshake fails");
        assert!(err.to_string().contains("handshake failed"));
        let captured = failures.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(!captured[0].is_empty());

        // Drop any lingering state so subsequent tests observe a clean table.
        adapter.drop_connection(&listen_addr);

        // Prevent the listener from being dropped to avoid cancelling the server loop
        std::mem::forget(listener);
    });
}

#[test]
fn verify_remote_certificate_matches_generated_material() {
    let _guard = transport_test_guard();
    runtime::block_on(async {
        let cfg = test_config();
        let factory = DefaultFactory::default();
        let registry = factory
            .create(&cfg, &TransportCallbacks::default())
            .expect("registry");
        let adapter = registry.inhouse().expect("inhouse adapter");

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let (listener, cert) = adapter.listen(addr).await.expect("listen");

        let (fingerprint, verifying_key, der) = match cert {
            CertificateHandle::Inhouse(cert) => {
                (cert.fingerprint, cert.verifying_key, cert.der.clone())
            }
            #[allow(unreachable_patterns)]
            _ => panic!("unexpected certificate handle"),
        };

        let verified = adapter
            .verify_remote_certificate(&verifying_key, &der)
            .expect("verify succeeds");
        assert_eq!(verified, fingerprint);

        let err = adapter
            .verify_remote_certificate(&[0u8; 32], &[])
            .expect_err("empty certificate rejected");
        assert!(err.to_string().contains("certificate"));

        // Prevent the listener from being dropped to avoid cancelling the server loop
        std::mem::forget(listener);
    });
}

#[test]
fn certificate_store_rotation_persists() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("quic_peer_certs.json");
    let store = inhouse_certificate_store(path.clone());

    let advert1 = store.initialize().expect("initialize");
    assert!(path.exists());
    let advert2 = store.rotate().expect("rotate");
    assert_ne!(advert1.fingerprint, advert2.fingerprint);
    assert_ne!(advert1.verifying_key, advert2.verifying_key);
    assert_ne!(advert1.verifying_key, [0u8; 32]);
    let current = store.current().expect("current advert");
    assert_eq!(current.fingerprint, advert2.fingerprint);
    assert_eq!(current.verifying_key, advert2.verifying_key);
}

#[test]
fn provider_capabilities_surface_in_registry() {
    let cfg = test_config();
    let factory = DefaultFactory::default();
    let registry = factory
        .create(&cfg, &TransportCallbacks::default())
        .expect("registry");
    let metadata = registry.metadata();
    assert_eq!(metadata.kind, ProviderKind::Inhouse);
    assert!(metadata
        .capabilities
        .contains(&ProviderCapability::CertificateRotation));

    let providers = available_providers();
    assert!(providers.iter().any(|p| p.kind == ProviderKind::Inhouse));
}

#[test]
fn handshake_metadata_tracks_latency_and_reuse() {
    let _guard = transport_test_guard();
    runtime::block_on(async {
        let cfg = test_config();
        let factory = DefaultFactory::default();
        let successes = Arc::new(AtomicUsize::new(0));
        let mut callbacks = TransportCallbacks::default();
        {
            let successes = Arc::clone(&successes);
            callbacks.inhouse.handshake_success = Some(Arc::new(move |_addr| {
                successes.fetch_add(1, Ordering::SeqCst);
            }));
        }

        let registry = factory.create(&cfg, &callbacks).expect("registry");
        let adapter = registry.inhouse().expect("inhouse adapter");

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let (listener, cert) = adapter.listen(addr).await.expect("listen");
        let listen_addr = listener
            .as_inhouse()
            .expect("inhouse listener")
            .local_addr();

        let first = adapter
            .connect(listen_addr, &cert)
            .await
            .expect("first handshake succeeds");
        assert_eq!(successes.load(Ordering::SeqCst), 1);

        let stats = adapter.connection_stats();
        assert_eq!(stats.len(), 1);
        let snapshot = &stats[0].1;
        assert!(snapshot.handshake_latency >= Duration::ZERO);

        let second = adapter
            .connect(listen_addr, &cert)
            .await
            .expect("second connect reuses session");
        assert_eq!(successes.load(Ordering::SeqCst), 1);

        let first_conn = match &first {
            transport::ConnectionHandle::Inhouse(conn) => conn,
            #[cfg(feature = "quinn")]
            transport::ConnectionHandle::Quinn(_) => {
                panic!("expected inhouse connection")
            }
        };
        let second_conn = match &second {
            transport::ConnectionHandle::Inhouse(conn) => conn,
            #[cfg(feature = "quinn")]
            transport::ConnectionHandle::Quinn(_) => {
                panic!("expected inhouse connection")
            }
        };
        assert!(Arc::ptr_eq(first_conn, second_conn));

        adapter.drop_connection(&listen_addr);

        // Prevent the listener from being dropped to avoid cancelling the server loop
        std::mem::forget(listener);
    });
}
