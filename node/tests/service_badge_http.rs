#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use foundation_serialization::json::Value;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};

use runtime::io::read_to_end;
use runtime::net::TcpStream;
use std::net::SocketAddr;
use util::timeout::expect_timeout;

mod util;

#[test]
fn badge_status_endpoint() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("badge_status");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".into(),
            RpcConfig::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        // Initially no badge should be active.
        let addr_socket: SocketAddr = addr.parse().unwrap();
        let mut stream = expect_timeout(TcpStream::connect(addr_socket))
            .await
            .unwrap();
        expect_timeout(stream.write_all(b"GET /badge/status HTTP/1.1\r\nHost: localhost\r\n\r\n"))
            .await
            .unwrap();
        let mut resp = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body: Value =
            foundation_serialization::json::from_slice(&resp[body_idx + 4..]).unwrap();
        assert!(!body["active"].as_bool().unwrap());
        assert!(matches!(body.get("last_mint"), Some(Value::Null)));
        assert!(matches!(body.get("last_burn"), Some(Value::Null)));

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

        let mut stream = expect_timeout(TcpStream::connect(addr_socket))
            .await
            .unwrap();
        expect_timeout(stream.write_all(b"GET /badge/status HTTP/1.1\r\nHost: localhost\r\n\r\n"))
            .await
            .unwrap();
        resp.clear();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let body_idx = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body: Value =
            foundation_serialization::json::from_slice(&resp[body_idx + 4..]).unwrap();
        assert!(body["active"].as_bool().unwrap());
        assert!(body["last_mint"].as_u64().is_some());

        handle.abort();
        let _ = handle.await;
    });
}
