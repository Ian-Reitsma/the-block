use colored::*;
use crypto_suite::signatures::Signer;
use foundation_serialization::json::{self, json, Value};
use hex;
use httpd::{BlockingClient, ClientError as HttpClientError, Method, Uri};
use regex::Regex;
use runtime::net::TcpStream;
use runtime::{
    self,
    ws::{self, ClientStream, Message as WsMessage},
};
use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use terminal_size::{terminal_size, Width};
use the_block::net::load_net_key;

use std::process;

use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};

mod cli_support;
use cli_support::{collect_args, parse_matches};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OutputFormat {
    Table,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            other => Err(format!("invalid format '{other}'")),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SortKey {
    Latency,
    DropRate,
    Reputation,
}

impl std::str::FromStr for SortKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "latency" => Ok(Self::Latency),
            "droprate" | "drop-rate" => Ok(Self::DropRate),
            "reputation" => Ok(Self::Reputation),
            other => Err(format!("invalid sort key '{other}'")),
        }
    }
}

#[derive(Clone, Debug)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

impl std::str::FromStr for CompletionShell {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            "fish" => Ok(Self::Fish),
            other => Err(format!("unsupported shell '{other}'")),
        }
    }
}

fn ratio(v: &Value) -> f64 {
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

const DEFAULT_RPC: &str = "http://127.0.0.1:3030";
const DEFAULT_WS: &str = "ws://127.0.0.1:3030/ws/peer_metrics";

#[derive(Debug)]
struct Cli {
    cmd: Command,
}

#[derive(Debug)]
enum Command {
    /// Inspect or reset per-peer metrics via RPC
    Stats { action: StatsCmd },
    /// Manage backpressure state
    Backpressure { action: BackpressureCmd },
    /// Compute marketplace utilities
    Compute { action: ComputeCmd },
    /// Reputation utilities
    Reputation { action: ReputationCmd },
    /// Lookup gateway DNS verification status
    DnsLookup {
        /// Domain to query
        domain: String,
        /// RPC server address
        rpc: String,
    },
    /// Manage network configuration
    Config { action: ConfigCmd },
    /// Manage peer keys
    Key { action: KeyCmd },
    /// Generate shell completions
    Completions {
        /// Shell type
        shell: CompletionShell,
    },
}

#[derive(Debug)]
enum StatsCmd {
    /// Show per-peer rate-limit metrics
    Show {
        /// Base58-check overlay peer id
        peer_id: Option<String>,
        /// Return stats for all peers
        all: bool,
        /// Pagination offset
        offset: usize,
        /// Pagination limit
        limit: usize,
        /// Output format
        format: OutputFormat,
        /// Show only peers with active backpressure
        backpressure: bool,
        /// Filter by drop reason
        drop_reason: Option<String>,
        /// Minimum reputation to include
        min_reputation: Option<f64>,
        /// Sort rows by field
        sort_by: Option<SortKey>,
        /// Regex filter for peer id or address
        filter: Option<String>,
        /// Refresh interval in seconds
        watch: Option<u64>,
        /// Print only summary totals
        summary: bool,
        /// RPC server address
        rpc: String,
    },
    /// Reset metrics for a peer
    Reset {
        /// Base58-check overlay peer id
        peer_id: String,
        /// RPC server address
        rpc: String,
    },
    /// Show reputation score for a peer
    Reputation {
        /// Base58-check overlay peer id
        peer_id: String,
        /// RPC server address
        rpc: String,
    },
    /// Export metrics for a peer to a file
    Export {
        /// Base58-check overlay peer id
        peer_id: Option<String>,
        /// Export all peers
        all: bool,
        /// Destination path
        path: String,
        /// RPC server address
        rpc: String,
        /// Minimum reputation to include
        min_reputation: Option<f64>,
        /// Only include peers active within this many seconds
        active_within: Option<u64>,
        /// Encrypt archive with the in-house envelope recipient
        recipient: Option<String>,
        /// Encrypt archive using a shared password
        password: Option<String>,
    },
    /// Persist metrics to disk
    Persist {
        /// RPC server address
        rpc: String,
    },
    /// Throttle or clear throttle for a peer
    Throttle {
        /// Base58-check overlay peer id
        peer_id: String,
        /// Clear existing throttle
        clear: bool,
        /// RPC server address
        rpc: String,
    },
    /// Show handshake failure reasons for a peer
    Failures {
        /// Base58-check overlay peer id
        peer_id: String,
        /// RPC server address
        rpc: String,
    },
    /// Stream live metrics over WebSocket
    Watch {
        /// Base58-check overlay peer id to filter; if omitted all peers are shown
        peer_id: Option<String>,
        /// WebSocket endpoint
        ws: String,
    },
}

#[derive(Debug)]
enum BackpressureCmd {
    /// Clear backpressure for a peer
    Clear {
        /// Base58-check overlay peer id
        peer_id: String,
        /// RPC server address
        rpc: String,
    },
}

#[derive(Debug)]
enum ComputeCmd {
    /// Show scheduler statistics
    Stats {
        /// RPC server address
        rpc: String,
        /// Show only effective price
        effective: bool,
    },
}

#[derive(Debug)]
enum ReputationCmd {
    /// Broadcast local reputation scores to peers
    Sync {
        /// RPC server address
        rpc: String,
    },
}

#[derive(Debug)]
enum KeyCmd {
    /// Rotate the network key
    Rotate {
        /// Hex-encoded current peer id
        peer_id: String,
        /// Hex-encoded new public key
        new_key: String,
        /// RPC server address
        rpc: String,
    },
}

#[derive(Debug)]
enum ConfigCmd {
    /// Reload network configuration
    Reload {
        /// RPC server address
        rpc: String,
    },
}

fn build_command() -> CliCommand {
    CommandBuilder::new(CommandId("net"), "net", "Network diagnostics utilities")
        .subcommand(build_stats_command())
        .subcommand(build_backpressure_command())
        .subcommand(build_compute_command())
        .subcommand(build_reputation_command())
        .subcommand(
            CommandBuilder::new(
                CommandId("net.dns_lookup"),
                "dns-lookup",
                "Lookup gateway DNS verification status",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "domain",
                "Domain to query",
            )))
            .arg(rpc_option("rpc", "rpc"))
            .build(),
        )
        .subcommand(build_config_command())
        .subcommand(build_key_command())
        .subcommand(build_completions_command())
        .build()
}

