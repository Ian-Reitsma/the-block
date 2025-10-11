#![cfg(feature = "inhouse")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
            attempts: 2,
            backoff: Duration::from_millis(1),
        },
        handshake_timeout: Duration::from_millis(50),
        tls: Default::default(),
    }
}

#[test]
fn handshake_success_roundtrip() {
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
        let payload = adapter.recv(&conn).await.expect("recv payload");
        assert_eq!(payload, b"hello inhouse");

        let stats = adapter.connection_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].0, listen_addr);
        assert!(stats[0].1.deliveries >= 1);
    });
}

#[test]
fn handshake_rejects_mismatched_certificate() {
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

        // Derive an unrelated certificate to trigger the mismatch path.
        let bogus = adapter.certificate_from_der(Bytes::from(vec![1, 2, 3, 4]));
        let err = adapter
            .connect(listen_addr, &bogus)
            .await
            .err()
            .expect("handshake fails");
        assert!(err.to_string().contains("handshake failed"));

        // Ensure the real certificate continues to succeed after the failure.
        let conn = adapter
            .connect(listen_addr, &cert)
            .await
            .expect("retry succeeds");
        adapter
            .send(&conn, b"ok")
            .await
            .expect("send after failure");
    });
}

#[test]
fn verify_remote_certificate_matches_generated_material() {
    runtime::block_on(async {
        let cfg = test_config();
        let factory = DefaultFactory::default();
        let registry = factory
            .create(&cfg, &TransportCallbacks::default())
            .expect("registry");
        let adapter = registry.inhouse().expect("inhouse adapter");

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let (_listener, cert) = adapter.listen(addr).await.expect("listen");

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
