use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use crate::telemetry;

const WINDOW: usize = 50;

struct FeeStats {
    window: VecDeque<u64>,
    counts: BTreeMap<u64, usize>,
}

static CONSUMER_FEES: Mutex<FeeStats> = Mutex::new(FeeStats {
    window: VecDeque::new(),
    counts: BTreeMap::new(),
});

/// Record a consumer lane fee and update p50/p90 gauges.
pub fn record_consumer_fee(fee: u64) {
    let mut stats = CONSUMER_FEES.lock().unwrap();
    if stats.window.len() == WINDOW {
        if let Some(old) = stats.window.pop_front() {
            if let Some(count) = stats.counts.get_mut(&old) {
                *count -= 1;
                if *count == 0 {
                    stats.counts.remove(&old);
                }
            }
        }
    }
    stats.window.push_back(fee);
    *stats.counts.entry(fee).or_insert(0) += 1;
    update_metrics(&stats);
}

fn percentile(stats: &FeeStats, q: f64) -> u64 {
    if stats.window.is_empty() {
        return 0;
    }
    let target = ((stats.window.len() as f64) * q).floor() as usize;
    let mut acc = 0usize;
    for (fee, count) in stats.counts.iter() {
        acc += *count;
        if acc > target {
            return *fee;
        }
    }
    *stats.counts.iter().next_back().unwrap().0
}

fn update_metrics(stats: &FeeStats) {
    if stats.window.is_empty() {
        telemetry::CONSUMER_FEE_P50.set(0);
        telemetry::CONSUMER_FEE_P90.set(0);
        return;
    }
    let p50 = percentile(stats, 0.5);
    let p90 = percentile(stats, 0.9);
    telemetry::CONSUMER_FEE_P50.set(p50 as i64);
    telemetry::CONSUMER_FEE_P90.set(p90 as i64);
}

/// Return the current consumer fee p90 value.
pub fn consumer_p90() -> u64 {
    let stats = CONSUMER_FEES.lock().unwrap();
    percentile(&stats, 0.9)
}

/// Return the current consumer fee median.
pub fn consumer_p50() -> u64 {
    let stats = CONSUMER_FEES.lock().unwrap();
    percentile(&stats, 0.5)
}
