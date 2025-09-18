use rand::Rng;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::transaction::FeeLane;

/// Simple JSON-RPC client with jittered timeouts and retry backoff.
#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    base_timeout: Duration,
    jitter: Duration,
    max_retries: u32,
    fault_rate: f64,
}

impl RpcClient {
    /// Construct from environment variables:
    /// - `TB_RPC_TIMEOUT_MS` base timeout in milliseconds (default 5000)
    /// - `TB_RPC_TIMEOUT_JITTER_MS` added random jitter (default 1000)
    /// - `TB_RPC_MAX_RETRIES` number of retries on failure (default 3)
    pub fn from_env() -> Self {
        let base = std::env::var("TB_RPC_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        let jitter = std::env::var("TB_RPC_TIMEOUT_JITTER_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);
        let retries = std::env::var("TB_RPC_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        let fault = std::env::var("TB_RPC_FAULT_RATE")
            .ok()
            .and_then(|v| v.parse().ok())
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
        // exponential backoff with jitter
        let base = self.base_timeout * (1 << attempt);
        let extra = rand::thread_rng().gen_range(0..=self.jitter.as_millis() as u64);
        base + Duration::from_millis(extra)
    }

    fn maybe_inject_fault(&self) -> Result<(), reqwest::Error> {
        if self.fault_rate > 0.0 && rand::thread_rng().gen_bool(self.fault_rate) {
            return Err(reqwest::Error::from(io::Error::new(
                io::ErrorKind::Other,
                "injected fault",
            )));
        }
        Ok(())
    }

    /// Perform a JSON-RPC call to `url` with `payload`, retrying on timeout.
    pub fn call<T: Serialize>(&self, url: &str, payload: &T) -> Result<Response, reqwest::Error> {
        let mut attempt = 0;
        loop {
            let timeout = self.timeout_with_jitter();
            let start = Instant::now();
            self.maybe_inject_fault()?;
            let res = self.http.post(url).json(payload).timeout(timeout).send();
            match res {
                Ok(r) => return Ok(r),
                Err(e) if attempt < self.max_retries && e.is_timeout() => {
                    attempt += 1;
                    let delay = self.backoff_with_jitter(attempt);
                    // ensure we don't spin too fast if server responds immediately
                    if delay > start.elapsed() {
                        sleep(delay - start.elapsed());
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct InflationParams {
    pub beta_storage_sub_ct: i64,
    pub gamma_read_sub_ct: i64,
    pub kappa_cpu_sub_ct: i64,
    pub lambda_bytes_out_sub_ct: i64,
    pub rent_rate_ct_per_byte: i64,
    pub industrial_multiplier: i64,
    pub industrial_backlog: u64,
    pub industrial_utilization: u64,
}

impl RpcClient {
    pub fn mempool_stats(&self, url: &str, lane: FeeLane) -> Result<MempoolStats, reqwest::Error> {
        #[derive(Serialize)]
        struct Payload<'a> {
            jsonrpc: &'static str,
            id: u32,
            method: &'static str,
            params: serde_json::Value,
            #[serde(skip_serializing_if = "Option::is_none")]
            auth: Option<&'a str>,
        }
        #[derive(Deserialize)]
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
        let res = self.call(url, &payload)?.json::<Envelope<MempoolStats>>()?;
        Ok(res.result)
    }

    pub fn record_wallet_qos_event(
        &self,
        url: &str,
        event: WalletQosEvent<'_>,
    ) -> Result<(), WalletQosError> {
        #[derive(Serialize)]
        struct Payload<'a> {
            jsonrpc: &'static str,
            id: u32,
            method: &'static str,
            params: WalletQosParams<'a>,
        }

        #[derive(Serialize)]
        struct WalletQosParams<'a> {
            event: &'a str,
            lane: &'a str,
            fee: u64,
            floor: u64,
        }

        #[derive(Deserialize)]
        struct WalletQosAck {
            #[serde(default)]
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

    pub fn inflation_params(&self, url: &str) -> Result<InflationParams, reqwest::Error> {
        #[derive(Serialize)]
        struct Payload<'a> {
            jsonrpc: &'static str,
            id: u32,
            method: &'static str,
            params: &'a serde_json::Value,
        }
        #[derive(Deserialize)]
        struct Envelope<T> {
            result: T,
        }
        let params = serde_json::Value::Null;
        let payload = Payload {
            jsonrpc: "2.0",
            id: 1,
            method: "inflation.params",
            params: &params,
        };
        let res = self
            .call(url, &payload)?
            .json::<Envelope<InflationParams>>()?;
        Ok(res.result)
    }

    pub fn stake_role(&self, url: &str, id: &str, role: &str) -> Result<u64, reqwest::Error> {
        #[derive(Serialize)]
        struct Payload {
            jsonrpc: &'static str,
            id: u32,
            method: &'static str,
            params: serde_json::Value,
        }
        #[derive(Deserialize)]
        struct Envelope {
            result: serde_json::Value,
        }
        let params = serde_json::json!({"id": id, "role": role});
        let payload = Payload {
            jsonrpc: "2.0",
            id: 1,
            method: "stake.role",
            params,
        };
        let res = self.call(url, &payload)?.json::<Envelope>()?;
        Ok(res.result["stake"].as_u64().unwrap_or(0))
    }
}

#[derive(Debug, Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope<T> {
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    error: Option<RpcErrorBody>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Clone, Copy, Debug)]
pub struct WalletQosEvent<'a> {
    pub event: &'a str,
    pub lane: &'a str,
    pub fee: u64,
    pub floor: u64,
}

#[derive(Debug)]
pub enum WalletQosError {
    Transport(reqwest::Error),
    Rpc { code: i64, message: String },
    MissingStatus,
    InvalidStatus(String),
}

impl fmt::Display for WalletQosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Rpc { code, message } => {
                write!(f, "rpc error {code}: {message}")
            }
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

impl From<reqwest::Error> for WalletQosError {
    fn from(err: reqwest::Error) -> Self {
        Self::Transport(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consume_http_request(stream: &mut std::net::TcpStream) {
        use std::io::Read;

        let mut buf = Vec::new();
        let mut tmp = [0u8; 512];

        loop {
            let n = stream.read(&mut tmp).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_header_end(&buf) {
                let content_len = parse_content_length(&buf[..pos]);
                let mut remaining = content_len.saturating_sub(buf.len() - pos);
                while remaining > 0 {
                    let n = stream.read(&mut tmp).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    remaining = remaining.saturating_sub(n);
                }
                break;
            }
        }
    }

    fn find_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|idx| idx + 4)
    }

    fn parse_content_length(headers: &[u8]) -> usize {
        let text = String::from_utf8_lossy(headers);
        for line in text.lines() {
            let mut parts = line.splitn(2, ':');
            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                if name.trim().eq_ignore_ascii_case("content-length") {
                    if let Ok(len) = value.trim().parse::<usize>() {
                        return len;
                    }
                }
            }
        }
        0
    }

    #[test]
    fn timeout_jitter_within_bounds() {
        let client = RpcClient {
            http: Client::new(),
            base_timeout: Duration::from_millis(100),
            jitter: Duration::from_millis(50),
            max_retries: 1,
            fault_rate: 0.0,
        };
        for _ in 0..20 {
            let t = client.timeout_with_jitter();
            assert!(t >= Duration::from_millis(100));
            assert!(t <= Duration::from_millis(150));
        }
    }

    #[test]
    fn record_wallet_qos_event_propagates_rpc_error() {
        use std::io::Write;
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            consume_http_request(&mut stream);
            let body = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"rejected"},"id":1}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let client = RpcClient {
            http: Client::new(),
            base_timeout: Duration::from_millis(100),
            jitter: Duration::from_millis(0),
            max_retries: 0,
            fault_rate: 0.0,
        };
        let url = format!("http://{}", addr);
        let event = WalletQosEvent {
            event: "warning",
            lane: "consumer",
            fee: 1,
            floor: 0,
        };

        let err = client
            .record_wallet_qos_event(&url, event)
            .expect_err("rpc error should propagate to caller");
        match err {
            WalletQosError::Rpc { code, message } => {
                assert_eq!(code, -32000);
                assert_eq!(message, "rejected");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }

        handle.join().unwrap();
    }
}
