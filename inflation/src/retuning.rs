use foundation_serialization::json::{self, Map, Number, Value};
use std::fs;
use std::path::Path;

struct KalmanState {
    x: f64,
}

fn decode_state(bytes: &[u8]) -> Option<KalmanState> {
    let value: Value = json::from_slice(bytes).ok()?;
    match value {
        Value::Object(map) => map.get("x").and_then(|value| match value {
            Value::Number(num) => Some(KalmanState { x: num.as_f64() }),
            Value::String(s) => s
                .parse::<f64>()
                .ok()
                .map(|x| KalmanState { x }),
            Value::Bool(b) => Some(KalmanState { x: if *b { 1.0 } else { 0.0 } }),
            Value::Null => Some(KalmanState { x: 0.0 }),
            _ => None,
        }),
        _ => None,
    }
}

fn encode_state(state: &KalmanState) -> Vec<u8> {
    let mut map = Map::new();
    let number = Number::from_f64(state.x).unwrap_or_else(|| Number::from(0.0));
    map.insert("x".to_owned(), Value::Number(number));
    json::to_vec(&Value::Object(map)).expect("serialize KalmanState")
}

/// Retune the industrial subsidy multiplier using backlog and utilisation metrics.
///
/// The multiplier is clamped to ±15 % of the previous value and persisted
/// under `governance/history/industrial_kalman.json`.
pub fn retune_industrial_multiplier(
    base: &Path,
    current: i64,
    backlog: f64,
    utilisation: f64,
) -> i64 {
    let hist_dir = base.join("governance/history");
    let _ = fs::create_dir_all(&hist_dir);
    let state_path = hist_dir.join("industrial_kalman.json");
    let mut state: KalmanState = fs::read(&state_path)
        .ok()
        .and_then(|b| decode_state(&b))
        .unwrap_or(KalmanState { x: current as f64 });
    let mut m = if utilisation <= 0.0 {
        state.x * 2.0
    } else {
        state.x * (1.0 + backlog / utilisation.max(1e-9))
    };
    let max = current as f64 * 1.15;
    let min = current as f64 * 0.85;
    m = m.clamp(min, max);
    state.x = m;
    let _ = fs::write(&state_path, encode_state(&state));
    m.round() as i64
}
