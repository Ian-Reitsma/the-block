use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

/// Estimated capacity metrics for the industrial lane.
#[derive(Clone, Copy, Debug, Default)]
pub struct Capacity {
    pub shards_per_sec: u64,
}

static HISTORY: Mutex<VecDeque<u64>> = Mutex::new(VecDeque::new());
const WINDOW: usize = 8;

// --- Fair-share accounting -------------------------------------------------

#[derive(Clone, Debug)]
struct Usage {
    shards_seconds: f64,
    last_update: Instant,
}

#[derive(Clone, Debug)]
struct Quota {
    remaining: f64,
    last_refill: Instant,
}

static BUYER_USAGE: Lazy<DashMap<String, Usage>> = Lazy::new(DashMap::new);
static PROVIDER_USAGE: Lazy<DashMap<String, Usage>> = Lazy::new(DashMap::new);
static BUYER_QUOTA: Lazy<DashMap<String, Quota>> = Lazy::new(DashMap::new);
static PROVIDER_QUOTA: Lazy<DashMap<String, Quota>> = Lazy::new(DashMap::new);

#[cfg(test)]
const WINDOW_SECS: f64 = 6.0;
#[cfg(not(test))]
const WINDOW_SECS: f64 = 60.0;
const FAIR_SHARE_CAP: f64 = 0.25; // 25% of capacity window
#[cfg(test)]
const BURST_QUOTA: f64 = 3.0; // micro-shard-seconds
#[cfg(not(test))]
const BURST_QUOTA: f64 = 30.0; // micro-shard-seconds

/// Reasons an admission can be rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    Capacity,
    FairShare,
    BurstExhausted,
}

fn decay_usage(u: &mut Usage, now: Instant) {
    let elapsed = now.duration_since(u.last_update).as_secs_f64();
    let factor = (WINDOW_SECS - elapsed).max(0.0) / WINDOW_SECS;
    u.shards_seconds *= factor;
    u.last_update = now;
}

fn refill_quota(q: &mut Quota, now: Instant) {
    let elapsed = now.duration_since(q.last_refill).as_secs_f64();
    q.remaining = (q.remaining + elapsed * BURST_QUOTA / WINDOW_SECS).min(BURST_QUOTA);
    q.last_refill = now;
}

/// Check fair-share and burst quotas. `demand` is in micro-shard-seconds.
pub fn check_and_record(buyer: &str, provider: &str, demand: u64) -> Result<(), RejectReason> {
    let cap = capacity_estimator();
    let window_cap = cap.shards_per_sec as f64 * WINDOW_SECS;
    let demand_f = demand as f64;
    let now = Instant::now();

    // Capacity gate
    if demand as u64 > cap.shards_per_sec {
        return Err(RejectReason::Capacity);
    }

    let mut b_entry = BUYER_USAGE
        .entry(buyer.to_string())
        .or_insert(Usage {
            shards_seconds: 0.0,
            last_update: now,
        });
    decay_usage(&mut *b_entry, now);
    let buyer_proj = b_entry.shards_seconds + demand_f;
    let buyer_share = if window_cap > 0.0 {
        buyer_proj / window_cap
    } else {
        1.0
    };
    drop(b_entry);

    let mut p_entry = PROVIDER_USAGE
        .entry(provider.to_string())
        .or_insert(Usage {
            shards_seconds: 0.0,
            last_update: now,
        });
    decay_usage(&mut *p_entry, now);
    let provider_proj = p_entry.shards_seconds + demand_f;
    let provider_share = if window_cap > 0.0 {
        provider_proj / window_cap
    } else {
        1.0
    };
    drop(p_entry);

    if buyer_share > FAIR_SHARE_CAP || provider_share > FAIR_SHARE_CAP {
        let mut bq = BUYER_QUOTA
            .entry(buyer.to_string())
            .or_insert(Quota {
                remaining: BURST_QUOTA,
                last_refill: now,
            });
        refill_quota(&mut *bq, now);
        let mut pq = PROVIDER_QUOTA
            .entry(provider.to_string())
            .or_insert(Quota {
                remaining: BURST_QUOTA,
                last_refill: now,
            });
        refill_quota(&mut *pq, now);
        if bq.remaining >= demand_f && pq.remaining >= demand_f {
            bq.remaining -= demand_f;
            pq.remaining -= demand_f;
            #[cfg(feature = "telemetry")]
            {
                use crate::telemetry::ACTIVE_BURST_QUOTA;
                ACTIVE_BURST_QUOTA
                    .with_label_values(&[buyer])
                    .set(bq.remaining as i64);
                ACTIVE_BURST_QUOTA
                    .with_label_values(&[provider])
                    .set(pq.remaining as i64);
            }
        } else if bq.remaining == 0.0 || pq.remaining == 0.0 {
            return Err(RejectReason::FairShare);
        } else {
            return Err(RejectReason::BurstExhausted);
        }
    }

    let mut b_entry = BUYER_USAGE
        .entry(buyer.to_string())
        .or_insert(Usage {
            shards_seconds: 0.0,
            last_update: now,
        });
    b_entry.shards_seconds += demand_f;
    b_entry.last_update = now;
    drop(b_entry);

    let mut p_entry = PROVIDER_USAGE
        .entry(provider.to_string())
        .or_insert(Usage {
            shards_seconds: 0.0,
            last_update: now,
        });
    p_entry.shards_seconds += demand_f;
    p_entry.last_update = now;

    Ok(())
}

/// Record observed available shard throughput.
pub fn record_available_shards(shards: u64) {
    let mut h = HISTORY.lock().unwrap();
    if h.len() == WINDOW {
        h.pop_front();
    }
    h.push_back(shards);
}

/// Estimate current capacity using a moving average over the sample window.
pub fn capacity_estimator() -> Capacity {
    let h = HISTORY.lock().unwrap();
    if h.is_empty() {
        Capacity { shards_per_sec: 0 }
    } else {
        let avg = h.iter().sum::<u64>() / h.len() as u64;
        Capacity {
            shards_per_sec: avg,
        }
    }
}

pub fn reset() {
    BUYER_USAGE.clear();
    PROVIDER_USAGE.clear();
    BUYER_QUOTA.clear();
    PROVIDER_QUOTA.clear();
    HISTORY.lock().unwrap().clear();
}
