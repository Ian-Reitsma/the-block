use foundation_rpc::Request as RpcRequest;
use foundation_serialization::json::{self, Map, Number, Value};
use std::env;
use std::thread::sleep;
use std::time::{Duration, Instant};
use the_block::rpc::client::RpcClient;

/// Smoke test that checks overlay peer counts across a running 3-node cluster.
/// Provide comma-separated RPC endpoints via `TB_MULTI_NODE_RPC` (e.g.,
/// `TB_MULTI_NODE_RPC=192.168.1.10:3030,192.168.1.11:4030,192.168.1.12:5030`).
/// Skips if the env var is missing.
#[test]
fn multi_node_overlay_converges() {
    let endpoints = env::var("TB_MULTI_NODE_RPC").unwrap_or_default();
    if endpoints.trim().is_empty() {
        eprintln!("skipping multi_node_overlay_converges (TB_MULTI_NODE_RPC not set)");
        return;
    }
    let rpc_urls: Vec<String> = endpoints
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("http://{}/", s.trim()))
        .collect();
    assert!(
        rpc_urls.len() >= 2,
        "need at least two endpoints in TB_MULTI_NODE_RPC"
    );

    let client = RpcClient::from_env();
    let target_peers = rpc_urls.len().saturating_sub(1) as u64;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last_error = String::new();

    while Instant::now() < deadline {
        let mut ok = true;
        for url in &rpc_urls {
            match overlay_status(&client, url) {
                Ok(status) => {
                    let active = status
                        .get("active_peers")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if active < target_peers {
                        ok = false;
                        last_error =
                            format!("{} active_peers={} (want >= {})", url, active, target_peers);
                        break;
                    }
                }
                Err(err) => {
                    ok = false;
                    last_error = format!("{} error: {err}", url);
                    break;
                }
            }
        }
        if ok {
            return;
        }
        sleep(Duration::from_millis(500));
    }

    panic!("overlay convergence failed: {last_error}");
}

fn overlay_status(client: &RpcClient, url: &str) -> Result<Map, String> {
    let req = RpcRequest::new("net.overlay_status", Value::Array(Vec::new())).with_id(1);
    let response = client
        .call(url, &request_to_value(&req))
        .map_err(|e| format!("rpc call failed: {e}"))?;
    let body = response.into_body();
    let value: Value = json::value_from_slice(&body).map_err(|e| format!("decode failed: {e}"))?;
    let result = value
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing result field: {value:?}"))?;
    Ok(result.clone())
}

fn request_to_value(request: &RpcRequest) -> Value {
    let mut map = Map::new();
    if let Some(version) = &request.version {
        map.insert("jsonrpc".to_owned(), Value::String(version.clone()));
    }
    map.insert("method".to_owned(), Value::String(request.method.clone()));
    map.insert("params".to_owned(), Value::from(request.params.clone()));
    if let Some(id) = &request.id {
        map.insert("id".to_owned(), id.clone());
    } else {
        map.insert("id".to_owned(), Value::Number(Number::from(1)));
    }
    if let Some(badge) = &request.badge {
        map.insert("badge".to_owned(), Value::String(badge.clone()));
    }
    Value::Object(map)
}
