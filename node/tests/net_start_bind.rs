#![cfg(feature = "integration-tests")]

use diagnostics::internal;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use sys::tempfile::tempdir;
use the_block::{net, net::Node, Blockchain};

#[test]
fn node_start_logs_bind_conflict() {
    let dir = tempdir().expect("tempdir");
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key"));
    std::env::set_var("TB_NET_KEY_SEED", "bind-conflict");
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind holder");
    let addr = listener.local_addr().expect("local addr");

    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let guard = internal::install_subscriber(move |record| {
        if record.target.as_ref() == "net" {
            if let Ok(mut logs) = sink.lock() {
                logs.push(record.message.to_string());
            }
        }
    });

    let node = Node::new(addr, Vec::new(), Blockchain::default());
    let result = node.start();
    assert!(result.is_err(), "binding conflict should surface as error");

    drop(guard);
    drop(listener);

    let logs = captured.lock().expect("log capture");
    assert!(
        logs.iter()
            .any(|msg| msg.contains("gossip_listener_bind_failed")),
        "expected bind failure warning, got {:?}",
        *logs
    );
}
