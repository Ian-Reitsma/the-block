use clap::{Parser, Subcommand};
use ed25519_dalek::Signer;
use hex;
use reqwest::blocking::Client;
use serde_json::json;
use std::time::Duration;
use the_block::net::load_net_key;
use tungstenite::{connect, protocol::Message as WsMessage};

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
    /// Reputation utilities
    Reputation {
        #[command(subcommand)]
        action: ReputationCmd,
    },
    /// Lookup gateway DNS verification status
    DnsLookup {
        /// Domain to query
        domain: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Manage network configuration
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Manage peer keys
    Key {
        #[command(subcommand)]
        action: KeyCmd,
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
        peer_id: Option<String>,
        /// Export all peers
        #[arg(long)]
        all: bool,
        /// Destination path
        #[arg(long)]
        path: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Persist metrics to disk
    Persist {
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Show handshake failure reasons for a peer
    Failures {
        /// Hex-encoded peer id
        peer_id: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Stream live metrics over WebSocket
    Watch {
        /// Hex-encoded peer id to filter; if omitted all peers are shown
        peer_id: Option<String>,
        /// WebSocket endpoint
        #[arg(long, default_value = "ws://127.0.0.1:3030/ws/peer_metrics")]
        ws: String,
    },
}

#[derive(Subcommand)]
enum ComputeCmd {
    /// Show scheduler statistics
    Stats {
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
        /// Show only effective price
        #[arg(long)]
        effective: bool,
    },
}

#[derive(Subcommand)]
enum ReputationCmd {
    /// Broadcast local reputation scores to peers
    Sync {
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Rotate the network key
    Rotate {
        /// Hex-encoded current peer id
        peer_id: String,
        /// Hex-encoded new public key
        new_key: String,
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Reload network configuration
    Reload {
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
}

fn post_json(rpc: &str, req: serde_json::Value) -> Result<serde_json::Value, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .post(rpc)
        .header(reqwest::header::CONNECTION, "close")
        .json(&req)
        .send()?
        .json()
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
                    match post_json(&rpc, req) {
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
                        Err(e) => eprintln!("request error: {e}"),
                    }
                } else if let Some(id) = peer_id {
                    let req = json!({
                        "method": "net.peer_stats",
                        "params": {"peer_id": id},
                    });
                    match post_json(&rpc, req) {
                        Ok(val) => {
                            let res = &val["result"];
                            let reqs = res["requests"].as_u64().unwrap_or(0);
                            let bytes = res["bytes_sent"].as_u64().unwrap_or(0);
                            let drops = res["drops"]
                                .as_object()
                                .map(|o| {
                                    o.iter()
                                        .map(|(k, v)| format!("{k}:{}", v.as_u64().unwrap_or(0)))
                                        .collect::<Vec<_>>()
                                        .join(",")
                                })
                                .unwrap_or_else(|| "".into());
                            let hf = res["handshake_fail"]
                                .as_object()
                                .map(|o| {
                                    o.iter()
                                        .map(|(k, v)| format!("{k}:{}", v.as_u64().unwrap_or(0)))
                                        .collect::<Vec<_>>()
                                        .join(",")
                                })
                                .unwrap_or_else(|| "".into());
                            println!(
                                "requests={reqs} bytes_sent={bytes} drops={drops} handshake_fail={hf}"
                            );
                        }
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
                match post_json(&rpc, req) {
                    Ok(val) => {
                        if val["result"]["status"].as_str() == Some("ok") {
                            println!("reset");
                        } else {
                            eprintln!("reset failed");
                        }
                    }
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
            StatsCmd::Reputation { peer_id, rpc } => {
                let req = json!({
                    "method": "net.peer_stats",
                    "params": {"peer_id": peer_id},
                });
                match post_json(&rpc, req) {
                    Ok(val) => {
                        let rep = val["result"]["reputation"].as_f64().unwrap_or(0.0);
                        println!("reputation={rep}");
                    }
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
            StatsCmd::Export {
                peer_id,
                all,
                path,
                rpc,
            } => {
                if !all && peer_id.is_none() {
                    eprintln!("peer_id required unless --all is specified");
                } else {
                    let params = if all {
                        json!({"path": path, "all": true})
                    } else {
                        json!({"peer_id": peer_id.unwrap(), "path": path})
                    };
                    let req = json!({
                        "method": "net.peer_stats_export",
                        "params": params,
                    });
                    match post_json(&rpc, req) {
                        Ok(val) => {
                            if val["result"]["status"].as_str() == Some("ok") {
                                if val["result"]["overwritten"].as_bool() == Some(true) {
                                    eprintln!("warning: overwrote existing file");
                                }
                                println!("exported");
                            } else {
                                eprintln!("export failed");
                            }
                        }
                        Err(e) => eprintln!("request error: {e}"),
                    }
                }
            }
            StatsCmd::Persist { rpc } => {
                let req = json!({ "method": "net.peer_stats_persist" });
                match post_json(&rpc, req) {
                    Ok(val) => {
                        if val["result"]["status"].as_str() == Some("ok") {
                            println!("persisted");
                        } else {
                            eprintln!("persist failed");
                        }
                    }
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
            StatsCmd::Failures { peer_id, rpc } => {
                let req = json!({
                    "method": "net.peer_stats",
                    "params": {"peer_id": peer_id},
                });
                match post_json(&rpc, req) {
                    Ok(val) => {
                        if let Some(obj) = val["result"]["handshake_fail"].as_object() {
                            for (k, v) in obj {
                                println!("{k}:{}", v.as_u64().unwrap_or(0));
                            }
                        }
                    }
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
            StatsCmd::Watch { peer_id, ws } => match connect(&ws) {
                Ok((mut socket, _)) => loop {
                    match socket.read_message() {
                        Ok(WsMessage::Text(txt)) => {
                            if let Ok(snap) = serde_json::from_str::<serde_json::Value>(&txt) {
                                if peer_id
                                    .as_ref()
                                    .map_or(true, |p| snap["peer_id"].as_str() == Some(p))
                                {
                                    println!("{}", txt);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                },
                Err(e) => eprintln!("ws connect error: {e}"),
            },
        },
        Command::Compute { action } => match action {
            ComputeCmd::Stats { rpc, effective } => {
                let req = json!({ "method": "compute_market.scheduler_stats" });
                match post_json(&rpc, req) {
                    Ok(val) => {
                        if effective {
                            if let Some(p) = val["result"]["effective_price"].as_u64() {
                                println!("{p}");
                            } else {
                                eprintln!("missing effective_price");
                            }
                        } else {
                            println!("{}", val);
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
        },
        Command::Reputation { action } => match action {
            ReputationCmd::Sync { rpc } => {
                let req = json!({ "method": "net.reputation_sync" });
                match post_json(&rpc, req) {
                    Ok(_) => println!("sync triggered"),
                    Err(e) => eprintln!("{e}"),
                }
            }
        },
        Command::Config { action } => match action {
            ConfigCmd::Reload { rpc } => {
                let req = json!({ "method": "net.config_reload" });
                match post_json(&rpc, req) {
                    Ok(_) => println!("reload triggered"),
                    Err(e) => eprintln!("{e}"),
                }
            }
        },
        Command::Key { action } => match action {
            KeyCmd::Rotate {
                peer_id,
                new_key,
                rpc,
            } => {
                if let Ok(bytes) = hex::decode(&new_key) {
                    let sk = load_net_key();
                    let sig = sk.sign(&bytes);
                    let req = json!({
                        "method": "net.key_rotate",
                        "params": {
                            "peer_id": peer_id,
                            "new_key": new_key,
                            "signature": hex::encode(sig.to_bytes()),
                        }
                    });
                    match post_json(&rpc, req) {
                        Ok(_) => println!("rotation complete"),
                        Err(e) => eprintln!("{e}"),
                    }
                } else {
                    eprintln!("invalid key");
                }
            }
        },
        Command::DnsLookup { domain, rpc } => {
            let body = json!({
                "method": "gateway.dns_lookup",
                "params": {"domain": domain},
            });
            match post_json(&rpc, body) {
                Ok(v) => println!("{}", v),
                Err(e) => eprintln!("{e}"),
            }
        }
    }
}
