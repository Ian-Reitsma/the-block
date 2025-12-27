use contract_cli::{
    compute::{
        handle_with_writer as compute_handle_with_writer, parse_sla_history_from_str,
        provider_balances_payload, stats_request_payload, write_provider_balances_from_str,
        write_stats_from_str, ComputeCmd,
    },
    explorer::{handle_with_writer as explorer_handle_with_writer, ExplorerCmd},
};
use crypto_suite::hex;
use explorer::{router as explorer_router, ComputeSlaHistoryRecord, Explorer, ExplorerHttpState};
use foundation_serialization::json::{
    to_string_value, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use httpd::{Method, StatusCode};
use runtime;
use std::io::{self, Read, Write as IoWrite};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use sys::tempfile;
use the_block::compute_market::{
    snark::{self, ProofBundle, SnarkBackend},
    workloads,
};
use the_block::simple_db::EngineKind;

fn stats_response_json() -> String {
    let recommended = EngineKind::default_for_build().label();

    let mut settlement_engine = JsonMap::new();
    settlement_engine.insert(
        "engine".to_string(),
        JsonValue::String(recommended.to_owned()),
    );
    settlement_engine.insert("legacy_mode".to_string(), JsonValue::Bool(true));

    let mut lane_recent = JsonMap::new();
    lane_recent.insert("job".to_string(), JsonValue::String("job-1".to_string()));
    lane_recent.insert(
        "provider".to_string(),
        JsonValue::String("alice".to_string()),
    );
    lane_recent.insert("price".to_string(), JsonValue::Number(JsonNumber::from(11)));
    lane_recent.insert(
        "issued_at".to_string(),
        JsonValue::Number(JsonNumber::from(123)),
    );

    let mut lane = JsonMap::new();
    lane.insert("lane".to_string(), JsonValue::String("gpu".to_string()));
    lane.insert(
        "pending".to_string(),
        JsonValue::Number(JsonNumber::from(4)),
    );
    lane.insert(
        "admitted".to_string(),
        JsonValue::Number(JsonNumber::from(2)),
    );
    lane.insert(
        "recent".to_string(),
        JsonValue::Array(vec![JsonValue::Object(lane_recent)]),
    );

    let mut recent_match = JsonMap::new();
    recent_match.insert("job_id".to_string(), JsonValue::String("job-2".to_string()));
    recent_match.insert("provider".to_string(), JsonValue::String("bob".to_string()));
    recent_match.insert("price".to_string(), JsonValue::Number(JsonNumber::from(13)));
    recent_match.insert(
        "issued_at".to_string(),
        JsonValue::Number(JsonNumber::from(456)),
    );

    let mut recent_matches_map = JsonMap::new();
    recent_matches_map.insert(
        "gpu".to_string(),
        JsonValue::Array(vec![JsonValue::Object(recent_match)]),
    );

    let mut lane_stats_entry = JsonMap::new();
    lane_stats_entry.insert("lane".to_string(), JsonValue::String("gpu".to_string()));
    lane_stats_entry.insert("bids".to_string(), JsonValue::Number(JsonNumber::from(5)));
    lane_stats_entry.insert("asks".to_string(), JsonValue::Number(JsonNumber::from(7)));
    lane_stats_entry.insert(
        "oldest_bid_ms".to_string(),
        JsonValue::Number(JsonNumber::from(33)),
    );
    lane_stats_entry.insert(
        "oldest_ask_ms".to_string(),
        JsonValue::Number(JsonNumber::from(44)),
    );

    let mut starvation_entry = JsonMap::new();
    starvation_entry.insert("lane".to_string(), JsonValue::String("gpu".to_string()));
    starvation_entry.insert("job_id".to_string(), JsonValue::String("job-3".to_string()));
    starvation_entry.insert(
        "waited_for_secs".to_string(),
        JsonValue::Number(JsonNumber::from(88)),
    );

    let mut result = JsonMap::new();
    result.insert(
        "settlement_engine".to_string(),
        JsonValue::Object(settlement_engine),
    );
    result.insert(
        "industrial_backlog".to_string(),
        JsonValue::Number(JsonNumber::from(3)),
    );
    result.insert(
        "industrial_utilization".to_string(),
        JsonValue::Number(JsonNumber::from(75)),
    );
    result.insert(
        "industrial_units_total".to_string(),
        JsonValue::Number(JsonNumber::from(9)),
    );
    result.insert(
        "industrial_price_per_unit".to_string(),
        JsonValue::Number(JsonNumber::from(21)),
    );
    result.insert(
        "lanes".to_string(),
        JsonValue::Array(vec![JsonValue::Object(lane)]),
    );
    result.insert(
        "recent_matches".to_string(),
        JsonValue::Object(recent_matches_map),
    );
    result.insert(
        "lane_stats".to_string(),
        JsonValue::Array(vec![JsonValue::Object(lane_stats_entry)]),
    );
    result.insert(
        "lane_starvation".to_string(),
        JsonValue::Array(vec![JsonValue::Object(starvation_entry)]),
    );

    let mut root = JsonMap::new();
    root.insert("jsonrpc".to_string(), JsonValue::String("2.0".to_string()));
    root.insert("result".to_string(), JsonValue::Object(result));

    to_string_value(&JsonValue::Object(root))
}

fn provider_balances_response_json() -> String {
    let mut alice = JsonMap::new();
    alice.insert(
        "provider".to_string(),
        JsonValue::String("alice".to_string()),
    );
    alice.insert("ct".to_string(), JsonValue::Number(JsonNumber::from(42)));
    alice.insert(
        "industrial".to_string(),
        JsonValue::Number(JsonNumber::from(7)),
    );

    let mut bob = JsonMap::new();
    bob.insert("provider".to_string(), JsonValue::String("bob".to_string()));
    bob.insert("ct".to_string(), JsonValue::Number(JsonNumber::from(1)));
    bob.insert("it".to_string(), JsonValue::Number(JsonNumber::from(2)));

    let mut providers = Vec::new();
    providers.push(JsonValue::Object(alice));
    providers.push(JsonValue::Object(bob));

    let mut result = JsonMap::new();
    result.insert("providers".to_string(), JsonValue::Array(providers));

    let mut root = JsonMap::new();
    root.insert("jsonrpc".to_string(), JsonValue::String("2.0".to_string()));
    root.insert("result".to_string(), JsonValue::Object(result));

    to_string_value(&JsonValue::Object(root))
}

#[test]
fn stats_request_payload_includes_accelerator() {
    let payload = stats_request_payload(Some("gpu"));
    let payload_obj = payload.as_object().expect("rpc object");
    assert_eq!(
        payload_obj.get("method").and_then(JsonValue::as_str),
        Some("compute_market.stats"),
    );
    let params = payload_obj.get("params").expect("params");
    let params_obj = params.as_object().expect("params object");
    assert_eq!(
        params_obj.get("accelerator").and_then(JsonValue::as_str),
        Some("gpu"),
    );
}

#[test]
fn stats_request_payload_without_accelerator_uses_null_params() {
    let payload = stats_request_payload(None);
    let payload_obj = payload.as_object().expect("rpc object");
    let params = payload_obj.get("params").expect("params");
    assert!(matches!(params, JsonValue::Null));
}

#[test]
fn stats_writer_formats_market_snapshot() {
    let json = stats_response_json();
    let mut buffer = Vec::new();
    write_stats_from_str(&json, &mut buffer).expect("write stats");
    let recommended = EngineKind::default_for_build().label();
    let expected = [
        format!("settlement engine: {recommended}"),
        "warning: settlement engine running in legacy mode".to_string(),
        "industrial backlog: 3".to_string(),
        "industrial utilization: 75%".to_string(),
        "industrial units total: 9".to_string(),
        "industrial price per unit: 21".to_string(),
        "lane gpu: pending 4 admitted 2".to_string(),
        "recent lane gpu job job-1 provider alice price 11 issued_at 123".to_string(),
        "recent lane gpu job job-2 provider bob price 13 issued_at 456".to_string(),
        "lane gpu bids: 5 asks: 7 oldest_bid_ms: 33 oldest_ask_ms: 44".to_string(),
        "starvation lane gpu job job-3 waited_secs: 88".to_string(),
    ]
    .join(
        "
",
    ) + "
";
    assert_eq!(String::from_utf8(buffer).expect("utf8"), expected);
}

#[test]
fn provider_balances_writer_formats_rows() {
    let json = provider_balances_response_json();
    let mut buffer = Vec::new();
    write_provider_balances_from_str(&json, &mut buffer).expect("write balances");
    let expected = [
        "provider: alice ct: 42 it: 7".to_string(),
        "provider: bob ct: 1 it: 2".to_string(),
    ]
    .join(
        "
",
    ) + "
";
    assert_eq!(String::from_utf8(buffer).expect("utf8"), expected);
}

#[test]
fn provider_balances_payload_uses_fixed_request_id() {
    let payload = provider_balances_payload();
    let payload_obj = payload.as_object().expect("rpc object");
    assert_eq!(
        payload_obj.get("method").and_then(JsonValue::as_str),
        Some("compute_market.provider_balances"),
    );
    assert_eq!(payload_obj.get("id").and_then(JsonValue::as_u64), Some(2),);
    assert!(matches!(
        payload_obj.get("params").expect("params"),
        JsonValue::Null
    ));
}

#[test]
fn cli_proofs_fetch_and_verify() {
    // Clear ALL TLS-related environment variables to ensure plain HTTP
    for prefix in &["TB_RPC_TLS", "TB_HTTP_TLS", "SSL", "TLS"] {
        for suffix in &[
            "_CA_CERT",
            "_CLIENT_CERT",
            "_CLIENT_KEY",
            "_CERT_FILE",
            "_KEY_FILE",
            "_CACERT",
        ] {
            let var_name = format!("{}{}", prefix, suffix);
            std::env::remove_var(&var_name);
        }
    }
    // Also clear common SSL/TLS env vars
    std::env::remove_var("SSL_CERT_FILE");
    std::env::remove_var("SSL_CERT_DIR");
    std::env::remove_var("REQUESTS_CA_BUNDLE");
    std::env::remove_var("CURL_CA_BUNDLE");

    // Enable plain HTTP mode for testing with mock servers
    std::env::set_var("TB_HTTP_PLAIN", "1");

    let wasm = b"cli-proof-wasm";
    let output = workloads::snark::run(wasm);
    let bundle = snark::prove_with_backend(wasm, &output, SnarkBackend::Cpu).expect("cpu proof");
    let payload = mock_sla_history_response(&bundle);
    let (url, handle) = match spawn_mock_rpc(payload.clone()) {
        Ok(pair) => pair,
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("skipping cli_proofs_fetch_and_verify: {err}");
            return;
        }
        Err(err) => panic!("spawn mock rpc: {err}"),
    };
    let mut buffer = Vec::new();
    eprintln!("[TEST] Calling compute_handle_with_writer...");
    compute_handle_with_writer(
        ComputeCmd::Proofs {
            url: url.clone(),
            limit: 4,
        },
        &mut buffer,
    )
    .expect("cli proofs command");
    eprintln!("[TEST] compute_handle_with_writer done, joining server thread...");
    handle.join().expect("server thread");
    eprintln!("[TEST] Server thread joined, parsing response...");
    let parsed = parse_sla_history_from_str(&payload).expect("parse history");
    assert_eq!(parsed.len(), 1);
    let proof = parsed[0].proofs.first().expect("proof entry");
    assert!(snark::verify(proof, wasm, &output).expect("verify proof"));
    let stdout = String::from_utf8(buffer).expect("utf8 stdout");
    let fingerprint = hex::encode(bundle.fingerprint());
    assert!(stdout.contains(&fingerprint));

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer db");
    explorer
        .record_sla_history(&parsed)
        .expect("persist sla history");
    let stored = explorer.compute_sla_history(4).expect("history query");
    assert_eq!(stored.len(), 1);
    let stored_proof = stored[0].proofs.first().expect("stored proof");
    let decoded_bundle = stored_proof.to_bundle().expect("rehydrate stored proof");
    assert!(snark::verify(&decoded_bundle, wasm, &output).expect("verify stored bundle"));
}

