use foundation_rpc::{Request as RpcEnvelopeRequest, RpcError};
use foundation_serialization::json::{self, Map, Value};
use foundation_serialization::Serialize;
use httpd::{ClientError as HttpClientError, ClientResponse, HttpClient, Method};
use rand::Rng;
use std::fmt;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::http_client;
use crate::transaction::FeeLane;

const MAX_BACKOFF_EXPONENT: u32 = 30;

/// Simple JSON-RPC client with jittered timeouts and retry backoff.
#[derive(Clone)]
pub struct RpcClient {
    http: HttpClient,
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
    /// - `TB_RPC_FAULT_RATE` probability for fault injection, clamped to the
    ///   inclusive `[0.0, 1.0]` range (default 0.0)
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
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| !v.is_nan())
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(0.0);
        Self {
            http: http_client::http_client(),
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
        // exponential backoff with jitter. The multiplier saturates once the
        // exponent exceeds `MAX_BACKOFF_EXPONENT` so operators can raise
        // `TB_RPC_MAX_RETRIES` without triggering shift overflows while we still
        // add jitter on top of the capped exponential delay.
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
        let mut attempt = 0;
        loop {
            let timeout = self.timeout_with_jitter();
            let start = Instant::now();
            self.maybe_inject_fault()?;
            let res = runtime::block_on(async {
                let builder = self.http.request(Method::Post, url)?;
                let builder = builder.json(payload)?;
                builder.timeout(timeout).send().await
            })
            .map_err(RpcClientError::from);
            match res {
                Ok(r) => return Ok(r),
                Err(RpcClientError::Transport(HttpClientError::Timeout))
                    if attempt < self.max_retries =>
                {
                    attempt += 1;
                    let delay = self.backoff_with_jitter(attempt);
                    // ensure we don't spin too fast if server responds immediately
                    if delay > start.elapsed() {
                        sleep(delay - start.elapsed());
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn send_request(
        &self,
        url: &str,
        request: &RpcEnvelopeRequest,
    ) -> Result<Value, RpcClientError> {
        let payload = request_to_value(request);
        let response = self.call(url, &payload)?;
        let body = response.into_body();
        json::value_from_slice(&body).map_err(RpcClientError::Decode)
    }
}

fn request_to_value(request: &RpcEnvelopeRequest) -> Value {
    let mut map = Map::new();
    if let Some(version) = &request.version {
        map.insert("jsonrpc".to_owned(), Value::String(version.clone()));
    }
    map.insert("method".to_owned(), Value::String(request.method.clone()));
    map.insert("params".to_owned(), Value::from(request.params.clone()));
    if let Some(id) = &request.id {
        map.insert("id".to_owned(), id.clone());
    }
    if let Some(badge) = &request.badge {
        map.insert("badge".to_owned(), Value::String(badge.clone()));
    }
    Value::Object(map)
}

#[derive(Debug)]
pub enum RpcClientError {
    Transport(HttpClientError),
    InjectedFault,
    Decode(foundation_serialization::Error),
    Rpc(foundation_rpc::RpcError),
    InvalidResponse(String),
}

impl fmt::Display for RpcClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcClientError::Transport(err) => write!(f, "transport error: {err}"),
            RpcClientError::InjectedFault => f.write_str("fault injection triggered"),
            RpcClientError::Decode(err) => write!(f, "decode error: {err}"),
            RpcClientError::Rpc(err) => {
                write!(f, "rpc error {}: {}", err.code, err.message())
            }
            RpcClientError::InvalidResponse(message) => {
                write!(f, "invalid rpc response: {message}")
            }
        }
    }
}

impl std::error::Error for RpcClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcClientError::Transport(err) => Some(err),
            RpcClientError::InjectedFault => None,
            RpcClientError::Decode(err) => Some(err),
            RpcClientError::Rpc(err) => Some(err),
            RpcClientError::InvalidResponse(_) => None,
        }
    }
}

impl From<HttpClientError> for RpcClientError {
    fn from(err: HttpClientError) -> Self {
        RpcClientError::Transport(err)
    }
}

