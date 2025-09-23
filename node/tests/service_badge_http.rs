#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::timeout::expect_timeout;

mod util;

#[tokio::test]
async fn badge_status_endpoint() {
    let dir = util::temp::temp_dir("badge_status");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".into(),
        RpcConfig::default(),
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    // Initially no badge should be active.
    let mut stream = expect_timeout(TcpStream::connect(&addr)).await.unwrap();
    expect_timeout(stream.write_all(b"GET /badge/status HTTP/1.1\r\nHost: localhost\r\n\r\n"))
        .await
        .unwrap();
    let mut resp = vec![0u8; 256];
    let n = expect_timeout(stream.read(&mut resp)).await.unwrap();
    let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&resp[body_idx + 4..n]).unwrap();
    assert!(!body["active"].as_bool().unwrap());
    assert!(body["last_mint"].is_null());
    assert!(body["last_burn"].is_null());

    // Mint a badge and verify the endpoint reflects it.
    {
        let mut chain = bc.lock().unwrap();
        for _ in 0..90 {
            chain.badge_tracker_mut().record_epoch(
                "node",
                true,
                std::time::Duration::from_millis(0),
            );
        }
    }

    let mut stream = expect_timeout(TcpStream::connect(&addr)).await.unwrap();
    expect_timeout(stream.write_all(b"GET /badge/status HTTP/1.1\r\nHost: localhost\r\n\r\n"))
        .await
        .unwrap();
    let n = expect_timeout(stream.read(&mut resp)).await.unwrap();
    let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&resp[body_idx + 4..n]).unwrap();
    assert!(body["active"].as_bool().unwrap());
    assert!(body["last_mint"].as_u64().is_some());

    handle.abort();
    let _ = handle.await;
}
