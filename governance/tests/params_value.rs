use governance::Params;

#[test]
fn params_to_value_roundtrip() {
    let mut params = Params::default();
    params.treasury_percent_ct = 15;
    let value = params.to_value().expect("serialize params");
    let obj = value.as_object().expect("params value should be an object");
    assert_eq!(
        obj.get("treasury_percent_ct").and_then(|v| v.as_i64()),
        Some(15)
    );
    let decoded = Params::deserialize(&value).expect("deserialize params");
    assert_eq!(decoded.treasury_percent_ct, 15);
}

#[test]
fn params_deserialize_rejects_non_object() {
    let value = foundation_serialization::json::Value::from(42);
    assert!(Params::deserialize(&value).is_err());
}
