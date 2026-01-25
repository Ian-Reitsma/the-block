#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::Value;
use runtime::{io::read_to_end, net::TcpStream};
use std::convert::TryInto;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    generate_keypair,
    rpc::run_rpc_server,
    Blockchain,
};
#[path = "util/temp.rs"]
mod temp;
#[path = "util/timeout.rs"]
mod timeout;
use temp::temp_dir;
use timeout::expect_timeout;

async fn rpc(addr: &str, body: &str) -> Value {
    let addr: SocketAddr = addr.parse().unwrap();
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    expect_timeout(stream.write_all(req.as_bytes()))
        .await
        .unwrap();
    let mut resp = Vec::new();
    expect_timeout(read_to_end(&mut stream, &mut resp))
        .await
        .unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let body = &resp[body_idx + 4..];
    foundation_serialization::json::from_str(body).unwrap()
}

#[testkit::tb_serial]
fn dns_publish_invalid_sig_rejected() {
    runtime::block_on(async {
        let dir = temp_dir("dns_publish");
        std::env::set_var(
            "TB_DNS_DB_PATH",
            dir.path().join("dns_db").to_str().unwrap(),
        );
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let rpc_cfg = RpcConfig::default();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let (sk_bytes, _) = generate_keypair();
        let sk_arr: [u8; 32] = sk_bytes.try_into().unwrap();
        let sk = SigningKey::from_bytes(&sk_arr);
        let pk_hex = crypto_suite::hex::encode(sk.verifying_key().to_bytes());
        let bad_sig = vec![0u8; 64];
        let body = format!(
            r#"{{"method":"dns.publish_record","params":{{"domain":"example.com","txt":"hello","pubkey":"{}","sig":"{}"}}}}"#,
            pk_hex,
            crypto_suite::hex::encode(bad_sig)
        );
        let val = rpc(&addr, &body).await;
        // The response structure is {"Error": {"error": {...}}}
        let error_msg = val["Error"]["error"]["message"]
            .as_str()
            .or_else(|| val["error"]["message"].as_str());
        // Check that we got an error response (the specific error message may vary)
        assert!(error_msg.is_some(), "Expected an error response");

        Settlement::shutdown();
        handle.abort();
    });
}
