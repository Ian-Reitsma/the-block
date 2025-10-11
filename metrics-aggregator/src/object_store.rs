use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::time::SystemTime;

use crypto_suite::mac::{hmac_sha256, sha256_digest};
use foundation_time::{FormatError as TimeFormatError, FormatKind, UtcDateTime};
use http_env::http_client as env_http_client;
use httpd::{ClientError, HttpClient, Method};

const METRICS_OBJECT_KEY: &str = "metrics/latest.zip";

fn http_client() -> HttpClient {
    env_http_client(&["TB_AGGREGATOR_TLS", "TB_HTTP_TLS"], "metrics-aggregator")
}

pub fn upload_metrics_snapshot(bucket: &str, data: Vec<u8>) -> Result<(), UploadError> {
    let config = S3Config::from_env()?;
    let bucket = bucket.to_owned();
    runtime::handle().block_on(async move {
        let now = UtcDateTime::from(SystemTime::now());
        put_object(config, bucket, METRICS_OBJECT_KEY, data, now).await
    })
}

async fn put_object(
    config: S3Config,
    bucket: String,
    key: &str,
    body: Vec<u8>,
    now: UtcDateTime,
) -> Result<(), UploadError> {
    let artifacts = signing_artifacts(&config, &bucket, key, &body, now)?;
    let client = http_client();
    let mut request = client
        .request(Method::Put, &artifacts.url)
        .map_err(UploadError::Client)?
        .header("authorization", artifacts.authorization.clone());
    for (name, value) in &artifacts.headers {
        request = request.header(name.clone(), value.clone());
    }
    let response = request
        .body(body)
        .send()
        .await
        .map_err(UploadError::Client)?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status().as_u16();
    let body = response.into_body();
    let body_text = String::from_utf8_lossy(&body).to_string();
    Err(UploadError::UnexpectedResponse {
        status,
        body: body_text,
    })
}

#[derive(Clone, Debug)]
struct S3Config {
    endpoint: Endpoint,
    region: String,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
}

impl S3Config {
    fn from_env() -> Result<Self, UploadError> {
        let endpoint =
            env::var("S3_ENDPOINT").map_err(|_| UploadError::MissingEnv("S3_ENDPOINT"))?;
        let region = env_or(&["S3_REGION", "AWS_REGION", "AWS_DEFAULT_REGION"]).ok_or(
            UploadError::MissingEnv("S3_REGION (or AWS_REGION/AWS_DEFAULT_REGION)"),
        )?;
        let access_key = env_or(&["S3_ACCESS_KEY", "AWS_ACCESS_KEY_ID"]).ok_or(
            UploadError::MissingEnv("S3_ACCESS_KEY (or AWS_ACCESS_KEY_ID)"),
        )?;
        let secret_key = env_or(&["S3_SECRET_KEY", "AWS_SECRET_ACCESS_KEY"]).ok_or(
            UploadError::MissingEnv("S3_SECRET_KEY (or AWS_SECRET_ACCESS_KEY)"),
        )?;
        let session_token = env_or(&["S3_SESSION_TOKEN", "AWS_SESSION_TOKEN"]);
        Ok(Self {
            endpoint: Endpoint::parse(&endpoint)?,
            region,
            access_key,
            secret_key,
            session_token,
        })
    }
}

#[derive(Clone, Debug)]
struct Endpoint {
    scheme: Scheme,
    host: String,
    explicit_port: Option<u16>,
    base_path: Vec<String>,
}

impl Endpoint {
    fn parse(raw: &str) -> Result<Self, UploadError> {
        let (scheme_str, rest) = raw
            .split_once("://")
            .ok_or_else(|| UploadError::InvalidEndpoint(raw.to_string()))?;
        let scheme = Scheme::parse(scheme_str)?;
        let mut split = rest.splitn(2, '/');
        let authority = split
            .next()
            .ok_or_else(|| UploadError::InvalidEndpoint(raw.to_string()))?;
        if authority.is_empty() {
            return Err(UploadError::InvalidEndpoint(raw.to_string()));
        }
        let (host, port) = parse_authority(authority)?;
        let base_path = split
            .next()
            .map(|path| {
                path.split('/')
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| segment.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            scheme,
            host,
            explicit_port: port,
            base_path,
        })
    }