#[derive(Debug)]
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
    pub fn mempool_stats(&self, url: &str, lane: FeeLane) -> Result<MempoolStats, RpcClientError> {
        let mut params = Map::new();
        params.insert("lane".to_owned(), Value::String(lane.as_str().to_owned()));
        let params = Value::Object(params);
        let request =
            RpcEnvelopeRequest::new("mempool.stats", params).with_id(json::Value::from(1u64));

        let response = self.send_request(url, &request)?;
        match parse_rpc_response(response)? {
            RpcPayload::Success(result) => parse_mempool_stats(&result),
            RpcPayload::Error(err) => Err(RpcClientError::Rpc(err)),
        }
    }

    pub fn record_wallet_qos_event(
        &self,
        url: &str,
        event: WalletQosEvent<'_>,
    ) -> Result<(), WalletQosError> {
        let mut params = Map::new();
        params.insert("event".to_owned(), Value::String(event.event.to_owned()));
        params.insert("lane".to_owned(), Value::String(event.lane.to_owned()));
        params.insert("fee".to_owned(), Value::from(event.fee));
        params.insert("floor".to_owned(), Value::from(event.floor));
        let params = Value::Object(params);
        let request =
            RpcEnvelopeRequest::new("mempool.qos_event", params).with_id(json::Value::from(1u64));
        let response = self
            .send_request(url, &request)
            .map_err(WalletQosError::from)?;
        match parse_rpc_response(response).map_err(WalletQosError::from)? {
            RpcPayload::Success(result) => {
                let status = parse_status(&result).map_err(WalletQosError::from)?;
                let status = status.ok_or(WalletQosError::MissingStatus)?;
                if status != "ok" {
                    return Err(WalletQosError::InvalidStatus(status));
                }
                Ok(())
            }
            RpcPayload::Error(error) => Err(WalletQosError::Rpc {
                code: i64::from(error.code),
                message: error.message().to_string(),
            }),
        }
    }

    pub fn inflation_params(&self, url: &str) -> Result<InflationParams, RpcClientError> {
        let request = RpcEnvelopeRequest::new("inflation.params", json::Value::Null)
            .with_id(json::Value::from(1u64));
        let response = self.send_request(url, &request)?;
        match parse_rpc_response(response)? {
            RpcPayload::Success(result) => parse_inflation_params(&result),
            RpcPayload::Error(err) => Err(RpcClientError::Rpc(err)),
        }
    }

    pub fn stake_role(&self, url: &str, id: &str, role: &str) -> Result<u64, RpcClientError> {
        let mut params = Map::new();
        params.insert("id".to_owned(), Value::String(id.to_owned()));
        params.insert("role".to_owned(), Value::String(role.to_owned()));
        let params = Value::Object(params);
        let request =
            RpcEnvelopeRequest::new("stake.role", params).with_id(json::Value::from(1u64));
        let response = self.send_request(url, &request)?;
        match parse_rpc_response(response)? {
            RpcPayload::Success(result) => parse_stake_role(&result),
            RpcPayload::Error(err) => Err(RpcClientError::Rpc(err)),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MempoolStats {
    pub size: u64,
    pub age_p50: u64,
    pub age_p95: u64,
    pub fee_p50: u64,
    pub fee_p90: u64,
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
    Transport(RpcClientError),
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

impl From<RpcClientError> for WalletQosError {
    fn from(err: RpcClientError) -> Self {
        Self::Transport(err)
    }
}

enum RpcPayload {
    Success(Value),
    Error(RpcError),
}

fn parse_rpc_response(value: Value) -> Result<RpcPayload, RpcClientError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_response("rpc response must be an object"))?;
    if let Some(error_value) = map.get("error") {
        let error = parse_rpc_error(error_value)?;
        return Ok(RpcPayload::Error(error));
    }
    let result = map
        .get("result")
        .cloned()
        .ok_or_else(|| invalid_response("rpc response missing result field"))?;
    Ok(RpcPayload::Success(result))
}

fn parse_rpc_error(value: &Value) -> Result<RpcError, RpcClientError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_response("rpc error payload must be an object"))?;
    let code = map
        .get("code")
        .ok_or_else(|| invalid_response("rpc error missing code field"))?
        .as_i64()
        .ok_or_else(|| invalid_response("rpc error code must be an integer"))?;
    let message = map
        .get("message")
        .ok_or_else(|| invalid_response("rpc error missing message field"))?
        .as_str()
        .ok_or_else(|| invalid_response("rpc error message must be a string"))?;
    Ok(RpcError::new(code as i32, message.to_owned()))
}

fn parse_mempool_stats(value: &Value) -> Result<MempoolStats, RpcClientError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_response("mempool.stats result must be an object"))?;
    Ok(MempoolStats {
        size: field_u64_default(map, "size")?,
        age_p50: field_u64_default(map, "age_p50")?,
        age_p95: field_u64_default(map, "age_p95")?,
        fee_p50: field_u64_default(map, "fee_p50")?,
        fee_p90: field_u64_default(map, "fee_p90")?,
        fee_floor: field_u64_default(map, "fee_floor")?,
    })
}

