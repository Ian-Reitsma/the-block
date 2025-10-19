#![cfg(feature = "integration-tests")]

use std::sync::{atomic::AtomicBool, Arc, Mutex};

use foundation_serialization::json::{self, json, Value};
use runtime::io::AsyncWriteExt;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::{config::RpcConfig, governance::GovStore, rpc::run_rpc_server, Blockchain};

mod util;
use util::timeout::expect_timeout;

fn rpc(addr: &str, payload: Value) -> Value {
    runtime::block_on(async {
        use runtime::io::read_to_end;
        use runtime::net::TcpStream;
        use std::net::SocketAddr;

        let addr: SocketAddr = addr.parse().expect("invalid socket addr");
        let mut stream = expect_timeout(TcpStream::connect(addr))
            .await
            .expect("connect rpc");
        let body = json::to_string(&payload).expect("serialize payload");
        let request = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        expect_timeout(stream.write_all(request.as_bytes()))
            .await
            .expect("send request");
        let mut resp = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .expect("read response");
        let response = String::from_utf8(resp).expect("response utf8");
        let body_idx = response.find("\r\n\r\n").expect("body delimiter");
        let body = &response[body_idx + 4..];
        json::from_str::<Value>(body).expect("decode response")
    })
}

#[testkit::tb_serial]
fn rpc_treasury_endpoints_surface_history() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_treasury_history");
        let gov_path = dir.path().join("gov.db");
        std::env::set_var(
            "TB_GOVERNANCE_DB_PATH",
            gov_path.to_string_lossy().to_string(),
        );
        let store = GovStore::open(&gov_path);
        store.record_treasury_accrual(1_000).expect("accrual");
        let cancelled = store
            .queue_disbursement("dest-1", 120, "initial", 42)
            .expect("queue cancelled");
        let executed = store
            .queue_disbursement("dest-2", 80, "payout", 55)
            .expect("queue executed");
        store
            .execute_disbursement(executed.id, "0xfeed")
            .expect("execute disbursement");
        store
            .cancel_disbursement(cancelled.id, "duplicate")
            .expect("cancel disbursement");
        store.record_treasury_accrual(275).expect("second accrual");

        let chain_dir = dir.path().join("chain");
        let bc = Arc::new(Mutex::new(Blockchain::new(chain_dir.to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            RpcConfig::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.expect("rpc address");

        let disbursements = rpc(
            &addr,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "gov.treasury.disbursements",
                "params": {"limit": 8}
            }),
        )
        .await;
        let list = disbursements["result"]["disbursements"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        let statuses: Vec<_> = list
            .iter()
            .map(|entry| {
                entry["status"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .next()
                    .unwrap()
                    .clone()
            })
            .collect();
        assert!(statuses.contains(&"executed".to_string()));
        assert!(statuses.contains(&"cancelled".to_string()));
        assert!(disbursements["result"]["next_cursor"].is_null());

        let paged = rpc(
            &addr,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "gov.treasury.disbursements",
                "params": {"limit": 1}
            }),
        )
        .await;
        assert!(paged["result"]["next_cursor"].is_number());

        let balance = rpc(
            &addr,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "gov.treasury.balance"
            }),
        )
        .await;
        let balance_ct = balance["result"]["balance_ct"].as_u64().unwrap();
        assert!(balance_ct >= 1_155);
        let last_snapshot = balance["result"]["last_snapshot"].as_object().unwrap();
        assert_eq!(last_snapshot["event"].as_str(), Some("accrual"));

        let history = rpc(
            &addr,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "gov.treasury.balance_history",
                "params": {"limit": 4}
            }),
        )
        .await;
        let snapshots = history["result"]["snapshots"].as_array().unwrap();
        assert!(!snapshots.is_empty());
        assert_eq!(
            history["result"]["current_balance_ct"].as_u64().unwrap(),
            balance_ct
        );

        Settlement::shutdown();
        handle.abort();
        let _ = handle.await;
        std::env::remove_var("TB_GOVERNANCE_DB_PATH");
    });
}
