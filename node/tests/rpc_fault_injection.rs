#![cfg(feature = "integration-tests")]
use serde_json::Value;
use the_block::rpc::client::{RpcClient, RpcClientError};

#[test]
fn env_fault_rate_triggers_request_errors() {
    std::env::set_var("TB_RPC_FAULT_RATE", "1.0");
    let client = RpcClient::from_env();
    std::env::remove_var("TB_RPC_FAULT_RATE");
    let err = client
        .call("http://localhost:1", &Value::Null)
        .expect_err("fault injection should error");
    assert!(matches!(err, RpcClientError::InjectedFault));
}
