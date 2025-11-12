#![cfg(feature = "integration-tests")]

use diagnostics::internal;
use runtime::sync::oneshot;
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use sys::tempfile::tempdir;
use the_block::config::RpcConfig;
use the_block::rpc;
use the_block::Blockchain;

#[test]
fn rpc_server_logs_bind_conflict() {
    std::env::set_var("TB_RPC_TOKENS_PER_SEC", "100");

    let dir = tempdir().expect("tempdir");
    let cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(dir.path()).expect("chdir temp");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind holder");
    let addr = listener.local_addr().expect("addr");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let guard = internal::install_subscriber(move |record| {
        if record.target.as_ref() == "rpc" {
            if let Ok(mut logs) = sink.lock() {
                logs.push(record.message.to_string());
            }
        }
    });

    let (ready_tx, ready_rx) = oneshot::channel();
    let result = runtime::block_on(async {
        rpc::run_rpc_server_with_market(
            Arc::new(Mutex::new(Blockchain::default())),
            Arc::new(AtomicBool::new(false)),
            None,
            None,
            None,
            addr.to_string(),
            RpcConfig::default(),
            ready_tx,
        )
        .await
    });
    assert!(result.is_err(), "binding conflict should return error");
    drop(ready_rx);

    drop(guard);
    drop(listener);
    std::env::remove_var("TB_RPC_TOKENS_PER_SEC");
    std::env::set_current_dir(cwd).expect("restore cwd");

    let logs = captured.lock().expect("capture");
    assert!(
        logs.iter()
            .any(|msg| msg.contains("rpc_listener_bind_failed")),
        "expected rpc bind warning, got {:?}",
        *logs
    );
}
