use rustdct::DctPlanner;

/// Returns true if a burst is detected in the last 8 samples using a DCT-based
/// high/low frequency energy ratio.
pub fn haar_burst_veto(data: &[f64], eta: f64) -> bool {
    if data.len() < 8 {
        return false;
    }
    let window = &data[data.len() - 8..];
    let mut buf = window.to_vec();
    let mut planner = DctPlanner::new();
    let dct = planner.plan_dct2(8);
    dct.process_dct2(&mut buf);
    let low: f64 = buf[..4].iter().map(|v| v * v).sum();
    let high: f64 = buf[4..].iter().map(|v| v * v).sum();
    high > eta * eta * low
}