fn parse_status(value: &Value) -> Result<Option<String>, RpcClientError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_response("qos response must be an object"))?;
    match map.get("status") {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(other) => Err(invalid_response(format!(
            "field 'status' expected string, found {}",
            describe_type(other)
        ))),
    }
}

fn parse_inflation_params(value: &Value) -> Result<InflationParams, RpcClientError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_response("inflation.params result must be an object"))?;
    Ok(InflationParams {
        beta_storage_sub_ct: field_i64(map, "beta_storage_sub_ct")?,
        gamma_read_sub_ct: field_i64(map, "gamma_read_sub_ct")?,
        kappa_cpu_sub_ct: field_i64(map, "kappa_cpu_sub_ct")?,
        lambda_bytes_out_sub_ct: field_i64(map, "lambda_bytes_out_sub_ct")?,
        rent_rate_ct_per_byte: field_i64(map, "rent_rate_ct_per_byte")?,
        industrial_multiplier: field_i64(map, "industrial_multiplier")?,
        industrial_backlog: field_u64(map, "industrial_backlog")?,
        industrial_utilization: field_u64(map, "industrial_utilization")?,
    })
}

fn parse_stake_role(value: &Value) -> Result<u64, RpcClientError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid_response("stake.role result must be an object"))?;
    Ok(optional_field_u64(map, "stake")?.unwrap_or(0))
}

fn field_u64_default(map: &Map, key: &str) -> Result<u64, RpcClientError> {
    match map.get(key) {
        Some(Value::Number(num)) => num
            .as_u64()
            .ok_or_else(|| invalid_response(format!("field '{key}' must be an unsigned integer"))),
        Some(Value::Null) | None => Ok(0),
        Some(other) => Err(invalid_response(format!(
            "field '{key}' expected number, found {}",
            describe_type(other)
        ))),
    }
}

fn field_u64(map: &Map, key: &str) -> Result<u64, RpcClientError> {
    match map.get(key) {
        Some(Value::Number(num)) => num
            .as_u64()
            .ok_or_else(|| invalid_response(format!("field '{key}' must be an unsigned integer"))),
        Some(Value::Null) => Err(invalid_response(format!(
            "field '{key}' must be an unsigned integer"
        ))),
        None => Err(invalid_response(format!("missing field '{key}'"))),
        Some(other) => Err(invalid_response(format!(
            "field '{key}' expected number, found {}",
            describe_type(other)
        ))),
    }
}

fn optional_field_u64(map: &Map, key: &str) -> Result<Option<u64>, RpcClientError> {
    match map.get(key) {
        Some(Value::Number(num)) => num
            .as_u64()
            .ok_or_else(|| invalid_response(format!("field '{key}' must be an unsigned integer")))
            .map(Some),
        Some(Value::Null) | None => Ok(None),
        Some(other) => Err(invalid_response(format!(
            "field '{key}' expected number, found {}",
            describe_type(other)
        ))),
    }
}

fn field_i64(map: &Map, key: &str) -> Result<i64, RpcClientError> {
    match map.get(key) {
        Some(Value::Number(num)) => num
            .as_i64()
            .or_else(|| num.as_u64().map(|v| v as i64))
            .ok_or_else(|| invalid_response(format!("field '{key}' must be a signed integer"))),
        Some(Value::Null) => Err(invalid_response(format!(
            "field '{key}' must be a signed integer"
        ))),
        None => Err(invalid_response(format!("missing field '{key}'"))),
        Some(other) => Err(invalid_response(format!(
            "field '{key}' expected number, found {}",
            describe_type(other)
        ))),
    }
}

fn invalid_response(message: impl Into<String>) -> RpcClientError {
    RpcClientError::InvalidResponse(message.into())
}

