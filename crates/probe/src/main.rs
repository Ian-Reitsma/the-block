use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use http_env::blocking_client as env_blocking_client;
use httpd::{BlockingClient, Method};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use thiserror::Error;

fn http_client() -> BlockingClient {
    env_blocking_client(&["TB_PROBE_TLS", "TB_HTTP_TLS"], "probe")
}

#[derive(Error, Debug)]
enum ProbeError {
    #[error("request failed: {0}")]
    Http(String),
    #[error("timeout")]
    Timeout,
    #[error("missing height in metrics output")]
    NoHeight,
    #[error("io: {0}")]
    Io(String),
}

fn main() {
    if let Err(err) = run_cli() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            CliError::Failure(err, prom) => {
                if prom {
                    println!("probe_success 0");
                }
                match err {
                    ProbeError::Timeout => std::process::exit(2),
                    other => {
                        eprintln!("{other}");
                        std::process::exit(1);
                    }
                }
            }
            CliError::Success(latency, prom) => {
                if prom {
                    println!(
                        "probe_success 1\nprobe_duration_seconds {}",
                        latency.as_secs_f64()
                    );
                }
                std::process::exit(0);
            }
        }
    }
}

enum CliError {
    Usage(String),
    Success(Duration, bool),
    Failure(ProbeError, bool),
}

fn run_cli() -> Result<(), CliError> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "probe".to_string());
    let args: Vec<String> = argv.collect();

    let command = build_command();
    if args.is_empty() {
        print_root_help(&command, &bin);
        return Ok(());
    }

    let parser = Parser::new(&command);
    let matches = match parser.parse(&args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(&command, &path);
            return Ok(());
        }
        Err(err) => return Err(CliError::Usage(err.to_string())),
    };

    let timeout = matches
        .get("timeout")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| CliError::Usage(err.to_string()))
        })
        .transpose()?;
    let timeout = Duration::from_secs(timeout.unwrap_or(5));
    let expect = matches
        .get("expect")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| CliError::Usage(err.to_string()))
        })
        .transpose()?;
    let prom = matches.get_flag("prom");

    match matches
        .subcommand()
        .ok_or_else(|| CliError::Usage("missing subcommand".into()))?
    {
        ("ping-rpc", sub) => {
            let url = sub
                .get_positional("url")
                .and_then(|vals| vals.first().cloned())
                .unwrap_or_else(|| "http://127.0.0.1:3050".to_string());
            match ping_rpc(&url, timeout, expect.unwrap_or(0)) {
                Ok(lat) => Err(CliError::Success(lat, prom)),
                Err(err) => Err(CliError::Failure(err, prom)),
            }
        }
        ("mine-one", sub) => {
            let url = sub
                .get_positional("url")
                .and_then(|vals| vals.first().cloned())
                .unwrap_or_else(|| "http://127.0.0.1:3050".to_string());
            let miner = sub
                .get_positional("miner")
                .and_then(|vals| vals.first().cloned())
                .unwrap_or_else(|| "miner".to_string());
            match mine_one(&url, &miner, timeout, expect.unwrap_or(0)) {
                Ok(lat) => Err(CliError::Success(lat, prom)),
                Err(err) => Err(CliError::Failure(err, prom)),
            }
        }
        ("gossip-check", sub) => {
            let addr = sub
                .get_positional("addr")
                .and_then(|vals| vals.first().cloned())
                .unwrap_or_else(|| "127.0.0.1:3030".to_string());
            match gossip_check(&addr, timeout) {
                Ok(lat) => Err(CliError::Success(lat, prom)),
                Err(err) => Err(CliError::Failure(err, prom)),
            }
        }
        ("tip", sub) => {
            let url = sub
                .get_positional("url")
                .and_then(|vals| vals.first().cloned())
                .unwrap_or_else(|| "http://127.0.0.1:3050".to_string());
            match tip(&url, expect.unwrap_or(0), timeout) {
                Ok(lat) => Err(CliError::Success(lat, prom)),
                Err(err) => Err(CliError::Failure(err, prom)),
            }
        }
        (other, _) => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
    }
}