fn build_stats_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("net.stats"),
        "stats",
        "Inspect or reset per-peer metrics via RPC",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.show"),
            "show",
            "Show per-peer rate-limit metrics",
        )
        .arg(ArgSpec::Positional(
            PositionalSpec::new("peer_id", "Base58-check overlay peer id").optional(),
        ))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "all",
            "all",
            "Return stats for all peers",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("offset", "offset", "Pagination offset").default("0"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("limit", "limit", "Pagination limit").default("100"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("format", "format", "Output format")
                .default("table")
                .value_enum(&["table", "json"]),
        ))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "backpressure",
            "backpressure",
            "Show only peers with active backpressure",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "drop_reason",
            "drop-reason",
            "Filter by drop reason",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "min_reputation",
            "min-reputation",
            "Minimum reputation to include",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("sort_by", "sort-by", "Sort rows by field").value_enum(&[
                "latency",
                "drop-rate",
                "reputation",
            ]),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "filter",
            "filter",
            "Regex filter for peer id or address",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "watch",
            "watch",
            "Refresh interval in seconds",
        )))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "summary",
            "summary",
            "Print only summary totals",
        )))
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.reset"),
            "reset",
            "Reset metrics for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer_id",
            "Base58-check overlay peer id",
        )))
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.reputation"),
            "reputation",
            "Show reputation score for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer_id",
            "Base58-check overlay peer id",
        )))
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.export"),
            "export",
            "Export metrics for a peer to a file",
        )
        .arg(ArgSpec::Positional(
            PositionalSpec::new("peer_id", "Base58-check overlay peer id").optional(),
        ))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "all",
            "all",
            "Export all peers",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("path", "path", "Destination path").required(true),
        ))
        .arg(rpc_option("rpc", "rpc"))
        .arg(ArgSpec::Option(OptionSpec::new(
            "min_reputation",
            "min-reputation",
            "Minimum reputation to include",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "active_within",
            "active-within",
            "Only include peers active within this many seconds",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "recipient",
            "recipient",
            "Encrypt archive with the in-house envelope recipient",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "password",
            "password",
            "Encrypt archive using a shared password",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.persist"),
            "persist",
            "Persist metrics to disk",
        )
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.throttle"),
            "throttle",
            "Throttle or clear throttle for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer_id",
            "Base58-check overlay peer id",
        )))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "clear",
            "clear",
            "Clear existing throttle",
        )))
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.failures"),
            "failures",
            "Show handshake failure reasons for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer_id",
            "Base58-check overlay peer id",
        )))
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.stats.watch"),
            "watch",
            "Stream live metrics over WebSocket",
        )
        .arg(ArgSpec::Positional(
            PositionalSpec::new(
                "peer_id",
                "Base58-check overlay peer id to filter; if omitted all peers are shown",
            )
            .optional(),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("ws", "ws", "WebSocket endpoint").default(DEFAULT_WS),
        ))
        .build(),
    )
    .build()
}

