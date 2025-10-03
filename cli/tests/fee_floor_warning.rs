mod support;

use contract_cli::rpc::RpcClient;
use contract_cli::tx::FeeLane;
use contract_cli::wallet::{build_tx_default_locale, BuildTxStatus};
use support::json_rpc::JsonRpcMock;

#[test]
fn auto_bump_emits_warning_event() {
    let stats = "{\"jsonrpc\":\"2.0\",\"result\":{\"fee_floor\":10,\"size\":0,\"age_p50\":0,\"age_p95\":0,\"fee_p50\":0,\"fee_p90\":0}}".to_string();
    let ack = "{\"status\":\"ok\"}".to_string();
    let server = JsonRpcMock::start(vec![stats.clone(), ack.clone()]);
    let client = RpcClient::from_env();
    let report = build_tx_default_locale(
        &client,
        server.url(),
        FeeLane::Consumer,
        "alice",
        "bob",
        100,
        2,
        100,
        0,
        &[],
        true,
        false,
        false,
    )
    .expect("build tx");
    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(report.auto_bumped);
    assert_eq!(report.effective_fee, 10);
    let bodies = server.captured();
    assert!(bodies[0].contains("\"method\":\"mempool.stats\""));
    assert!(bodies[1].contains("\"method\":\"mempool.qos_event\""));
    assert!(bodies[1].contains("\"event\":\"warning\""));
}

#[test]
fn force_records_override_metric() {
    let stats = "{\"jsonrpc\":\"2.0\",\"result\":{\"fee_floor\":50,\"size\":0,\"age_p50\":0,\"age_p95\":0,\"fee_p50\":0,\"fee_p90\":0}}".to_string();
    let ack = "{\"status\":\"ok\"}".to_string();
    let server = JsonRpcMock::start(vec![stats, ack]);
    let client = RpcClient::from_env();
    let report = build_tx_default_locale(
        &client,
        server.url(),
        FeeLane::Consumer,
        "carol",
        "dave",
        200,
        5,
        100,
        1,
        &[],
        false,
        true,
        false,
    )
    .expect("build tx");
    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(report.forced);
    assert_eq!(report.effective_fee, 5);
    let bodies = server.captured();
    assert!(bodies[0].contains("\"method\":\"mempool.stats\""));
    assert!(bodies[1].contains("\"event\":\"override\""));
}
