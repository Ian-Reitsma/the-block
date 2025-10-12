use foundation_serialization::json::{self, Map, Value};
use std::{collections::HashMap, fs, path::PathBuf};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub deprecated: bool,
}

#[derive(Debug)]
pub enum DashboardError {
    Io { path: PathBuf, source: std::io::Error },
    Parse { path: PathBuf, source: foundation_serialization::Error },
    InvalidStructure(&'static str),
    InvalidMetric { name: Option<String>, field: &'static str },
}

impl DashboardError {
    fn invalid_metric<N: Into<Option<String>>>(name: N, field: &'static str) -> Self {
        DashboardError::InvalidMetric {
            name: name.into(),
            field,
        }
    }
}

impl std::fmt::Display for DashboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DashboardError::Io { path, source } => {
                write!(f, "failed to read '{}': {}", path.display(), source)
            }
            DashboardError::Parse { path, source } => {
                write!(f, "failed to parse '{}': {}", path.display(), source)
            }
            DashboardError::InvalidStructure(msg) => f.write_str(msg),
            DashboardError::InvalidMetric { name, field } => {
                match name {
                    Some(name) => write!(f, "invalid metric '{}': missing/invalid '{}' field", name, field),
                    None => write!(f, "invalid metric entry: missing/invalid '{}' field", field),
                }
            }
        }
    }
}

impl std::error::Error for DashboardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DashboardError::Io { source, .. } => Some(source),
            DashboardError::Parse { source, .. } => Some(source),
            DashboardError::InvalidStructure(_) | DashboardError::InvalidMetric { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricValue {
    Float(f64),
    Integer(i64),
    Unsigned(u64),
}

impl MetricValue {}

pub type MetricSnapshot = HashMap<String, MetricValue>;

const TLS_PANEL_TITLE: &str = "TLS env warnings (5m delta)";
const TLS_PANEL_EXPR: &str = "sum by (prefix, code)(increase(tls_env_warning_total[5m]))";
const TLS_LAST_SEEN_PANEL_TITLE: &str = "TLS env warnings (last seen)";
const TLS_LAST_SEEN_EXPR: &str = "max by (prefix, code)(tls_env_warning_last_seen_seconds)";
const TLS_FRESHNESS_PANEL_TITLE: &str = "TLS env warnings (age seconds)";
const TLS_FRESHNESS_EXPR: &str =
    "clamp_min(time() - max by (prefix, code)(tls_env_warning_last_seen_seconds), 0)";
const TLS_ACTIVE_PANEL_TITLE: &str = "TLS env warnings (active snapshots)";
const TLS_ACTIVE_EXPR: &str = "tls_env_warning_active_snapshots";
const TLS_STALE_PANEL_TITLE: &str = "TLS env warnings (stale snapshots)";
const TLS_STALE_EXPR: &str = "tls_env_warning_stale_snapshots";
const TLS_RETENTION_PANEL_TITLE: &str = "TLS env warnings (retention seconds)";
const TLS_RETENTION_EXPR: &str = "tls_env_warning_retention_seconds";
const TLS_DETAIL_FINGERPRINT_PANEL_TITLE: &str = "TLS env warnings (detail fingerprint hash)";
const TLS_DETAIL_FINGERPRINT_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_detail_fingerprint)";
const TLS_VARIABLES_FINGERPRINT_PANEL_TITLE: &str = "TLS env warnings (variables fingerprint hash)";
const TLS_VARIABLES_FINGERPRINT_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_variables_fingerprint)";
const TLS_DETAIL_UNIQUE_PANEL_TITLE: &str = "TLS env warnings (unique detail fingerprints)";
const TLS_DETAIL_UNIQUE_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_detail_unique_fingerprints)";
const TLS_VARIABLES_UNIQUE_PANEL_TITLE: &str = "TLS env warnings (unique variables fingerprints)";
const TLS_VARIABLES_UNIQUE_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_variables_unique_fingerprints)";
const TLS_DETAIL_FINGERPRINT_DELTA_PANEL_TITLE: &str =
    "TLS env warnings (detail fingerprint 5m delta)";
const TLS_DETAIL_FINGERPRINT_DELTA_EXPR: &str =
    "sum by (prefix, code, fingerprint)(increase(tls_env_warning_detail_fingerprint_total[5m]))";
