use foundation_serialization::json::{Map as JsonMap, Number, Value};

/// Format the discovery response in a stable JSON structure for CLI automation.
pub fn discovery_result_json(timeout_ms: u64, signers: &[String]) -> Value {
    let signers_json = signers
        .iter()
        .map(|endpoint| Value::String(endpoint.clone()))
        .collect::<Vec<_>>();
    let mut map = JsonMap::new();
    map.insert("timeout_ms".into(), Value::Number(Number::from(timeout_ms)));
    map.insert("signers".into(), Value::Array(signers_json));
    Value::Object(map)
}