#[test]
fn explorer_sync_proofs_serves_http_history() {
    // Clear ALL TLS-related environment variables to ensure plain HTTP
    for prefix in &["TB_RPC_TLS", "TB_HTTP_TLS", "SSL", "TLS"] {
        for suffix in &[
            "_CA_CERT",
            "_CLIENT_CERT",
            "_CLIENT_KEY",
            "_CERT_FILE",
            "_KEY_FILE",
            "_CACERT",
        ] {
            let var_name = format!("{}{}", prefix, suffix);
            std::env::remove_var(&var_name);
        }
    }
    // Also clear common SSL/TLS env vars
    std::env::remove_var("SSL_CERT_FILE");
    std::env::remove_var("SSL_CERT_DIR");
    std::env::remove_var("REQUESTS_CA_BUNDLE");
    std::env::remove_var("CURL_CA_BUNDLE");

    // Enable plain HTTP mode for testing with mock servers
    std::env::set_var("TB_HTTP_PLAIN", "1");

    let wasm = b"cli-proof-wasm";
    let output = workloads::snark::run(wasm);
    let bundle = snark::prove_with_backend(wasm, &output, SnarkBackend::Cpu).expect("cpu proof");
    let payload = mock_sla_history_response(&bundle);
    let (url, handle) = match spawn_mock_rpc(payload.clone()) {
        Ok(pair) => pair,
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("skipping explorer_sync_proofs_serves_http_history: {err}");
            return;
        }
        Err(err) => panic!("spawn mock rpc: {err}"),
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let db_arg = db_path.to_string_lossy().into_owned();
    explorer_handle_with_writer(
        ExplorerCmd::SyncProofs {
            db: db_arg,
            url: url.clone(),
            limit: 8,
        },
        &mut Vec::new(),
    )
    .expect("sync proofs");
    handle.join().expect("rpc thread");

    let explorer = Explorer::open(&db_path).expect("open explorer");
    let router = explorer_router(ExplorerHttpState::new(Arc::new(explorer)));
    let request = router
        .request_builder()
        .method(Method::Get)
        .path("/compute/sla/history")
        .query_param("limit", "4")
        .build();
    let response = runtime::block_on(router.handle(request)).expect("route success");
    assert_eq!(response.status(), StatusCode::OK);
    let records: Vec<ComputeSlaHistoryRecord> =
        foundation_serialization::json::from_slice(response.body()).expect("decode response");
    assert_eq!(records.len(), 1);
    let proof = records[0]
        .proofs
        .first()
        .expect("proof entry from http")
        .to_bundle()
        .expect("rehydrate proof");
    assert!(snark::verify(&proof, wasm, &output).expect("verify http proof"));
}

