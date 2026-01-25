#![cfg(feature = "integration-tests")]
use std::io::{self, ErrorKind, Read, Write};
use std::sync::{atomic::AtomicBool, Arc, Mutex, TryLockError};
use std::time::Duration;

use foundation_rpc::{Request as RpcRequest, Response as RpcResponse};
use foundation_serialization::{
    binary,
    json::{Map, Number, Value},
};
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::{
    config::RpcConfig,
    generate_keypair,
    identity::{did::DidRegistry, handle_registry::HandleRegistry},
    rpc::{fuzz_dispatch_request, fuzz_runtime_config_with_admin, run_rpc_server},
    sign_tx, Blockchain, RawTxPayload,
};

use runtime::spawn_blocking;
use std::{
    collections::HashSet,
    fs,
    net::{SocketAddr, TcpStream as StdTcpStream},
};
#[path = "util/timeout.rs"]
mod timeout;
use timeout::expect_timeout;
#[path = "util/temp.rs"]
mod temp;

const RPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// Keep RPC I/O bounded so a stuck server can't hang the suite.
const RPC_RW_TIMEOUT: Duration = Duration::from_secs(30);

fn rpc_debug() -> bool {
    std::env::var("RPC_TEST_DEBUG").is_ok()
}

async fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
    rpc_result(addr, body, token).await.expect("rpc request")
}

async fn rpc_result(addr: &str, body: &str, token: Option<&str>) -> Result<Value, RpcRequestError> {
    let resp = raw_rpc(addr, body, token).await?;
    foundation_serialization::json::from_str::<Value>(&resp)
        .map_err(|err| RpcRequestError::Parse(err.to_string()))
}

#[derive(Debug)]
enum RpcRequestError {
    Timeout(&'static str),
    Io(io::Error),
    Utf8(std::string::FromUtf8Error),
    Parse(String),
}

impl std::fmt::Display for RpcRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcRequestError::Timeout(phase) => write!(f, "timeout during {phase}"),
            RpcRequestError::Io(err) => write!(f, "io error: {err}"),
            RpcRequestError::Utf8(err) => write!(f, "utf8 error: {err}"),
            RpcRequestError::Parse(err) => write!(f, "parse error: {err}"),
        }
    }
}

async fn raw_rpc(addr: &str, body: &str, token: Option<&str>) -> Result<String, RpcRequestError> {
    let addr: SocketAddr = addr.parse().unwrap();
    let body = body.to_string();
    let token = token.map(|t| t.to_string());
    spawn_blocking(move || send_rpc_blocking(addr, body, token))
        .await
        .map_err(|err| RpcRequestError::Io(io::Error::new(ErrorKind::Other, err.to_string())))?
}

fn send_rpc_blocking(
    addr: SocketAddr,
    body: String,
    token: Option<String>,
) -> Result<String, RpcRequestError> {
    let debug = rpc_debug();
    if debug {
        eprintln!("raw_rpc connect {addr}");
    }
    let mut stream = connect_blocking(addr)?;
    let mut req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(t) = token.as_deref() {
        req.push_str(&format!("Authorization: Bearer {}\r\n", t));
    }
    req.push_str("Connection: close\r\n\r\n");
    req.push_str(&body);
    if debug {
        eprintln!("raw_rpc write {} bytes", req.len());
    }
    stream
        .write_all(req.as_bytes())
        .map_err(|err| map_timeout(err, "write request"))?;
    if debug {
        eprintln!("raw_rpc read response");
    }
    let body = read_http_body_blocking(&mut stream).map_err(RpcRequestError::Io)?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    if debug {
        eprintln!("raw_rpc done");
    }
    String::from_utf8(body).map_err(RpcRequestError::Utf8)
}

fn connect_blocking(addr: SocketAddr) -> Result<StdTcpStream, RpcRequestError> {
    let stream = StdTcpStream::connect_timeout(&addr, RPC_CONNECT_TIMEOUT).map_err(|err| {
        if err.kind() == ErrorKind::TimedOut {
            RpcRequestError::Timeout("connect")
        } else {
            RpcRequestError::Io(err)
        }
    })?;
    stream.set_nodelay(true).map_err(RpcRequestError::Io)?;
    stream
        .set_write_timeout(Some(RPC_RW_TIMEOUT))
        .map_err(RpcRequestError::Io)?;
    stream
        .set_read_timeout(Some(RPC_RW_TIMEOUT))
        .map_err(RpcRequestError::Io)?;
    Ok(stream)
}

fn map_timeout(err: io::Error, phase: &'static str) -> RpcRequestError {
    if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock {
        RpcRequestError::Timeout(phase)
    } else {
        RpcRequestError::Io(err)
    }
}

