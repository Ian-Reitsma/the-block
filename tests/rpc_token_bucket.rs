#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use serial_test::serial;
use the_block::telemetry::{RPC_BANS_TOTAL, RPC_TOKENS};
use the_block::{
    rpc::{check_client, run_rpc_server},
    Blockchain,
};

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
    let mut buf = vec![0u8; 0];
    let mut tmp = [0u8; 1024];
    while let Ok(n) = stream.read(&mut tmp).await {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let body_idx = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let headers = &buf[..body_idx];
    let len = headers
        .split(|b| *b == b'\n')
        .find_map(|line| {
            let line = std::str::from_utf8(line).ok()?.trim();
            line.strip_prefix("Content-Length:")?
                .trim()
                .parse::<usize>()
                .ok()
        })
        .unwrap_or(0);
    while buf.len() < body_idx + 4 + len {
        let n = stream.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    serde_json::from_slice(&buf[body_idx + 4..body_idx + 4 + len]).unwrap()
}

#[tokio::test]
#[serial]
async fn rpc_token_bucket_burst() {
    std::env::set_var("TB_RPC_TOKENS_PER_SEC", "5");
    std::env::set_var("TB_RPC_BAN_SECS", "60");
    let dir = util::temp::temp_dir("rpc_token_bucket_burst");
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
    let start = std::time::Instant::now();
    for _ in 0..5 {
        let ok = rpc(&addr, req).await;
        assert!(ok["error"].is_null());
    }
    let limited = rpc(&addr, req).await;
    assert_eq!(limited["error"]["message"], "rate limited");
    let banned = rpc(&addr, req).await;
    assert_eq!(banned["error"]["message"], "banned");
    println!("handled burst in {:?}", start.elapsed());
    handle.abort();
    std::env::remove_var("TB_RPC_TOKENS_PER_SEC");
    std::env::remove_var("TB_RPC_BAN_SECS");
}

#[tokio::test]
#[serial]
async fn rpc_token_bucket_metrics() {
    std::env::set_var("TB_RPC_TOKENS_PER_SEC", "1");
    std::env::set_var("TB_RPC_BAN_SECS", "60");
    RPC_BANS_TOTAL.reset();
    RPC_TOKENS.reset();
    let clients = Arc::new(Mutex::new(HashMap::new()));
    let tokens = 1.0;
    let ban_secs = 60;
    let start = std::time::Instant::now();
    for i in 1..=3u8 {
        let ip = IpAddr::from([127, 0, 0, i]);
        assert!(check_client(&ip, &clients, tokens, ban_secs, 10).is_ok());
        let _ = check_client(&ip, &clients, tokens, ban_secs, 10);
    }
    assert_eq!(RPC_BANS_TOTAL.get(), 3);
    let remaining = RPC_TOKENS.with_label_values(&["127.0.0.1"]).get();
    assert!(remaining < 1.0);
    println!("burst processed in {:?}", start.elapsed());
    std::env::remove_var("TB_RPC_TOKENS_PER_SEC");
    std::env::remove_var("TB_RPC_BAN_SECS");
}

#[tokio::test]
#[serial]
async fn rpc_token_bucket_refill() {
    std::env::set_var("TB_RPC_TOKENS_PER_SEC", "1");
    std::env::set_var("TB_RPC_BAN_SECS", "1");
    let dir = util::temp::temp_dir("rpc_token_bucket_refill");
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
    tokio::time::sleep(Duration::from_secs(1)).await;
    let ok2 = rpc(&addr, req).await;
    assert!(ok2["error"].is_null());

    handle.abort();
    std::env::remove_var("TB_RPC_TOKENS_PER_SEC");
    std::env::remove_var("TB_RPC_BAN_SECS");
}

#[tokio::test]
#[serial]
async fn rpc_token_bucket_eviction() {
    std::env::set_var("TB_RPC_TOKENS_PER_SEC", "1");
    std::env::set_var("TB_RPC_CLIENT_TIMEOUT_SECS", "1");
    let clients = Arc::new(Mutex::new(HashMap::new()));
    let ban_secs = 60;
    let tokens = 1.0;
    for i in 1..6u8 {
        let ip = IpAddr::from([127, 0, 0, i]);
        check_client(&ip, &clients, tokens, ban_secs, 1).unwrap();
    }
    assert!(clients.lock().unwrap().len() <= 5);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let ip = IpAddr::from([127, 0, 0, 99]);
    check_client(&ip, &clients, tokens, ban_secs, 1).unwrap();
    let len = clients.lock().unwrap().len();
    println!("clients after eviction: {len}");
    assert!(len <= 2);
    std::env::remove_var("TB_RPC_TOKENS_PER_SEC");
    std::env::remove_var("TB_RPC_CLIENT_TIMEOUT_SECS");
}
