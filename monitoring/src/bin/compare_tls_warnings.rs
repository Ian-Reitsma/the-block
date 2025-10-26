extern crate foundation_serialization as serde;

use foundation_serialization::{json, Deserialize};
use http_env::blocking_client;
use httpd::{client::ClientError, Method, StatusCode};
use monitoring_build::{parse_prometheus_snapshot, MetricSnapshot, MetricValue};
use std::{
    collections::{BTreeMap, HashMap},
    env, fs, io, process,
    time::Duration,
};

const COMPONENT: &str = "monitoring.compare_tls_warnings";
const DEFAULT_ENDPOINT_ENV: &str = "TB_MONITORING_ENDPOINT";
const DEFAULT_SNAPSHOT_ENV: &str = "TB_MONITORING_TLS_WARNINGS";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TLS_PREFIXES: &[&str] = &["TB_AGGREGATOR_TLS", "TB_MONITORING_TLS", "TB_HTTP_TLS"];
const EPSILON: f64 = 1e-6;

#[derive(Debug, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct Snapshot {
    prefix: String,
    code: String,
    total: u64,
    #[serde(default)]
    detail_fingerprint_counts: BTreeMap<String, u64>,
    #[serde(default)]
    variables_fingerprint_counts: BTreeMap<String, u64>,
}

#[derive(Debug)]
enum CompareError {
    Usage,
    MissingEndpoint,
    MissingSnapshot,
    Io(io::Error),
    Json(foundation_serialization::Error),
    Http(ClientError),
    Status(StatusCode, String),
    Mismatch(Vec<String>),
}

impl std::fmt::Display for CompareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompareError::Usage => write!(
                f,
                "usage: compare-tls-warnings <aggregator-endpoint> <cli-json-snapshot>"
            ),
            CompareError::MissingEndpoint => write!(
                f,
                "aggregator endpoint missing: pass an argument or set {}",
                DEFAULT_ENDPOINT_ENV
            ),
            CompareError::MissingSnapshot => write!(
                f,
                "CLI snapshot missing: pass an argument or set {}",
                DEFAULT_SNAPSHOT_ENV
            ),
            CompareError::Io(err) => write!(f, "i/o error: {err}"),
            CompareError::Json(err) => write!(f, "failed to parse snapshot: {err}"),
            CompareError::Http(err) => write!(f, "request failed: {err}"),
            CompareError::Status(code, body) => {
                if body.is_empty() {
                    write!(f, "endpoint returned status {}", code.as_u16())
                } else {
                    write!(f, "endpoint returned status {}: {}", code.as_u16(), body)
                }
            }
            CompareError::Mismatch(details) => {
                write!(
                    f,
                    "{} mismatch{} detected",
                    details.len(),
                    if details.len() == 1 { "" } else { "es" }
                )
            }
        }
    }
}

impl std::error::Error for CompareError {}

impl From<io::Error> for CompareError {
    fn from(value: io::Error) -> Self {
        CompareError::Io(value)
    }
}

impl From<ClientError> for CompareError {
    fn from(value: ClientError) -> Self {
        CompareError::Http(value)
    }
}

impl From<foundation_serialization::Error> for CompareError {
    fn from(value: foundation_serialization::Error) -> Self {
        CompareError::Json(value)
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(CompareError::Mismatch(details)) => {
            for detail in details {
                eprintln!("{COMPONENT}: {detail}");
            }
            process::exit(2);
        }
        Err(err) => {
            eprintln!("{COMPONENT}: {err}");
            process::exit(1);
        }
    }
}

fn run() -> Result<(), CompareError> {
    let mut args = env::args().skip(1);
    let endpoint = args
        .next()
        .or_else(|| env::var(DEFAULT_ENDPOINT_ENV).ok())
        .ok_or(CompareError::MissingEndpoint)?;
    let snapshot_path = args
        .next()
        .or_else(|| env::var(DEFAULT_SNAPSHOT_ENV).ok())
        .ok_or(CompareError::MissingSnapshot)?;
    if args.next().is_some() {
        return Err(CompareError::Usage);
    }

    let cli_snapshots = load_cli_snapshots(&snapshot_path)?;
    let trimmed = endpoint.trim_end_matches('/');
    let aggregator_snapshots = fetch_aggregator_snapshots(trimmed)?;
    let metrics = fetch_metrics(trimmed)?;

    let mismatches = compare_snapshots(&cli_snapshots, &aggregator_snapshots, &metrics);
    if mismatches.is_empty() {
        println!(
            "{COMPONENT}: CLI snapshot matches aggregator metrics for {} prefixes",
            cli_snapshots.len()
        );
        Ok(())
    } else {
        Err(CompareError::Mismatch(mismatches))
    }
}

