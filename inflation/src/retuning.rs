use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct KalmanState {
    x: f64,
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
        .and_then(|b| serde_json::from_slice(&b).ok())
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
    let _ = fs::write(&state_path, serde_json::to_vec(&state).unwrap());
    m.round() as i64
}
