#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::HashSet;
use std::env;
use std::fs;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use foundation_rpc::{Request as RpcRequest, Response as RpcResponse};
use foundation_serialization::json::{Map, Value};
use the_block::{
    identity::{did::DidRegistry, handle_registry::HandleRegistry},
    rpc::{fuzz_dispatch_request, fuzz_runtime_config},
    Blockchain,
};
use util::temp::temp_dir;

mod settlement_util;
mod util;
use settlement_util::SettlementCtx;

#[testkit::tb_serial]
fn price_board_no_data_errors() {
    let _ctx = SettlementCtx::new();
    let dir = temp_dir("rpc_market_err");
    let bc = Arc::new(Mutex::new(Blockchain::new(
        dir.path().to_str().expect("blockchain path"),
    )));
    let handles_dir = dir.path().join("handles");
    fs::create_dir_all(&handles_dir).expect("create handles dir");
    let handles = Arc::new(Mutex::new(HandleRegistry::open(
        handles_dir.to_str().expect("handles path"),
    )));

    let did_dir = dir.path().join("did_db");
    fs::create_dir_all(&did_dir).expect("create did dir");
    env::set_var("TB_DID_DB_PATH", did_dir.to_str().expect("did path"));
    let dids = Arc::new(Mutex::new(DidRegistry::open(&DidRegistry::default_path())));
    env::remove_var("TB_DID_DB_PATH");

    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::<(String, u64)>::new()));
    let runtime_cfg = fuzz_runtime_config();
    let mut params = Map::new();
    params.insert("lane".to_string(), Value::String("consumer".into()));
    let request = RpcRequest::new("price_board_get", Value::Object(params));

    let response = fuzz_dispatch_request(
        bc,
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg,
        None,
        None,
        request,
        None,
        None,
    );

    match response {
        RpcResponse::Error { error, .. } => {
            assert_eq!(error.code, -33000);
            assert_eq!(error.message, "no price data");
        }
        other => panic!("expected rpc error, got {other:?}"),
    }
}