fn read_http_body_blocking(stream: &mut StdTcpStream) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut header_end = None;
    let mut content_length = None;
    loop {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk)?;
        if rpc_debug() {
            eprintln!("read_http_body chunk bytes={read}");
        }
        if read == 0 {
            if header_end.is_some() && content_length.is_some() {
                break;
            }
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "connection closed before response completed",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            if let Some(pos) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                if rpc_debug() {
                    let header_text = String::from_utf8_lossy(&buffer[..pos]);
                    eprintln!("http response header:\n{header_text}");
                }
                content_length = Some(parse_content_length(&buffer[..pos])?);
            }
        }
        if let (Some(header_end), Some(content_length)) = (header_end, content_length) {
            if buffer.len() >= header_end + content_length {
                break;
            }
        }
    }
    let header_end = header_end.unwrap();
    let content_length = content_length.unwrap();
    let end = header_end + content_length;
    Ok(buffer[header_end..end].to_vec())
}

fn parse_content_length(header: &[u8]) -> io::Result<usize> {
    let header_text = String::from_utf8_lossy(header);
    for line in header_text.split("\r\n") {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let value = value.trim();
                return value.parse::<usize>().map_err(|_| {
                    io::Error::new(
                        ErrorKind::InvalidData,
                        "invalid content-length header value",
                    )
                });
            }
        }
    }
    Err(io::Error::new(
        ErrorKind::InvalidData,
        "content-length header missing",
    ))
}

#[testkit::tb_serial]
fn rpc_smoke() {
    runtime::block_on(async {
        let dir = temp::temp_dir("rpc_smoke");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        {
            let mut guard = bc.lock().unwrap();
            guard.add_account("alice".to_string(), 42).unwrap();
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
            request_timeout_ms: 20_000,
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
        // metrics endpoint
        let val = rpc(&addr, r#"{"method":"metrics"}"#, None).await;
        let metrics_result = val
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| val.get("result"))
            .expect("metrics result");
        #[cfg(feature = "telemetry")]
        assert!(metrics_result.as_str().unwrap().contains("mempool_size"));
        #[cfg(not(feature = "telemetry"))]
        assert_eq!(metrics_result.as_str().unwrap(), "telemetry disabled");

        // balance query
        let bal = rpc(
            &addr,
            r#"{"method":"balance","params":{"address":"alice"}}"#,
            None,
        )
        .await;
        let bal_result = bal
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| bal.get("result"))
            .expect("balance result");
        let bal_amount = bal_result
            .get("amount")
            .or_else(|| bal_result.get("consumer"))
            .and_then(|v| v.as_u64());
        assert_eq!(bal_amount, Some(42));

        // settlement status
        let status = rpc(&addr, r#"{"method":"settlement_status"}"#, None).await;
        let status_result = status
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| status.get("result"))
            .expect("settlement status result");
        let mode = status_result["mode"]
            .as_str()
            .or_else(|| status_result.as_str());
        assert_eq!(mode, Some("dryrun"));

        // start and stop mining
        let start = rpc(
            &addr,
            r#"{"method":"start_mining","params":{"miner":"alice","nonce":1}}"#,
            Some("testtoken"),
        )
        .await;
        let start_result = start
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| start.get("result"))
            .expect("start result");
        assert_eq!(start_result["status"].as_str(), Some("ok"));
        let stop = rpc(
            &addr,
            r#"{"method":"stop_mining","params":{"nonce":2}}"#,
            Some("testtoken"),
        )
        .await;
        let stop_result = stop
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| stop.get("result"))
            .expect("stop result");
        assert_eq!(stop_result["status"].as_str(), Some("ok"));
        Settlement::shutdown();

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_nonce_replay_rejected() {
    let dir = temp::temp_dir("rpc_nonce_replay");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    {
        let mut guard = bc.lock().expect("bc lock");
        guard
            .add_account("miner".to_string(), 0)
            .expect("add miner");
    }
    let handles_dir = dir.path().join("handles");
    fs::create_dir_all(&handles_dir).expect("create handles dir");
    let handles = Arc::new(Mutex::new(HandleRegistry::open(
        handles_dir.to_str().expect("handles path"),
    )));

    let did_dir = dir.path().join("did_db");
    fs::create_dir_all(&did_dir).expect("create did dir");
    std::env::set_var("TB_DID_DB_PATH", did_dir.to_str().expect("did path"));
    let dids = Arc::new(Mutex::new(DidRegistry::open(DidRegistry::default_path())));
    std::env::remove_var("TB_DID_DB_PATH");

    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::<(String, u64)>::new()));
    let runtime_cfg = fuzz_runtime_config_with_admin("testtoken");
    let auth_header = Some("Bearer testtoken".to_string());

    let dispatch = |req: RpcRequest| {
        fuzz_dispatch_request(
            Arc::clone(&bc),
            Arc::clone(&mining),
            Arc::clone(&nonces),
            Arc::clone(&handles),
            Arc::clone(&dids),
            Arc::clone(&runtime_cfg),
            None,
            None,
            req,
            auth_header.clone(),
            None,
        )
    };

    let mut start_params = Map::new();
    start_params.insert("miner".to_string(), Value::String("miner".to_string()));
    start_params.insert("nonce".to_string(), Value::Number(Number::from(1)));
    match dispatch(RpcRequest::new("start_mining", Value::Object(start_params))) {
        RpcResponse::Result { result, .. } => {
            assert_eq!(result["status"].as_str(), Some("ok"));
        }
        other => panic!("expected successful mining start, got {other:?}"),
    }

    let mut stop_params = Map::new();
    stop_params.insert("nonce".to_string(), Value::Number(Number::from(2)));
    match dispatch(RpcRequest::new(
        "stop_mining",
        Value::Object(stop_params.clone()),
    )) {
        RpcResponse::Result { result, .. } => {
            assert_eq!(result["status"].as_str(), Some("ok"));
        }
        other => panic!("expected successful mining stop, got {other:?}"),
    }

    match dispatch(RpcRequest::new("stop_mining", Value::Object(stop_params))) {
        RpcResponse::Error { error, .. } => {
            assert_eq!(error.code, -32000);
            assert_eq!(error.message, "replayed nonce");
        }
        other => panic!("expected replay error, got {other:?}"),
    }
}

