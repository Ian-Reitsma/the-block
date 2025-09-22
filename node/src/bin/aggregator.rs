use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::VecDeque;

#[derive(Parser)]
#[command(author, version, about = "Metrics aggregator administration")]
struct Cli {
    /// Path to aggregator database
    #[arg(long, default_value = "peer_metrics.db")]
    db: String,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Remove samples before the given UNIX timestamp
    Prune {
        #[arg(long)]
        before: u64,
    },
    /// Fetch latest telemetry summaries from the aggregator HTTP API
    Telemetry {
        #[arg(long, default_value = "http://localhost:8080")]
        url: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let db = sled::open(&cli.db)?;
    match cli.cmd {
        Command::Prune { before } => {
            let mut total = 0u64;
            for item in db.iter() {
                let (k, v) = item?;
                let mut deque: VecDeque<(u64, Value)> =
                    serde_json::from_slice(&v).unwrap_or_default();
                let before_len = deque.len();
                deque.retain(|(ts, _)| *ts >= before);
                let after_len = deque.len();
                total += (before_len - after_len) as u64;
                if after_len == 0 {
                    db.remove(&k)?;
                } else {
                    db.insert(&k, serde_json::to_vec(&deque)?)?;
                }
            }
            db.flush()?;
            println!("pruned {total}");
        }
        Command::Telemetry { url } => {
            let client = Client::new();
            let resp = client.get(format!("{}/telemetry", url)).send()?;
            if !resp.status().is_success() {
                eprintln!("telemetry request failed: {}", resp.status());
            }
            let body = resp.text()?;
            println!("{}", body);
        }
    }
    Ok(())
}
