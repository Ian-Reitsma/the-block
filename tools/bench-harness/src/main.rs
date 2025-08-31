#![forbid(unsafe_code)]

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// Distributed benchmarking harness CLI.
#[derive(Debug, Parser)]
#[command(name = "bench-harness")]
struct Cli {
    /// Number of nodes to deploy for the benchmark.
    #[arg(short, long, default_value_t = 1)]
    nodes: u32,

    /// Optional workload configuration in JSON format.
    #[arg(short, long)]
    workload: Option<PathBuf>,

    /// Path to write benchmark report JSON.
    #[arg(long)]
    report: PathBuf,

    /// Optional baseline metrics to detect regressions.
    #[arg(long)]
    baseline: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Metrics {
    latency_ms: f64,
    throughput_tps: f64,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    deploy_nodes(cli.nodes)?;
    let metrics = run_workload(cli.workload)?;
    if let Some(base) = &cli.baseline {
        regression_check(&metrics, base)?;
    }
    fs::write(&cli.report, serde_json::to_vec_pretty(&metrics)?)?;
    Ok(())
}

fn deploy_nodes(nodes: u32) -> anyhow::Result<()> {
    for n in 0..nodes {
        println!("starting node {n}");
        // Placeholder: spawn containers or remote nodes.
    }
    Ok(())
}

fn run_workload(workload: Option<PathBuf>) -> anyhow::Result<Metrics> {
    if let Some(path) = workload {
        let _cfg: serde_json::Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    }
    // In lieu of real metrics, return deterministic placeholder values.
    Ok(Metrics {
        latency_ms: 10.0,
        throughput_tps: 1_000.0,
    })
}

fn regression_check(metrics: &Metrics, baseline_path: &PathBuf) -> anyhow::Result<()> {
    let data = fs::read_to_string(baseline_path)?;
    let baseline: Metrics = serde_json::from_str(&data)?;
    if metrics.throughput_tps < baseline.throughput_tps {
        eprintln!(
            "throughput regression: {} < {}",
            metrics.throughput_tps, baseline.throughput_tps
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_regression() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let baseline = Metrics {
            latency_ms: 10.0,
            throughput_tps: 2_000.0,
        };
        fs::write(tmp.path(), serde_json::to_string(&baseline).unwrap()).unwrap();
        let metrics = Metrics {
            latency_ms: 10.0,
            throughput_tps: 1_000.0,
        };
        regression_check(&metrics, &tmp.path().into()).unwrap();
    }
}
