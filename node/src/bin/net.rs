use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use colored::*;
use crypto_suite::signatures::Signer;
use hex;
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use terminal_size::{terminal_size, Width};
use the_block::net::load_net_key;
use tungstenite::{connect, protocol::Message as WsMessage};

#[derive(Copy, Clone, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

#[derive(Copy, Clone, ValueEnum)]
enum SortKey {
    Latency,
    DropRate,
    Reputation,
}

fn ratio(v: &serde_json::Value) -> f64 {
    let reqs = v["requests"].as_u64().unwrap_or(0);
    if reqs > 0 {
        v["drops"].as_u64().unwrap_or(0) as f64 / reqs as f64
    } else {
        0.0
    }
}

fn gettext(s: &str) -> String {
    s.to_string()
}

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
    /// Manage backpressure state
    Backpressure {
        #[command(subcommand)]
        action: BackpressureCmd,
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
    /// Generate shell completions
    Completions {
        /// Shell type
        #[arg(value_enum)]
        shell: Shell,
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
        /// Output format
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
        /// Show only peers with active backpressure
        #[arg(long)]
        backpressure: bool,
        /// Filter by drop reason
        #[arg(long)]
        drop_reason: Option<String>,
        /// Minimum reputation to include
        #[arg(long)]
        min_reputation: Option<f64>,
        /// Sort rows by field
        #[arg(long, value_enum)]
        sort_by: Option<SortKey>,
        /// Regex filter for peer id or address
        #[arg(long)]
        filter: Option<String>,
        /// Refresh interval in seconds
        #[arg(long)]
        watch: Option<u64>,
        /// Print only summary totals
        #[arg(long)]
        summary: bool,
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
        /// Minimum reputation to include
        #[arg(long)]
        min_reputation: Option<f64>,
        /// Only include peers active within this many seconds
        #[arg(long)]
        active_within: Option<u64>,
        /// Encrypt archive with age recipient
        #[arg(long)]
        age_recipient: Option<String>,
        /// Encrypt archive with OpenSSL using passphrase
        #[arg(long)]
        openssl_pass: Option<String>,
    },
    /// Persist metrics to disk
    Persist {
        /// RPC server address
        #[arg(long, default_value = "http://127.0.0.1:3030")]
        rpc: String,
    },
    /// Throttle or clear throttle for a peer
    Throttle {
        /// Hex-encoded peer id
        peer_id: String,
        /// Clear existing throttle
        #[arg(long)]
        clear: bool,
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
enum BackpressureCmd {
    /// Clear backpressure for a peer
    Clear {
        /// Hex-encoded peer id
        peer_id: String,
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
        .header(reqwest::header::HOST, "localhost")
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
                format,
                backpressure,
                drop_reason,
                min_reputation,
                sort_by,
                filter,
                watch,
                summary,
                rpc,
            } => {
                const DROP_ALERT: f64 = 0.1;
                let filter_re = filter.as_ref().and_then(|f| Regex::new(f).ok());
                let do_once = |peer_id: Option<String>| {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    if backpressure {
                        let mut off = offset;
                        let mut rows = Vec::new();
                        loop {
                            let req = json!({
                                "method": "net.peer_stats_all",
                                "params": {"offset": off, "limit": limit},
                            });
                            let val = match post_json(&rpc, req) {
                                Ok(v) => v,
                                Err(e) => {
                                    eprintln!("request error: {e}");
                                    std::process::exit(1);
                                }
                            };
                            let arr = val["result"].as_array().cloned().unwrap_or_default();
                            if arr.is_empty() {
                                break;
                            }
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            for entry in &arr {
                                let m = &entry["metrics"];
                                let until = m["throttled_until"].as_u64().unwrap_or(0);
                                if until > now {
                                    rows.push(json!({
                                        "peer": entry["peer_id"].as_str().unwrap_or(""),
                                        "reason": m["throttle_reason"].as_str().unwrap_or(""),
                                        "until": until,
                                    }));
                                }
                            }
                            off += arr.len();
                            if arr.len() < limit {
                                break;
                            }
                        }
                        match format {
                            OutputFormat::Json => {
                                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
                            }
                            OutputFormat::Table => {
                                for r in &rows {
                                    println!(
                                        "peer={} reason={} until={}",
                                        r["peer"].as_str().unwrap(),
                                        r["reason"].as_str().unwrap(),
                                        r["until"].as_u64().unwrap()
                                    );
                                }
                                println!("total_peers={}", rows.len());
                            }
                        }
                    } else if all {
                        let mut off = offset;
                        let mut total_req = 0u64;
                        let mut total_bytes = 0u64;
                        let mut total_drop = 0u64;
                        let mut active = 0u64;
                        let mut rows = Vec::new();
                        loop {
                            let req = json!({
                                "method": "net.peer_stats_all",
                                "params": {"offset": off, "limit": limit},
                            });
                            let val = match post_json(&rpc, req) {
                                Ok(v) => v,
                                Err(e) => {
                                    eprintln!("request error: {e}");
                                    std::process::exit(1);
                                }
                            };
                            let arr = val["result"].as_array().cloned().unwrap_or_default();
                            if arr.is_empty() {
                                break;
                            }
                            for entry in &arr {
                                let id = entry["peer_id"].as_str().unwrap_or("").to_string();
                                if let Some(ref re) = filter_re {
                                    if !re.is_match(&id) {
                                        continue;
                                    }
                                }
                                let m = &entry["metrics"];
                                let rep = m["reputation"]["score"].as_f64().unwrap_or(0.0);
                                if let Some(min) = min_reputation {
                                    if rep < min {
                                        continue;
                                    }
                                }
                                let drops_map = m["drops"].as_object();
                                if let Some(reason) = &drop_reason {
                                    if drops_map
                                        .and_then(|o| o.get(reason))
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        == 0
                                    {
                                        continue;
                                    }
                                }
                                let reqs = m["requests"].as_u64().unwrap_or(0);
                                let bytes = m["bytes_sent"].as_u64().unwrap_or(0);
                                let drops_total = drops_map
                                    .map(|o| o.values().filter_map(|v| v.as_u64()).sum())
                                    .unwrap_or(0);
                                let latency =
                                    now.saturating_sub(m["last_updated"].as_u64().unwrap_or(0));
                                if reqs > 0 {
                                    active += 1;
                                }
                                total_req += reqs;
                                total_bytes += bytes;
                                total_drop += drops_total;
                                rows.push(json!({
                                    "peer": id,
                                    "requests": reqs,
                                    "bytes_sent": bytes,
                                    "drops": drops_total,
                                    "reputation": rep,
                                    "latency": latency,
                                }));
                            }
                            off += arr.len();
                            if arr.len() < limit {
                                break;
                            }
                            if matches!(format, OutputFormat::Table) {
                                eprint!("-- more --");
                                let _ = std::io::stdin().read_line(&mut String::new());
                            }
                        }
                        if let Some(key) = sort_by {
                            rows.sort_by(|a, b| match key {
                                SortKey::Latency => {
                                    a["latency"].as_u64().cmp(&b["latency"].as_u64())
                                }
                                SortKey::DropRate => {
                                    let ar = ratio(a);
                                    let br = ratio(b);
                                    ar.partial_cmp(&br).unwrap_or(std::cmp::Ordering::Equal)
                                }
                                SortKey::Reputation => a["reputation"]
                                    .as_f64()
                                    .partial_cmp(&b["reputation"].as_f64())
                                    .unwrap_or(std::cmp::Ordering::Equal),
                            });
                        }
                        match format {
                            OutputFormat::Json => {
                                let out = json!({
                                    "peers": if summary { serde_json::Value::Array(vec![]) } else { serde_json::Value::Array(rows.clone()) },
                                    "summary": {
                                        "total_peers": rows.len(),
                                        "active": active,
                                        "requests": total_req,
                                        "bytes_sent": total_bytes,
                                        "drops": total_drop,
                                    }
                                });
                                println!("{}", serde_json::to_string_pretty(&out).unwrap());
                            }
                            OutputFormat::Table => {
                                let width = terminal_size()
                                    .map(|(Width(w), _)| w as usize)
                                    .unwrap_or(80);
                                let max_lat =
                                    rows.iter()
                                        .map(|r| r["latency"].as_u64().unwrap_or(0))
                                        .max()
                                        .unwrap_or(1) as f64;
                                let bars = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
                                if !summary {
                                    for r in &rows {
                                        let drop_ratio = ratio(r);
                                        let lat = r["latency"].as_u64().unwrap_or(0) as f64;
                                        let idx = ((lat / max_lat) * 7.0).round() as usize;
                                        let bar = bars[idx.min(7)];
                                        let line = format!(
                                            "peer={} req={} bytes={} drops={} rep={:.2} lat={}s {}",
                                            r["peer"].as_str().unwrap(),
                                            r["requests"].as_u64().unwrap_or(0),
                                            r["bytes_sent"].as_u64().unwrap_or(0),
                                            r["drops"].as_u64().unwrap_or(0),
                                            r["reputation"].as_f64().unwrap_or(0.0),
                                            r["latency"].as_u64().unwrap_or(0),
                                            bar
                                        );
                                        let line = if line.len() > width {
                                            &line[..width]
                                        } else {
                                            &line
                                        };
                                        if drop_ratio > DROP_ALERT {
                                            println!("{}", line.red());
                                        } else {
                                            println!("{line}");
                                        }
                                    }
                                }
                                let total_line = gettext("total_peers={total}")
                                    .replace("{total}", &rows.len().to_string());
                                println!("{}", total_line);
                                let summary_line = gettext(
                                "active={active} requests={requests} bytes_sent={bytes} drops={drops}"
                            )
                            .replace("{active}", &active.to_string())
                            .replace("{requests}", &total_req.to_string())
                            .replace("{bytes}", &total_bytes.to_string())
                            .replace("{drops}", &total_drop.to_string());
                                println!("{}", summary_line);
                            }
                        }
                    } else if let Some(id) = peer_id {
                        let req = json!({
                            "method": "net.peer_stats",
                            "params": {"peer_id": id},
                        });
                        match post_json(&rpc, req) {
                            Ok(val) => {
                                if let Some(err) = val.get("error") {
                                    let msg = err["message"].as_str().unwrap_or("");
                                    if msg.contains("unknown peer") {
                                        eprintln!("unknown peer");
                                        std::process::exit(2);
                                    } else if msg.contains("unauthorized") {
                                        eprintln!("unauthorized");
                                        std::process::exit(3);
                                    }
                                    eprintln!("{msg}");
                                    std::process::exit(1);
                                }
                                let res = &val["result"];
                                let rep = res["reputation"]["score"].as_f64().unwrap_or(0.0);
                                if let Some(min) = min_reputation {
                                    if rep < min {
                                        eprintln!("filtered");
                                        std::process::exit(1);
                                    }
                                }
                                if let Some(reason) = &drop_reason {
                                    let drops = res["drops"].as_object();
                                    if drops
                                        .and_then(|o| o.get(reason))
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        == 0
                                    {
                                        eprintln!("filtered");
                                        std::process::exit(1);
                                    }
                                }
                                match format {
                                    OutputFormat::Json => {
                                        println!("{}", serde_json::to_string_pretty(res).unwrap());
                                    }
                                    OutputFormat::Table => {
                                        let reqs = res["requests"].as_u64().unwrap_or(0);
                                        let bytes = res["bytes_sent"].as_u64().unwrap_or(0);
                                        let drops_total = res["drops"]
                                            .as_object()
                                            .map(|o| o.values().filter_map(|v| v.as_u64()).sum())
                                            .unwrap_or(0);
                                        let drop_ratio = if reqs > 0 {
                                            drops_total as f64 / reqs as f64
                                        } else {
                                            0.0
                                        };
                                        let thr = res["throttle_reason"].as_str().unwrap_or("");
                                        let until = res["throttled_until"].as_u64().unwrap_or(0);
                                        let line = format!(
                                        "requests={reqs} bytes_sent={bytes} drops={drops_total} reputation={:.2} throttle={} until={}",
                                        rep, thr, until
                                    );
                                        if drop_ratio > DROP_ALERT {
                                            println!("{}", line.red());
                                        } else {
                                            println!("{line}");
                                        }
                                        println!(
                                        "total_peers=1 active={} requests={reqs} bytes_sent={bytes} drops={drops_total}",
                                        if reqs > 0 { 1 } else { 0 }
                                    );
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("request error: {e}");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        eprintln!("{}", gettext("peer_id required unless --all is specified"));
                        std::process::exit(1);
                    }
                }; // end do_once
                if let Some(interval) = watch {
                    loop {
                        do_once(peer_id.clone());
                        sleep(Duration::from_secs(interval));
                    }
                } else {
                    do_once(peer_id);
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
                min_reputation: _,
                active_within: _,
                age_recipient,
                openssl_pass,
            } => {
                if all {
                    if age_recipient.is_some() && openssl_pass.is_some() {
                        eprintln!("cannot combine --age-recipient and --openssl-pass");
                        return;
                    }
                    let mut url = format!("{}/export/all", rpc);
                    if let Some(rec) = age_recipient {
                        url.push_str(&format!("?recipient={rec}"));
                    } else if let Some(pass) = openssl_pass {
                        url.push_str(&format!("?password={pass}"));
                    }
                    match Client::builder().timeout(Duration::from_secs(30)).build() {
                        Ok(client) => match client.get(&url).send() {
                            Ok(mut resp) => {
                                if resp.status().is_success() {
                                    if let Ok(mut file) = File::create(&path) {
                                        let total = resp.content_length().unwrap_or(0);
                                        let mut buf = [0u8; 8192];
                                        let mut downloaded = 0u64;
                                        loop {
                                            match resp.read(&mut buf) {
                                                Ok(0) => break,
                                                Ok(n) => {
                                                    downloaded += n as u64;
                                                    let _ = file.write_all(&buf[..n]);
                                                    if total > 0 {
                                                        let pct = downloaded * 100 / total;
                                                        eprint!("\r{}%", pct);
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("read error: {e}");
                                                    break;
                                                }
                                            }
                                        }
                                        eprintln!("");
                                        println!("exported");
                                    } else {
                                        eprintln!("failed to write file");
                                    }
                                } else {
                                    eprintln!("export failed");
                                }
                            }
                            Err(e) => eprintln!("request error: {e}"),
                        },
                        Err(e) => eprintln!("client error: {e}"),
                    }
                } else if peer_id.is_none() {
                    eprintln!("{}", gettext("peer_id required unless --all is specified"));
                } else {
                    let params = json!({"peer_id": peer_id.unwrap(), "path": path});
                    let req = json!({
                        "method": "net.peer_stats_export",
                        "params": params,
                    });
                    match post_json(&rpc, req) {
                        Ok(val) => {
                            if let Some(err) = val.get("error") {
                                let msg = err["message"].as_str().unwrap_or("export failed");
                                eprintln!("export failed: {msg}");
                            } else if val["result"]["status"].as_str() == Some("ok") {
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
            StatsCmd::Throttle {
                peer_id,
                clear,
                rpc,
            } => {
                let req = json!({
                    "method": "net.peer_throttle",
                    "params": { "peer_id": peer_id, "clear": clear },
                });
                match post_json(&rpc, req) {
                    Ok(val) => {
                        if val["result"]["status"].as_str() == Some("ok") {
                            if clear {
                                println!("cleared");
                            } else {
                                println!("throttled");
                            }
                        } else {
                            eprintln!("throttle failed");
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
                    match socket.read() {
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
        Command::Backpressure { action } => match action {
            BackpressureCmd::Clear { peer_id, rpc } => {
                let req = json!({
                    "method": "net.backpressure_clear",
                    "params": {"peer_id": peer_id},
                });
                match post_json(&rpc, req) {
                    Ok(val) => {
                        if val["result"]["status"].as_str() == Some("ok") {
                            println!("cleared");
                        } else {
                            eprintln!("clear failed");
                        }
                    }
                    Err(e) => eprintln!("request error: {e}"),
                }
            }
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
        Command::Completions { shell } => {
            use clap_complete::generate;
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "net", &mut std::io::stdout());
        }
    }
}
