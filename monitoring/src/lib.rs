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
    let mut other = Vec::new();

    for metric in metrics.iter().filter(|m| !m.deprecated) {
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

/// Parse a Prometheus text-format payload into a metric/value map.
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_prometheus_snapshot(payload: &str) -> HashMap<String, f64> {
    let mut values = HashMap::new();
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
                if let Ok(value) = value_str.parse::<f64>() {
                    values.insert(name.to_string(), value);
                }
            }
        }
    }
    values
}

/// Render the in-house HTML snapshot used by the telemetry dashboard helpers.
#[cfg_attr(not(test), allow(dead_code))]
pub fn render_html_snapshot(
    endpoint: &str,
    metrics: &[Metric],
    snapshot: &HashMap<String, f64>,
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
fn render_section(title: &str, metrics: &[&Metric], snapshot: &HashMap<String, f64>) -> String {
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
            Some(value) => format_metric_value(*value),
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
fn format_metric_value(value: f64) -> String {
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
    use std::collections::HashMap;

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
        assert_eq!(parsed.get("dex_trades_total"), Some(&42.0));
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
        let mut snapshot = HashMap::new();
        snapshot.insert("dex_trades_total".to_string(), 42.0);
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
        assert_eq!(format_metric_value(42.0), "42");
        assert_eq!(format_metric_value(3.140000), "3.14");
        assert_eq!(format_metric_value(0.0), "0");
        assert_eq!(format_metric_value(f64::INFINITY), "inf");
    }
}
