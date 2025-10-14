#![cfg(all(feature = "quinn", feature = "inhouse"))]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use transport::{
    Config, ConnectionHandle, DefaultFactory, ProviderKind, RetryPolicy, TransportCallbacks,
    TransportFactory,
};

fn config_for(kind: ProviderKind) -> Config {
    Config {
        provider: kind,
        certificate_cache: None,
        retry: RetryPolicy {
            attempts: 1,
            backoff: Duration::from_millis(10),
        },
        handshake_timeout: Duration::from_millis(750),
        tls: Default::default(),
    }
}

#[test]
fn quinn_rejects_inhouse_handles() {
    runtime::block_on(async {
        let factory = DefaultFactory::default();

        // Prepare an in-house listener and certificate handle.
        let inhouse_cfg = config_for(ProviderKind::Inhouse);
        let inhouse_registry = factory
            .create(&inhouse_cfg, &TransportCallbacks::default())
            .expect("inhouse registry");
        let inhouse = inhouse_registry.inhouse().expect("inhouse adapter");

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let (listener, cert) = inhouse.listen(addr).await.expect("listen");
        let listen_addr = listener
            .as_inhouse()
            .expect("inhouse listener")
            .local_addr();

        let inhouse_conn = inhouse
            .connect(listen_addr, &cert)
            .await
            .expect("establish inhouse connection");

        // Instantiate the Quinn adapter under test.
        let quinn_cfg = config_for(ProviderKind::Quinn);
        let quinn_registry = factory
            .create(&quinn_cfg, &TransportCallbacks::default())
            .expect("quinn registry");
        let quinn = quinn_registry.quinn().expect("quinn adapter");

        let err = quinn
            .connect(listen_addr, &cert)
            .await
            .err()
            .expect("quinn rejects inhouse cert");
        let message = err.to_string();
        assert!(message.contains("certificate incompatible"));

        if let ConnectionHandle::Inhouse(conn) = &inhouse_conn {
            let send_err = quinn
                .send(&inhouse_conn, b"probe")
                .await
                .expect_err("quinn send rejects inhouse connection");
            assert!(send_err
                .to_string()
                .contains("connection incompatible with quinn provider"));

            let payload = quinn.recv(&inhouse_conn).await;
            assert!(payload.is_none());

            // Drop via quinn adapter should not panic when handed an inhouse address.
            quinn.drop_connection(&listen_addr);

            // The inhouse adapter still owns the connection and can inspect its metadata.
            assert_eq!(conn.peer_addr(), listen_addr);
        } else {
            panic!("expected inhouse connection handle");
        }

        inhouse.drop_connection(&listen_addr);
    });
}
