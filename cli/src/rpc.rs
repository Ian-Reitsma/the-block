#![allow(clippy::module_name_repetitions)]

use rand::Rng;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::tx::FeeLane;

const MAX_BACKOFF_EXPONENT: u32 = 30;

/// Simple JSON-RPC client with configurable timeouts and retry backoff.
#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    base_timeout: Duration,
    jitter: Duration,
    max_retries: u32,
    fault_rate: f64,
}

impl RpcClient {
    /// Construct a client from environment overrides.
    pub fn from_env() -> Self {
        let base = env_var("TB_RPC_TIMEOUT_MS", 5000);
        let jitter = env_var("TB_RPC_TIMEOUT_JITTER_MS", 1000);
        let retries = env_var("TB_RPC_MAX_RETRIES", 3);
        let fault = std::env::var("TB_RPC_FAULT_RATE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| !v.is_nan())
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(0.0);
        Self {
            http: Client::new(),
            base_timeout: Duration::from_millis(base),
            jitter: Duration::from_millis(jitter),
            max_retries: retries,
            fault_rate: fault,
        }
    }

    fn timeout_with_jitter(&self) -> Duration {
        let extra = rand::thread_rng().gen_range(0..=self.jitter.as_millis() as u64);
        self.base_timeout + Duration::from_millis(extra)
    }

    fn backoff_with_jitter(&self, attempt: u32) -> Duration {
        let exponent = attempt.min(MAX_BACKOFF_EXPONENT);
        let multiplier = 1u64 << exponent;
        let base = self
            .base_timeout
            .checked_mul(multiplier as u32)
            .unwrap_or(Duration::MAX);
        let jitter =
            Duration::from_millis(rand::thread_rng().gen_range(0..=self.jitter.as_millis() as u64));
        base.checked_add(jitter).unwrap_or(Duration::MAX)
    }

    fn maybe_inject_fault(&self) -> Result<(), RpcClientError> {
        if self.fault_rate > 0.0 && rand::thread_rng().gen_bool(self.fault_rate) {
            return Err(RpcClientError::InjectedFault);
        }
        Ok(())
    }

    /// Perform a JSON-RPC call to `url` with `payload`, retrying on timeout.
    pub fn call<T: Serialize>(&self, url: &str, payload: &T) -> Result<Response, RpcClientError> {
        let mut attempt = 0;
        loop {
            let timeout = self.timeout_with_jitter();
            let start = Instant::now();
            self.maybe_inject_fault()?;
            let result = self
                .http
                .post(url)
                .json(payload)
                .timeout(timeout)
                .send()
                .map_err(RpcClientError::from);
            match result {
                Ok(resp) => return Ok(resp),
                Err(RpcClientError::Transport(err))
                    if attempt < self.max_retries && err.is_timeout() =>
                {
                    attempt += 1;
                    let delay = self.backoff_with_jitter(attempt);
                    if delay > start.elapsed() {
                        sleep(delay - start.elapsed());
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    #[allow(dead_code)]
    pub fn mempool_stats(&self, url: &str, lane: FeeLane) -> Result<MempoolStats, RpcClientError> {
        #[derive(Serialize)]
        #[allow(dead_code)]
        struct Payload<'a> {
            jsonrpc: &'static str,
            id: u32,
            method: &'static str,
            params: Value,
            #[serde(skip_serializing_if = "Option::is_none")]
            auth: Option<&'a str>,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Envelope<T> {
            result: T,
        }
        let params = serde_json::json!({ "lane": lane.as_str() });
        let payload = Payload {
            jsonrpc: "2.0",
            id: 1,
            method: "mempool.stats",
            params,
            auth: None,
        };
        let res = self
            .call(url, &payload)?
            .json::<Envelope<MempoolStats>>()
            .map_err(RpcClientError::from)?;
        Ok(res.result)
    }

    #[allow(dead_code)]
    pub fn record_wallet_qos_event(
        &self,
        url: &str,
        event: WalletQosEvent<'_>,
    ) -> Result<(), WalletQosError> {
        #[derive(Serialize)]
        #[allow(dead_code)]
        struct Payload<'a> {
            jsonrpc: &'static str,
            id: u32,
            method: &'static str,
            params: WalletQosParams<'a>,
        }
        #[derive(Serialize)]
        #[allow(dead_code)]
        struct WalletQosParams<'a> {
            event: &'a str,
            lane: &'a str,
            fee: u64,
            floor: u64,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct WalletQosAck {
            status: Option<String>,
        }

        let params = WalletQosParams {
            event: event.event,
            lane: event.lane,
            fee: event.fee,
            floor: event.floor,
        };
        let payload = Payload {
            jsonrpc: "2.0",
            id: 1,
            method: "mempool.qos_event",
            params,
        };
        let envelope = self
            .call(url, &payload)
            .map_err(WalletQosError::from)?
            .json::<RpcEnvelope<WalletQosAck>>()
            .map_err(RpcClientError::from)
            .map_err(WalletQosError::from)?;

        if let Some(error) = envelope.error {
            return Err(WalletQosError::Rpc {
                code: error.code,
                message: error.message,
            });
        }

        let status = envelope
            .result
            .and_then(|ack| ack.status)
            .ok_or(WalletQosError::MissingStatus)?;

        if status != "ok" {
            return Err(WalletQosError::InvalidStatus(status));
        }

        Ok(())
    }
}

fn env_var<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[derive(Debug)]
pub enum RpcClientError {
    Transport(reqwest::Error),
    InjectedFault,
}

impl fmt::Display for RpcClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcClientError::Transport(err) => write!(f, "transport error: {err}"),
            RpcClientError::InjectedFault => f.write_str("fault injection triggered"),
        }
    }
}

impl std::error::Error for RpcClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcClientError::Transport(err) => Some(err),
            RpcClientError::InjectedFault => None,
        }
    }
}

impl From<reqwest::Error> for RpcClientError {
    fn from(err: reqwest::Error) -> Self {
        RpcClientError::Transport(err)
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MempoolStats {
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub age_p50: u64,
    #[serde(default)]
    pub age_p95: u64,
    #[serde(default)]
    pub fee_p50: u64,
    #[serde(default)]
    pub fee_p90: u64,
    #[serde(default)]
    pub fee_floor: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RpcEnvelope<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct WalletQosEvent<'a> {
    pub event: &'a str,
    pub lane: &'a str,
    pub fee: u64,
    pub floor: u64,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum WalletQosError {
    Transport(RpcClientError),
    Rpc { code: i64, message: String },
    MissingStatus,
    InvalidStatus(String),
}

impl fmt::Display for WalletQosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Rpc { code, message } => write!(f, "rpc error {code}: {message}"),
            Self::MissingStatus => write!(f, "rpc response missing status field"),
            Self::InvalidStatus(status) => {
                write!(f, "rpc response returned unexpected status '{status}'")
            }
        }
    }
}

impl std::error::Error for WalletQosError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(err) => Some(err),
            _ => None,
        }
    }
}

impl From<RpcClientError> for WalletQosError {
    fn from(err: RpcClientError) -> Self {
        Self::Transport(err)
    }
}
