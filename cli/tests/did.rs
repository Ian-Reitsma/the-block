use contract_cli::light_client::{
    anchor_record_from_value, anchor_request_params, resolved_did_from_value, AnchorRecord,
    LightHeader, ResolvedDid,
};
use contract_cli::tx::{TxDidAnchor, TxDidAnchorAttestation};
use foundation_serialization::json::{
    from_value, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};

fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.to_owned())
}

fn json_number_u64(value: u64) -> JsonValue {
    JsonValue::Number(JsonNumber::from(value))
}

fn json_null() -> JsonValue {
    JsonValue::Null
}

fn json_object(entries: impl IntoIterator<Item = (&'static str, JsonValue)>) -> JsonValue {
    let mut map = JsonMap::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    JsonValue::Object(map)
}

fn anchor_envelope_value_to_record(value: JsonValue) -> AnchorRecord {
    let envelope = value.as_object().expect("anchor envelope object");
    if let Some(error) = envelope.get("error") {
        panic!("anchor error response: {error:?}");
    }
    let result = envelope
        .get("result")
        .cloned()
        .expect("anchor envelope contains result payload");
    anchor_record_from_value(result).expect("anchor record parse")
}

fn latest_header_value_to_header(value: JsonValue) -> LightHeader {
    let envelope = value.as_object().expect("latest_header envelope object");
    if let Some(error) = envelope.get("error") {
        panic!("latest_header error response: {error:?}");
    }
    let result = envelope
        .get("result")
        .cloned()
        .expect("latest_header envelope contains result payload");
    from_value(result).expect("latest_header result parse")
}

fn resolved_did_value_to_record(value: JsonValue) -> ResolvedDid {
    let envelope = value.as_object().expect("resolve envelope object");
    if let Some(error) = envelope.get("error") {
        panic!("resolve error response: {error:?}");
    }
    let result = envelope
        .get("result")
        .cloned()
        .expect("resolve envelope contains result payload");
    resolved_did_from_value(result).expect("resolved did parsing")
}

fn sample_anchor_record_value() -> JsonValue {
    json_object([
        ("address", json_string("addr1")),
        ("document", json_string(r#"{"foo":"bar"}"#)),
        ("hash", json_string("hash1")),
        ("nonce", json_number_u64(7)),
        ("updated_at", json_number_u64(42)),
        ("public_key", json_string("pk1")),
        (
            "remote_attestation",
            json_object([
                ("signer", json_string("remote-signer")),
                ("signature", json_string("remote-signature")),
            ]),
        ),
    ])
}

fn sample_resolved_did_value() -> JsonValue {
    json_object([
        ("address", json_string("addr1")),
        ("document", json_string(r#"{"hello":1}"#)),
        ("hash", json_string("hash2")),
        ("nonce", json_number_u64(8)),
        ("updated_at", json_number_u64(100)),
        ("public_key", json_string("pk2")),
        (
            "remote_attestation",
            json_object([
                ("signer", json_string("remote")),
                ("signature", json_string("sig")),
            ]),
        ),
    ])
}

fn sample_anchor_tx() -> TxDidAnchor {
    TxDidAnchor {
        address: "addr1".to_string(),
        public_key: vec![0x11, 0x22, 0x33],
        document: r#"{"foo":"bar"}"#.to_string(),
        nonce: 9,
        signature: vec![0xaa, 0xbb],
        remote_attestation: Some(TxDidAnchorAttestation {
            signer: "remote-signer".to_string(),
            signature: "remote-signature".to_string(),
        }),
    }
}

#[test]
fn anchor_request_params_serializes_binary_fields() {
    let tx = sample_anchor_tx();
    let params = anchor_request_params(&tx);
    let object = params.as_object().expect("anchor params object");
    assert_eq!(
        object.get("address").and_then(JsonValue::as_str),
        Some("addr1"),
    );
    let public_key = object
        .get("public_key")
        .and_then(JsonValue::as_array)
        .expect("public key array");
    let bytes: Vec<u64> = public_key
        .iter()
        .map(|value| value.as_u64().expect("u64"))
        .collect();
    assert_eq!(bytes, vec![0x11, 0x22, 0x33]);
    let signature = object
        .get("signature")
        .and_then(JsonValue::as_array)
        .expect("signature array");
    let sig_bytes: Vec<u64> = signature
        .iter()
        .map(|value| value.as_u64().expect("u64"))
        .collect();
    assert_eq!(sig_bytes, vec![0xaa, 0xbb]);
    let attestation = object
        .get("remote_attestation")
        .and_then(JsonValue::as_object)
        .expect("attestation object");
    assert_eq!(
        attestation.get("signer").and_then(JsonValue::as_str),
        Some("remote-signer"),
    );
    assert_eq!(
        attestation.get("signature").and_then(JsonValue::as_str),
        Some("remote-signature"),
    );
    assert_eq!(
        object.get("document").and_then(JsonValue::as_str),
        Some(r#"{"foo":"bar"}"#),
    );
    assert_eq!(object.get("nonce").and_then(JsonValue::as_u64), Some(9));
}

#[test]
fn anchor_record_from_value_parses_string_document() {
    let value = sample_anchor_record_value();
    let record = anchor_record_from_value(value.clone()).expect("anchor record");
    assert_eq!(record.address, "addr1");
    assert_eq!(record.hash, "hash1");
    assert_eq!(record.nonce, 7);
    assert_eq!(record.updated_at, 42);
    assert_eq!(record.public_key, "pk1");
    assert_eq!(record.document, json_object([("foo", json_string("bar"))]));
    let attestation = record
        .remote_attestation
        .expect("remote attestation present");
    assert_eq!(attestation.signer, "remote-signer");
    assert_eq!(attestation.signature, "remote-signature");

    // Ensure original value not consumed for future assertions when needed
    let original_doc = value
        .as_object()
        .and_then(|map| map.get("document"))
        .and_then(JsonValue::as_str)
        .expect("original document string");
    assert_eq!(original_doc, r#"{"foo":"bar"}"#);
}

#[test]
fn anchor_envelope_value_to_record_parses_envelope() {
    let envelope = json_object([
        ("jsonrpc", json_string("2.0")),
        ("id", json_number_u64(1)),
        ("result", sample_anchor_record_value()),
    ]);
    let record = anchor_envelope_value_to_record(envelope);
    assert_eq!(record.address, "addr1");
    assert_eq!(record.hash, "hash1");
}

#[test]
fn resolved_did_from_value_parses_optional_fields() {
    let value = sample_resolved_did_value();
    let record = resolved_did_from_value(value).expect("resolved DID");
    assert_eq!(record.address, "addr1");
    assert_eq!(record.hash.as_deref(), Some("hash2"));
    assert_eq!(record.nonce, Some(8));
    assert_eq!(record.updated_at, Some(100));
    assert_eq!(record.public_key.as_deref(), Some("pk2"));
    let document = record.document.expect("document present");
    assert_eq!(document, json_object([("hello", json_number_u64(1))]));
    assert!(record.remote_attestation.is_some());
}

#[test]
fn resolved_did_value_to_record_parses_envelope() {
    let envelope = json_object([
        ("jsonrpc", json_string("2.0")),
        ("id", json_number_u64(1)),
        ("result", sample_resolved_did_value()),
    ]);
    let record = resolved_did_value_to_record(envelope);
    assert_eq!(record.address, "addr1");
    assert_eq!(record.hash.as_deref(), Some("hash2"));
}

#[test]
fn latest_header_value_to_header_parses_result() {
    let header = json_object([
        ("height", json_number_u64(55)),
        ("hash", json_string("hash-55")),
        ("difficulty", json_number_u64(9000)),
    ]);
    let envelope = json_object([
        ("jsonrpc", json_string("2.0")),
        ("id", json_number_u64(2)),
        ("result", header),
        ("error", json_null()),
    ]);
    let parsed = latest_header_value_to_header(envelope);
    assert_eq!(parsed.height, 55);
    assert_eq!(parsed.hash, "hash-55");
    assert_eq!(parsed.difficulty, 9000);
}
