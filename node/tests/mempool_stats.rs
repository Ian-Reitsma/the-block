use serial_test::serial;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tempfile::tempdir;
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    generate_keypair,
    rpc::run_rpc_server,
    sign_tx, Blockchain, RawTxPayload,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::timeout::expect_timeout;

mod util;

async fn rpc(addr: &str, body: &str) -> serde_json::Value {
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    expect_timeout(stream.write_all(req.as_bytes()))
        .await
        .unwrap();
    let mut resp = Vec::new();
    expect_timeout(stream.read_to_end(&mut resp)).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    serde_json::from_str(&resp[body_idx + 4..]).unwrap()
}

#[tokio::test]
#[serial]
async fn mempool_stats_rpc() {
    let dir = tempdir().unwrap();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    {
        let mut guard = bc.lock().unwrap();
        guard.add_account("alice".into(), 1000, 0).unwrap();
        let (sk, _) = generate_keypair();
        for i in 0..2 {
            let payload = RawTxPayload {
                from_: "alice".into(),
                to: "bob".into(),
                amount_consumer: 1,
                amount_industrial: 0,
                fee: (i + 1) * 10,
                fee_selector: 0,
                nonce: i + 1,
                memo: Vec::new(),
            };
            let tx = sign_tx(sk.to_vec(), payload).unwrap();
            let entry = the_block::MempoolEntry {
                tx,
                timestamp_millis: 0,
                timestamp_ticks: 0,
                serialized_size: 100,
            };
            guard
                .mempool_consumer
                .insert(("alice".into(), i + 1), entry);
        }
    }
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        Default::default(),
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();
    let val = rpc(
        &addr,
        r#"{"method":"mempool.stats","params":{"lane":"consumer"}}"#,
    )
    .await;
    assert_eq!(val["result"]["size"].as_u64().unwrap(), 2);
    assert_eq!(val["result"]["fee_p90"].as_u64().unwrap(), 20);
    handle.abort();
    Settlement::shutdown();
}
