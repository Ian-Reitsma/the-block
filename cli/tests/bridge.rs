use contract_cli::bridge::{handle_with_transport, BridgeCmd, BridgeRpcTransport};
use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;

struct MockTransport {
    responses: RefCell<VecDeque<String>>,
    captured: RefCell<Vec<(String, String)>>,
}

impl MockTransport {
    fn new(responses: Vec<JsonValue>) -> Self {
        let queue = responses
            .into_iter()
            .map(|value| json::to_string_value(&value))
            .collect();
        Self {
            responses: RefCell::new(queue),
            captured: RefCell::new(Vec::new()),
        }
    }

    fn captured_requests(&self) -> Vec<String> {
        self.captured
            .borrow()
            .iter()
            .map(|(_, body)| body.clone())
            .collect()
    }

    fn captured_urls(&self) -> Vec<String> {
        self.captured
            .borrow()
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self {
            responses: RefCell::new(VecDeque::new()),
            captured: RefCell::new(Vec::new()),
        }
    }
}

impl BridgeRpcTransport for MockTransport {
    fn call(&self, url: &str, payload: &JsonValue) -> io::Result<String> {
        let body = json::to_string_value(payload);
        self.captured.borrow_mut().push((url.to_string(), body));
        let mut responses = self.responses.borrow_mut();
        let text = responses
            .pop_front()
            .unwrap_or_else(|| "{\"jsonrpc\":\"2.0\",\"result\":null,\"id\":1}".to_string());
        Ok(text)
    }
}

fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.to_owned())
}

fn json_number(value: u64) -> JsonValue {
    JsonValue::Number(JsonNumber::from(value))
}

fn json_bool(value: bool) -> JsonValue {
    JsonValue::Bool(value)
}

