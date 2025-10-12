use http_env::blocking_client;
use httpd::{client::ClientError, Method, StatusCode};
use monitoring_build::{
    load_metrics_spec, parse_prometheus_snapshot, render_html_snapshot, DashboardError,
    MetricSnapshot,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_PREFIXES: &[&str] = &["TB_MONITORING_TLS", "TB_NODE_TLS"];
const COMPONENT: &str = "monitoring.snapshot";
const DEFAULT_REFRESH_SECONDS: u64 = 5;

fn main() {
    if let Err(err) = run() {
        eprintln!("{COMPONENT}: {err}");
        std::process::exit(1);
    }
}

type Result<T> = std::result::Result<T, SnapshotError>;

#[derive(Debug)]
enum SnapshotError {
    Usage,
    MissingEndpoint,
    Metrics(DashboardError),
    Http(ClientError),
    Status(StatusCode, String),
    Io(std::io::Error),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Usage => write!(f, "usage: telemetry-snapshot [endpoint] [output]"),
            SnapshotError::MissingEndpoint => write!(
                f,
                "telemetry endpoint missing: pass an argument or set TB_MONITORING_ENDPOINT",
            ),
            SnapshotError::Metrics(err) => write!(f, "failed to load metrics specification: {err}"),
            SnapshotError::Http(err) => write!(f, "telemetry request failed: {err}"),
            SnapshotError::Status(code, body) => {
                if body.is_empty() {
                    write!(f, "telemetry endpoint returned {}", code.as_u16())
                } else {
                    write!(f, "telemetry endpoint returned {}: {}", code.as_u16(), body)
                }
            }
            SnapshotError::Io(err) => write!(f, "i/o error: {err}"),
        }
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SnapshotError::Metrics(err) => Some(err),
            SnapshotError::Http(err) => Some(err),
            SnapshotError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<DashboardError> for SnapshotError {
    fn from(value: DashboardError) -> Self {
        SnapshotError::Metrics(value)
    }
}

impl From<ClientError> for SnapshotError {
    fn from(value: ClientError) -> Self {
        SnapshotError::Http(value)
    }
}

impl From<std::io::Error> for SnapshotError {
    fn from(value: std::io::Error) -> Self {
        SnapshotError::Io(value)
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let endpoint_arg = args.next();
    let output_arg = args.next();
    if args.next().is_some() {
        return Err(SnapshotError::Usage);
    }

    let endpoint = endpoint_arg
        .or_else(|| env::var("TB_MONITORING_ENDPOINT").ok())
        .ok_or(SnapshotError::MissingEndpoint)?;
    let metrics_path = env::var("TB_MONITORING_METRICS")
        .unwrap_or_else(|_| default_metrics_path().to_string_lossy().into_owned());
    let output_path = output_arg
        .or_else(|| env::var("TB_MONITORING_OUTPUT").ok())
        .map(PathBuf::from)
        .unwrap_or_else(default_output_path);

    let metrics = load_metrics_spec(&metrics_path)?;
    let snapshot = fetch_snapshot(&endpoint)?;
    let html = render_html_snapshot(&endpoint, &metrics, &snapshot);
    write_output(&output_path, &html)?;
    println!("{COMPONENT}: wrote {}", output_path.display());
    Ok(())
}

fn fetch_snapshot(endpoint: &str) -> Result<MetricSnapshot> {
    let client = blocking_client(DEFAULT_PREFIXES, COMPONENT);
    let response = client
        .request(Method::Get, endpoint)?
        .header("accept", "text/plain")
        .timeout(Duration::from_secs(DEFAULT_REFRESH_SECONDS))
        .send()?;
    if !response.status().is_success() {
        let body = response.text().unwrap_or_default();
        return Err(SnapshotError::Status(response.status(), body));
    }
    let payload = response.text().map_err(SnapshotError::Http)?;
    Ok(parse_prometheus_snapshot(&payload))
}

fn write_output(path: &Path, html: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, html)?;
    Ok(())
}

fn default_output_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("output/index.html")
}

fn default_metrics_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("metrics.json")
}
