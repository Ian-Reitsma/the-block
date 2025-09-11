use clap::{Parser, Subcommand};
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
    }
    Ok(())
}
