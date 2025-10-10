#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use the_block::rpc::limiter::{check_client, ClientState};
use the_block::telemetry;

#[test]
fn limiter_counts_once() {
    let addr: IpAddr = "127.0.0.1".parse().unwrap();
    let clients = Arc::new(Mutex::new(HashMap::<IpAddr, ClientState>::new()));
    telemetry::RPC_RATE_LIMIT_ATTEMPT_TOTAL.reset();
    telemetry::RPC_RATE_LIMIT_REJECT_TOTAL.reset();
    assert!(check_client(&addr, &clients, 1.0, 1, 60).is_ok());
    assert!(check_client(&addr, &clients, 1.0, 1, 60).is_err());
    assert_eq!(telemetry::RPC_RATE_LIMIT_ATTEMPT_TOTAL.value(), 2);
    assert_eq!(telemetry::RPC_RATE_LIMIT_REJECT_TOTAL.value(), 1);
}
