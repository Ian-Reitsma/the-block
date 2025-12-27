#![cfg(feature = "integration-tests")]
use foundation_serialization::json::Value;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    rpc::run_rpc_server,
    Blockchain,
};

mod util;
use util::timeout::expect_timeout;

fn rpc(addr: &str, body: &str) -> foundation_serialization::json::Value {
    runtime::block_on(async {
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
        foundation_serialization::json::from_str::<foundation_serialization::json::Value>(body)
            .unwrap()
    })
}

#[testkit::tb_serial]
fn rpc_inflation_reports_industrial() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_inflation");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            RpcConfig::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let val = rpc(&addr, r#"{"method":"inflation.params"}"#);
        assert!(matches!(
            val["result"]["industrial_multiplier"],
            Value::Number(_)
        ));
        assert!(matches!(
            val["result"]["rent_rate_per_byte"],
            Value::Number(_)
        ));

        let val2 = rpc(&addr, r#"{"method":"compute_market.stats"}"#);
        assert!(matches!(
            val2["result"]["industrial_backlog"],
            Value::Number(_)
        ));
        assert!(matches!(
            val2["result"]["industrial_units_total"],
            Value::Number(_)
        ));
        assert!(matches!(
            val2["result"]["industrial_price_per_unit"],
            Value::Number(_)
        ));

        let balances = rpc(&addr, r#"{"method":"compute_market.provider_balances"}"#);
        assert!(matches!(balances["result"], Value::Array(_)));

        let audit = rpc(&addr, r#"{"method":"compute_market.audit"}"#);
        assert!(matches!(audit["result"], Value::Object(_)));

        let scheduler = rpc(&addr, r#"{"method":"compute_market.scheduler_metrics"}"#);
        assert!(matches!(scheduler["result"], Value::Object(_)));
    });
}
