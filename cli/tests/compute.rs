use contract_cli::compute::{
    provider_balances_payload, stats_request_payload, write_provider_balances_from_str,
    write_stats_from_str,
};
use foundation_serialization::json::{
    to_string_value, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
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
