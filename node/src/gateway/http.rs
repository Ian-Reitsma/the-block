use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use concurrency::Lazy;

// lightweight HTTP request parsing for fuzzing
use httparse;

use super::read_receipt;

#[cfg(feature = "telemetry")]
use crate::telemetry::READ_DENIED_TOTAL;

#[derive(Clone, Copy)]
pub struct RateConfig {
    pub tokens_per_minute: f64,
    pub burst: f64,
}

impl Default for RateConfig {
    fn default() -> Self {
        Self {
            tokens_per_minute: 60.0,
            burst: 60.0,
        }
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

static IP_BUCKETS: Lazy<Mutex<HashMap<String, Bucket>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static ID_BUCKETS: Lazy<Mutex<HashMap<String, Bucket>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn take(map: &Mutex<HashMap<String, Bucket>>, key: &str, cfg: &RateConfig, now: Instant) -> bool {
    let mut m = map.lock().unwrap_or_else(|e| e.into_inner());
    let b = m.entry(key.to_owned()).or_insert(Bucket {
        tokens: cfg.burst,
        last: now,
    });
    let dt = now.duration_since(b.last).as_secs_f64() / 60.0;
    b.tokens = (b.tokens + dt * cfg.tokens_per_minute).min(cfg.burst);
    b.last = now;
    if b.tokens >= 1.0 {
        b.tokens -= 1.0;
        true
    } else {
        #[cfg(feature = "telemetry")]
        {
            READ_DENIED_TOTAL
                .ensure_handle_for_label_values(&["limit"])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc();
        }
        false
    }
}

pub fn check(
    ip: &str,
    identity: Option<&str>,
    domain: &str,
    provider_id: &str,
    cfg: &RateConfig,
) -> bool {
    let now = Instant::now();
    if !take(&IP_BUCKETS, ip, cfg, now) {
        let _ = read_receipt::append(domain, provider_id, 0, false, false);
        return false;
    }
    if let Some(id) = identity {
        if !take(&ID_BUCKETS, id, cfg, now) {
            let _ = read_receipt::append(domain, provider_id, 0, false, false);
            return false;
        }
    }
    true
}

/// Parse an HTTP request and return without panicking on malformed input.
/// Used by fuzz targets to ensure graceful handling of arbitrary data.
pub fn parse_request(data: &[u8]) {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    let _ = req.parse(data);
}
