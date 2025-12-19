#![allow(clippy::module_name_repetitions)]

use foundation_serialization::json::Value;
use foundation_serialization::{Deserialize, Serialize};
use httpd::{BlockingClient, ClientError as HttpClientError, ClientResponse, Method};
use rand::Rng;
use std::fmt;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::http_client;
use crate::json_helpers::{
    json_array_from, json_object_from, json_rpc_request, json_string, json_u64,
};
use crate::tx::FeeLane;

const MAX_BACKOFF_EXPONENT: u32 = 30;

/// Simple JSON-RPC client with configurable timeouts and retry backoff.
#[derive(Clone)]
pub struct RpcClient {
    http: BlockingClient,
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
            http: http_client::blocking_client(),
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
    pub fn call<T: Serialize>(
        &self,
        url: &str,
        payload: &T,
    ) -> Result<ClientResponse, RpcClientError> {
        self.call_with_auth(url, payload, None)
    }

    /// Perform a JSON-RPC call with an optional `Authorization` header.
    pub fn call_with_auth<T: Serialize>(
        &self,
        url: &str,
        payload: &T,
        auth: Option<&str>,
    ) -> Result<ClientResponse, RpcClientError> {
        let mut attempt = 0;
        loop {
            let timeout = self.timeout_with_jitter();
            let start = Instant::now();
            self.maybe_inject_fault()?;
            let request = self
                .http
                .request(Method::Post, url)
                .map_err(RpcClientError::from)?
                .timeout(timeout);
            let request = if let Some(token) = auth {
                request.header("authorization", token)
            } else {
                request
            };
            let request = request.json(payload).map_err(RpcClientError::from)?;
            let result = request.send().map_err(RpcClientError::from);
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
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Envelope<T> {
            result: T,
        }
        let params = json_object_from([("lane", json_string(lane.as_str()))]);
        let payload = json_rpc_request("mempool.stats", params);
        let res = self
            .call(url, &payload)?
            .json::<Envelope<MempoolStats>>()
            .map_err(RpcClientError::from)?;
        Ok(res.result)
    }

    pub fn governor_status(&self, url: &str) -> Result<Value, RpcClientError> {
        let payload = json_rpc_request("governor.status", Value::Array(vec![]));
        let envelope = self
            .call(url, &payload)?
            .json::<RpcEnvelope<Value>>()
            .map_err(RpcClientError::from)?;
        extract_rpc_result(envelope)
    }

    pub fn governor_decisions(&self, url: &str, limit: u64) -> Result<Value, RpcClientError> {
        let params = json_array_from(vec![json_u64(limit)]);
        let payload = json_rpc_request("governor.decisions", params);
        let envelope = self
            .call(url, &payload)?
            .json::<RpcEnvelope<Value>>()
            .map_err(RpcClientError::from)?;
        extract_rpc_result(envelope)
    }

    #[allow(dead_code)]
    pub fn record_wallet_qos_event(
        &self,
        url: &str,
        event: WalletQosEvent<'_>,
    ) -> Result<(), WalletQosError> {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct WalletQosAck {
            status: Option<String>,
        }

        let params = json_object_from([
            ("event", json_string(event.event)),
            ("lane", json_string(event.lane)),
            ("fee", json_u64(event.fee)),
            ("floor", json_u64(event.floor)),
        ]);
        let payload = json_rpc_request("mempool.qos_event", params);
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
    Transport(HttpClientError),
    InjectedFault,
    Rpc { code: i64, message: String },
}

impl fmt::Display for RpcClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcClientError::Transport(err) => write!(f, "transport error: {err}"),
            RpcClientError::InjectedFault => f.write_str("fault injection triggered"),
            RpcClientError::Rpc { code, message } => {
                write!(f, "rpc error {code}: {message}")
            }
        }
    }
}

impl std::error::Error for RpcClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcClientError::Transport(err) => Some(err),
            RpcClientError::InjectedFault => None,
            RpcClientError::Rpc { .. } => None,
        }
    }
}

impl From<HttpClientError> for RpcClientError {
    fn from(err: HttpClientError) -> Self {
        RpcClientError::Transport(err)
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MempoolStats {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub size: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub age_p50: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub age_p95: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub fee_p50: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub fee_p90: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
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

fn extract_rpc_result(envelope: RpcEnvelope<Value>) -> Result<Value, RpcClientError> {
    if let Some(error) = envelope.error {
        Err(RpcClientError::Rpc {
            code: error.code,
            message: error.message,
        })
    } else if let Some(result) = envelope.result {
        Ok(result)
    } else {
        Err(RpcClientError::Rpc {
            code: -1,
            message: "missing result".into(),
        })
    }
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