fn describe_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.as_ref() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

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
    fn rpc_client_fault_rate_clamping() {
        for (value, expected) in [("-3.0", 0.0), ("1.5", 1.0)] {
            {
                let _guard = EnvGuard::set("TB_RPC_FAULT_RATE", value);
                let client = RpcClient::from_env();
                assert_eq!(client.fault_rate, expected, "value {value}");
            }
        }

        {
            let _guard = EnvGuard::set("TB_RPC_FAULT_RATE", "NaN");
            let client = RpcClient::from_env();
            assert_eq!(client.fault_rate, 0.0);
        }
    }

    #[test]
    fn maybe_inject_fault_respects_clamped_rate() {
        let client_full = {
            let _guard = EnvGuard::set("TB_RPC_FAULT_RATE", "1.5");
            let client = RpcClient::from_env();
            assert_eq!(client.fault_rate, 1.0);
            client
        };

        for _ in 0..8 {
            let err = client_full
                .maybe_inject_fault()
                .expect_err("sanitized rate of 1.0 should always inject faults");
            assert!(matches!(err, RpcClientError::InjectedFault));
        }

        let client_zero = {
            let _guard = EnvGuard::set("TB_RPC_FAULT_RATE", "-3.0");
            let client = RpcClient::from_env();
            assert_eq!(client.fault_rate, 0.0);
            client
        };

        for _ in 0..8 {
            client_zero
                .maybe_inject_fault()
                .expect("sanitized rate of 0.0 should never inject faults");
        }
    }

    #[test]
    fn env_guard_restores_previous_value() {
        const KEY: &str = "TB_RPC_FAULT_RATE";
        let original = std::env::var_os(KEY);
        std::env::set_var(KEY, "0.42");
        {
            let guard = EnvGuard::set(KEY, "1.0");
            assert_eq!(std::env::var(KEY).unwrap(), "1.0");
            drop(guard);
        }
        assert_eq!(std::env::var(KEY).unwrap(), "0.42");

        match original {
            Some(value) => std::env::set_var(KEY, value),
            None => std::env::remove_var(KEY),
        }
    }

    #[test]
    fn backoff_with_jitter_matches_legacy_for_small_attempts() {
        let client = RpcClient {
            http: http_client::http_client(),
            base_timeout: Duration::from_millis(25),
            jitter: Duration::from_millis(0),
            max_retries: 3,
            fault_rate: 0.0,
        };

        for attempt in 0..=3 {
            let expected = client.base_timeout * (1u32 << attempt);
            let actual = client.backoff_with_jitter(attempt);
            assert_eq!(actual, expected, "attempt {attempt}");
        }
    }

    fn assert_backoff_saturates_for_large_attempts() {
        let client = RpcClient {
            http: http_client::http_client(),
            base_timeout: Duration::from_millis(10),
            jitter: Duration::from_millis(0),
            max_retries: 100,
            fault_rate: 0.0,
        };

        let delay = client.backoff_with_jitter(100);
        let expected_multiplier = 1u64 << MAX_BACKOFF_EXPONENT;
        let expected = client
            .base_timeout
            .checked_mul(expected_multiplier as u32)
            .unwrap();
        assert_eq!(delay, expected);
        assert!(delay < Duration::MAX);
    }

    #[test]
    fn backoff_with_jitter_saturates_for_large_attempts() {
        assert_backoff_saturates_for_large_attempts();
    }

    #[test]
    fn rpc_client_backoff_handles_large_retries() {
        assert_backoff_saturates_for_large_attempts();
    }

    #[test]
    fn backoff_with_jitter_is_monotonic() {
        let client = RpcClient {
            http: http_client::http_client(),
            base_timeout: Duration::from_millis(5),
            jitter: Duration::from_millis(0),
            max_retries: 100,
            fault_rate: 0.0,
        };

        let mut previous = Duration::ZERO;
        for attempt in 0..=100 {
            let delay = client.backoff_with_jitter(attempt);
            assert!(
                delay >= previous,
                "backoff decreased at attempt {attempt}: {delay:?} < {previous:?}"
            );
            previous = delay;
        }
    }

    #[test]
    fn timeout_jitter_within_bounds() {
        let client = RpcClient {
            http: http_client::http_client(),
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

        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(_) => return, // likely running in a sandbox without socket permissions
        };
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
            http: http_client::http_client(),
            base_timeout: Duration::from_millis(1_000),
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

    #[test]
    fn call_returns_fault_injection_error() {
        let client = RpcClient {
            http: http_client::http_client(),
            base_timeout: Duration::from_millis(10),
            jitter: Duration::from_millis(0),
            max_retries: 0,
            fault_rate: 1.0,
        };
        let mut payload = Map::new();
        payload.insert("jsonrpc".to_owned(), Value::String("2.0".to_owned()));
        payload.insert("method".to_owned(), Value::String("noop".to_owned()));
        let payload = Value::Object(payload);
        let err = client
            .call("http://127.0.0.1:0", &payload)
            .expect_err("fault injection should abort the request");
        match err {
            RpcClientError::InjectedFault => (),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }
}