fn build_backpressure_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("net.backpressure"),
        "backpressure",
        "Manage backpressure state",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.backpressure.clear"),
            "clear",
            "Clear backpressure for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer_id",
            "Base58-check overlay peer id",
        )))
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .build()
}

fn build_compute_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("net.compute"),
        "compute",
        "Compute marketplace utilities",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.compute.stats"),
            "stats",
            "Show scheduler statistics",
        )
        .arg(rpc_option("rpc", "rpc"))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "effective",
            "effective",
            "Show only effective price",
        )))
        .build(),
    )
    .build()
}

fn build_reputation_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("net.reputation"),
        "reputation",
        "Reputation utilities",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.reputation.sync"),
            "sync",
            "Broadcast local reputation scores to peers",
        )
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .build()
}

fn build_config_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("net.config"),
        "config",
        "Manage network configuration",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("net.config.reload"),
            "reload",
            "Reload network configuration",
        )
        .arg(rpc_option("rpc", "rpc"))
        .build(),
    )
    .build()
}

fn build_key_command() -> CliCommand {
    CommandBuilder::new(CommandId("net.key"), "key", "Manage peer keys")
        .subcommand(
            CommandBuilder::new(
                CommandId("net.key.rotate"),
                "rotate",
                "Rotate the network key",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "peer_id",
                "Hex-encoded current peer id",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "new_key",
                "Hex-encoded new public key",
            )))
            .arg(rpc_option("rpc", "rpc"))
            .build(),
        )
        .build()
}

fn build_completions_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("net.completions"),
        "completions",
        "Generate shell completions",
    )
    .arg(ArgSpec::Positional(PositionalSpec::new(
        "shell",
        "Shell type (bash|zsh|fish)",
    )))
    .build()
}

fn rpc_option(name: &'static str, long: &'static str) -> ArgSpec {
    ArgSpec::Option(OptionSpec::new(name, long, "RPC server address").default(DEFAULT_RPC))
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let cmd = match sub {
        "stats" => Command::Stats {
            action: parse_stats(sub_matches)?,
        },
        "backpressure" => Command::Backpressure {
            action: parse_backpressure(sub_matches)?,
        },
        "compute" => Command::Compute {
            action: parse_compute(sub_matches)?,
        },
        "reputation" => Command::Reputation {
            action: parse_reputation(sub_matches)?,
        },
        "dns-lookup" => Command::DnsLookup {
            domain: require_positional(sub_matches, "domain")?,
            rpc: sub_matches
                .get_string("rpc")
                .unwrap_or_else(|| DEFAULT_RPC.to_string()),
        },
        "config" => Command::Config {
            action: parse_config(sub_matches)?,
        },
        "key" => Command::Key {
            action: parse_key(sub_matches)?,
        },
        "completions" => Command::Completions {
            shell: require_positional(sub_matches, "shell")?.parse()?,
        },
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(Cli { cmd })
}

fn parse_stats(matches: &Matches) -> Result<StatsCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing stats command".to_string())?;

    match sub {
        "show" => parse_stats_show(sub_matches),
        "reset" => Ok(StatsCmd::Reset {
            peer_id: require_positional(sub_matches, "peer_id")?,
            rpc: rpc_value(sub_matches),
        }),
        "reputation" => Ok(StatsCmd::Reputation {
            peer_id: require_positional(sub_matches, "peer_id")?,
            rpc: rpc_value(sub_matches),
        }),
        "export" => parse_stats_export(sub_matches),
        "persist" => Ok(StatsCmd::Persist {
            rpc: rpc_value(sub_matches),
        }),
        "throttle" => Ok(StatsCmd::Throttle {
            peer_id: require_positional(sub_matches, "peer_id")?,
            clear: sub_matches.get_flag("clear"),
            rpc: rpc_value(sub_matches),
        }),
        "failures" => Ok(StatsCmd::Failures {
            peer_id: require_positional(sub_matches, "peer_id")?,
            rpc: rpc_value(sub_matches),
        }),
        "watch" => Ok(StatsCmd::Watch {
            peer_id: optional_positional(sub_matches, "peer_id"),
            ws: sub_matches
                .get_string("ws")
                .unwrap_or_else(|| DEFAULT_WS.to_string()),
        }),
        other => Err(format!("unknown stats command '{other}'")),
    }
}

