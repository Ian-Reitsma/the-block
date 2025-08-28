use std::sync::{atomic::AtomicBool, Arc, Mutex};

use serde_json::Value;
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

mod util;

async fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {}\r\n", t));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    serde_json::from_str(&resp[body_idx + 4..]).unwrap()
}

#[tokio::test]
async fn rpc_auth_and_host_filters() {
    let dir = util::temp::temp_dir("rpc_security");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token_file = dir.path().join("token");
    std::fs::write(&token_file, "testtoken").unwrap();
    let rpc_cfg = RpcConfig {
        admin_token_file: Some(token_file.to_str().unwrap().to_string()),
        enable_debug: true,
        ..Default::default()
    };
    tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = rx.await.unwrap();

    // host filter
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    stream
        .write_all(b"POST / HTTP/1.1\r\nHost: evil.com\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let resp = String::from_utf8(buf).unwrap();
    assert!(resp.starts_with("HTTP/1.1 403"));

    // admin without token
    let val = rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"a","nonce":1}}"#,
        None,
    )
    .await;
    assert!(val["error"].is_object());

    // admin with token
    let val = rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"a","nonce":2}}"#,
        Some("testtoken"),
    )
    .await;
    assert_eq!(val["result"]["status"], "ok");
}