const TLS_VARIABLES_FINGERPRINT_DELTA_PANEL_TITLE: &str =
    "TLS env warnings (variables fingerprint 5m delta)";
const TLS_VARIABLES_FINGERPRINT_DELTA_EXPR: &str =
    "sum by (prefix, code, fingerprint)(increase(tls_env_warning_variables_fingerprint_total[5m]))";

impl Metric {
    fn from_value(value: &Value) -> Result<Self, DashboardError> {
        let map = match value {
            Value::Object(map) => map,
            _ => {
                return Err(DashboardError::InvalidStructure(
                    "metric entries must be JSON objects",
                ))
            }
        };

        let name = Self::string_field(map, "name")
            .ok_or_else(|| DashboardError::invalid_metric(None::<String>, "name"))?;
        let description = Self::string_field(map, "description").unwrap_or_default();
        let unit = Self::string_field(map, "unit").unwrap_or_default();
        let deprecated = match map.get("deprecated") {
            Some(Value::Bool(flag)) => *flag,
            Some(_) => return Err(DashboardError::invalid_metric(Some(name.clone()), "deprecated")),
            None => false,
        };

        Ok(Self {
            name,
            description,
            unit,
            deprecated,
        })
    }

    fn string_field(map: &Map, key: &str) -> Option<String> {
        map.get(key).and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            _ => None,
        })
    }
}

pub fn generate_dashboard(metrics_path: &str, overrides_path: Option<&str>) -> Result<Value, DashboardError> {
    let metrics = load_metrics_spec(metrics_path)?;
    let overrides = match overrides_path {
        Some(path) => Some(read_json(path)?),
        None => None,
    };
    generate(&metrics, overrides)
}

pub fn load_metrics_spec(path: &str) -> Result<Vec<Metric>, DashboardError> {
    let value = read_json(path)?;
    extract_metrics(&value)
}

fn read_json(path: &str) -> Result<Value, DashboardError> {
    let path_buf = PathBuf::from(path);
    let raw = fs::read_to_string(&path_buf).map_err(|source| DashboardError::Io {
        path: path_buf.clone(),
        source,
    })?;
    json::value_from_str(&raw).map_err(|source| DashboardError::Parse {
        path: path_buf,
        source,
    })
}

fn extract_metrics(root: &Value) -> Result<Vec<Metric>, DashboardError> {
    let metrics_value = match root {
        Value::Object(map) => map.get("metrics"),
        _ => None,
    }
    .ok_or(DashboardError::InvalidStructure(
        "metrics specification must be an object containing a 'metrics' array",
    ))?;

    let metrics_array = match metrics_value {
        Value::Array(items) => items,
        _ => {
            return Err(DashboardError::InvalidStructure(
                "the 'metrics' field must be an array",
            ))
        }
    };

    metrics_array
        .iter()
        .map(Metric::from_value)
        .collect::<Result<Vec<_>, _>>()
}

