use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use serial_test::serial;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::{
    config::RpcConfig, generate_keypair, rpc::run_rpc_server, sign_tx, Blockchain, RawTxPayload,
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::timeout::expect_timeout;

mod util;

async fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
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
    expect_timeout(stream.read_to_end(&mut resp)).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let body = &resp[body_idx + 4..];
    serde_json::from_str::<Value>(body).unwrap()
}

#[tokio::test]
#[serial]
async fn rpc_smoke() {
    let dir = util::temp::temp_dir("rpc_smoke");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    {
        let mut guard = bc.lock().unwrap();
        guard.add_account("alice".to_string(), 42, 0).unwrap();
    }
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0);
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token_file = dir.path().join("token");
    std::fs::write(&token_file, "testtoken").unwrap();
    let rpc_cfg = RpcConfig {
        admin_token_file: Some(token_file.to_str().unwrap().to_string()),
        enable_debug: true,
        ..Default::default()
    };
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    // metrics endpoint
    let val = expect_timeout(rpc(&addr, r#"{"method":"metrics"}"#, None)).await;
    #[cfg(feature = "telemetry")]
    assert!(val["result"].as_str().unwrap().contains("mempool_size"));
    #[cfg(not(feature = "telemetry"))]
    assert_eq!(val["result"].as_str().unwrap(), "telemetry disabled");

    // balance query
    let bal = expect_timeout(rpc(
        &addr,
        r#"{"method":"balance","params":{"address":"alice"}}"#,
        None,
    ))
    .await;
    assert_eq!(bal["result"]["consumer"].as_u64().unwrap(), 42);

    // settlement status
    let status = expect_timeout(rpc(&addr, r#"{"method":"settlement_status"}"#, None)).await;
    let mode = status["result"]["mode"]
        .as_str()
        .or_else(|| status["result"].as_str());
    assert_eq!(mode, Some("dryrun"));

    // start and stop mining
    let start = expect_timeout(rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"alice","nonce":1}}"#,
        Some("testtoken"),
    ))
    .await;
    assert_eq!(start["result"]["status"], "ok");
    let stop = expect_timeout(rpc(
        &addr,
        r#"{"method":"stop_mining","params":{"nonce":2}}"#,
        Some("testtoken"),
    ))
    .await;
    assert_eq!(stop["result"]["status"], "ok");
    Settlement::shutdown();

    handle.abort();
}

#[tokio::test]
#[serial]
async fn rpc_nonce_replay_rejected() {
    let dir = util::temp::temp_dir("rpc_nonce_replay");
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
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    let first = expect_timeout(rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"alice","nonce":1}}"#,
        Some("testtoken"),
    ))
    .await;
    assert_eq!(first["result"]["status"], "ok");
    let replay = expect_timeout(rpc(
        &addr,
        r#"{"method":"stop_mining","params":{"nonce":1}}"#,
        Some("testtoken"),
    ))
    .await;
    assert_eq!(
        replay["error"]["message"].as_str().unwrap(),
        "replayed nonce"
    );

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
#[serial]
async fn rpc_concurrent_controls() {
    let dir = util::temp::temp_dir("rpc_concurrent");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    {
        let mut guard = bc.lock().unwrap();
        guard
            .add_account("alice".to_string(), 1_000_000, 0)
            .unwrap();
        guard.add_account("bob".to_string(), 0, 0).unwrap();
        guard.mine_block("alice").unwrap();
    }
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token_file = dir.path().join("token");
    std::fs::write(&token_file, "testtoken").unwrap();
    let rpc_cfg = RpcConfig {
        admin_token_file: Some(token_file.to_str().unwrap().to_string()),
        enable_debug: true,
        ..Default::default()
    };
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    let tx_hex = hex::encode(bincode::serialize(&tx).unwrap());
    let tx_arc = Arc::new(tx_hex);

    let mut handles = Vec::new();
    for i in 0..6 {
        let addr = addr.clone();
        let tx = Arc::clone(&tx_arc);
        handles.push(tokio::spawn(async move {
            let body = match i % 3 {
                0 => format!(
                    "{{\"method\":\"start_mining\",\"params\":{{\"miner\":\"alice\",\"nonce\":{i}}}}}",
                    i = i
                ),
                1 => format!("{{\"method\":\"stop_mining\",\"params\":{{\"nonce\":{i}}}}}", i = i),
                _ => format!(
                    "{{\"method\":\"submit_tx\",\"params\":{{\"tx\":\"{tx}\",\"nonce\":{i}}}}}",
                    tx = tx,
                    i = i
                ),
            };
            let _ = expect_timeout(rpc(&addr, &body, Some("testtoken"))).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    let _ = expect_timeout(rpc(
        &addr,
        r#"{"method":"stop_mining","params":{"nonce":999}}"#,
        Some("testtoken"),
    ))
    .await;
    assert!(bc.lock().unwrap().mempool_consumer.len() <= 1);

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
#[serial]
async fn rpc_error_responses() {
    let dir = util::temp::temp_dir("rpc_errors");
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
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    // malformed JSON
    let mut stream = expect_timeout(TcpStream::connect(&addr)).await.unwrap();
    let bad = "{\"method\":\"balance\""; // missing closing brace
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        bad.len(),
        bad
    );
    expect_timeout(stream.write_all(req.as_bytes()))
        .await
        .unwrap();
    let mut resp = Vec::new();
    expect_timeout(stream.read_to_end(&mut resp)).await.unwrap();
    let body = String::from_utf8(resp).unwrap();
    let body = body.split("\r\n\r\n").nth(1).unwrap();
    let val: Value = serde_json::from_str(body).unwrap();
    assert_eq!(val["error"]["code"].as_i64().unwrap(), -32700);

    // unknown method
    let val = expect_timeout(rpc(&addr, r#"{"method":"unknown"}"#, None)).await;
    assert_eq!(val["error"]["code"].as_i64().unwrap(), -32601);

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
#[serial]
async fn rpc_fragmented_request() {
    let dir = util::temp::temp_dir("rpc_fragmented");
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
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    let body = r#"{"method":"stop_mining","params":{"nonce":1}}"#;
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer testtoken\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    let mid = req.len() / 2;
    stream.write_all(&req.as_bytes()[..mid]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    stream.write_all(&req.as_bytes()[mid..]).await.unwrap();
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let val: Value = serde_json::from_str(&resp[body_idx + 4..]).unwrap();
    assert_eq!(val["result"]["status"], "ok");

    handle.abort();
}
