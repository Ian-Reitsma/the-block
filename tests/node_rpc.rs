use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use serde_json::Value;
use serial_test::serial;
use the_block::{rpc::spawn_rpc_server, Blockchain};

mod util;

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

    let rpc = |body: &str| {
        let mut stream = TcpStream::connect(&addr).unwrap();
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
    };

    // metrics endpoint
    let val = rpc(r#"{"method":"metrics"}"#);
    #[cfg(feature = "telemetry")]
    assert!(val["result"].as_str().unwrap().contains("mempool_size"));
    #[cfg(not(feature = "telemetry"))]
    assert_eq!(val["result"].as_str().unwrap(), "telemetry disabled");

    // balance query
    let bal = rpc(r#"{"method":"balance","params":{"address":"alice"}}"#);
    assert_eq!(bal["result"]["consumer"].as_u64().unwrap(), 42);

    // start and stop mining
    let start = rpc(r#"{"method":"start_mining","params":{"miner":"alice"}}"#);
    assert_eq!(start["result"]["status"], "ok");
    let stop = rpc(r#"{"method":"stop_mining"}"#);
    assert_eq!(stop["result"]["status"], "ok");
}
