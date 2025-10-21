use foundation_serialization::json::{Map as JsonMap, Number, Value};

pub fn json_string<S: Into<String>>(value: S) -> Value {
    Value::String(value.into())
}

pub fn json_option_string<S>(value: Option<S>) -> Value
where
    S: Into<String>,
{
    value
        .map(|inner| Value::String(inner.into()))
        .unwrap_or(Value::Null)
}

#[allow(dead_code)]
pub fn json_bool(value: bool) -> Value {
    Value::Bool(value)
}

pub fn json_null() -> Value {
    Value::Null
}

pub fn json_u64(value: u64) -> Value {
    Value::Number(Number::from(value))
}

#[allow(dead_code)]
pub fn json_i64(value: i64) -> Value {
    Value::Number(Number::from(value))
}

pub fn json_f64(value: f64) -> Value {
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

#[allow(dead_code)]
pub fn json_array_from<I>(values: I) -> Value
where
    I: IntoIterator<Item = Value>,
{
    Value::Array(values.into_iter().collect())
}

pub fn json_map_from<I, K>(pairs: I) -> JsonMap
where
    I: IntoIterator<Item = (K, Value)>,
    K: Into<String>,
{
    let mut map = JsonMap::new();
    for (key, value) in pairs {
        map.insert(key.into(), value);
    }
    map
}

pub fn json_object_from<I, K>(pairs: I) -> Value
where
    I: IntoIterator<Item = (K, Value)>,
    K: Into<String>,
{
    Value::Object(json_map_from(pairs))
}

pub fn empty_object() -> Value {
    Value::Object(JsonMap::new())
}

pub fn json_rpc_request(method: &str, params: Value) -> Value {
    json_rpc_request_with_id(method, params, 1)
}

pub fn json_rpc_request_with_id(method: &str, params: Value, id: u64) -> Value {
    json_rpc_request_with_auth_and_id(method, params, id, None)
}

#[allow(dead_code)]
pub fn json_rpc_request_with_auth(method: &str, params: Value, auth: Option<&str>) -> Value {
    json_rpc_request_with_auth_and_id(method, params, 1, auth)
}

pub fn json_rpc_request_with_auth_and_id(
    method: &str,
    params: Value,
    id: u64,
    auth: Option<&str>,
) -> Value {
    let mut map = JsonMap::new();
    map.insert("jsonrpc".to_owned(), json_string("2.0"));
    map.insert("id".to_owned(), json_u64(id));
    map.insert("method".to_owned(), json_string(method));
    map.insert("params".to_owned(), params);
    if let Some(token) = auth {
        map.insert("auth".to_owned(), json_string(token));
    }
    Value::Object(map)
}
