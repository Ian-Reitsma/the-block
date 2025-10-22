use foundation_serialization::json::{Map as JsonMap, Number, Value};
use the_block::rpc::client::{RpcClient, RpcClientError};

/// Query the node's current difficulty via JSON-RPC.
///
/// Run the node with an RPC address, then execute:
/// `cargo run -p the_block --example difficulty`.
/// Optionally pass the RPC URL as the first argument.
fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:3030".to_string());
    let client = RpcClient::from_env();
    let mut payload_map = JsonMap::new();
    payload_map.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    payload_map.insert("id".to_string(), Value::Number(Number::from(1)));
    payload_map.insert(
        "method".to_string(),
        Value::String("consensus.difficulty".to_string()),
    );
    payload_map.insert("params".to_string(), Value::Object(JsonMap::new()));
    let payload = Value::Object(payload_map);
    match client
        .call(&url, &payload)
        .and_then(|r| r.json::<Value>().map_err(RpcClientError::from))
    {
        Ok(res) => {
            if let Some(d) = res
                .get("result")
                .and_then(|v| v.get("difficulty"))
                .and_then(|v| v.as_u64())
            {
                println!("current difficulty: {}", d);
            } else {
                eprintln!("unexpected response: {}", res);
            }
        }
        Err(e) => eprintln!("RPC error: {e}"),
    }
}
