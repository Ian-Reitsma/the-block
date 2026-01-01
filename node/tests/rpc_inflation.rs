#![cfg(feature = "integration-tests")]
use foundation_serialization::json::Value;
use std::sync::{atomic::AtomicBool, Arc, Mutex, Once};

use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    rpc::run_rpc_server,
    Blockchain,
};

mod util;
use util::timeout::expect_timeout;

fn configure_runtime() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("TB_RUNTIME_BACKEND", "inhouse");
        the_block::simple_db::configure_engines(the_block::simple_db::EngineConfig {
            default_engine: the_block::simple_db::EngineKind::Memory,
            overrides: Default::default(),
        });
    });
}

async fn expect_timeout_with<F, T>(fut: F, context: &str) -> T
where
    F: std::future::Future<Output = T>,
{
    the_block::timeout(std::time::Duration::from_secs(60), fut)
        .await
        .unwrap_or_else(|_| panic!("operation timed out: {context}"))
}

async fn read_http_body(stream: runtime::net::TcpStream) -> std::io::Result<Vec<u8>> {
    use runtime::io::BufferedTcpStream;
    use std::io::{Error, ErrorKind};

    let mut reader = BufferedTcpStream::new(stream);
    let mut line = String::new();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
        return Err(Error::new(ErrorKind::UnexpectedEof, "missing status line"));
    }
    let mut content_length = None;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "truncated headers"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let len = content_length.unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body).await?;
    }
    Ok(body)
}

async fn rpc(addr: &str, body: &str) -> foundation_serialization::json::Value {
    use runtime::net::TcpStream;
    use std::net::SocketAddr;
    let addr: SocketAddr = addr.parse().unwrap();
    let connect_ctx = format!("connect {body}");
    let write_ctx = format!("write {body}");
    let read_ctx = format!("read {body}");
    let mut stream = expect_timeout_with(TcpStream::connect(addr), &connect_ctx)
        .await
        .unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    expect_timeout_with(stream.write_all(req.as_bytes()), &write_ctx)
        .await
        .unwrap();
    let body = expect_timeout_with(read_http_body(stream), &read_ctx)
        .await
        .unwrap();
    let body = String::from_utf8(body).unwrap();
    foundation_serialization::json::from_str::<foundation_serialization::json::Value>(&body)
        .unwrap()
}

#[testkit::tb_serial]
fn rpc_inflation_reports_industrial() {
    runtime::block_on(async {
        configure_runtime();
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

        let val = rpc(&addr, r#"{"method":"inflation.params"}"#).await;
        let result = val
            .get("result")
            .or_else(|| val.get("Result").and_then(|r| r.get("result")))
            .expect("inflation.params result");
        assert!(matches!(result["industrial_multiplier"], Value::Number(_)));
        assert!(matches!(result["rent_rate_per_byte"], Value::Number(_)));

        let val2 = rpc(&addr, r#"{"method":"compute_market.stats"}"#).await;
        let stats = val2
            .get("result")
            .or_else(|| val2.get("Result").and_then(|r| r.get("result")))
            .expect("compute_market.stats result");
        assert!(matches!(stats["industrial_backlog"], Value::Number(_)));
        assert!(matches!(stats["industrial_units_total"], Value::Number(_)));
        assert!(matches!(
            stats["industrial_price_per_unit"],
            Value::Number(_)
        ));

        let balances = rpc(&addr, r#"{"method":"compute_market.provider_balances"}"#).await;
        let balances_result = balances
            .get("result")
            .or_else(|| balances.get("Result").and_then(|r| r.get("result")))
            .expect("compute_market.provider_balances result");
        match balances_result {
            Value::Array(_) => {}
            Value::Object(map) => {
                assert!(
                    matches!(map.get("providers"), Some(Value::Array(_))),
                    "unexpected provider_balances shape: {balances_result:?}"
                );
            }
            _ => panic!("unexpected provider_balances shape: {balances_result:?}"),
        }

        let audit = rpc(&addr, r#"{"method":"compute_market.audit"}"#).await;
        let audit_result = audit
            .get("result")
            .or_else(|| audit.get("Result").and_then(|r| r.get("result")))
            .expect("compute_market.audit result");
        match audit_result {
            Value::Array(_) => {}
            Value::Object(map) => {
                assert!(
                    matches!(map.get("records"), Some(Value::Array(_))),
                    "unexpected audit shape: {audit_result:?}"
                );
            }
            _ => panic!("unexpected audit shape: {audit_result:?}"),
        }

        let scheduler = rpc(&addr, r#"{"method":"compute_market.scheduler_metrics"}"#).await;
        let scheduler_result = scheduler
            .get("result")
            .or_else(|| scheduler.get("Result").and_then(|r| r.get("result")))
            .expect("compute_market.scheduler_metrics result");
        assert!(matches!(scheduler_result, Value::Object(_)));
    });
}
