#![cfg(feature = "integration-tests")]
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use runtime::{io::read_to_end, net::TcpStream};
use serde_json::Value;
use std::net::SocketAddr;
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};
use util::timeout::expect_timeout;

mod util;

async fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
    let addr: SocketAddr = addr.parse().unwrap();
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let mut req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {}\r\n", t));
    }
    req.push_str("\r\n");
    req.push_str(body);
    expect_timeout(stream.write_all(req.as_bytes()))
        .await
        .unwrap();
    let mut resp = Vec::new();
    expect_timeout(read_to_end(&mut stream, &mut resp))
        .await
        .unwrap();
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
        relay_only: false,
        ..Default::default()
    };
    let handle = the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    // host filter
    let addr_socket: SocketAddr = addr.parse().unwrap();
    let mut stream = expect_timeout(TcpStream::connect(addr_socket))
        .await
        .unwrap();
    expect_timeout(
        stream.write_all(b"POST / HTTP/1.1\r\nHost: evil.com\r\nContent-Length: 0\r\n\r\n"),
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    expect_timeout(read_to_end(&mut stream, &mut buf))
        .await
        .unwrap();
    let resp = String::from_utf8(buf).unwrap();
    assert!(resp.starts_with("HTTP/1.1 403"));

    // admin without token
    let val = expect_timeout(rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"a","nonce":1}}"#,
        None,
    ))
    .await;
    assert!(val["error"].is_object());

    // admin with token
    let val = expect_timeout(rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"a","nonce":2}}"#,
        Some("testtoken"),
    ))
    .await;
    assert_eq!(val["result"]["status"], "ok");

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn relay_only_rejects_start_mining() {
    let dir = util::temp::temp_dir("rpc_relay_only");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token_file = dir.path().join("token");
    std::fs::write(&token_file, "relaytoken").unwrap();
    let rpc_cfg = RpcConfig {
        admin_token_file: Some(token_file.to_str().unwrap().to_string()),
        enable_debug: true,
        relay_only: true,
        ..Default::default()
    };
    let handle = the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    let val = expect_timeout(rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"a","nonce":1}}"#,
        Some("relaytoken"),
    ))
    .await;
    assert_eq!(val["result"]["error"]["code"], -32075);
    assert_eq!(val["result"]["error"]["message"], "relay_only");
    assert!(!mining.load(std::sync::atomic::Ordering::SeqCst));

    handle.abort();
    let _ = handle.await;
}