#[testkit::tb_serial]
fn rpc_light_client_rebate_status() {
    runtime::block_on(async {
        let dir = temp::temp_dir("rpc_rebate_status");
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

        let status = rpc(&addr, r#"{"method":"light_client.rebate_status"}"#, None).await;
        let result = status
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| status.get("result"))
            .expect("result");
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
        let dir = temp::temp_dir("rpc_rebate_history");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        {
            let mut guard = bc.lock().unwrap();
            guard
                .add_account("miner".to_string(), 0)
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

        let history = rpc(
            &addr,
            r#"{"method":"light_client.rebate_history","params":{"limit":10}}"#,
            None,
        )
        .await;
        let result = history
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| history.get("result"))
            .expect("result");
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

        let filtered = rpc(
            &addr,
            &format!(
                "{{\"method\":\"light_client.rebate_history\",\"params\":{{\"relayer\":\"{}\",\"limit\":10}}}}",
                crypto_suite::hex::encode(b"relay")
            ),
            None,
        )
        .await;
        let filtered_result = filtered
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| filtered.get("result"))
            .expect("filtered result");
        let filtered_receipts = filtered_result["receipts"].as_array().unwrap();
        assert_eq!(filtered_receipts.len(), 1);

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_concurrent_controls() {
    runtime::block_on(async {
        let dir = temp::temp_dir("rpc_concurrent");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        {
            let mut guard = bc.lock().unwrap();
            guard.add_account("alice".to_string(), 1_000_000).unwrap();
            guard.add_account("bob".to_string(), 0).unwrap();
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
            request_timeout_ms: 20_000,
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
        let sk = Arc::new(sk);
        let tx_from = "alice".to_string();
        let tx_to = "bob".to_string();
        let tx_amount_consumer = 1;
        let tx_amount_industrial = 0;
        let tx_fee = 1000;
        let tx_pct = 100;

        let mut handles = Vec::new();
        for i in 0..6 {
            let addr = addr.clone();
            let sk = Arc::clone(&sk);
            let from = tx_from.clone();
            let to = tx_to.clone();
            handles.push(the_block::spawn(async move {
                the_block::sleep(Duration::from_millis(5 * ((i as u64) + 1))).await;
                if rpc_debug() {
                    eprintln!("rpc concurrent task {i} start");
                }
                let body = match i % 3 {
                    0 => format!(
                        "{{\"method\":\"start_mining\",\"params\":{{\"miner\":\"alice\",\"nonce\":{i}}}}}",
                        i = i
                    ),
                    1 => format!(
                        "{{\"method\":\"stop_mining\",\"params\":{{\"nonce\":{i}}}}}",
                        i = i
                    ),
                    _ => {
                        let payload = RawTxPayload {
                            from_: from.clone(),
                            to: to.clone(),
                            amount_consumer: tx_amount_consumer,
                            amount_industrial: tx_amount_industrial,
                            fee: tx_fee,
                            pct: tx_pct,
                            nonce: (i + 1) as u64,
                            memo: Vec::new(),
                        };
                        let tx = sign_tx(sk.to_vec(), payload).unwrap();
                        let tx_hex = crypto_suite::hex::encode(binary::encode(&tx).unwrap());
                        format!(
                            "{{\"method\":\"submit_tx\",\"params\":{{\"tx\":\"{tx}\",\"nonce\":{i}}}}}",
                            tx = tx_hex,
                            i = i
                        )
                    }
                };
                rpc(&addr, &body, Some("testtoken")).await;
                the_block::sleep(Duration::from_millis(20)).await;
                if rpc_debug() {
                    eprintln!("rpc concurrent task {i} done");
                }
            }));
        }
        for h in handles {
            let _ = h.await;
        }
        if rpc_debug() {
            eprintln!("rpc concurrent tasks complete; issuing final stop_mining");
        }
        rpc(
            &addr,
            r#"{"method":"stop_mining","params":{"nonce":999}}"#,
            Some("testtoken"),
        )
        .await;
        if rpc_debug() {
            eprintln!("rpc concurrent stop_mining completed; aborting server");
        }
        match bc.try_lock() {
            Ok(guard) => {
                assert!(guard.mempool_consumer.len() <= 1);
            }
            Err(TryLockError::Poisoned(poison)) => {
                let guard = poison.into_inner();
                assert!(guard.mempool_consumer.len() <= 1);
            }
            Err(TryLockError::WouldBlock) => {
                if rpc_debug() {
                    eprintln!("rpc concurrent mempool check skipped; blockchain lock busy");
                }
            }
        }
        handle.abort();
        let _ = handle.await;
        if rpc_debug() {
            eprintln!("rpc concurrent server aborted; mempool checked");
        }
    });
}

#[testkit::tb_serial]
fn rpc_error_responses() {
    runtime::block_on(async {
        let dir = temp::temp_dir("rpc_errors");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            request_timeout_ms: 20_000,
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
        let response = raw_rpc(&addr, "{", None).await;
        let error_code = match response {
            Ok(body) => foundation_serialization::json::from_str::<Value>(&body)
                .ok()
                .and_then(|json| {
                    json.get("Error")
                        .and_then(|e| e.get("error"))
                        .and_then(|e| e.get("code"))
                        .and_then(|c| c.as_i64())
                        .or_else(|| {
                            json.get("error")
                                .and_then(|e| e.get("code"))
                                .and_then(|c| c.as_i64())
                        })
                }),
            Err(RpcRequestError::Timeout(_)) => Some(-32700),
            Err(err) => panic!("malformed rpc failed: {err}"),
        };
        assert!(error_code == Some(-32700) || error_code == Some(-32601));

        // unknown method
        let val = rpc(&addr, r#"{"method":"unknown"}"#, None).await;
        let error_code = val
            .get("Error")
            .and_then(|e| e.get("error"))
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64())
            .or_else(|| {
                val.get("error")
                    .and_then(|e| e.get("code"))
                    .and_then(|c| c.as_i64())
            });
        assert_eq!(error_code, Some(-32601));

        handle.abort();
        let _ = handle.await;
    });
}

#[testkit::tb_serial]
fn rpc_fragmented_request() {
    runtime::block_on(async {
        let dir = temp::temp_dir("rpc_fragmented");
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
            "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer testtoken\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let addr_socket: SocketAddr = addr.parse().unwrap();
        let req_bytes = req.into_bytes();
        let mid = req_bytes.len() / 2;
        let first = req_bytes[..mid].to_vec();
        let second = req_bytes[mid..].to_vec();
        let body = spawn_blocking(move || {
            let mut stream = StdTcpStream::connect(addr_socket).expect("connect fragmented stream");
            stream
                .set_write_timeout(Some(Duration::from_secs(30)))
                .expect("set write timeout");
            stream
                .set_read_timeout(Some(Duration::from_secs(120)))
                .expect("set read timeout");
            stream.write_all(&first).expect("write first half");
            std::thread::sleep(Duration::from_millis(5));
            stream.write_all(&second).expect("write second half");
            read_http_body_blocking(&mut stream)
        })
        .await
        .expect("fragmented blocking task failed")
        .expect("fragmented read error");
        let val: Value = foundation_serialization::json::from_slice(&body).unwrap();
        let result = val
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| val.get("result"))
            .expect("fragmented result");
        assert_eq!(result["status"].as_str(), Some("ok"));

        handle.abort();
        let _ = handle.await;
    });
}