fn load_cli_snapshots(path: &str) -> Result<Vec<Snapshot>, CompareError> {
    let raw = fs::read_to_string(path)?;
    let snapshots = json::from_str(&raw)?;
    Ok(snapshots)
}

fn fetch_aggregator_snapshots(base: &str) -> Result<Vec<Snapshot>, CompareError> {
    let client = blocking_client(TLS_PREFIXES, COMPONENT);
    let url = format!("{}/tls/warnings/latest", base);
    let response = client
        .request(Method::Get, &url)?
        .header("accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()?;
    if !response.status().is_success() {
        let body = response.text().unwrap_or_default();
        return Err(CompareError::Status(response.status(), body));
    }
    let body = response.text().map_err(CompareError::Http)?;
    let snapshots = json::from_str(&body)?;
    Ok(snapshots)
}

fn fetch_metrics(base: &str) -> Result<MetricSnapshot, CompareError> {
    let client = blocking_client(TLS_PREFIXES, COMPONENT);
    let url = format!("{}/metrics", base);
    let response = client
        .request(Method::Get, &url)?
        .header("accept", "text/plain")
        .timeout(REQUEST_TIMEOUT)
        .send()?;
    if !response.status().is_success() {
        let body = response.text().unwrap_or_default();
        return Err(CompareError::Status(response.status(), body));
    }
    let payload = response.text().map_err(CompareError::Http)?;
    Ok(parse_prometheus_snapshot(&payload))
}

fn compare_snapshots(
    cli: &[Snapshot],
    aggregator: &[Snapshot],
    metrics: &MetricSnapshot,
) -> Vec<String> {
    let aggregator_index: HashMap<(&str, &str), &Snapshot> = aggregator
        .iter()
        .map(|snapshot| ((snapshot.prefix.as_str(), snapshot.code.as_str()), snapshot))
        .collect();

    let mut mismatches = Vec::new();

    for cli_snapshot in cli {
        let key = (cli_snapshot.prefix.as_str(), cli_snapshot.code.as_str());
        let Some(aggregator_snapshot) = aggregator_index.get(&key) else {
            mismatches.push(format!(
                "aggregator missing snapshot for {}:{}",
                cli_snapshot.prefix, cli_snapshot.code
            ));
            continue;
        };

        let total_metric = format!(
            "tls_env_warning_total{{prefix=\"{}\",code=\"{}\"}}",
            cli_snapshot.prefix, cli_snapshot.code
        );
        if let Some(value) = metrics.get(&total_metric) {
            if !metric_ge_u64(value, cli_snapshot.total) {
                mismatches.push(format!(
                    "cluster total {} below CLI total {} for {}:{}",
                    describe_metric(value),
                    cli_snapshot.total,
                    cli_snapshot.prefix,
                    cli_snapshot.code
                ));
            }
        } else {
            mismatches.push(format!(
                "missing metric {} in aggregator snapshot",
                total_metric
            ));
        }

        check_unique_counts(
            &cli_snapshot.prefix,
            &cli_snapshot.code,
            cli_snapshot.detail_fingerprint_counts.len(),
            "tls_env_warning_detail_unique_fingerprints",
            metrics,
            &mut mismatches,
        );
        check_unique_counts(
            &cli_snapshot.prefix,
            &cli_snapshot.code,
            cli_snapshot.variables_fingerprint_counts.len(),
            "tls_env_warning_variables_unique_fingerprints",
            metrics,
            &mut mismatches,
        );

        for (fingerprint, cli_count) in &cli_snapshot.detail_fingerprint_counts {
            let aggregator_count = aggregator_snapshot
                .detail_fingerprint_counts
                .get(fingerprint)
                .copied()
                .unwrap_or(0);
            if aggregator_count < *cli_count {
                mismatches.push(format!(
                    "aggregator detail count {} below CLI count {} for {}:{} fingerprint {}",
                    aggregator_count,
                    cli_count,
                    cli_snapshot.prefix,
                    cli_snapshot.code,
                    fingerprint
                ));
            }
            check_counter(
                &cli_snapshot.prefix,
                &cli_snapshot.code,
                fingerprint,
                *cli_count,
                "tls_env_warning_detail_fingerprint_total",
                metrics,
                &mut mismatches,
            );
        }

        for (fingerprint, cli_count) in &cli_snapshot.variables_fingerprint_counts {
            let aggregator_count = aggregator_snapshot
                .variables_fingerprint_counts
                .get(fingerprint)
                .copied()
                .unwrap_or(0);
            if aggregator_count < *cli_count {
                mismatches.push(format!(
                    "aggregator variables count {} below CLI count {} for {}:{} fingerprint {}",
                    aggregator_count,
                    cli_count,
                    cli_snapshot.prefix,
                    cli_snapshot.code,
                    fingerprint
                ));
            }
            check_counter(
                &cli_snapshot.prefix,
                &cli_snapshot.code,
                fingerprint,
                *cli_count,
                "tls_env_warning_variables_fingerprint_total",
                metrics,
                &mut mismatches,
            );
        }
    }

    mismatches
}

fn check_unique_counts(
    prefix: &str,
    code: &str,
    cli_unique: usize,
    metric: &str,
    metrics: &MetricSnapshot,
    mismatches: &mut Vec<String>,
) {
    if cli_unique == 0 {
        return;
    }
    let key = format!("{}{{prefix=\"{}\",code=\"{}\"}}", metric, prefix, code);
    match metrics.get(&key) {
        Some(value) if metric_ge_usize(value, cli_unique) => {}
        Some(value) => mismatches.push(format!(
            "{} below CLI unique count {} for {}:{} (value={})",
            metric,
            cli_unique,
            prefix,
            code,
            describe_metric(value)
        )),
        None => mismatches.push(format!("missing metric {}", key)),
    }
}

fn check_counter(
    prefix: &str,
    code: &str,
    fingerprint: &str,
    cli_count: u64,
    metric: &str,
    metrics: &MetricSnapshot,
    mismatches: &mut Vec<String>,
) {
    let key = format!(
        "{}{{prefix=\"{}\",code=\"{}\",fingerprint=\"{}\"}}",
        metric, prefix, code, fingerprint
    );
    match metrics.get(&key) {
        Some(value) if metric_ge_u64(value, cli_count) => {}
        Some(value) => mismatches.push(format!(
            "{} below CLI count {} for {}:{} fingerprint {} (value={})",
            metric,
            cli_count,
            prefix,
            code,
            fingerprint,
            describe_metric(value)
        )),
        None => mismatches.push(format!("missing metric {}", key)),
    }
}

fn describe_metric(value: &MetricValue) -> String {
    match value {
        MetricValue::Float(v) => format_float_for_display(*v),
        MetricValue::Integer(v) => v.to_string(),
        MetricValue::Unsigned(v) => v.to_string(),
    }
}

fn metric_ge_u64(value: &MetricValue, target: u64) -> bool {
    match value {
        MetricValue::Float(v) => v.is_finite() && *v + EPSILON >= target as f64,
        MetricValue::Integer(v) => *v >= 0 && (*v as u64) >= target,
        MetricValue::Unsigned(v) => *v >= target,
    }
}

fn metric_ge_usize(value: &MetricValue, target: usize) -> bool {
    match value {
        MetricValue::Float(v) => v.is_finite() && *v + EPSILON >= target as f64,
        MetricValue::Integer(v) => *v >= 0 && (*v as usize) >= target,
        MetricValue::Unsigned(v) => (*v as usize) >= target,
    }
}

fn format_float_for_display(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let mut formatted = format!("{value:.6}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    if formatted.is_empty() {
        formatted.push('0');
    }
    formatted
}