fn parse_stats_show(matches: &Matches) -> Result<StatsCmd, String> {
    let peer_id = optional_positional(matches, "peer_id");
    let offset = parse_usize(matches, "offset", 0)?;
    let limit = parse_usize(matches, "limit", 100)?;
    let format = matches
        .get_string("format")
        .unwrap_or_else(|| "table".to_string())
        .parse::<OutputFormat>()?;
    let sort_by = matches
        .get_string("sort_by")
        .map(|value| value.parse::<SortKey>())
        .transpose()?;
    let min_reputation = matches
        .get_string("min_reputation")
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|err| format!("invalid min-reputation: {err}"))
        })
        .transpose()?;
    let watch = matches
        .get_string("watch")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| format!("invalid watch interval: {err}"))
        })
        .transpose()?;

    Ok(StatsCmd::Show {
        peer_id,
        all: matches.get_flag("all"),
        offset,
        limit,
        format,
        backpressure: matches.get_flag("backpressure"),
        drop_reason: matches.get_string("drop_reason"),
        min_reputation,
        sort_by,
        filter: matches.get_string("filter"),
        watch,
        summary: matches.get_flag("summary"),
        rpc: rpc_value(matches),
    })
}

fn parse_stats_export(matches: &Matches) -> Result<StatsCmd, String> {
    let min_reputation = matches
        .get_string("min_reputation")
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|err| format!("invalid min-reputation: {err}"))
        })
        .transpose()?;
    let active_within = matches
        .get_string("active_within")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| format!("invalid active-within: {err}"))
        })
        .transpose()?;

    Ok(StatsCmd::Export {
        peer_id: optional_positional(matches, "peer_id"),
        all: matches.get_flag("all"),
        path: matches
            .get_string("path")
            .ok_or_else(|| "missing --path".to_string())?,
        rpc: rpc_value(matches),
        min_reputation,
        active_within,
        recipient: matches.get_string("recipient"),
        password: matches.get_string("password"),
    })
}

fn parse_backpressure(matches: &Matches) -> Result<BackpressureCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing backpressure command".to_string())?;

    match sub {
        "clear" => Ok(BackpressureCmd::Clear {
            peer_id: require_positional(sub_matches, "peer_id")?,
            rpc: rpc_value(sub_matches),
        }),
        other => Err(format!("unknown backpressure command '{other}'")),
    }
}

fn parse_compute(matches: &Matches) -> Result<ComputeCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing compute command".to_string())?;

    match sub {
        "stats" => Ok(ComputeCmd::Stats {
            rpc: rpc_value(sub_matches),
            effective: sub_matches.get_flag("effective"),
        }),
        other => Err(format!("unknown compute command '{other}'")),
    }
}

fn parse_reputation(matches: &Matches) -> Result<ReputationCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing reputation command".to_string())?;

    match sub {
        "sync" => Ok(ReputationCmd::Sync {
            rpc: rpc_value(sub_matches),
        }),
        other => Err(format!("unknown reputation command '{other}'")),
    }
}

fn parse_config(matches: &Matches) -> Result<ConfigCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing config command".to_string())?;

    match sub {
        "reload" => Ok(ConfigCmd::Reload {
            rpc: rpc_value(sub_matches),
        }),
        other => Err(format!("unknown config command '{other}'")),
    }
}

fn parse_key(matches: &Matches) -> Result<KeyCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing key command".to_string())?;

    match sub {
        "rotate" => Ok(KeyCmd::Rotate {
            peer_id: require_positional(sub_matches, "peer_id")?,
            new_key: require_positional(sub_matches, "new_key")?,
            rpc: rpc_value(sub_matches),
        }),
        other => Err(format!("unknown key command '{other}'")),
    }
}

fn rpc_value(matches: &Matches) -> String {
    matches
        .get_string("rpc")
        .unwrap_or_else(|| DEFAULT_RPC.to_string())
}

fn require_positional(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| format!("missing argument '{name}'"))
}

fn optional_positional(matches: &Matches, name: &str) -> Option<String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
}

fn parse_usize(matches: &Matches, name: &str, default: usize) -> Result<usize, String> {
    matches
        .get_string(name)
        .unwrap_or_else(|| default.to_string())
        .parse::<usize>()
        .map_err(|err| format!("invalid {name}: {err}"))
}

fn top_level_commands() -> Vec<&'static str> {
    vec![
        "stats",
        "backpressure",
        "compute",
        "reputation",
        "dns-lookup",
        "config",
        "key",
        "completions",
    ]
}

fn post_json(rpc: &str, req: Value) -> Result<Value, HttpClientError> {
    BlockingClient::default()
        .request(Method::Post, rpc)?
        .timeout(Duration::from_secs(5))
        .header("host", "localhost")
        .header("connection", "close")
        .json(&req)?
        .send()?
        .json()
}

