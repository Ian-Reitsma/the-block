#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use serde_json::Value;
use serial_test::serial;
use the_block::{rpc::run_rpc_server, Blockchain};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

mod util;

async fn rpc(addr: &str, body: &str) -> Value {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 1024];
    let n = stream.read(&mut resp).await.unwrap();
    let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let val: Value = serde_json::from_slice(&resp[body_idx + 4..n]).unwrap();
    val
}

#[tokio::test]
#[serial]
async fn rpc_rate_limit_and_ban() {
    tokio::time::pause();
    std::env::set_var("TB_RPC_MAX_PER_SEC", "1");
    std::env::set_var("TB_RPC_BAN_SECS", "60");
    let dir = util::temp::temp_dir("rpc_rate_limit");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        tx,
    ));
    let addr = rx.await.unwrap();

    let req = r#"{"method":"metrics"}"#;
    let ok = rpc(&addr, req).await;
    assert!(ok["error"].is_null());
    let limited = rpc(&addr, req).await;
    assert_eq!(limited["error"]["message"], "rate limited");
    assert_eq!(limited["error"]["code"], -32001);
    let banned = rpc(&addr, req).await;
    assert_eq!(banned["error"]["message"], "banned");
    assert_eq!(banned["error"]["code"], -32002);

    handle.abort();
    std::env::remove_var("TB_RPC_MAX_PER_SEC");
    std::env::remove_var("TB_RPC_BAN_SECS");
}
