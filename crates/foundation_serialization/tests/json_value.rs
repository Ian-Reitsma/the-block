use foundation_serialization::json::{self, Value};
use foundation_serialization::Error;

#[test]
fn json_value_round_trip_preserves_nested_structures() {
    let value = foundation_serialization::json!({
        "service": {
            "name": "aggregator",
            "ports": [9000, 9443],
            "features": {
                "mTLS": true,
                "warnings": ["stale", "missing_anchor"],
            }
        },
        "retention": null,
        "metrics": [1, 2, 3, 4],
    });

    let encoded = json::to_vec(&value).expect("encode nested value");
    let decoded: Value = json::from_slice(&encoded).expect("decode nested value");
    assert_eq!(decoded, value);
    assert!(decoded
        .as_object()
        .and_then(|map| map.get("service"))
        .and_then(Value::as_object)
        .is_some());
}

#[test]
fn json_value_encoder_rejects_non_finite_floats() {
    let err = json::to_value(f64::INFINITY).expect_err("non-finite float must error");
    match err {
        Error::Json(inner) => {
            assert!(
                format!("{inner}").contains("non-finite"),
                "unexpected error: {inner}"
            );
        }
        other => panic!("unexpected error variant: {:?}", other),
    }
}

#[test]
fn json_value_object_last_key_wins() {
    let parsed = json::value_from_str("{\"count\": 1, \"count\": 5}").expect("parse object");
    let map = parsed.as_object().expect("object value");
    assert_eq!(map.get("count"), Some(&Value::from(5))); // duplicate keys keep last value
}
