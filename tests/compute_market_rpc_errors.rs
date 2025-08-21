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
    serde_json::from_slice(&resp[body_idx + 4..n]).unwrap()
}

#[tokio::test]
#[serial]
async fn price_board_no_data_errors() {
    let dir = util::temp::temp_dir("rpc_market_err");
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

    let req = r#"{"method":"price_board_get"}"#;
    let val = rpc(&addr, req).await;
    assert_eq!(val["error"]["code"], -33000);
    assert_eq!(val["error"]["message"], "no price data");
    handle.abort();
}
