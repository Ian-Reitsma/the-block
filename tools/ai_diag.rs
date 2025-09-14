use anyhow::Result;
use serde::Deserialize;
use std::env;

#[derive(Deserialize)]
struct Metrics {
    avg_latency_ms: u64,
}

fn suggest(metrics: &Metrics) -> Vec<String> {
    let mut out = Vec::new();
    if metrics.avg_latency_ms > 1_000 {
        out.push(format!(
            "high latency ({}) detected; consider increasing worker threads",
            metrics.avg_latency_ms
        ));
    }
    out
}

fn main() -> Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "metrics.json".into());
    let data = std::fs::read_to_string(path)?;
    let metrics: Metrics = serde_json::from_str(&data)?;
    for s in suggest(&metrics) {
        println!("{s}");
    }
    Ok(())
}
