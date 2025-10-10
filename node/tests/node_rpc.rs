#![cfg(feature = "integration-tests")]
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::Duration;

use foundation_serialization::{binary, json::Value};
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::{
    config::RpcConfig, generate_keypair, rpc::run_rpc_server, sign_tx, Blockchain, RawTxPayload,
};

use runtime::{io::read_to_end, net::TcpStream};
use std::net::SocketAddr;
use util::timeout::expect_timeout;

mod util;

fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
    runtime::block_on(async {
        let addr: SocketAddr = addr.parse().unwrap();
        let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
        let mut req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(t) = token {
            req.push_str(&format!("Authorization: Bearer {}\r\n", t));
        }
        req.push_str("\r\n");
        req.push_str(body);
        expect_timeout(stream.write_all(req.as_bytes()))
            .await
            .unwrap();
        let mut resp = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let resp = String::from_utf8(resp).unwrap();
        let body_idx = resp.find("\r\n\r\n").unwrap();
        let body = &resp[body_idx + 4..];
        foundation_serialization::json::from_str::<Value>(body).unwrap()
    })
}

#[testkit::tb_serial]
fn rpc_smoke() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_smoke");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        {
            let mut guard = bc.lock().unwrap();
            guard.add_account("alice".to_string(), 42, 0).unwrap();
        }
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();
        let addr_socket: SocketAddr = addr.parse().unwrap();

        // metrics endpoint
        let val = expect_timeout(rpc(&addr, r#"{"method":"metrics"}"#, None)).await;
        #[cfg(feature = "telemetry")]
        assert!(val["result"].as_str().unwrap().contains("mempool_size"));
        #[cfg(not(feature = "telemetry"))]
        assert_eq!(val["result"].as_str().unwrap(), "telemetry disabled");

        // balance query
        let bal = expect_timeout(rpc(
            &addr,
            r#"{"method":"balance","params":{"address":"alice"}}"#,
            None,
        ))
        .await;
        assert_eq!(bal["result"]["consumer"].as_u64().unwrap(), 42);

        // settlement status
        let status = expect_timeout(rpc(&addr, r#"{"method":"settlement_status"}"#, None)).await;
        let mode = status["result"]["mode"]
            .as_str()
            .or_else(|| status["result"].as_str());
        assert_eq!(mode, Some("dryrun"));

        // start and stop mining
        let start = expect_timeout(rpc(
            &addr,
            r#"{"method":"start_mining","params":{"miner":"alice","nonce":1}}"#,
            Some("testtoken"),
        ))
        .await;
        assert_eq!(start["result"]["status"], "ok");
        let stop = expect_timeout(rpc(
            &addr,
            r#"{"method":"stop_mining","params":{"nonce":2}}"#,
            Some("testtoken"),
        ))
        .await;
        assert_eq!(stop["result"]["status"], "ok");
        Settlement::shutdown();

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_nonce_replay_rejected() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_nonce_replay");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let start = expect_timeout(rpc(
            &addr,
            r#"{"method":"start_mining","params":{"miner":"alice","nonce":1}}"#,
            Some("testtoken"),
        ))
        .await;
        assert_eq!(start["result"]["status"], "ok");
        let stop = expect_timeout(rpc(
            &addr,
            r#"{"method":"stop_mining","params":{"nonce":2}}"#,
            Some("testtoken"),
        ))
        .await;
        assert_eq!(stop["result"]["status"], "ok");
        let replay = expect_timeout(rpc(
            &addr,
            r#"{"method":"stop_mining","params":{"nonce":2}}"#,
            Some("testtoken"),
        ))
        .await;
        assert_eq!(replay["error"]["message"].as_str(), Some("replayed nonce"));

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_light_client_rebate_status() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_rebate_status");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        {
            let mut guard = bc.lock().unwrap();
            guard.record_proof_relay(b"relay", 3);
        }
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let rpc_cfg = RpcConfig {
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let status = expect_timeout(rpc(
            &addr,
            r#"{"method":"light_client.rebate_status"}"#,
            None,
        ))
        .await;
        let result = status.get("result").expect("result");
        assert_eq!(result["pending_total"].as_u64().unwrap(), 3);
        let relayers = result["relayers"].as_array().expect("array");
        assert_eq!(relayers.len(), 1);
        let relayer = &relayers[0];
        let expected_id = crypto_suite::hex::encode(b"relay");
        assert_eq!(relayer["id"].as_str(), Some(expected_id.as_str()));
        assert_eq!(relayer["pending"].as_u64().unwrap(), 3);
        assert_eq!(relayer["total_proofs"].as_u64().unwrap(), 3);
        assert_eq!(relayer["total_claimed"].as_u64().unwrap(), 0);
        assert!(relayer.get("last_claim_height").is_none());

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_light_client_rebate_history() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_rebate_history");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        {
            let mut guard = bc.lock().unwrap();
            guard
                .add_account("miner".to_string(), 0, 0)
                .expect("add miner");
            guard.record_proof_relay(b"relay", 5);
            guard.mine_block("miner").expect("mine block");
        }
        {
            let guard = bc.lock().unwrap();
            let page = guard.proof_tracker.receipt_history(None, None, 10);
            assert_eq!(page.receipts.len(), 1);
        }
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let rpc_cfg = RpcConfig {
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let history = expect_timeout(rpc(
            &addr,
            r#"{"method":"light_client.rebate_history","params":{"limit":10}}"#,
            None,
        ))
        .await;
        let result = history.get("result").expect("result");
        let receipts = result["receipts"].as_array().expect("array");
        assert_eq!(receipts.len(), 1);
        let receipt = &receipts[0];
        assert_eq!(receipt["height"].as_u64().unwrap(), 0);
        assert_eq!(receipt["amount"].as_u64().unwrap(), 5);
        let relayers = receipt["relayers"].as_array().expect("relayers");
        assert_eq!(relayers.len(), 1);
        let relayer = &relayers[0];
        assert_eq!(
            relayer["id"].as_str().unwrap(),
            crypto_suite::hex::encode(b"relay")
        );
        assert_eq!(relayer["amount"].as_u64().unwrap(), 5);

        let filtered = expect_timeout(rpc(
        &addr,
        &format!(
            "{{\"method\":\"light_client.rebate_history\",\"params\":{{\"relayer\":\"{}\",\"limit\":10}}}}",
            crypto_suite::hex::encode(b"relay")
        ),
        None,
    ))
    .await;
        let filtered_receipts = filtered["result"]["receipts"].as_array().unwrap();
        assert_eq!(filtered_receipts.len(), 1);

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_concurrent_controls() {
    runtime::block_on(async {
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
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let (sk, _pk) = generate_keypair();
        let payload = RawTxPayload {
            from_: "alice".into(),
            to: "bob".into(),
            amount_consumer: 1,
            amount_industrial: 0,
            fee: 1000,
            pct_ct: 100,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        let tx_hex = crypto_suite::hex::encode(binary::encode(&tx).unwrap());
        let tx_arc = Arc::new(tx_hex);

        let mut handles = Vec::new();
        for i in 0..6 {
            let addr = addr.clone();
            let tx = Arc::clone(&tx_arc);
            handles.push(the_block::spawn(async move {
            let body = match i % 3 {
                0 => format!(
                    "{{\"method\":\"start_mining\",\"params\":{{\"miner\":\"alice\",\"nonce\":{i}}}}}",
                    i = i
                ),
                1 => format!("{{\"method\":\"stop_mining\",\"params\":{{\"nonce\":{i}}}}}", i = i),
                _ => format!(
                    "{{\"method\":\"submit_tx\",\"params\":{{\"tx\":\"{tx}\",\"nonce\":{i}}}}}",
                    tx = tx,
                    i = i
                ),
            };
            let _ = expect_timeout(rpc(&addr, &body, Some("testtoken"))).await;
        }));
        }
        for h in handles {
            let _ = h.await;
        }
        let _ = expect_timeout(rpc(
            &addr,
            r#"{"method":"stop_mining","params":{"nonce":999}}"#,
            Some("testtoken"),
        ))
        .await;
        assert!(bc.lock().unwrap().mempool_consumer.len() <= 1);

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_error_responses() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_errors");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        // malformed JSON
        let addr_socket: SocketAddr = addr.parse().unwrap();
        let mut stream = expect_timeout(TcpStream::connect(addr_socket))
            .await
            .unwrap();
        let bad = "{\"method\":\"balance\""; // missing closing brace
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            bad.len(),
            bad
        );
        expect_timeout(stream.write_all(req.as_bytes()))
            .await
            .unwrap();
        let mut resp = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let body = String::from_utf8(resp).unwrap();
        let body = body.split("\r\n\r\n").nth(1).unwrap();
        let val: Value = foundation_serialization::json::from_str(body).unwrap();
        assert_eq!(val["error"]["code"].as_i64().unwrap(), -32700);

        // unknown method
        let val = expect_timeout(rpc(&addr, r#"{"method":"unknown"}"#, None)).await;
        assert_eq!(val["error"]["code"].as_i64().unwrap(), -32601);

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_fragmented_request() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_fragmented");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let body = r#"{"method":"stop_mining","params":{"nonce":1}}"#;
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer testtoken\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let addr_socket: SocketAddr = addr.parse().unwrap();
        let mut stream = TcpStream::connect(addr_socket).await.unwrap();
        let mid = req.len() / 2;
        stream.write_all(&req.as_bytes()[..mid]).await.unwrap();
        the_block::sleep(Duration::from_millis(5)).await;
        stream.write_all(&req.as_bytes()[mid..]).await.unwrap();
        let mut resp = Vec::new();
        read_to_end(&mut stream, &mut resp).await.unwrap();
        let resp = String::from_utf8(resp).unwrap();
        let body_idx = resp.find("\r\n\r\n").unwrap();
        let val: Value = foundation_serialization::json::from_str(&resp[body_idx + 4..]).unwrap();
        assert_eq!(val["result"]["status"], "ok");

        handle.abort();
        let _ = handle.await;
    });
}
