use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use std::net::TcpStream;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Parser)]
#[command(author, version, about = "Synthetic health probe for The-Block nodes")]
struct Cli {
    #[arg(long)]
    timeout: Option<u64>,
    #[arg(long)]
    expect: Option<u64>,
    #[arg(long)]
    prom: bool,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ping the JSON-RPC endpoint
    PingRpc {
        #[arg(default_value = "http://127.0.0.1:3050")]
        url: String,
    },
    /// Mine blocks until tip height increases
    MineOne {
        #[arg(default_value = "http://127.0.0.1:3050")]
        url: String,
        #[arg(default_value = "miner")]
        miner: String,
    },
    /// Attempt to connect to the gossip port
    GossipCheck {
        #[arg(default_value = "127.0.0.1:3030")]
        addr: String,
    },
    /// Fetch current tip height via metrics
    Tip {
        #[arg(default_value = "http://127.0.0.1:3050")]
        url: String,
    },
}

#[derive(Error, Debug)]
enum ProbeError {
    #[error("request failed: {0}")]
    Reqwest(String),
    #[error("timeout")]
    Timeout,
    #[error("missing height in metrics output")]
    NoHeight,
    #[error("io: {0}")]
    Io(String),
}

fn main() {
    let cli = Cli::parse();
    let timeout = Duration::from_secs(cli.timeout.unwrap_or(5));
    let expect = cli.expect.unwrap_or(0);
    let res = match cli.cmd {
        Command::PingRpc { url } => ping_rpc(&url, timeout, expect),
        Command::MineOne { url, miner } => mine_one(&url, &miner, timeout, expect),
        Command::GossipCheck { addr } => gossip_check(&addr, timeout),
        Command::Tip { url } => tip(&url, expect, timeout),
    };
    match res {
        Ok(lat) => {
            if cli.prom {
                println!(
                    "probe_success 1\nprobe_duration_seconds {}",
                    lat.as_secs_f64()
                );
            }
            std::process::exit(0);
        }
        Err(ProbeError::Timeout) => {
            if cli.prom {
                println!("probe_success 0");
            }
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("{e}");
            if cli.prom {
                println!("probe_success 0");
            }
            std::process::exit(1);
        }
    }
}

fn ping_rpc(url: &str, timeout: Duration, expect_ms: u64) -> Result<Duration, ProbeError> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?;
    let start = Instant::now();
    let req = serde_json::json!({"jsonrpc":"2.0","id":0,"method":"metrics","params":{}});
    client
        .post(url)
        .json(&req)
        .send()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?;
    let elapsed = start.elapsed();
    if expect_ms > 0 && elapsed > Duration::from_millis(expect_ms) {
        return Err(ProbeError::Timeout);
    }
    Ok(elapsed)
}

fn fetch_height(url: &str, client: &Client) -> Result<u64, ProbeError> {
    let req = serde_json::json!({"jsonrpc":"2.0","id":0,"method":"metrics","params":{}});
    let text = client
        .post(url)
        .json(&req)
        .send()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?
        .text()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?;
    for line in text.lines() {
        if let Some(val) = line.strip_prefix("block_height ") {
            return val
                .trim()
                .parse::<u64>()
                .map_err(|e| ProbeError::Reqwest(e.to_string()));
        }
    }
    Err(ProbeError::NoHeight)
}

fn mine_one(
    url: &str,
    miner: &str,
    timeout: Duration,
    expect_delta: u64,
) -> Result<Duration, ProbeError> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?;
    let start_height = fetch_height(url, &client)?;
    let req = serde_json::json!({"jsonrpc":"2.0","id":0,"method":"start_mining","params":{"miner":miner}});
    client
        .post(url)
        .json(&req)
        .send()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?;
    let start = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let h = fetch_height(url, &client)?;
        if h >= start_height + expect_delta.max(1) {
            let _ = client
                .post(url)
                .json(
                    &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"stop_mining","params":{}}),
                )
                .send();
            return Ok(start.elapsed());
        }
        if start.elapsed() > timeout {
            let _ = client
                .post(url)
                .json(
                    &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"stop_mining","params":{}}),
                )
                .send();
            return Err(ProbeError::Timeout);
        }
    }
}

fn gossip_check(addr: &str, timeout: Duration) -> Result<Duration, ProbeError> {
    let socket: std::net::SocketAddr = addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| ProbeError::Io(e.to_string()))?;
    let start = Instant::now();
    TcpStream::connect_timeout(&socket, timeout).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            ProbeError::Timeout
        } else {
            ProbeError::Io(e.to_string())
        }
    })?;
    Ok(start.elapsed())
}

fn tip(url: &str, expect: u64, timeout: Duration) -> Result<Duration, ProbeError> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| ProbeError::Reqwest(e.to_string()))?;
    let h = fetch_height(url, &client)?;
    if expect > 0 && h < expect {
        return Err(ProbeError::Timeout);
    }
    println!("{h}");
    Ok(Duration::from_secs(0))
}
