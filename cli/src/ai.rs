use crate::codec_helpers::json_from_str;
use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;

/// Minimal snapshot of metrics for diagnostics.
#[derive(Clone, Deserialize, Default)]
pub struct Metrics {
    pub avg_latency_ms: u64,
}

/// Placeholder node configuration used for suggestion safety tests.
#[derive(Clone, Default)]
pub struct NodeConfig {
    pub consensus_version: u64,
}

/// Generate suggestions based on metrics without mutating the input config.
pub fn suggest_config(cfg: &NodeConfig, metrics: &Metrics) -> Vec<String> {
    let mut out = Vec::new();
    if metrics.avg_latency_ms > 1_000 {
        out.push(format!(
            "high latency ({}) detected; consider increasing worker threads",
            metrics.avg_latency_ms
        ));
    }
    // the configuration is intentionally left untouched
    let _ = cfg.consensus_version;
    out
}

/// Runs diagnostics from a JSON metrics snapshot.
pub fn diagnose(path: &str) -> Result<()> {
    let data = std::fs::read_to_string(path)?;
    let metrics: Metrics = json_from_str(&data)?;
    let cfg = NodeConfig::default();
    for s in suggest_config(&cfg, &metrics) {
        println!("{s}");
    }
    Ok(())
}

#[derive(Subcommand)]
pub enum AiCmd {
    /// Run diagnostics from a metrics snapshot
    Diagnose {
        #[arg(long, default_value = "metrics.json")]
        snapshot: String,
    },
}

pub fn handle(cmd: AiCmd) {
    match cmd {
        AiCmd::Diagnose { snapshot } => {
            if let Err(e) = diagnose(&snapshot) {
                eprintln!("{e}");
            }
        }
    }
}
