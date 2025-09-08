use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(author, version, about = "Network diagnostics utilities")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect or reset per-peer metrics via RPC
    Stats {
        #[command(subcommand)]
        action: StatsCmd,
    },
    /// Compute marketplace utilities
    Compute {
        #[command(subcommand)]
        action: ComputeCmd,
    },
}

#[derive(Subcommand)]
enum StatsCmd {
    /// Show per-peer rate-limit metrics
    Show {
        /// Hex-encoded peer id
        peer_id: Option<String>,
        /// Return stats for all peers
        #[arg(long)]
        all: bool,
        /// Pagination offset
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Pagination limit
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Reset metrics for a peer
    Reset {
        /// Hex-encoded peer id
        peer_id: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Show reputation score for a peer
    Reputation {
        /// Hex-encoded peer id
        peer_id: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Export metrics for a peer to a file
    Export {
        /// Hex-encoded peer id
        peer_id: String,
        /// Destination path
        #[arg(long)]
        path: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
}

#[derive(Subcommand)]
enum ComputeCmd {
    /// Show scheduler statistics
    Stats {
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Stats { action } => match action {
            StatsCmd::Show {
                peer_id,
                all,
                offset,
                limit,
                rpc,
            } => {
                if all {
                    let req = json!({
                        "method": "net.peer_stats_all",
                        "params": {"offset": offset, "limit": limit},
                    });
                    match reqwest::blocking::Client::new()
                        .post(&rpc)
                        .json(&req)
                        .send()
                    {
                        Ok(resp) => match resp.json::<serde_json::Value>() {
                            Ok(val) => {
                                if let Some(arr) = val["result"].as_array() {
                                    for entry in arr {
                                        let id = entry["peer_id"].as_str().unwrap_or("");
                                        let m = &entry["metrics"];
                                        let reqs = m["requests"].as_u64().unwrap_or(0);
                                        let bytes = m["bytes_sent"].as_u64().unwrap_or(0);
                                        let drops = m["drops"]
                                            .as_object()
                                            .map(|o| {
                                                o.iter()
                                                    .map(|(k, v)| {
                                                        format!("{k}:{}", v.as_u64().unwrap_or(0))
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(",")
                                            })
                                            .unwrap_or_else(|| "".into());
                                        let hf = m["handshake_fail"]
                                            .as_object()
                                            .map(|o| {
                                                o.iter()
                                                    .map(|(k, v)| {
                                                        format!("{k}:{}", v.as_u64().unwrap_or(0))
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(",")
                                            })
                                            .unwrap_or_else(|| "".into());
                                        println!(
                                            "peer={id} requests={reqs} bytes_sent={bytes} drops={drops} handshake_fail={hf}"
                                        );
                                    }
                                }
                            }
                            Err(e) => eprintln!("parse error: {e}"),
                        },
                        Err(e) => eprintln!("request error: {e}"),
                    }
                } else if let Some(id) = peer_id {
                    let req = json!({
                        "method": "net.peer_stats",
                        "params": {"peer_id": id},
                    });
                    match reqwest::blocking::Client::new()
                        .post(&rpc)
                        .json(&req)
                        .send()
                    {
                        Ok(resp) => match resp.json::<serde_json::Value>() {
                            Ok(val) => {
                                let res = &val["result"];
                                let reqs = res["requests"].as_u64().unwrap_or(0);
                                let bytes = res["bytes_sent"].as_u64().unwrap_or(0);
                                let drops = res["drops"]
                                    .as_object()
                                    .map(|o| {
                                        o.iter()
                                            .map(|(k, v)| {
                                                format!("{k}:{}", v.as_u64().unwrap_or(0))
                                            })
                                            .collect::<Vec<_>>()
                                            .join(",")
                                    })
                                    .unwrap_or_else(|| "".into());
                                let hf = res["handshake_fail"]
                                    .as_object()
                                    .map(|o| {
                                        o.iter()
                                            .map(|(k, v)| {
                                                format!("{k}:{}", v.as_u64().unwrap_or(0))
                                            })
                                            .collect::<Vec<_>>()
                                            .join(",")
                                    })
                                    .unwrap_or_else(|| "".into());
                                println!(
                                    "requests={reqs} bytes_sent={bytes} drops={drops} handshake_fail={hf}"
                                );
                            }
                            Err(e) => eprintln!("parse error: {e}"),
                        },
                        Err(e) => eprintln!("request error: {e}"),
                    }
                } else {
                    eprintln!("peer_id required unless --all is specified");
                }
            }
            StatsCmd::Reset { peer_id, rpc } => {
                let req = json!({
                    "method": "net.peer_stats_reset",
                    "params": {"peer_id": peer_id},
                });
                match reqwest::blocking::Client::new()
                    .post(&rpc)
                    .json(&req)
                    .send()
                {
                    Ok(resp) => match resp.json::<serde_json::Value>() {
                        Ok(val) => {
                            if val["result"]["status"].as_str() == Some("ok") {
                                println!("reset");
                            } else {
                                eprintln!("reset failed");
                            }
                        }
                        Err(e) => eprintln!("parse error: {e}"),
                    },
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
            StatsCmd::Reputation { peer_id, rpc } => {
                let req = json!({
                    "method": "net.peer_stats",
                    "params": {"peer_id": peer_id},
                });
                match reqwest::blocking::Client::new()
                    .post(&rpc)
                    .json(&req)
                    .send()
                {
                    Ok(resp) => match resp.json::<serde_json::Value>() {
                        Ok(val) => {
                            let rep = val["result"]["reputation"].as_f64().unwrap_or(0.0);
                            println!("reputation={rep}");
                        }
                        Err(e) => eprintln!("parse error: {e}"),
                    },
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
            StatsCmd::Export { peer_id, path, rpc } => {
                let req = json!({
                    "method": "net.peer_stats_export",
                    "params": {"peer_id": peer_id, "path": path},
                });
                match reqwest::blocking::Client::new()
                    .post(&rpc)
                    .json(&req)
                    .send()
                {
                    Ok(resp) => match resp.json::<serde_json::Value>() {
                        Ok(val) => {
                            if val["result"]["status"].as_str() == Some("ok") {
                                println!("exported");
                            } else {
                                eprintln!("export failed");
                            }
                        }
                        Err(e) => eprintln!("parse error: {e}"),
                    },
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
        },
        Command::Compute { action } => match action {
            ComputeCmd::Stats { rpc } => {
                let req = json!({ "method": "compute_market.scheduler_stats" });
                match reqwest::blocking::Client::new()
                    .post(&rpc)
                    .json(&req)
                    .send()
                {
                    Ok(resp) => match resp.text() {
                        Ok(t) => println!("{t}"),
                        Err(e) => eprintln!("{e}"),
                    },
                    Err(e) => eprintln!("{e}"),
                }
            }
        },
    }
}
