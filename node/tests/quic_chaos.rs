#![cfg(feature = "integration-tests")]
#[cfg(feature = "quic")]
fn env_ratio(var: &str, default: f64) -> f64 {
    std::env::var(var)
        .ok()
        .and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            let ratio = if let Some(percent) = trimmed.strip_suffix('%') {
                percent.parse::<f64>().ok().map(|v| v / 100.0)
            } else {
                trimmed.parse::<f64>().ok()
            }?;
            Some(ratio.clamp(0.0, 1.0))
        })
        .unwrap_or(default)
}

#[cfg(feature = "quic")]
fn generate_cert() -> (Vec<u8>, Vec<u8>) {
    use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};

    let mut params = CertificateParams::new(vec!["the-block".into()]);
    params.alg = &rcgen::PKCS_ED25519;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "the-block chaos harness");
    params.distinguished_name = dn;
    let cert = Certificate::from_params(params).expect("cert params");
    let cert_der = cert.serialize_der().expect("cert der");
    let key_der = cert.serialize_private_key_der();
    (cert_der, key_der)
}

#[cfg(feature = "quic")]
#[test]
fn quic_chaos_smoke() {
    runtime::block_on(async {
        use bytes::Bytes;
        use s2n_quic::provider::io::testing::{self, Model};
        use s2n_quic::{client::Connect, Client, Server};
        use std::sync::{Arc, Mutex, OnceLock};
        use std::time::Duration;

        let loss = env_ratio("TB_QUIC_PACKET_LOSS", 0.05);
        let dup = env_ratio("TB_QUIC_PACKET_DUP", 0.02);

        let model = Model::default();
        model.set_drop_rate(loss);
        model.set_retransmit_rate(dup);
        model.set_delay(Duration::from_millis(10));
        model.set_network_jitter(Duration::from_millis(1));

        let payload = Bytes::from_static(b"chaos payload frame");
        let payload_len = payload.len();
        let ack_bytes = Bytes::from_static(b"ack");

        let delivered = Arc::new(Mutex::new(Vec::new()));
        let acked = Arc::new(Mutex::new(Vec::new()));
        let server_addr = Arc::new(OnceLock::new());

        let duration = testing::test_seed(model, 0xD15EA5ED, |handle| -> testing::Result<()> {
            let (cert_der, key_der) = generate_cert();

            let server_handle = handle.clone();
            let addr_cell = Arc::clone(&server_addr);
            let server_store = Arc::clone(&delivered);
            let payload_copy = payload.clone();
            let ack_copy = ack_bytes.clone();
            testing::spawn(async move {
                let io = server_handle.builder().build().expect("server io builder");
                let mut server = Server::builder()
                    .with_tls((&cert_der[..], &key_der[..]))
                    .expect("server tls")
                    .with_io(io)
                    .expect("server io")
                    .start()
                    .await
                    .expect("server start");

                let addr = server.local_addr().expect("server addr");
                let _ = addr_cell.set(addr);

                if let Some(mut conn) = server.accept().await {
                    if let Ok(Some(mut stream)) = conn.accept_bidirectional_stream().await {
                        let mut data = Vec::new();
                        while let Some(chunk) = stream.receive().await.expect("server receive") {
                            data.extend_from_slice(&chunk);
                            if data.len() >= payload_copy.len() {
                                break;
                            }
                        }
                        data.truncate(payload_copy.len());
                        stream
                            .send(ack_copy.clone())
                            .await
                            .expect("server send ack");
                        stream.finish().expect("server finish stream");
                        *server_store.lock().expect("store lock") = data;
                    }
                }
            });

            let client_handle = handle.clone();
            let trust_der = cert_der.clone();
            let addr_cell = Arc::clone(&server_addr);
            let ack_store = Arc::clone(&acked);
            let payload_client = payload.clone();
            testing::spawn(async move {
                let io = client_handle.builder().build().expect("client io builder");
                let client = Client::builder()
                    .with_tls(&trust_der[..])
                    .expect("client tls")
                    .with_io(io)
                    .expect("client io")
                    .start()
                    .await
                    .expect("client start");

                let addr = loop {
                    if let Some(addr) = addr_cell.get() {
                        break *addr;
                    }
                    testing::time::delay(Duration::from_millis(1)).await;
                };

                let connection = client
                    .connect(Connect::new(addr, "the-block"))
                    .await
                    .expect("client connect");
                let mut stream = connection
                    .open_bidirectional_stream()
                    .await
                    .expect("open stream");
                stream
                    .send(payload_client.clone())
                    .await
                    .expect("send payload");
                stream.finish().expect("client finish stream");

                let mut ack = Vec::new();
                while let Some(chunk) = stream.receive().await.expect("client receive") {
                    ack.extend_from_slice(&chunk);
                }
                *ack_store.lock().expect("ack lock") = ack;
            });

            Ok(())
        })
        .expect("chaos runtime");

        let observed = delivered.lock().expect("delivered lock").clone();
        assert_eq!(observed.len(), payload_len, "payload truncated");
        assert_eq!(observed, payload.to_vec(), "payload mismatch under chaos");

        let ack = acked.lock().expect("acked lock").clone();
        assert_eq!(ack, ack_bytes.to_vec(), "ack mismatch");
        assert!(duration > Duration::ZERO);
    });
}
