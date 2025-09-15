use super::constants::{DIFFICULTY_CLAMP_FACTOR, TARGET_SPACING_MS};
use crate::governance::Params;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    DIFFICULTY_WINDOW_LONG, DIFFICULTY_WINDOW_MED, DIFFICULTY_WINDOW_SHORT,
};

fn ema(intervals: &[f64], window: usize) -> f64 {
    if intervals.is_empty() {
        return TARGET_SPACING_MS as f64;
    }
    let k = 2.0 / (window as f64 + 1.0);
    let mut ema = intervals[0];
    for &v in &intervals[1..] {
        ema = v * k + ema * (1.0 - k);
    }
    ema
}

/// Multi-window difficulty retarget with simple Kalman-style weighting.
pub fn retune(prev: u64, timestamps: &[u64], hint: i8, params: &Params) -> (u64, i8) {
    if timestamps.len() < 2 {
        return (prev.max(1), 0);
    }
    let intervals: Vec<f64> = timestamps
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]) as f64)
        .collect();
    let short = ema(&intervals, 5);
    let med = ema(&intervals, 15);
    let long = ema(&intervals, 60);
    #[cfg(feature = "telemetry")]
    {
        DIFFICULTY_WINDOW_SHORT.set(short.round() as i64);
        DIFFICULTY_WINDOW_MED.set(med.round() as i64);
        DIFFICULTY_WINDOW_LONG.set(long.round() as i64);
    }
    let ks = params.kalman_r_short.max(1) as f64;
    let km = params.kalman_r_med.max(1) as f64;
    let kl = params.kalman_r_long.max(1) as f64;
    let total = ks + km + kl;
    let predicted = (short * ks + med * km + long * kl) / total;
    let mut next = (prev as f64) * predicted / TARGET_SPACING_MS as f64;
    // Apply previous hint as ±5% adjustment.
    let adjust = 1.0 + (hint as f64) * 0.05;
    next *= adjust;
    let min = (prev / DIFFICULTY_CLAMP_FACTOR) as f64;
    let max = (prev.saturating_mul(DIFFICULTY_CLAMP_FACTOR)) as f64;
    if next < min {
        next = min;
    }
    if next > max {
        next = max;
    }
    let trend = short - long;
    let new_hint = if trend < -1.0 {
        -1
    } else if trend > 1.0 {
        1
    } else {
        0
    };
    ((next.round() as u64).max(1), new_hint)
}
