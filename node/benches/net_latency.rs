#![cfg(feature = "quic")]

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::{rngs::StdRng, Rng, SeedableRng};
use s2n_quic::provider::io::testing::{self, Model};
use s2n_quic::{client::Connect, Client, Server};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

fn generate_cert() -> (Vec<u8>, Vec<u8>) {
    use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};

    let mut params = CertificateParams::new(vec!["the-block".into()]);
    params.alg = &rcgen::PKCS_ED25519;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "bench harness");
    params.distinguished_name = dn;
    let cert = Certificate::from_params(params).expect("cert params");
    let cert_der = cert.serialize_der().expect("cert der");
    let key_der = cert.serialize_private_key_der();
    (cert_der, key_der)
}

fn quic_latency(loss: f64, dup: f64) -> Duration {
    let model = Model::default();
    model.set_drop_rate(loss);
    model.set_retransmit_rate(dup);
    model.set_delay(Duration::from_millis(5));

    let payload = Bytes::from_static(b"bench-payload");
    let server_addr = Arc::new(OnceLock::new());

    testing::test_seed(model, 0x5eed1234, |handle| -> testing::Result<()> {
        let (cert_der, key_der) = generate_cert();
        let server_handle = handle.clone();
        let addr_cell = Arc::clone(&server_addr);
        testing::spawn(async move {
            let io = server_handle.builder().build().expect("server io");
            let mut server = Server::builder()
                .with_tls((&cert_der[..], &key_der[..]))
                .expect("server tls")
                .with_io(io)
                .expect("server io configure")
                .start()
                .await
                .expect("server start");
            let addr = server.local_addr().expect("server addr");
            let _ = addr_cell.set(addr);

            if let Some(mut conn) = server.accept().await {
                if let Ok(Some(mut stream)) = conn.accept_bidirectional_stream().await {
                    while stream.receive().await.expect("recv chunk").is_some() {}
                    stream.finish().ok();
                }
            }
        });

        let client_handle = handle.clone();
        let trust_der = cert_der.clone();
        let wait_addr = Arc::clone(&server_addr);
        let payload_clone = payload.clone();
        testing::spawn(async move {
            let io = client_handle.builder().build().expect("client io");
            let client = Client::builder()
                .with_tls(&trust_der[..])
                .expect("client tls")
                .with_io(io)
                .expect("client io configure")
                .start()
                .await
                .expect("client start");

            let addr = loop {
                if let Some(addr) = wait_addr.get() {
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
                .send(payload_clone.clone())
                .await
                .expect("send payload");
            stream.finish().expect("client finish");
        });

        Ok(())
    })
    .expect("quic bench runtime")
}

fn tcp_latency(loss: f64, rng: &mut StdRng) -> Duration {
    let base = Duration::from_millis(60);
    let mut attempts = 0;
    loop {
        attempts += 1;
        if rng.gen::<f64>() >= loss {
            break;
        }
    }
    let millis = base.as_secs_f64() * 1000.0 * attempts as f64;
    Duration::from_millis(millis.round() as u64)
}

fn bench_net_latency(c: &mut Criterion) {
    c.bench_function("quic_loss_5_dup_2", |b| {
        b.iter(|| {
            let dur = quic_latency(0.05, 0.02);
            black_box(dur);
        });
    });

    c.bench_function("tcp_loss_5", |b| {
        let mut rng = StdRng::seed_from_u64(0x51eed); // deterministic noise
        b.iter(|| {
            let dur = tcp_latency(0.05, &mut rng);
            black_box(dur);
        });
    });
}

criterion_group!(net_latency, bench_net_latency);
criterion_main!(net_latency);
