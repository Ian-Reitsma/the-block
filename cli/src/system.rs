use std::collections::BTreeMap;

use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use httpd::{BlockingClient, Method};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SystemCmd {
    /// Fetch wrapper dependency metrics from the metrics aggregator.
    Dependencies {
        /// Metrics aggregator base URL.
        aggregator: String,
    },
}

impl SystemCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("system"), "system", "System-level diagnostics")
            .subcommand(
                CommandBuilder::new(
                    CommandId("system.dependencies"),
                    "dependencies",
                    "Fetch wrapper dependency metrics",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("aggregator", "aggregator", "Metrics aggregator base URL")
                        .default("http://localhost:9000"),
                ))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'system'".to_string())?;

        match name {
            "dependencies" => {
                let aggregator = sub_matches
                    .get_string("aggregator")
                    .unwrap_or_else(|| "http://localhost:9000".to_string());
                Ok(SystemCmd::Dependencies { aggregator })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct WrapperMetric {
    metric: String,
    labels: BTreeMap<String, String>,
    value: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct WrapperSummary {
    #[serde(default)]
    metrics: Vec<WrapperMetric>,
}

pub fn handle(cmd: SystemCmd) {
    match cmd {
        SystemCmd::Dependencies { aggregator } => match fetch_dependencies(&aggregator) {
            Ok(report) => print!("{}", report),
            Err(err) => eprintln!("system dependencies failed: {err}"),
        },
    }
}

fn fetch_dependencies(base: &str) -> anyhow::Result<String> {
    let client = BlockingClient::default();
    let url = format!("{}/wrappers", base.trim_end_matches('/'));
    let response = client.request(Method::Get, &url)?.send()?;
    if !response.status().is_success() {
        anyhow::bail!(
            "aggregator responded with status {}",
            response.status().as_u16()
        );
    }
    let summaries: BTreeMap<String, WrapperSummary> = response.json()?;
    Ok(render_dependencies(summaries))
}

fn render_dependencies(mut summaries: BTreeMap<String, WrapperSummary>) -> String {
    if summaries.is_empty() {
        return "no wrapper metrics reported\n".to_string();
    }

    let mut output = String::new();
    for (node, summary) in summaries.iter_mut() {
        output.push_str(&format!("node: {}\n", node));
        summary
            .metrics
            .sort_by(|a, b| match a.metric.cmp(&b.metric) {
                std::cmp::Ordering::Equal => a.labels.cmp(&b.labels),
                other => other,
            });
        for metric in &summary.metrics {
            let mut parts: Vec<String> = metric
                .labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect();
            parts.sort();
            if parts.is_empty() {
                output.push_str(&format!("  {} {:.3}\n", metric.metric, metric.value));
            } else {
                output.push_str(&format!(
                    "  {}{{{}}} {:.3}\n",
                    metric.metric,
                    parts.join(","),
                    metric.value
                ));
            }
        }
        output.push('\n');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpd::{Response, Router, ServerConfig, StatusCode};
    use runtime::{self, net::TcpListener};

    fn start_wrappers_server(body: String) -> (String, runtime::JoinHandle<std::io::Result<()>>) {
        runtime::block_on(async move {
            let router = Router::new(body).get("/wrappers", |req| {
                let state = req.state().clone();
                async move {
                    Ok(Response::new(StatusCode::OK)
                        .with_header("content-type", "application/json")
                        .with_body(state.as_ref().as_bytes().to_vec()))
                }
            });
            let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
                .await
                .expect("bind test listener");
            let addr = format!("http://{}", listener.local_addr().expect("listener addr"));
            let handle = runtime::spawn(async move {
                httpd::serve(listener, router, ServerConfig::default()).await
            });
            (addr, handle)
        })
    }

    fn sample_summary() -> BTreeMap<String, WrapperSummary> {
        let mut summaries = BTreeMap::new();
        summaries.insert(
            "node-b".to_string(),
            WrapperSummary {
                metrics: vec![WrapperMetric {
                    metric: "crypto_backend_info".to_string(),
                    labels: BTreeMap::from([
                        ("algorithm".to_string(), "ed25519".to_string()),
                        ("backend".to_string(), "inhouse".to_string()),
                        ("version".to_string(), "0.1.0".to_string()),
                    ]),
                    value: 1.0,
                }],
            },
        );
        summaries.insert(
            "node-a".to_string(),
            WrapperSummary {
                metrics: vec![
                    WrapperMetric {
                        metric: "codec_deserialize_fail_total".to_string(),
                        labels: BTreeMap::from([
                            ("codec".to_string(), "json".to_string()),
                            ("profile".to_string(), "none".to_string()),
                            ("version".to_string(), "0.1.0".to_string()),
                        ]),
                        value: 2.0,
                    },
                    WrapperMetric {
                        metric: "codec_serialize_fail_total".to_string(),
                        labels: BTreeMap::from([
                            ("codec".to_string(), "json".to_string()),
                            ("profile".to_string(), "none".to_string()),
                            ("version".to_string(), "0.1.0".to_string()),
                        ]),
                        value: 1.0,
                    },
                ],
            },
        );
        summaries
    }

    #[test]
    fn render_dependencies_sorts_nodes_and_metrics() {
        let output = render_dependencies(sample_summary());
        let expected = "\
node: node-a\n  codec_deserialize_fail_total{codec=\"json\",profile=\"none\",version=\"0.1.0\"} 2.000\n  codec_serialize_fail_total{codec=\"json\",profile=\"none\",version=\"0.1.0\"} 1.000\n\nnode: node-b\n  crypto_backend_info{algorithm=\"ed25519\",backend=\"inhouse\",version=\"0.1.0\"} 1.000\n\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn render_dependencies_reports_empty() {
        let output = render_dependencies(BTreeMap::new());
        assert_eq!(output, "no wrapper metrics reported\n");
    }

    #[test]
    fn fetch_dependencies_parses_response() {
        let body = foundation_serialization::json::to_string(&sample_summary()).unwrap();
        let (url, handle) = start_wrappers_server(body);
        let report = fetch_dependencies(&url).expect("report");
        assert!(report.starts_with("node: node-a"));
        assert!(report.contains("codec_serialize_fail_total"));
        assert!(report.contains("node: node-b"));
        handle.abort();
    }
}