fn json_object(entries: impl IntoIterator<Item = (&'static str, JsonValue)>) -> JsonValue {
    let mut map = JsonMap::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    JsonValue::Object(map)
}

fn json_null() -> JsonValue {
    JsonValue::Null
}

fn parse_json(text: &str) -> JsonValue {
    json::from_str(text).expect("valid json")
}

fn rpc_envelope(method: &str, params: JsonValue) -> JsonValue {
    json_object([
        ("jsonrpc", json_string("2.0")),
        ("id", json_number(1)),
        ("method", json_string(method)),
        ("params", params),
    ])
}

fn ok_response(result: JsonValue) -> JsonValue {
    json_object([
        ("jsonrpc", json_string("2.0")),
        ("id", json_number(1)),
        ("result", result),
    ])
}

#[test]
fn bridge_claim_command_sends_payload_and_prints_response() {
    let claim_body = json_object([
        ("status", json_string("ok")),
        (
            "claim",
            json_object([
                ("id", json_number(7)),
                ("relayer", json_string("alice")),
                ("amount", json_number(120)),
                ("approval_key", json_string("approval-1")),
                ("claimed_at", json_number(88)),
                ("pending_before", json_number(240)),
                ("pending_after", json_number(120)),
            ]),
        ),
    ]);
    let response = ok_response(claim_body.clone());
    let mock = MockTransport::new(vec![response.clone()]);

    let mut output = Vec::new();
    handle_with_transport(
        BridgeCmd::Claim {
            relayer: "alice".into(),
            amount: 120,
            approval_key: "approval-1".into(),
            url: "http://mock.bridge".into(),
        },
        &mock,
        &mut output,
    )
    .expect("claim command");

    let captured = mock.captured_requests();
    assert_eq!(captured.len(), 1);
    let request_value = parse_json(&captured[0]);
    let expected_request = rpc_envelope(
        "bridge.claim_rewards",
        json_object([
            ("relayer", json_string("alice")),
            ("amount", json_number(120)),
            ("approval_key", json_string("approval-1")),
        ]),
    );
    assert_eq!(request_value, expected_request);

    let printed = String::from_utf8(output).expect("utf8");
    let printed_value = parse_json(printed.trim());
    assert_eq!(printed_value, response);

    let urls = mock.captured_urls();
    assert_eq!(urls, vec!["http://mock.bridge".to_string()]);
}

#[test]
fn bridge_settlement_command_round_trips_payload() {
    let settlement_entry = json_object([
        ("status", json_string("ok")),
        (
            "settlement",
            json_object([
                ("asset", json_string("btc")),
                ("commitment", json_string("0xfeed")),
                ("relayer", json_string("bob")),
                ("settlement_chain", json_string("bitcoin-mainnet")),
                ("proof_hash", json_string("0xdeadbeef")),
                ("settlement_height", json_number(1_234)),
                ("submitted_at", json_number(9_999)),
            ]),
        ),
    ]);
    let response = ok_response(settlement_entry.clone());
    let mock = MockTransport::new(vec![response.clone()]);

    let mut output = Vec::new();
    handle_with_transport(
        BridgeCmd::Settlement {
            asset: "btc".into(),
            relayer: "bob".into(),
            commitment: "0xfeed".into(),
            settlement_chain: "bitcoin-mainnet".into(),
            proof_hash: "0xdeadbeef".into(),
            settlement_height: 1_234,
            url: "http://mock.bridge".into(),
        },
        &mock,
        &mut output,
    )
    .expect("settlement command");

    let captured = mock.captured_requests();
    assert_eq!(captured.len(), 1);
    let request_value = parse_json(&captured[0]);
    let expected_request = rpc_envelope(
        "bridge.submit_settlement",
        json_object([
            ("asset", json_string("btc")),
            ("relayer", json_string("bob")),
            ("commitment", json_string("0xfeed")),
            ("settlement_chain", json_string("bitcoin-mainnet")),
            ("proof_hash", json_string("0xdeadbeef")),
            ("settlement_height", json_number(1_234)),
        ]),
    );
    assert_eq!(request_value, expected_request);

    let printed = String::from_utf8(output).expect("utf8");
    let printed_value = parse_json(printed.trim());
    assert_eq!(printed_value, response);
}

#[test]
fn bridge_reward_claims_paginates_requests() {
    let claims_response = ok_response(json_object([
        (
            "claims",
            JsonValue::Array(vec![json_object([
                ("id", json_number(4)),
                ("relayer", json_string("carol")),
                ("amount", json_number(77)),
                ("approval_key", json_string("approval-7")),
                ("claimed_at", json_number(55)),
                ("pending_before", json_number(100)),
                ("pending_after", json_number(23)),
            ])]),
        ),
        ("next_cursor", json_number(200)),
    ]));
    let mock = MockTransport::new(vec![claims_response.clone()]);

    let mut output = Vec::new();
    handle_with_transport(
        BridgeCmd::RewardClaims {
            relayer: Some("carol".into()),
            cursor: Some(150),
            limit: 25,
            url: "http://mock.bridge".into(),
        },
        &mock,
        &mut output,
    )
    .expect("reward claims command");

    let captured = mock.captured_requests();
    assert_eq!(captured.len(), 1);
    let request_value = parse_json(&captured[0]);
    let expected_request = rpc_envelope(
        "bridge.reward_claims",
        json_object([
            ("relayer", json_string("carol")),
            ("cursor", json_number(150)),
            ("limit", json_number(25)),
        ]),
    );
    assert_eq!(request_value, expected_request);

    let printed = String::from_utf8(output).expect("utf8");
    let printed_value = parse_json(printed.trim());
    assert_eq!(printed_value, claims_response);
}

#[test]
fn bridge_settlement_log_paginates_requests() {
    let settlement_log = ok_response(json_object([
        (
            "settlements",
            JsonValue::Array(vec![json_object([
                ("asset", json_string("eth")),
                ("commitment", json_string("0xabc")),
                ("relayer", json_string("dan")),
                ("settlement_chain", json_null()),
                ("proof_hash", json_string("0xbeef")),
                ("settlement_height", json_number(9_001)),
                ("submitted_at", json_number(44)),
            ])]),
        ),
        ("next_cursor", json_null()),
    ]));
    let mock = MockTransport::new(vec![settlement_log.clone()]);

    let mut output = Vec::new();
    handle_with_transport(
        BridgeCmd::SettlementLog {
            asset: Some("eth".into()),
            cursor: None,
            limit: 10,
            url: "http://mock.bridge".into(),
        },
        &mock,
        &mut output,
    )
    .expect("settlement log command");

    let captured = mock.captured_requests();
    assert_eq!(captured.len(), 1);
    let request_value = parse_json(&captured[0]);
    let expected_request = rpc_envelope(
        "bridge.settlement_log",
        json_object([
            ("asset", json_string("eth")),
            ("cursor", json_null()),
            ("limit", json_number(10)),
        ]),
    );
    assert_eq!(request_value, expected_request);

    let printed = String::from_utf8(output).expect("utf8");
    let printed_value = parse_json(printed.trim());
    assert_eq!(printed_value, settlement_log);
}

#[test]
fn bridge_configure_command_includes_optional_fields() {
    let configure_response = ok_response(json_object([("status", json_string("ok"))]));
    let mock = MockTransport::new(vec![configure_response.clone()]);

    let mut output = Vec::new();
    handle_with_transport(
        BridgeCmd::ConfigureAsset {
            asset: "dot".into(),
            confirm_depth: Some(12),
            fee_per_byte: Some(5),
            challenge_period_secs: Some(7200),
            relayer_quorum: Some(4),
            headers_dir: Some("/tmp/headers".into()),
            requires_settlement_proof: Some(true),
            settlement_chain: None,
            clear_settlement_chain: true,
            url: "http://mock.bridge".into(),
        },
        &mock,
        &mut output,
    )
    .expect("configure asset command");

    let captured = mock.captured_requests();
    assert_eq!(captured.len(), 1);
    let request_value = parse_json(&captured[0]);
    let expected_request = rpc_envelope(
        "bridge.configure_asset",
        json_object([
            ("asset", json_string("dot")),
            ("confirm_depth", json_number(12)),
            ("fee_per_byte", json_number(5)),
            ("challenge_period_secs", json_number(7200)),
            ("relayer_quorum", json_number(4)),
            ("headers_dir", json_string("/tmp/headers")),
            ("requires_settlement_proof", json_bool(true)),
            ("settlement_chain", json_null()),
        ]),
    );
    assert_eq!(request_value, expected_request);

    let printed = String::from_utf8(output).expect("utf8");
    let printed_value = parse_json(printed.trim());
    assert_eq!(printed_value, configure_response);
}