fn generate(metrics: &[Metric], overrides: Option<Value>) -> Result<Value, DashboardError> {
    let mut dex = Vec::new();
    let mut compute = Vec::new();
    let mut gossip = Vec::new();
    let mut tls = Vec::new();
    let mut other = Vec::new();

    for metric in metrics.iter().filter(|m| !m.deprecated) {
        if metric.name == "tls_env_warning_total" {
            tls.push(build_tls_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_last_seen_seconds" {
            tls.push(build_tls_last_seen_panel(metric));
            tls.push(build_tls_freshness_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_detail_fingerprint" {
            tls.push(build_tls_hash_panel(
                TLS_DETAIL_FINGERPRINT_PANEL_TITLE,
                TLS_DETAIL_FINGERPRINT_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_variables_fingerprint" {
            tls.push(build_tls_hash_panel(
                TLS_VARIABLES_FINGERPRINT_PANEL_TITLE,
                TLS_VARIABLES_FINGERPRINT_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_detail_unique_fingerprints" {
            tls.push(build_tls_unique_panel(
                TLS_DETAIL_UNIQUE_PANEL_TITLE,
                TLS_DETAIL_UNIQUE_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_variables_unique_fingerprints" {
            tls.push(build_tls_unique_panel(
                TLS_VARIABLES_UNIQUE_PANEL_TITLE,
                TLS_VARIABLES_UNIQUE_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_detail_fingerprint_total" {
            tls.push(build_tls_fingerprint_delta_panel(
                TLS_DETAIL_FINGERPRINT_DELTA_PANEL_TITLE,
                TLS_DETAIL_FINGERPRINT_DELTA_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_variables_fingerprint_total" {
            tls.push(build_tls_fingerprint_delta_panel(
                TLS_VARIABLES_FINGERPRINT_DELTA_PANEL_TITLE,
                TLS_VARIABLES_FINGERPRINT_DELTA_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_active_snapshots" {
            tls.push(build_tls_active_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_stale_snapshots" {
            tls.push(build_tls_stale_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_retention_seconds" {
            tls.push(build_tls_retention_panel(metric));
            continue;
        }

        let mut panel = Map::new();
        panel.insert("type".into(), Value::from("timeseries"));
        panel.insert("title".into(), Value::from(metric.name.clone()));

        let mut target = Map::new();
        target.insert("expr".into(), Value::from(metric.name.clone()));
        panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

        let mut legend = Map::new();
        legend.insert("showLegend".into(), Value::from(false));
        let mut options = Map::new();
        options.insert("legend".into(), Value::Object(legend));
        panel.insert("options".into(), Value::Object(options));

        let mut datasource = Map::new();
        datasource.insert("type".into(), Value::from("foundation-telemetry"));
        datasource.insert("uid".into(), Value::from("foundation"));
        panel.insert("datasource".into(), Value::Object(datasource));

        let panel_value = Value::Object(panel);
        if metric.name.starts_with("dex_") {
            dex.push(panel_value);
        } else if metric.name.starts_with("compute_") || metric.name.starts_with("scheduler_") {
            compute.push(panel_value);
        } else if metric.name.starts_with("gossip_") {
            gossip.push(panel_value);
        } else {
            other.push(panel_value);
        }
    }

    let mut panels = Vec::new();
    let mut next_id: u64 = 1;

    for (title, mut entries) in [
        ("DEX", dex),
        ("Compute", compute),
        ("Gossip", gossip),
        ("TLS", tls),
        ("Other", other),
    ] {
        if entries.is_empty() {
            continue;
        }
        let mut row = Map::new();
        row.insert("type".into(), Value::from("row"));
        row.insert("title".into(), Value::from(title));
        row.insert("id".into(), Value::from(next_id));
        next_id += 1;
        panels.push(Value::Object(row));

        for entry in entries.iter_mut() {
            if let Value::Object(ref mut map) = entry {
                map.insert("id".into(), Value::from(next_id));
            }
            next_id += 1;
        }
        panels.extend(entries);
    }

    let mut dashboard_map = Map::new();
    dashboard_map.insert("title".into(), Value::from("The-Block Auto"));
    dashboard_map.insert("schemaVersion".into(), Value::from(37u64));
    dashboard_map.insert("version".into(), Value::from(1u64));
    dashboard_map.insert("panels".into(), Value::Array(panels));
    let mut dashboard = Value::Object(dashboard_map);

    if let Some(override_value) = overrides {
        apply_overrides(&mut dashboard, override_value)?;
    }

    Ok(dashboard)
}

fn build_tls_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(TLS_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert("description".into(), Value::from(metric.description.clone()));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(TLS_PANEL_EXPR));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_last_seen_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(TLS_LAST_SEEN_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert("description".into(), Value::from(metric.description.clone()));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(TLS_LAST_SEEN_EXPR));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_freshness_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(TLS_FRESHNESS_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert("description".into(), Value::from(metric.description.clone()));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(TLS_FRESHNESS_EXPR));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_active_panel(metric: &Metric) -> Value {
    build_tls_scalar_panel(TLS_ACTIVE_PANEL_TITLE, TLS_ACTIVE_EXPR, metric)
}

fn build_tls_stale_panel(metric: &Metric) -> Value {
    build_tls_scalar_panel(TLS_STALE_PANEL_TITLE, TLS_STALE_EXPR, metric)
}

fn build_tls_retention_panel(metric: &Metric) -> Value {
    build_tls_scalar_panel(TLS_RETENTION_PANEL_TITLE, TLS_RETENTION_EXPR, metric)
}

fn build_tls_hash_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert("description".into(), Value::from(metric.description.clone()));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_unique_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    build_tls_hash_panel(title, expr, metric)
}

fn build_tls_fingerprint_delta_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert("description".into(), Value::from(metric.description.clone()));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert(
        "legendFormat".into(),
        Value::from("{{prefix}} · {{code}} · {{fingerprint}}"),
    );
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_scalar_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert("description".into(), Value::from(metric.description.clone()));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(false));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

/// Parse a Prometheus text-format payload into a metric/value map.
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_prometheus_snapshot(payload: &str) -> MetricSnapshot {
    let mut values = MetricSnapshot::new();
    for line in payload.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        if let Some(name) = parts.next() {
            if name.ends_with("_bucket") || name.ends_with("_count") || name.ends_with("_sum") {
                continue;
            }
            if let Some(value_str) = parts.last() {
                if let Some(value) = parse_metric_value(value_str) {
                    values.insert(name.to_string(), value);
                }
            }
        }
    }
    values
}

fn parse_metric_value(value: &str) -> Option<MetricValue> {
    if let Ok(int) = value.parse::<i64>() {
        return Some(MetricValue::Integer(int));
    }
    if let Ok(uint) = value.parse::<u64>() {
        return Some(MetricValue::Unsigned(uint));
    }
    value
        .parse::<f64>()
        .ok()
        .map(MetricValue::Float)
}

/// Render the in-house HTML snapshot used by the telemetry dashboard helpers.
#[cfg_attr(not(test), allow(dead_code))]
pub fn render_html_snapshot(
    endpoint: &str,
    metrics: &[Metric],
    snapshot: &MetricSnapshot,
) -> String {
    let mut sections = [
        ("DEX", Vec::new()),
        ("Compute", Vec::new()),
        ("Gossip", Vec::new()),
        ("Other", Vec::new()),
    ];

    for metric in metrics.iter().filter(|metric| !metric.deprecated) {
        let bucket = match categorize_metric(metric) {
            MetricCategory::Dex => 0,
            MetricCategory::Compute => 1,
            MetricCategory::Gossip => 2,
            MetricCategory::Other => 3,
        };
        sections[bucket].1.push(metric);
    }

    let mut rendered_sections = Vec::new();
    for (title, metrics) in sections.into_iter() {
        let section = render_section(title, &metrics, snapshot);
        if !section.is_empty() {
            rendered_sections.push(section);
        }
    }

    let body = if rendered_sections.is_empty() {
        "<p>No metrics available.</p>".to_string()
    } else {
        rendered_sections.join("\n")
    };

    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\">\n    <meta http-equiv=\"refresh\" content=\"5\">\n    <title>The-Block Telemetry Snapshot</title>\n    <style>\n      body {{ font-family: system-ui, sans-serif; margin: 2rem; background: #0f1115; color: #f8fafc; }}\n      h1 {{ margin-bottom: 1rem; }}\n      table {{ width: 100%; border-collapse: collapse; margin-bottom: 2rem; }}\n      th, td {{ border-bottom: 1px solid #1f2937; padding: 0.5rem 0.75rem; text-align: left; }}\n      th {{ text-transform: uppercase; font-size: 0.75rem; letter-spacing: 0.08em; color: #94a3b8; }}\n      tr.metric-row:hover {{ background: rgba(148, 163, 184, 0.08); }}\n      .status {{ font-weight: 600; }}\n      .error {{ color: #fca5a5; }}\n    </style>\n  </head>\n  <body>\n    <h1>The-Block Telemetry Snapshot</h1>\n    <p class=\"status\">Source: {}</p>\n    {}\n  </body>\n</html>\n",
        html_escape(endpoint),
        body
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn render_section(title: &str, metrics: &[&Metric], snapshot: &MetricSnapshot) -> String {
    if metrics.is_empty() {
        return String::new();
    }
    let mut rows = String::new();
    for metric in metrics {
        let name = html_escape(&metric.name);
        let description = if metric.description.is_empty() {
            "&mdash;".to_string()
        } else {
            html_escape(&metric.description)
        };
        let value_display = match snapshot.get(&metric.name) {
            Some(value) => format_metric_value(value),
            None => "<span class=\"error\">missing</span>".to_string(),
        };
        rows.push_str(&format!(
            "      <tr class=\"metric-row\"><td><code>{name}</code></td><td>{description}</td><td>{}</td></tr>\n",
            value_display
        ));
    }

    format!(
        "<h2>{}</h2>\n<table>\n  <thead>\n    <tr><th>Metric</th><th>Description</th><th>Value</th></tr>\n  </thead>\n  <tbody>\n{}  </tbody>\n</table>",
        html_escape(title),
        rows
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg_attr(not(test), allow(dead_code))]
fn format_metric_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Float(v) => format_float(*v),
        MetricValue::Integer(v) => v.to_string(),
        MetricValue::Unsigned(v) => v.to_string(),
    }
}

fn format_float(value: f64) -> String {
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

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum MetricCategory {
    Dex,
    Compute,
    Gossip,
    Other,
}

#[cfg_attr(not(test), allow(dead_code))]
fn categorize_metric(metric: &Metric) -> MetricCategory {
    let name = metric.name.as_str();
    if name.starts_with("dex_") {
        MetricCategory::Dex
    } else if name.starts_with("compute_") || name.starts_with("scheduler_") {
        MetricCategory::Compute
    } else if name.starts_with("gossip_") {
        MetricCategory::Gossip
    } else {
        MetricCategory::Other
    }
}

pub(crate) fn apply_overrides(base: &mut Value, overrides: Value) -> Result<(), DashboardError> {
    let Value::Object(ref mut base_map) = base else {
        return Err(DashboardError::InvalidStructure(
            "generated dashboard must be a JSON object",
        ));
    };

    let Value::Object(override_map) = overrides else {
        return Err(DashboardError::InvalidStructure(
            "dashboard overrides must be a JSON object",
        ));
    };

    for (key, value) in override_map {
        base_map.insert(key, value);
    }
    Ok(())
}

pub fn render_pretty(dashboard: &Value) -> Result<String, foundation_serialization::Error> {
    let mut rendered = json::to_string_pretty(dashboard)?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_metrics_by_prefix() {
        let metrics = vec![
            Metric {
                name: "dex_trades_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "compute_jobs_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "gossip_peers".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "storage_reads".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        // Four rows + four panels should yield eight entries.
        assert_eq!(panels.len(), 8);

        let titles: Vec<&str> = panels
            .iter()
            .filter_map(|panel| match panel {
                Value::Object(map) => {
                    if matches!(map.get("type"), Some(Value::String(t)) if t == "row") {
                        match map.get("title") {
                            Some(Value::String(title)) => Some(title.as_str()),
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        assert_eq!(titles, vec!["DEX", "Compute", "Gossip", "Other"]);
    }

    #[test]
    fn tls_panel_is_included_with_custom_query() {
        let metrics = vec![Metric {
            name: "tls_env_warning_total".into(),
            description: "TLS warnings grouped by prefix/code".into(),
            unit: String::new(),
            deprecated: false,
        }];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 2);

        let rows: Vec<&str> = panels
            .iter()
            .filter_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    map.get("title").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows, vec!["TLS"]);

        let tls_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == super::TLS_PANEL_TITLE)
                        .unwrap_or(false) => Some(map),
                _ => None,
            })
            .expect("tls panel present");

        let expr = tls_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("tls panel expression");
        assert_eq!(expr, super::TLS_PANEL_EXPR);

        let legend_enabled = tls_panel
            .get("options")
            .and_then(|options| match options {
                Value::Object(map) => map.get("legend"),
                _ => None,
            })
            .and_then(|legend| match legend {
                Value::Object(map) => map.get("showLegend"),
                _ => None,
            })
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(legend_enabled, "TLS panel legend should be enabled");

        let legend_format = tls_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("legendFormat"),
                _ => None,
            })
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(legend_format, "{{prefix}} · {{code}}");
    }

    #[test]
    fn tls_last_seen_and_freshness_panels_are_included() {
        let metrics = vec![Metric {
            name: "tls_env_warning_last_seen_seconds".into(),
            description: String::from("TLS warning freshness"),
            unit: String::new(),
            deprecated: false,
        }];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 3);

        let last_seen_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == super::TLS_LAST_SEEN_PANEL_TITLE)
                        .unwrap_or(false) => Some(map),
                _ => None,
            })
            .expect("last seen panel present");
        let last_seen_expr = last_seen_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("last seen expr");
        assert_eq!(last_seen_expr, super::TLS_LAST_SEEN_EXPR);

        let freshness_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == super::TLS_FRESHNESS_PANEL_TITLE)
                        .unwrap_or(false) => Some(map),
                _ => None,
            })
            .expect("freshness panel present");
        let freshness_expr = freshness_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("freshness expr");
        assert_eq!(freshness_expr, super::TLS_FRESHNESS_EXPR);
    }

    #[test]
    fn tls_status_scalar_panels_are_included() {
        let metrics = vec![
            Metric {
                name: "tls_env_warning_active_snapshots".into(),
                description: String::from("Active TLS warning snapshots"),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "tls_env_warning_stale_snapshots".into(),
                description: String::from("Stale TLS warning snapshots"),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "tls_env_warning_retention_seconds".into(),
                description: String::from("Configured retention window"),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 4);

        let titles: Vec<&str> = panels
            .iter()
            .filter_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    None
                }
                Value::Object(map) => map.get("title").and_then(Value::as_str),
                _ => None,
            })
            .collect();

        assert!(titles.contains(&super::TLS_ACTIVE_PANEL_TITLE));
        assert!(titles.contains(&super::TLS_STALE_PANEL_TITLE));
        assert!(titles.contains(&super::TLS_RETENTION_PANEL_TITLE));
    }

    #[test]
    fn applies_overrides() {
        let metrics = vec![Metric {
            name: "dex_volume".into(),
            description: String::new(),
            unit: String::new(),
            deprecated: false,
        }];

        let overrides = json::value_from_str(r#"{"title":"Custom"}"#).unwrap();
        let dashboard = generate(&metrics, Some(overrides)).expect("dashboard");
        let title = match dashboard {
            Value::Object(map) => match map.get("title") {
                Some(Value::String(text)) => Some(text.clone()),
                _ => None,
            },
            _ => None,
        };
        assert_eq!(title.as_deref(), Some("Custom"));
    }

    #[test]
    fn rejects_invalid_metric() {
        let value = json::value_from_str(r#"{"metrics":[{"description":"missing name"}]}"#).unwrap();
        let error = extract_metrics(&value).expect_err("missing name should fail");
        match error {
            DashboardError::InvalidMetric { field, .. } => assert_eq!(field, "name"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parse_prometheus_snapshot_skips_histograms() {
        let payload = r#"
# HELP dex_trades_total Total trades
# TYPE dex_trades_total counter
dex_trades_total 42
scheduler_match_total_bucket{result="ok",le="0.5"} 1
scheduler_match_total_sum 10
"#;
        let parsed = parse_prometheus_snapshot(payload);
        assert_eq!(parsed.get("dex_trades_total"), Some(&MetricValue::Integer(42)));
        assert!(!parsed.contains_key("scheduler_match_total_bucket"));
        assert!(!parsed.contains_key("scheduler_match_total_sum"));
    }

    #[test]
    fn render_html_snapshot_renders_sections() {
        let metrics = vec![
            Metric {
                name: "dex_trades_total".into(),
                description: "Total DEX trades".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "compute_jobs_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
        ];
        let mut snapshot = MetricSnapshot::new();
        snapshot.insert("dex_trades_total".to_string(), MetricValue::Integer(42));
        let html = render_html_snapshot("http://localhost:9090/metrics", &metrics, &snapshot);
        assert!(html.contains("The-Block Telemetry Snapshot"));
        assert!(html.contains("<h2>DEX</h2>"));
        assert!(html.contains("<h2>Compute</h2>"));
        assert!(html.contains("dex_trades_total"));
        assert!(html.contains("42"));
        assert!(html.contains("missing"));
    }

    #[test]
    fn format_metric_value_trims_trailing_zeroes() {
        assert_eq!(
            format_metric_value(&MetricValue::Float(42.0)),
            "42"
        );
        assert_eq!(
            format_metric_value(&MetricValue::Float(3.140000)),
            "3.14"
        );
        assert_eq!(
            format_metric_value(&MetricValue::Float(0.0)),
            "0"
        );
        assert_eq!(
            format_metric_value(&MetricValue::Float(f64::INFINITY)),
            "inf"
        );
    }
}
