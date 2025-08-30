use serde_json::Value;
use serial_test::serial;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    rpc::run_rpc_server,
    Blockchain,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::{temp::temp_dir, timeout::expect_timeout};

mod util;

async fn rpc(addr: &str, body: &str) -> Value {
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
    let body = &resp[body_idx + 4..];
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
#[serial]
async fn recent_roots_via_rpc() {
    let dir = temp_dir("microshard_roots");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::tick(1, &[]);
    Settlement::tick(2, &[]);
    Settlement::tick(3, &[]);
    Settlement::shutdown();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let rpc_cfg = RpcConfig::default();
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();
    let body = r#"{"method":"microshard.roots.last","params":{"n":2}}"#;
    let val = expect_timeout(rpc(&addr, body)).await;
    let r3 = hex::encode(blake3::hash(&3u64.to_be_bytes()).as_bytes());
    let r2 = hex::encode(blake3::hash(&2u64.to_be_bytes()).as_bytes());
    assert_eq!(
        val["result"]["roots"],
        Value::Array(vec![Value::String(r3), Value::String(r2)])
    );
    Settlement::shutdown();
    handle.abort();
}
