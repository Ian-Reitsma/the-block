#![cfg(feature = "integration-tests")]

use std::sync::{atomic::AtomicBool, Arc, Mutex};

use foundation_serialization::{
    json::{self, Map, Number, Value},
    Deserialize,
};
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

fn request(method: &str, params: Option<Value>) -> Value {
    let mut envelope = Map::new();
    envelope.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    envelope.insert("id".to_string(), Value::Number(Number::from(1)));
    envelope.insert("method".to_string(), Value::String(method.to_string()));
    if let Some(p) = params {
        envelope.insert("params".to_string(), p);
    }
    Value::Object(envelope)
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

        let mut limit_params = Map::new();
        limit_params.insert("limit".to_string(), Value::Number(Number::from(8)));
        let disbursements = rpc(
            &addr,
            request(
                "gov.treasury.disbursements",
                Some(Value::Object(limit_params)),
            ),
        );
        let disbursements: RpcSuccess<TreasuryDisbursementsPayload> =
            json::from_value(disbursements).expect("disbursement payload");
        let list = disbursements.result.disbursements;
        assert_eq!(list.len(), 2);
        let mut executed = false;
        let mut cancelled = false;
        for entry in &list {
            match entry.status {
                DisbursementStatus::Executed { .. } => executed = true,
                DisbursementStatus::Cancelled { .. } => cancelled = true,
                DisbursementStatus::Scheduled => {}
            }
        }
        assert!(executed, "missing executed disbursement");
        assert!(cancelled, "missing cancelled disbursement");
        assert!(disbursements.result.next_cursor.is_none());

        let mut paged_params = Map::new();
        paged_params.insert("limit".to_string(), Value::Number(Number::from(1)));
        let paged = rpc(
            &addr,
            request(
                "gov.treasury.disbursements",
                Some(Value::Object(paged_params)),
            ),
        );
        let paged: RpcSuccess<TreasuryDisbursementsPayload> =
            json::from_value(paged).expect("paged disbursements");
        assert!(paged.result.next_cursor.is_some());

        let balance = rpc(&addr, request("gov.treasury.balance", None));
        let balance: RpcSuccess<TreasuryBalancePayload> =
            json::from_value(balance).expect("balance payload");
        let balance_ct = balance.result.balance_ct;
        assert!(balance_ct >= 1_155);
        let last_snapshot = balance
            .result
            .last_snapshot
            .as_ref()
            .expect("last snapshot present");
        assert!(matches!(last_snapshot.event, BalanceEventKind::Accrual));

        let mut history_params = Map::new();
        history_params.insert("limit".to_string(), Value::Number(Number::from(4)));
        let history = rpc(
            &addr,
            request(
                "gov.treasury.balance_history",
                Some(Value::Object(history_params)),
            ),
        );
        let history: RpcSuccess<TreasuryBalanceHistoryPayload> =
            json::from_value(history).expect("history payload");
        assert!(!history.result.snapshots.is_empty());
        assert_eq!(history.result.current_balance_ct, balance_ct);

        Settlement::shutdown();
        handle.abort();
        let _ = handle.await;
        std::env::remove_var("TB_GOVERNANCE_DB_PATH");
    });
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct RpcSuccess<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TreasuryDisbursementsPayload {
    disbursements: Vec<TreasuryDisbursementRecord>,
    #[serde(default)]
    next_cursor: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TreasuryDisbursementRecord {
    id: u64,
    destination: String,
    amount_ct: u64,
    memo: String,
    scheduled_epoch: u64,
    created_at: u64,
    status: DisbursementStatus,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(tag = "state", rename_all = "snake_case")]
enum DisbursementStatus {
    Scheduled,
    Executed { tx_hash: String, executed_at: u64 },
    Cancelled { reason: String, cancelled_at: u64 },
}

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TreasuryBalancePayload {
    balance_ct: u64,
    #[serde(default)]
    last_snapshot: Option<TreasuryBalanceSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TreasuryBalanceSnapshot {
    id: u64,
    balance_ct: u64,
    delta_ct: i64,
    recorded_at: u64,
    event: BalanceEventKind,
    #[serde(default)]
    disbursement_id: Option<u64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
#[serde(rename_all = "snake_case")]
enum BalanceEventKind {
    Accrual,
    Queued,
    Executed,
    Cancelled,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TreasuryBalanceHistoryPayload {
    snapshots: Vec<TreasuryBalanceSnapshot>,
    #[serde(default)]
    next_cursor: Option<u64>,
    current_balance_ct: u64,
}
