use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use serial_test::serial;
use the_block::{generate_keypair, rpc::spawn_rpc_server, sign_tx, Blockchain, RawTxPayload};

mod util;

fn rpc(addr: &str, body: &str) -> Value {
    let mut stream = TcpStream::connect(addr).unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let body = &resp[body_idx + 4..];
    serde_json::from_str::<Value>(body).unwrap()
}

#[test]
#[serial]
fn rpc_smoke() {
    let dir = util::temp::temp_dir("rpc_smoke");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    {
        let mut guard = bc.lock().unwrap();
        guard.add_account("alice".to_string(), 42, 0).unwrap();
    }
    let mining = Arc::new(AtomicBool::new(false));
    let (addr, _handle) =
        spawn_rpc_server(Arc::clone(&bc), Arc::clone(&mining), "127.0.0.1:0").unwrap();

    // metrics endpoint
    let val = rpc(&addr, r#"{"method":"metrics"}"#);
    #[cfg(feature = "telemetry")]
    assert!(val["result"].as_str().unwrap().contains("mempool_size"));
    #[cfg(not(feature = "telemetry"))]
    assert_eq!(val["result"].as_str().unwrap(), "telemetry disabled");

    // balance query
    let bal = rpc(
        &addr,
        r#"{"method":"balance","params":{"address":"alice"}}"#,
    );
    assert_eq!(bal["result"]["consumer"].as_u64().unwrap(), 42);

    // start and stop mining
    let start = rpc(
        &addr,
        r#"{"method":"start_mining","params":{"miner":"alice"}}"#,
    );
    assert_eq!(start["result"]["status"], "ok");
    let stop = rpc(&addr, r#"{"method":"stop_mining"}"#);
    assert_eq!(stop["result"]["status"], "ok");
}

#[test]
#[serial]
fn rpc_concurrent_controls() {
    let dir = util::temp::temp_dir("rpc_concurrent");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    {
        let mut guard = bc.lock().unwrap();
        guard
            .add_account("alice".to_string(), 1_000_000, 0)
            .unwrap();
        guard.add_account("bob".to_string(), 0, 0).unwrap();
        guard.mine_block("alice").unwrap();
    }
    let mining = Arc::new(AtomicBool::new(false));
    let (addr, _handle) =
        spawn_rpc_server(Arc::clone(&bc), Arc::clone(&mining), "127.0.0.1:0").unwrap();

    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    let tx_hex = hex::encode(bincode::serialize(&tx).unwrap());
    let tx_arc = Arc::new(tx_hex);
    let handles: Vec<_> = (0..6)
        .map(|i| {
            let addr = addr.clone();
            let tx = Arc::clone(&tx_arc);
            std::thread::spawn(move || {
                let body = match i % 3 {
                    0 => r#"{"method":"start_mining","params":{"miner":"alice"}}"#.to_string(),
                    1 => r#"{"method":"stop_mining"}"#.to_string(),
                    _ => format!("{{\"method\":\"submit_tx\",\"params\":{{\"tx\":\"{tx}\"}}}}"),
                };
                let _ = rpc(&addr, &body);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let _ = rpc(&addr, r#"{"method":"stop_mining"}"#);
    assert!(bc.lock().unwrap().mempool.len() <= 1);
}

#[test]
#[serial]
fn rpc_error_responses() {
    let dir = util::temp::temp_dir("rpc_errors");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (addr, _handle) =
        spawn_rpc_server(Arc::clone(&bc), Arc::clone(&mining), "127.0.0.1:0").unwrap();

    // malformed JSON
    let mut stream = TcpStream::connect(&addr).unwrap();
    let bad = "{\"method\":\"balance\""; // missing closing brace
    let req = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
        bad.len(),
        bad
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    let body = resp.split("\r\n\r\n").nth(1).unwrap();
    let val: Value = serde_json::from_str(body).unwrap();
    assert_eq!(val["error"]["code"].as_i64().unwrap(), -32700);

    // unknown method
    let val = rpc(&addr, r#"{"method":"unknown"}"#);
    assert_eq!(val["error"]["code"].as_i64().unwrap(), -32601);
}

#[test]
#[serial]
fn rpc_fragmented_request() {
    let dir = util::temp::temp_dir("rpc_fragmented");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (addr, _handle) =
        spawn_rpc_server(Arc::clone(&bc), Arc::clone(&mining), "127.0.0.1:0").unwrap();

    let body = r#"{"method":"stop_mining"}"#;
    let req = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = TcpStream::connect(&addr).unwrap();
    let mid = req.len() / 2;
    stream.write_all(&req.as_bytes()[..mid]).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    stream.write_all(&req.as_bytes()[mid..]).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let val: Value = serde_json::from_str(&resp[body_idx + 4..]).unwrap();
    assert_eq!(val["result"]["status"], "ok");
}