    fn authority(&self) -> String {
        match self.explicit_port {
            Some(port) => format!("{}:{}", self.host, port),
            None => self.host.clone(),
        }
    }

    fn canonical_uri(&self, bucket: &str, key: &str) -> String {
        let mut segments: Vec<String> = self
            .base_path
            .iter()
            .map(|segment| percent_encode(segment))
            .collect();
        segments.push(percent_encode(bucket));
        let trimmed = key
            .trim_start_matches('/')
            .split('/')
            .map(percent_encode)
            .collect::<Vec<_>>();
        segments.extend(trimmed.into_iter());
        if key.ends_with('/') && !segments.last().map_or(false, |s| s.is_empty()) {
            segments.push(String::new());
        }
        let mut path = String::from("/");
        path.push_str(&segments.join("/"));
        if key.is_empty() && !path.ends_with('/') {
            path.push('/');
        }
        path
    }

    fn request_url(&self, canonical_uri: &str) -> String {
        let mut url = match self.explicit_port {
            Some(port) => format!("{}://{}:{}", self.scheme.as_str(), self.host, port),
            None => format!("{}://{}", self.scheme.as_str(), self.host),
        };
        url.push_str(canonical_uri);
        url
    }
}

#[derive(Clone, Copy, Debug)]
enum Scheme {
    Http,
}