fn build_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("probe"),
        "probe",
        "Synthetic health probe for The-Block nodes",
    )
    .arg(ArgSpec::Option(OptionSpec::new(
        "timeout",
        "timeout",
        "Request timeout in seconds",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "expect",
        "expect",
        "Expected latency/height delta",
    )))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "prom",
        "prom",
        "Emit Prometheus output",
    )))
    .subcommand(
        CommandBuilder::new(
            CommandId("probe.ping-rpc"),
            "ping-rpc",
            "Ping the JSON-RPC endpoint",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "url",
            "RPC endpoint URL",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("probe.mine-one"),
            "mine-one",
            "Mine blocks until tip height increases",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "url",
            "RPC endpoint URL",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "miner",
            "Miner identifier",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("probe.gossip-check"),
            "gossip-check",
            "Attempt to connect to the gossip port",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "addr",
            "Gossip address",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("probe.tip"),
            "tip",
            "Fetch current tip height via metrics",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "url",
            "RPC endpoint URL",
        )))
        .build(),
    )
    .build()
}

fn print_root_help(command: &CliCommand, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details.");
}

fn print_help_for_path(root: &CliCommand, path: &str) {
    let segments: Vec<&str> = path.split_whitespace().collect();
    if let Some(cmd) = find_command(root, &segments) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(root: &'a CliCommand, path: &[&str]) -> Option<&'a CliCommand> {
    if path.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in path.iter().skip(1) {
        if let Some(next) = current
            .subcommands
            .iter()
            .find(|command| command.name == *segment)
        {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
}

fn ping_rpc(url: &str, timeout: Duration, expect_ms: u64) -> Result<Duration, ProbeError> {
    let client = http_client();
    let start = Instant::now();
    let req = foundation_serialization::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "metrics",
        "params": {}
    });
    client
        .request(Method::Post, url)
        .and_then(|builder| builder.timeout(timeout).json(&req))
        .and_then(|builder| builder.send())
        .map_err(|e| ProbeError::Http(e.to_string()))?;
    let elapsed = start.elapsed();
    if expect_ms > 0 && elapsed > Duration::from_millis(expect_ms) {
        return Err(ProbeError::Timeout);
    }
    Ok(elapsed)
}

fn fetch_height(url: &str, client: &BlockingClient, timeout: Duration) -> Result<u64, ProbeError> {
    let req = foundation_serialization::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "metrics",
        "params": {}
    });
    let text = client
        .request(Method::Post, url)
        .and_then(|builder| builder.timeout(timeout).json(&req))
        .and_then(|builder| builder.send())
        .map_err(|e| ProbeError::Http(e.to_string()))?
        .text()
        .map_err(|e| ProbeError::Http(e.to_string()))?;
    for line in text.lines() {
        if let Some(val) = line.strip_prefix("block_height ") {
            return val
                .trim()
                .parse::<u64>()
                .map_err(|e| ProbeError::Http(e.to_string()));
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
    let client = http_client();
    let start_height = fetch_height(url, &client, timeout)?;
    let req = foundation_serialization::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "start_mining",
        "params": {"miner": miner}
    });
    client
        .request(Method::Post, url)
        .and_then(|builder| builder.timeout(timeout).json(&req))
        .and_then(|builder| builder.send())
        .map_err(|e| ProbeError::Http(e.to_string()))?;
    let start = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let h = fetch_height(url, &client, timeout)?;
        if h >= start_height + expect_delta.max(1) {
            let _ = client
                .request(Method::Post, url)
                .and_then(|builder| {
                    builder
                        .timeout(timeout)
                        .json(&foundation_serialization::json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "stop_mining",
                            "params": {}
                        }))
                })
                .and_then(|builder| builder.send());
            return Ok(start.elapsed());
        }
        if start.elapsed() > timeout {
            let _ = client
                .request(Method::Post, url)
                .and_then(|builder| {
                    builder
                        .timeout(timeout)
                        .json(&foundation_serialization::json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "stop_mining",
                            "params": {}
                        }))
                })
                .and_then(|builder| builder.send());
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
    let client = http_client();
    let h = fetch_height(url, &client, timeout)?;
    if expect > 0 && h < expect {
        return Err(ProbeError::Timeout);
    }
    println!("{h}");
    Ok(Duration::from_secs(0))
}
