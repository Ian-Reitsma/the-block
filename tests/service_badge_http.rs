#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use the_block::{rpc::run_rpc_server, Blockchain};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

mod util;

#[tokio::test]
async fn badge_status_endpoint() {
    let dir = util::temp::temp_dir("badge_status");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".into(),
        tx,
    ));
    let addr = rx.await.unwrap();

    // Initially no badge should be active.
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    stream
        .write_all(b"GET /badge/status HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let mut resp = vec![0u8; 256];
    let n = stream.read(&mut resp).await.unwrap();
    let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&resp[body_idx + 4..n]).unwrap();
    assert!(!body["active"].as_bool().unwrap());
    assert!(body["last_mint"].is_null());
    assert!(body["last_burn"].is_null());

    // Mint a badge and verify the endpoint reflects it.
    {
        let mut chain = bc.lock().unwrap();
        for _ in 0..90 {
            chain
                .badge_tracker_mut()
                .record_epoch(true, std::time::Duration::from_millis(0));
        }
    }

    let mut stream = TcpStream::connect(&addr).await.unwrap();
    stream
        .write_all(b"GET /badge/status HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let n = stream.read(&mut resp).await.unwrap();
    let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&resp[body_idx + 4..n]).unwrap();
    assert!(body["active"].as_bool().unwrap());
    assert!(body["last_mint"].as_u64().is_some());
}
