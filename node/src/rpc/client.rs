use rand::Rng;
use reqwest::blocking::{Client, Response};
use reqwest::error::ErrorKind;
use serde::{Deserialize, Serialize};
use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

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
            return Err(reqwest::Error::new(
                ErrorKind::Request,
                io::Error::new(io::ErrorKind::Other, "injected fault"),
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
