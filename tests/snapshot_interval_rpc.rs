#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use serial_test::serial;
use the_block::{
    config::NodeConfig, rpc::run_rpc_server, telemetry, Blockchain, DEFAULT_SNAPSHOT_INTERVAL,
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

    assert_eq!(telemetry::SNAPSHOT_INTERVAL_CHANGED.get(), 20);

    drop(bc);
    let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(reopened.config.snapshot_interval, 20);
}

#[tokio::test]
#[serial]
async fn snapshot_interval_restart_cycle() {
    std::env::set_var("TB_PRESERVE", "1");
    let start = Instant::now();
    let dir = util::temp::temp_dir("snapshot_interval_cycle");
    let mut logger = logtest::Logger::start();
    let mut bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    for interval in [30u64, 40, 50] {
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            tx,
        ));
        let addr = rx.await.unwrap();
        let body =
            format!(r#"{{"method":"set_snapshot_interval","params":{{"interval":{interval}}}}}"#);
        let _ = rpc(&addr, &body).await;
        handle.abort();
        let _ = handle.await;
        log::logger().flush();
        drop(bc);
        let cfg_text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(cfg_text.contains(&format!("snapshot_interval = {interval}")));
        let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(reopened.config.snapshot_interval, interval);
        assert_eq!(telemetry::SNAPSHOT_INTERVAL.get(), interval as i64);
        assert_eq!(telemetry::SNAPSHOT_INTERVAL_CHANGED.get(), interval as i64);
        bc = Arc::new(Mutex::new(reopened));
        assert!(logger.any(|r| r.args() == format!("snapshot_interval_changed {interval}")));
    }
    assert!(start.elapsed() < Duration::from_secs(10));
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
    assert_eq!(
        telemetry::SNAPSHOT_INTERVAL.get(),
        DEFAULT_SNAPSHOT_INTERVAL as i64
    );
    assert_eq!(
        telemetry::SNAPSHOT_INTERVAL_CHANGED.get(),
        DEFAULT_SNAPSHOT_INTERVAL as i64
    );
}