async fn connect_peer_metrics_ws(url: &str) -> Result<ClientStream, String> {
    let parsed = Uri::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "ws" {
        return Err(format!("unsupported scheme {}", parsed.scheme()));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "missing host in websocket url".to_string())?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let host_owned = host.to_string();
    let addrs = runtime::spawn_blocking(move || {
        (host_owned.as_str(), port)
            .to_socket_addrs()
            .map(|iter| iter.collect::<Vec<_>>())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let addr = addrs
        .into_iter()
        .next()
        .ok_or_else(|| "no addresses resolved for websocket host".to_string())?;
    let mut stream = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;

    let key = ws::handshake_key();
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    let default_port = 80;
    let host_header = if port == default_port {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let expected_accept = ws::handshake_accept(&key).map_err(|err| err.to_string())?;
    ws::read_client_handshake(&mut stream, &expected_accept)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ClientStream::new(stream))
}

fn main() {
    let command = build_command();
    let (bin, args) = collect_args("net");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return,
    };

    let cli = match build_cli(matches) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };
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
                                println!("{}", json::to_string_pretty(&rows).unwrap());
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
                                    "peers": if summary { Value::Array(vec![]) } else { Value::Array(rows.clone()) },
                                    "summary": {
                                        "total_peers": rows.len(),
                                        "active": active,
                                        "requests": total_req,
                                        "bytes_sent": total_bytes,
                                        "drops": total_drop,
                                    }
                                });
                                println!("{}", json::to_string_pretty(&out).unwrap());
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
                                        println!("{}", json::to_string_pretty(res).unwrap());
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
                recipient,
                password,
            } => {
                if all {
                    if recipient.is_some() && password.is_some() {
                        eprintln!("cannot combine --recipient and --password");
                        return;
                    }
                    let mut url = format!("{}/export/all", rpc);
                    if let Some(rec) = recipient {
                        url.push_str(&format!("?recipient={rec}"));
                    } else if let Some(pass) = password {
                        url.push_str(&format!("?password={pass}"));
                    }
                    match BlockingClient::default().request(Method::Get, &url) {
                        Ok(builder) => match builder.timeout(Duration::from_secs(30)).send() {
                            Ok(resp) => {
                                if resp.status().is_success() {
                                    match File::create(&path) {
                                        Ok(mut file) => {
                                            let body = resp.into_body();
                                            if file.write_all(&body).is_ok() {
                                                println!("exported");
                                            } else {
                                                eprintln!("failed to write file");
                                            }
                                        }
                                        Err(e) => eprintln!("failed to create file: {e}"),
                                    }
                                } else {
                                    eprintln!("export failed");
                                }
                            }
                            Err(e) => eprintln!("request error: {e}"),
                        },
                        Err(e) => eprintln!("request error: {e}"),
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
            StatsCmd::Watch { peer_id, ws } => {
                runtime::block_on(async move {
                    match connect_peer_metrics_ws(&ws).await {
                        Ok(mut socket) => {
                            loop {
                                match socket.recv().await {
                                    Ok(Some(WsMessage::Text(txt))) => {
                                        if let Ok(snap) = json::from_str::<Value>(&txt) {
                                            if peer_id.as_ref().map_or(true, |p| {
                                                snap["peer_id"].as_str() == Some(p)
                                            }) {
                                                println!("{}", txt);
                                            }
                                        }
                                    }
                                    Ok(Some(WsMessage::Close(_))) | Ok(None) => break,
                                    Ok(Some(_)) => {}
                                    Err(err) => {
                                        eprintln!("ws error: {err}");
                                        break;
                                    }
                                }
                            }
                            let _ = socket.close().await;
                        }
                        Err(err) => eprintln!("ws connect error: {err}"),
                    }
                });
            }
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
        Command::Completions { shell } => match shell {
            CompletionShell::Bash => {
                println!("{}_completion() {{", "net");
                println!("    local cur prev");
                println!("    COMPREPLY=()");
                println!("    cur=\"${{COMP_WORDS[COMP_CWORD]}}\"");
                println!("    prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"");
                println!("    if [[ $COMP_CWORD -eq 1 ]]; then");
                println!(
                    "        COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                    top_level_commands().join(" "),
                );
                println!("    fi");
                println!("    return 0");
                println!("}}");
                println!("complete -F {}_completion net", "net");
            }
            CompletionShell::Zsh => {
                println!("#compdef net");
                println!("_arguments '1: :({})'", top_level_commands().join(" "));
            }
            CompletionShell::Fish => {
                for name in top_level_commands() {
                    println!("complete -c net -f -n '__fish_use_subcommand' -a {}", name);
                }
            }
        },
    }
}
