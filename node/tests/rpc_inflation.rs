#![cfg(feature = "integration-tests")]
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use serial_test::serial;
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    rpc::run_rpc_server,
    Blockchain,
};

mod util;
use util::timeout::expect_timeout;

async fn rpc(addr: &str, body: &str) -> serde_json::Value {
    use runtime::io::read_to_end;
    use runtime::net::TcpStream;
    use std::net::SocketAddr;
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
    let mut resp = Vec::new();
    expect_timeout(read_to_end(&mut stream, &mut resp))
        .await
        .unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let body = &resp[body_idx + 4..];
    serde_json::from_str::<serde_json::Value>(body).unwrap()
}

#[tokio::test]
#[serial]
async fn rpc_inflation_reports_industrial() {
    let dir = util::temp::temp_dir("rpc_inflation");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        RpcConfig::default(),
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    let val = rpc(&addr, r#"{"method":"inflation.params"}"#).await;
    assert!(val["result"]["industrial_multiplier"].is_number());
    assert!(val["result"]["rent_rate_ct_per_byte"].is_number());

    let val2 = rpc(&addr, r#"{"method":"compute_market.stats"}"#).await;
    assert!(val2["result"]["industrial_backlog"].is_number());
    assert!(val2["result"]["industrial_units_total"].is_number());
    assert!(val2["result"]["industrial_price_per_unit"].is_number());

    let balances = rpc(&addr, r#"{"method":"compute_market.provider_balances"}"#).await;
    assert!(balances["result"].is_array());

    let audit = rpc(&addr, r#"{"method":"compute_market.audit"}"#).await;
    assert!(audit["result"].is_object());

    let scheduler = rpc(&addr, r#"{"method":"compute_market.scheduler_metrics"}"#).await;
    assert!(scheduler["result"].is_object());
}
