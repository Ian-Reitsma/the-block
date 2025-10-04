mod support;

use contract_cli::compute::{handle_with_writer, ComputeCmd};
use support::json_rpc::JsonRpcMock;

#[test]
fn stats_includes_provider_balances() {
    let stats = "{\"jsonrpc\":\"2.0\",\"result\":{\"industrial_backlog\":3,\"industrial_utilization\":75,\"industrial_units_total\":9,\"industrial_price_per_unit\":21}}".to_string();
    let balances = "{\"jsonrpc\":\"2.0\",\"result\":{\"providers\":[{\"provider\":\"alice\",\"ct\":42,\"industrial\":7}]}}".to_string();
    let server = JsonRpcMock::start(vec![stats, balances]);
    let mut output = Vec::new();
    handle_with_writer(
        ComputeCmd::Stats {
            url: server.url().to_string(),
            accelerator: None,
        },
        &mut output,
    )
    .expect("stats command");
    let bodies = server.captured();
    assert!(bodies[0].contains("\"method\":\"compute_market.stats\""));
    assert!(bodies[1].contains("\"method\":\"compute_market.provider_balances\""));
    let printed = String::from_utf8(output).expect("stdout");
    assert!(printed.contains("provider: alice"));
    assert!(printed.contains("ct: 42"));
    assert!(printed.contains("it: 7"));
}