fn spawn_mock_rpc(payload: String) -> io::Result<(String, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = format!("http://{}", listener.local_addr()?);
    let handle = thread::spawn(move || {
        eprintln!("[SERVER] Thread started, waiting for connection...");
        if let Ok((mut stream, _)) = listener.accept() {
            eprintln!("[SERVER] Connection accepted, reading request...");
            // Read the incoming HTTP request to prevent client from hanging
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            eprintln!("[SERVER] Request read, sending response...");

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            eprintln!("[SERVER] Response sent, shutting down stream...");
            // Explicitly shutdown and drop the stream to close the connection
            let _ = stream.shutdown(std::net::Shutdown::Both);
            drop(stream);
            eprintln!("[SERVER] Stream closed");
        }
        // Explicitly drop the listener so the thread can exit
        drop(listener);
        eprintln!("[SERVER] Thread exiting");
    });
    Ok((addr, handle))
}

fn mock_sla_history_response(bundle: &ProofBundle) -> String {
    let mut proof_map = JsonMap::new();
    proof_map.insert("backend".to_string(), JsonValue::String("CPU".to_string()));
    proof_map.insert(
        "fingerprint".to_string(),
        JsonValue::String(hex::encode(bundle.fingerprint())),
    );
    proof_map.insert(
        "latency_ms".to_string(),
        JsonValue::Number(JsonNumber::from(bundle.latency_ms)),
    );
    proof_map.insert(
        "circuit_hash".to_string(),
        JsonValue::String(hex::encode(bundle.circuit_hash)),
    );
    proof_map.insert(
        "program_commitment".to_string(),
        JsonValue::String(hex::encode(bundle.program_commitment)),
    );
    proof_map.insert(
        "output_commitment".to_string(),
        JsonValue::String(hex::encode(bundle.output_commitment)),
    );
    proof_map.insert(
        "witness_commitment".to_string(),
        JsonValue::String(hex::encode(bundle.witness_commitment)),
    );
    let mut artifact_map = JsonMap::new();
    artifact_map.insert(
        "circuit_hash".to_string(),
        JsonValue::String(hex::encode(bundle.artifact.circuit_hash)),
    );
    artifact_map.insert(
        "wasm_hash".to_string(),
        JsonValue::String(hex::encode(bundle.artifact.wasm_hash)),
    );
    artifact_map.insert(
        "generated_at".to_string(),
        JsonValue::Number(JsonNumber::from(bundle.artifact.generated_at)),
    );
    proof_map.insert("artifact".to_string(), JsonValue::Object(artifact_map));
    proof_map.insert("verified".to_string(), JsonValue::Bool(bundle.self_check()));
    proof_map.insert(
        "proof".to_string(),
        JsonValue::String(hex::encode(&bundle.encoded)),
    );

    let mut entry = JsonMap::new();
    entry.insert("job_id".to_string(), JsonValue::String("job-1".to_string()));
    entry.insert(
        "provider".to_string(),
        JsonValue::String("provider-1".to_string()),
    );
    entry.insert(
        "buyer".to_string(),
        JsonValue::String("buyer-1".to_string()),
    );
    entry.insert(
        "outcome".to_string(),
        JsonValue::String("completed".to_string()),
    );
    entry.insert("burned".to_string(), JsonValue::Number(JsonNumber::from(0)));
    entry.insert(
        "refunded".to_string(),
        JsonValue::Number(JsonNumber::from(0)),
    );
    entry.insert(
        "deadline".to_string(),
        JsonValue::Number(JsonNumber::from(1)),
    );
    entry.insert(
        "resolved_at".to_string(),
        JsonValue::Number(JsonNumber::from(2)),
    );
    entry.insert(
        "proofs".to_string(),
        JsonValue::Array(vec![JsonValue::Object(proof_map)]),
    );

    let mut root = JsonMap::new();
    root.insert("jsonrpc".to_string(), JsonValue::String("2.0".to_string()));
    root.insert(
        "result".to_string(),
        JsonValue::Array(vec![JsonValue::Object(entry)]),
    );
    to_string_value(&JsonValue::Object(root))
}
