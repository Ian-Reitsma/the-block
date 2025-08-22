#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use serde_json::Value;
use serial_test::serial;
use the_block::{config::NodeConfig, rpc::run_rpc_server, Blockchain, DEFAULT_SNAPSHOT_INTERVAL, telemetry};

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
async fn snapshot_interval_persist() {
    std::env::set_var("TB_PRESERVE", "1");
    let dir = util::temp::temp_dir("snapshot_interval_rpc");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    std::fs::create_dir_all(dir.path()).unwrap();
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        tx,
    ));
    let addr = rx.await.unwrap();

    let small = rpc(
        &addr,
        r#"{"method":"set_snapshot_interval","params":{"interval":5}}"#,
    )
    .await;
    assert_eq!(small["error"]["message"], "interval too small");

    let ok = rpc(
        &addr,
        r#"{"method":"set_snapshot_interval","params":{"interval":20}}"#,
    )
    .await;
    assert!(ok["error"].is_null());

    handle.abort();
    let _ = handle.await;

    let cfg = NodeConfig::load(&bc.lock().unwrap().path);
    assert_eq!(cfg.snapshot_interval, 20);

    drop(bc);
    let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(reopened.config.snapshot_interval, 20);
}

#[tokio::test]
#[serial]
async fn snapshot_interval_restart_cycle() {
    std::env::set_var("TB_PRESERVE", "1");
    let dir = util::temp::temp_dir("snapshot_interval_cycle");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(Arc::clone(&bc), Arc::clone(&mining), "127.0.0.1:0".to_string(), tx));
    let addr = rx.await.unwrap();

    let _ = rpc(&addr, r#"{"method":"set_snapshot_interval","params":{"interval":30}}"#).await;
    handle.abort();
    let _ = handle.await;
    drop(bc);
    let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(reopened.config.snapshot_interval, 30);
    assert_eq!(telemetry::SNAPSHOT_INTERVAL.get(), 30);
    let bc = Arc::new(Mutex::new(reopened));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(Arc::clone(&bc), Arc::clone(&mining), "127.0.0.1:0".to_string(), tx));
    let addr = rx.await.unwrap();
    assert_eq!(telemetry::SNAPSHOT_INTERVAL.get(), 30);

    let _ = rpc(&addr, r#"{"method":"set_snapshot_interval","params":{"interval":40}}"#).await;
    handle.abort();
    let _ = handle.await;
    let cfg = NodeConfig::load(dir.path().to_str().unwrap());
    assert_eq!(cfg.snapshot_interval, 40);
    assert_eq!(telemetry::SNAPSHOT_INTERVAL.get(), 40);
}

#[test]
fn snapshot_interval_corrupt_config() {
    std::env::set_var("TB_PRESERVE", "1");
    let dir = util::temp::temp_dir("snapshot_interval_corrupt");
    {
        let bc = Blockchain::new(dir.path().to_str().unwrap());
        let mut cfg = bc.config.clone();
        cfg.snapshot_interval = 50;
        let _ = cfg.save(&bc.path);
    }
    std::fs::write(dir.path().join("config.toml"), b"not toml").unwrap();
    let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(reopened.config.snapshot_interval, DEFAULT_SNAPSHOT_INTERVAL);
}
