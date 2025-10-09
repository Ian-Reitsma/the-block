use foundation_serialization::json::{self, Map, Value};
use std::{fs, path::PathBuf};

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
    let metrics_value = read_json(metrics_path)?;
    let metrics = extract_metrics(&metrics_value)?;
    let overrides = match overrides_path {
        Some(path) => Some(read_json(path)?),
        None => None,
    };
    generate(&metrics, overrides)
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
}
