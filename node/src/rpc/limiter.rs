#[cfg(feature = "telemetry")]
use crate::telemetry;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[doc(hidden)]
pub struct ClientState {
    pub tokens: f64,
    pub last: Instant,
    pub banned_until: Option<Instant>,
}

#[derive(Copy, Clone, Debug)]
pub enum RpcClientErrorCode {
    RateLimit,
    Banned,
}

impl RpcClientErrorCode {
    pub fn rpc_code(&self) -> i32 {
        match self {
            Self::RateLimit => -32001,
            Self::Banned => -32002,
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            Self::RateLimit => "rate limited",
            Self::Banned => "banned",
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RateLimit => "2000",
            Self::Banned => "2001",
        }
    }
}

pub fn check_client(
    addr: &IpAddr,
    clients: &Arc<Mutex<HashMap<IpAddr, ClientState>>>,
    tokens_per_sec: f64,
    ban_secs: u64,
    timeout_secs: u64,
) -> Result<(), RpcClientErrorCode> {
    #[cfg(feature = "telemetry")]
    telemetry::RPC_RATE_LIMIT_ATTEMPT_TOTAL.inc();
    let mut map = clients.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    map.retain(|_, c| now.duration_since(c.last).as_secs() <= timeout_secs);
    let entry = map.entry(*addr).or_insert(ClientState {
        tokens: tokens_per_sec,
        last: Instant::now(),
        banned_until: None,
    });
    if let Some(until) = entry.banned_until {
        if until > now {
            #[cfg(feature = "telemetry")]
            telemetry::RPC_RATE_LIMIT_REJECT_TOTAL.inc();
            log::warn!("rate_limit_exceeded client={}", addr);
            return Err(RpcClientErrorCode::Banned);
        } else {
            entry.banned_until = None;
        }
    }
    let elapsed = now.duration_since(entry.last).as_secs_f64();
    entry.tokens = (entry.tokens + elapsed * tokens_per_sec).min(tokens_per_sec);
    entry.last = now;
    if entry.tokens >= 1.0 {
        entry.tokens -= 1.0;
        #[cfg(feature = "telemetry")]
        telemetry::RPC_TOKENS
            .with_label_values(&[&addr.to_string()])
            .set(entry.tokens);
        return Ok(());
    }
    entry.banned_until = Some(now + Duration::from_secs(ban_secs));
    #[cfg(feature = "telemetry")]
    {
        telemetry::RPC_TOKENS
            .with_label_values(&[&addr.to_string()])
            .set(entry.tokens);
        telemetry::RPC_BANS_TOTAL.inc();
        telemetry::RPC_RATE_LIMIT_REJECT_TOTAL.inc();
    }
    log::warn!("rate_limit_exceeded client={}", addr);
    Err(RpcClientErrorCode::RateLimit)
}
