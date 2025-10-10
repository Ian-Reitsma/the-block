#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::{Duration, Instant};

use foundation_serialization::json::Value;
use the_block::{
    config::NodeConfig, rpc::run_rpc_server, telemetry, Blockchain, DEFAULT_SNAPSHOT_INTERVAL,
};

use runtime::io::read_to_end;
use runtime::net::TcpStream;
use std::net::SocketAddr;
use util::timeout::expect_timeout;

mod util;

fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
    runtime::block_on(async {
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
        let mut resp = Vec::with_capacity(1024);
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        foundation_serialization::json::from_slice(&resp[body_idx + 4..]).unwrap()
    })
}

#[testkit::tb_serial]
#[ignore]
fn snapshot_interval_persist() {
    runtime::block_on(async {
        std::env::set_var("TB_PRESERVE", "1");
        let dir = util::temp::temp_dir("snapshot_interval_rpc");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        std::fs::create_dir_all(dir.path()).unwrap();
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = the_block::config::RpcConfig {
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

        let small = expect_timeout(rpc(
            &addr,
            r#"{"method":"set_snapshot_interval","params":{"interval":5}}"#,
            Some("testtoken"),
        ))
        .await;
        assert_eq!(small["error"]["message"], "interval too small");

        let ok = expect_timeout(rpc(
            &addr,
            r#"{"method":"set_snapshot_interval","params":{"interval":20}}"#,
            Some("testtoken"),
        ))
        .await;
        assert!(ok["error"].is_null());

        handle.abort();
        let _ = handle.await;

        let cfg = NodeConfig::load(&bc.lock().unwrap().path);
        assert_eq!(cfg.snapshot_interval, 20);

        assert_eq!(telemetry::SNAPSHOT_INTERVAL_CHANGED.value(), 20);

        drop(bc);
        let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(reopened.config.snapshot_interval, 20);
    });
}

#[testkit::tb_serial]
#[ignore]
fn snapshot_interval_restart_cycle() {
    runtime::block_on(async {
        std::env::set_var("TB_PRESERVE", "1");
        let start = Instant::now();
        let dir = util::temp::temp_dir("snapshot_interval_cycle");
        let mut logger = Logger::start();
        let mut bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        for interval in [30u64, 40, 50] {
            let mining = Arc::new(AtomicBool::new(false));
            let (tx, rx) = runtime::sync::oneshot::channel();
            let token_file = dir.path().join(format!("token{interval}"));
            std::fs::write(&token_file, "testtoken").unwrap();
            let rpc_cfg = the_block::config::RpcConfig {
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
            let body = format!(
                r#"{{"method":"set_snapshot_interval","params":{{"interval":{interval}}}}}"#
            );
            let _ = expect_timeout(rpc(&addr, &body, Some("testtoken"))).await;
            handle.abort();
            let _ = handle.await;
            diagnostics::log::logger().flush();
            drop(bc);
            let cfg_text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
            assert!(cfg_text.contains(&format!("snapshot_interval = {interval}")));
            let reopened = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
            assert_eq!(reopened.config.snapshot_interval, interval);
            assert_eq!(telemetry::SNAPSHOT_INTERVAL.value(), interval as i64);
            assert_eq!(
                telemetry::SNAPSHOT_INTERVAL_CHANGED.value(),
                interval as i64
            );
            bc = Arc::new(Mutex::new(reopened));
            assert!(logger.any(|r| r.args() == format!("snapshot_interval_changed {interval}")));
        }
        assert!(start.elapsed() < Duration::from_secs(10));
    });
}

#[derive(Default)]
struct Logger {
    records: Vec<LogRecord>,
}

impl Logger {
    fn start() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    fn any<F>(&mut self, mut predicate: F) -> bool
    where
        F: FnMut(&LogRecord) -> bool,
    {
        self.records.iter().any(|rec| predicate(rec))
    }
}

struct LogRecord {
    message: String,
}

impl LogRecord {
    fn args(&self) -> &str {
        &self.message
    }
}

#[test]
#[ignore]
fn snapshot_interval_corrupt_config() {
    // Ensure no leftover TB_SNAPSHOT_INTERVAL from prior tests so the default
    // value is used when the config file is unreadable.
    std::env::remove_var("TB_SNAPSHOT_INTERVAL");
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
        telemetry::SNAPSHOT_INTERVAL.value(),
        DEFAULT_SNAPSHOT_INTERVAL as i64
    );
    assert_eq!(
        telemetry::SNAPSHOT_INTERVAL_CHANGED.value(),
        DEFAULT_SNAPSHOT_INTERVAL as i64
    );
}
