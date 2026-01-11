use governance::Params;

#[test]
fn params_to_value_roundtrip() {
    let mut params = Params::default();
    params.treasury_percent = 15;
    params.lane_based_settlement_enabled = 1;
    let value = params.to_value().expect("serialize params");
    let obj = value.as_object().expect("params value should be an object");
    assert_eq!(
        obj.get("treasury_percent").and_then(|v| v.as_i64()),
        Some(15)
    );
    assert_eq!(
        obj.get("lane_based_settlement_enabled")
            .and_then(|v| v.as_i64()),
        Some(1)
    );
    let decoded = Params::deserialize(&value).expect("deserialize params");
    assert_eq!(decoded.treasury_percent, 15);
    assert_eq!(decoded.lane_based_settlement_enabled, 1);
}

#[test]
fn params_deserialize_rejects_non_object() {
    let value = foundation_serialization::json::Value::from(42);
    assert!(Params::deserialize(&value).is_err());
}