impl Scheme {
    fn parse(input: &str) -> Result<Self, UploadError> {
        match input {
            "http" | "HTTP" => Ok(Scheme::Http),
            other => Err(UploadError::UnsupportedScheme(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Scheme::Http => "http",
        }
    }
}

fn parse_authority(value: &str) -> Result<(String, Option<u16>), UploadError> {
    if value.starts_with('[') {
        let end = value
            .find(']')
            .ok_or_else(|| UploadError::InvalidEndpoint(value.to_string()))?;
        let host = value[..=end].to_string();
        let remainder = &value[end + 1..];
        if remainder.is_empty() {
            return Ok((host, None));
        }
        let port = remainder
            .strip_prefix(':')
            .ok_or_else(|| UploadError::InvalidEndpoint(value.to_string()))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| UploadError::InvalidPort(port.to_string()))?;
        return Ok((host, Some(port)));
    }
    if let Some(idx) = value.rfind(':') {
        if value[..idx].contains(':') {
            return Err(UploadError::InvalidEndpoint(value.to_string()));
        }
        let host = &value[..idx];
        let port = &value[idx + 1..];
        if port.is_empty() {
            return Err(UploadError::InvalidEndpoint(value.to_string()));
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| UploadError::InvalidPort(port.to_string()))?;
        return Ok((host.to_string(), Some(port)));
    }
    Ok((value.to_string(), None))
}

struct SigningArtifacts {
    url: String,
    headers: Vec<(String, String)>,
    authorization: String,
    #[cfg(test)]
    canonical_request: String,
}

fn signing_artifacts(
    config: &S3Config,
    bucket: &str,
    key: &str,
    body: &[u8],
    now: UtcDateTime,
) -> Result<SigningArtifacts, UploadError> {
    let canonical_uri = config.endpoint.canonical_uri(bucket, key);
    let url = config.endpoint.request_url(&canonical_uri);
    let payload_hash = hex(&sha256_digest(body));
    let amz_date = now.format(FormatKind::CompactDateTime)?;
    let date_stamp = now.format(FormatKind::CompactDate)?;

    let mut headers = vec![
        ("content-length".to_string(), body.len().to_string()),
        ("host".to_string(), config.endpoint.authority()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    if let Some(token) = &config.session_token {
        headers.push(("x-amz-security-token".to_string(), token.clone()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let mut canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{}:{}\n", name, value.trim()))
        .collect::<String>();
    canonical_headers.push('\n');
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "PUT\n{}\n\n{}{}\n{}",
        canonical_uri, canonical_headers, signed_headers, payload_hash
    );

    let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, config.region);
    let canonical_request_hash = hex(&sha256_digest(canonical_request.as_bytes()));
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, credential_scope, canonical_request_hash
    );

    let signing_key = derive_signing_key(&config.secret_key, &date_stamp, &config.region);
    let signature = hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        config.access_key, credential_scope, signed_headers, signature
    );

    #[cfg(test)]
    let canonical_clone = canonical_request.clone();

    Ok(SigningArtifacts {
        url,
        headers,
        authorization,
        #[cfg(test)]
        canonical_request: canonical_clone,
    })
}

fn derive_signing_key(secret_key: &str, date: &str, region: &str) -> [u8; 32] {
    let mut key = Vec::with_capacity(4 + secret_key.len());
    key.extend_from_slice(b"AWS4");
    key.extend_from_slice(secret_key.as_bytes());
    let k_date = hmac_sha256(&key, date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn percent_encode(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for &byte in segment.as_bytes() {
        if matches!(byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'~'
        ) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{:02X}", byte));
        }
    }
    encoded
}

fn env_or(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| env::var(key).ok())
}

#[derive(Debug)]
pub enum UploadError {
    MissingEnv(&'static str),
    InvalidEndpoint(String),
    UnsupportedScheme(String),
    InvalidPort(String),
    Time(TimeFormatError),
    Client(ClientError),
    UnexpectedResponse { status: u16, body: String },
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UploadError::MissingEnv(name) => {
                write!(f, "missing environment variable {name}")
            }
            UploadError::InvalidEndpoint(endpoint) => write!(f, "invalid endpoint: {endpoint}"),
            UploadError::UnsupportedScheme(scheme) => {
                write!(f, "unsupported endpoint scheme: {scheme}")
            }
            UploadError::InvalidPort(port) => write!(f, "invalid endpoint port: {port}"),
            UploadError::Time(err) => write!(f, "time formatting error: {err}"),
            UploadError::Client(err) => write!(f, "http client error: {err}"),
            UploadError::UnexpectedResponse { status, body } => {
                write!(f, "unexpected response status {status}: {body}")
            }
        }
    }
}

impl StdError for UploadError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            UploadError::Time(err) => Some(err),
            UploadError::Client(err) => Some(err),
            UploadError::UnexpectedResponse { .. } => None,
            UploadError::MissingEnv(_) => None,
            UploadError::InvalidEndpoint(_) => None,
            UploadError::UnsupportedScheme(_) => None,
            UploadError::InvalidPort(_) => None,
        }
    }
}

impl From<TimeFormatError> for UploadError {
    fn from(value: TimeFormatError) -> Self {
        UploadError::Time(value)
    }
}

impl From<ClientError> for UploadError {
    fn from(value: ClientError) -> Self {
        UploadError::Client(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_request_matches_known_example() {
        let config = S3Config {
            endpoint: Endpoint::parse("http://s3.amazonaws.com").unwrap(),
            region: "us-east-1".to_string(),
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            session_token: None,
        };
        let body = b"Welcome to Amazon S3.";
        let now = UtcDateTime::from_unix_timestamp(1_369_353_600).unwrap();
        let artifacts = signing_artifacts(&config, "examplebucket", "test.txt", body, now).unwrap();
        let expected_canonical = concat!(
            "PUT\n",
            "/examplebucket/test.txt\n\n",
            "content-length:21\n",
            "host:s3.amazonaws.com\n",
            "x-amz-content-sha256:44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072\n",
            "x-amz-date:20130524T000000Z\n\n",
            "content-length;host;x-amz-content-sha256;x-amz-date\n",
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        assert_eq!(artifacts.canonical_request, expected_canonical);
        assert!(artifacts.authorization.contains(
            "Signature=de6cbb44503ba40c02823691744666556126f10a46ec5b7f3745eb44271bf1ee"
        ));
    }

    #[test]
    fn percent_encoding_handles_reserved_characters() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a/b"), "a%2Fb");
        assert_eq!(percent_encode("~user"), "~user");
    }
}
