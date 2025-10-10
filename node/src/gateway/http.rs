use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use concurrency::Lazy;

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
    let _ = parse_http_request(data);
}

fn parse_http_request(data: &[u8]) -> Result<(), ()> {
    let (line, mut rest) = take_line(data).ok_or(())?;
    let tokens = split_ascii_whitespace(line);
    if tokens.len() != 3 {
        return Err(());
    }

    let mut content_length = None;
    loop {
        let (line, next) = match take_line(rest) {
            Some(v) => v,
            None => break,
        };
        rest = next;
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = split_header(line) {
            if equals_ignore_ascii_case(name, b"content-length") {
                if let Ok(len) = parse_decimal(value) {
                    content_length = Some(len);
                }
            }
        }
    }

    if let Some(len) = content_length {
        rest.get(..len).ok_or(())?;
    }

    Ok(())
}

fn take_line(data: &[u8]) -> Option<(&[u8], &[u8])> {
    if data.is_empty() {
        return None;
    }
    if let Some(pos) = data.iter().position(|&b| b == b'\n') {
        let line = trim_cr(&data[..pos]);
        let rest = &data[pos + 1..];
        Some((line, rest))
    } else {
        let line = trim_cr(data);
        Some((line, &[]))
    }
}

fn trim_cr(line: &[u8]) -> &[u8] {
    if line.ends_with(b"\r") {
        &line[..line.len() - 1]
    } else {
        line
    }
}

fn split_ascii_whitespace(line: &[u8]) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (idx, byte) in line.iter().enumerate() {
        if byte.is_ascii_whitespace() {
            if let Some(s) = start.take() {
                tokens.push(&line[s..idx]);
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(s) = start {
        tokens.push(&line[s..]);
    }
    tokens
}

fn split_header(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let position = line.iter().position(|&b| b == b':')?;
    let name = trim_ascii(&line[..position]);
    let mut value = &line[position + 1..];
    while let Some((first, rest)) = value.split_first() {
        if first.is_ascii_whitespace() {
            value = rest;
        } else {
            break;
        }
    }
    Some((name, trim_ascii(value)))
}

fn equals_ignore_ascii_case(lhs: &[u8], rhs: &[u8]) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }
    lhs.iter()
        .zip(rhs.iter())
        .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn parse_decimal(mut value: &[u8]) -> Result<usize, ()> {
    value = trim_ascii(value);
    if value.is_empty() {
        return Err(());
    }
    let mut total: usize = 0;
    for byte in value {
        if !byte.is_ascii_digit() {
            return Err(());
        }
        let digit = (byte - b'0') as usize;
        total = total
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit))
            .ok_or(())?;
    }
    Ok(total)
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while let Some((first, rest)) = value.split_first() {
        if first.is_ascii_whitespace() {
            value = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = value.split_last() {
        if last.is_ascii_whitespace() {
            value = rest;
        } else {
            break;
        }
    }
    value
}
