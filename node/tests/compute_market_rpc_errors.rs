#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use foundation_serialization::json::Value;
use runtime::io::read_to_end;
use runtime::net::TcpStream;
use std::net::SocketAddr;
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};
use util::timeout::expect_timeout;

mod util;

fn rpc(addr: &str, body: &str) -> Value {
    runtime::block_on(async {
        let addr: SocketAddr = addr.parse().unwrap();
        let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
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
fn price_board_no_data_errors() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_market_err");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            RpcConfig::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let req = r#"{"method":"price_board_get"}"#;
        let val = rpc(&addr, req);
        assert_eq!(val["error"]["code"].as_i64(), Some(-33000));
        assert_eq!(val["error"]["message"].as_str(), Some("no price data"));
        handle.abort();
        let _ = handle.await;
    });
}
