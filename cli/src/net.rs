use crate::parse_utils::{require_positional, take_string};
use crate::{
    codec_helpers::{json_from_str, json_to_string_pretty},
    rpc::RpcClient,
};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use hex;
use serde::Deserialize;
use serde_json::{json, Value};
use the_block::net::{PeerCertHistoryEntry, QuicStatsEntry};

#[derive(Deserialize)]
struct RpcEnvelope<T> {
    result: T,
}

#[derive(Deserialize)]
struct OverlayStatusView {
    backend: String,
    active_peers: usize,
    persisted_peers: usize,
    #[serde(default)]
    database_path: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayOutputFormat {
    Plain,
    Json,
}

impl Default for OverlayOutputFormat {
    fn default() -> Self {
        OverlayOutputFormat::Plain
    }
}

pub enum NetCmd {
    /// Reputation operations
    Reputation { action: ReputationCmd },
    /// DNS operations
    Dns { action: DnsCmd },
    /// Rotate a peer's public key
    RotateKey {
        peer_id: String,
        new_key: String,
        url: String,
    },
    /// Rotate the local QUIC certificate
    RotateCert { url: String },
    /// Rebate operations
    Rebate { action: RebateCmd },
    /// QUIC diagnostics
    Quic { action: QuicCmd },
    /// Display QUIC peer statistics
    QuicStats {
        url: String,
        token: Option<String>,
        json: bool,
    },
    /// Show gossip relay configuration and shard affinity
    GossipStatus { url: String, json: bool },
    /// Show overlay backend and persistence status
    OverlayStatus {
        url: String,
        format: Option<OverlayOutputFormat>,
        json: bool,
    },
}

pub enum ReputationCmd {
    /// Show reputation for a peer
    Show { peer: String, url: String },
}

pub enum DnsCmd {
    /// Verify DNS TXT record for a domain
    Verify { domain: String, url: String },
}

pub enum QuicCmd {
    /// Show recent handshake failures
    Failures { url: String },
    /// Inspect cached QUIC certificate history
    History { url: String, json: bool },
    /// Reload the QUIC certificate cache from disk
    Refresh { url: String },
}

pub enum RebateCmd {
    /// Claim rebate voucher for a peer
    Claim {
        peer: String,
        threshold: u64,
        epoch: u64,
        reward: u64,
        url: String,
    },
}

impl OverlayOutputFormat {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "plain" => Some(OverlayOutputFormat::Plain),
            "json" => Some(OverlayOutputFormat::Json),
            _ => None,
        }
    }
}

impl NetCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("net"), "net", "Networking utilities")
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.reputation"),
                    "reputation",
                    "Reputation operations",
                )
                .subcommand(ReputationCmd::command())
                .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("net.dns"), "dns", "DNS operations")
                    .subcommand(DnsCmd::command())
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.rotate_key"),
                    "rotate-key",
                    "Rotate a peer's public key",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "peer_id",
                    "Peer identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "new_key",
                    "New public key",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.rotate_cert"),
                    "rotate-cert",
                    "Rotate the local QUIC certificate",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("net.rebate"), "rebate", "Rebate operations")
                    .subcommand(RebateCmd::command())
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("net.quic"), "quic", "QUIC diagnostics")
                    .subcommand(QuicCmd::command())
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.quic_stats"),
                    "quic-stats",
                    "Display QUIC peer statistics",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "token",
                    "token",
                    "Bearer token for authorization",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.gossip_status"),
                    "gossip-status",
                    "Show gossip relay configuration and shard affinity",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.overlay_status"),
                    "overlay-status",
                    "Show overlay backend and persistence status",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "format",
                    "format",
                    "Output format (plain/json)",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'net'".to_string())?;

        match name {
            "reputation" => {
                let action = ReputationCmd::from_matches(sub_matches)?;
                Ok(NetCmd::Reputation { action })
            }
            "dns" => {
                let action = DnsCmd::from_matches(sub_matches)?;
                Ok(NetCmd::Dns { action })
            }
            "rotate-key" => {
                let peer_id = require_positional(sub_matches, "peer_id")?;
                let new_key = require_positional(sub_matches, "new_key")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(NetCmd::RotateKey {
                    peer_id,
                    new_key,
                    url,
                })
            }
            "rotate-cert" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(NetCmd::RotateCert { url })
            }
            "rebate" => {
                let action = RebateCmd::from_matches(sub_matches)?;
                Ok(NetCmd::Rebate { action })
            }
            "quic" => {
                let action = QuicCmd::from_matches(sub_matches)?;
                Ok(NetCmd::Quic { action })
            }
            "quic-stats" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let token = take_string(sub_matches, "token");
                let json = sub_matches.get_flag("json");
                Ok(NetCmd::QuicStats { url, token, json })
            }
            "gossip-status" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let json = sub_matches.get_flag("json");
                Ok(NetCmd::GossipStatus { url, json })
            }
            "overlay-status" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let format = match take_string(sub_matches, "format") {
                    Some(value) => {
                        let lowered = value.to_ascii_lowercase();
                        match OverlayOutputFormat::parse(&lowered) {
                            Some(fmt) => Some(fmt),
                            None => {
                                return Err(format!(
                                    "invalid value '{value}' for '--format': expected plain or json"
                                ))
                            }
                        }
                    }
                    None => None,
                };
                let json = sub_matches.get_flag("json");
                Ok(NetCmd::OverlayStatus { url, format, json })
            }
            other => Err(format!("unknown subcommand '{other}' for 'net'")),
        }
    }
}

impl ReputationCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("net.reputation.show"),
            "show",
            "Show reputation for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer",
            "Peer identifier",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
        ))
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let peer = require_positional(matches, "peer")?;
        let url =
            take_string(matches, "url").unwrap_or_else(|| "http://localhost:26658".to_string());
        Ok(ReputationCmd::Show { peer, url })
    }
}

impl DnsCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("net.dns.verify"),
            "verify",
            "Verify DNS TXT record for a domain",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "domain",
            "Domain name",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
        ))
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let domain = require_positional(matches, "domain")?;
        let url =
            take_string(matches, "url").unwrap_or_else(|| "http://localhost:26658".to_string());
        Ok(DnsCmd::Verify { domain, url })
    }
}

impl QuicCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("net.quic.root"), "quic", "QUIC diagnostics")
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.quic.failures"),
                    "failures",
                    "Show recent handshake failures",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.quic.history"),
                    "history",
                    "Inspect cached QUIC certificate history",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("net.quic.refresh"),
                    "refresh",
                    "Reload the QUIC certificate cache from disk",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<QuicCmd, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'net quic'".to_string())?;

        match name {
            "failures" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(QuicCmd::Failures { url })
            }
            "history" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let json = sub_matches.get_flag("json");
                Ok(QuicCmd::History { url, json })
            }
            "refresh" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(QuicCmd::Refresh { url })
            }
            other => Err(format!("unknown subcommand '{other}' for 'net quic'")),
        }
    }
}

impl RebateCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("net.rebate.claim"),
            "claim",
            "Claim rebate voucher for a peer",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "peer",
            "Peer identifier",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "threshold",
            "Threshold value",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "epoch",
            "Epoch number",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "reward",
            "Reward amount",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
        ))
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let peer = require_positional(matches, "peer")?;
        let threshold_raw = require_positional(matches, "threshold")?;
        let threshold = threshold_raw
            .parse::<u64>()
            .map_err(|_| format!("invalid value '{threshold_raw}' for 'threshold'"))?;
        let epoch = Self::parse_positional(matches, "epoch")?;
        let reward = Self::parse_positional(matches, "reward")?;
        let url =
            take_string(matches, "url").unwrap_or_else(|| "http://localhost:26658".to_string());
        Ok(RebateCmd::Claim {
            peer,
            threshold,
            epoch,
            reward,
            url,
        })
    }

    fn parse_positional(matches: &Matches, name: &str) -> Result<u64, String> {
        let value = require_positional(matches, name)?;
        value
            .parse::<u64>()
            .map_err(|_| format!("invalid value '{value}' for '{name}'"))
    }
}

pub fn handle(cmd: NetCmd) {
    match cmd {
        NetCmd::Reputation { action } => match action {
            ReputationCmd::Show { peer, url } => {
                let client = RpcClient::from_env();
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    jsonrpc: &'static str,
                    id: u32,
                    method: &'static str,
                    params: serde_json::Value,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auth: Option<&'a str>,
                }
                let payload = Payload {
                    jsonrpc: "2.0",
                    id: 1,
                    method: "net.reputation_show",
                    params: json!({"peer": peer}),
                    auth: None,
                };
                if let Ok(resp) = client.call(&url, &payload) {
                    if let Ok(text) = resp.text() {
                        println!("{}", text);
                    }
                }
            }
        },
        NetCmd::Dns { action } => match action {
            DnsCmd::Verify { domain, url } => {
                let client = RpcClient::from_env();
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    jsonrpc: &'static str,
                    id: u32,
                    method: &'static str,
                    params: serde_json::Value,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auth: Option<&'a str>,
                }
                let payload = Payload {
                    jsonrpc: "2.0",
                    id: 1,
                    method: "net.dns_verify",
                    params: json!({"domain": domain}),
                    auth: None,
                };
                if let Ok(resp) = client.call(&url, &payload) {
                    if let Ok(text) = resp.text() {
                        println!("{}", text);
                    }
                }
            }
        },
        NetCmd::RotateKey {
            peer_id,
            new_key,
            url,
        } => {
            let sk = the_block::net::load_net_key();
            let new_bytes = hex::decode(&new_key).expect("invalid new key hex");
            let sig = sk.sign(&new_bytes);
            let client = RpcClient::from_env();
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "net.key_rotate",
                params: json!({
                    "peer_id": peer_id,
                    "new_key": new_key,
                    "signature": hex::encode(sig.to_bytes()),
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        NetCmd::Quic { action } => match action {
            QuicCmd::Failures { url } => {
                let client = RpcClient::from_env();
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    jsonrpc: &'static str,
                    id: u32,
                    method: &'static str,
                    params: serde_json::Value,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auth: Option<&'a str>,
                }
                let payload = Payload {
                    jsonrpc: "2.0",
                    id: 1,
                    method: "net.handshake_failures",
                    params: serde_json::Value::Null,
                    auth: None,
                };
                if let Ok(resp) = client.call(&url, &payload) {
                    if let Ok(text) = resp.text() {
                        println!("{}", text);
                    }
                }
            }
            QuicCmd::History { url, json } => {
                let client = RpcClient::from_env();
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    jsonrpc: &'static str,
                    id: u32,
                    method: &'static str,
                    params: serde_json::Value,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auth: Option<&'a str>,
                }
                #[derive(Deserialize)]
                struct Envelope<T> {
                    result: T,
                }
                let payload = Payload {
                    jsonrpc: "2.0",
                    id: 1,
                    method: "net.quic_certs",
                    params: serde_json::Value::Null,
                    auth: None,
                };
                if let Ok(resp) = client.call(&url, &payload) {
                    if json {
                        if let Ok(data) = resp.json::<Envelope<Vec<PeerCertHistoryEntry>>>() {
                            if let Ok(text) = json_to_string_pretty(&data.result) {
                                println!("{}", text);
                            }
                        }
                    } else if let Ok(data) = resp.json::<Envelope<Vec<PeerCertHistoryEntry>>>() {
                        print_quic_cert_history(&data.result);
                    }
                }
            }
            QuicCmd::Refresh { url } => {
                let client = RpcClient::from_env();
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    jsonrpc: &'static str,
                    id: u32,
                    method: &'static str,
                    params: serde_json::Value,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auth: Option<&'a str>,
                }
                let payload = Payload {
                    jsonrpc: "2.0",
                    id: 1,
                    method: "net.quic_certs_refresh",
                    params: serde_json::Value::Null,
                    auth: None,
                };
                if let Ok(resp) = client.call(&url, &payload) {
                    if let Ok(text) = resp.text() {
                        println!("{}", text);
                    }
                }
            }
        },
        NetCmd::QuicStats { url, token, json } => {
            let client = RpcClient::from_env();
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            #[derive(Deserialize)]
            struct Envelope<T> {
                result: T,
            }
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "net.quic_stats",
                params: serde_json::Value::Null,
                auth: token.as_deref(),
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(data) = resp.json::<Envelope<Vec<QuicStatsEntry>>>() {
                    if json {
                        if let Ok(text) = json_to_string_pretty(&data.result) {
                            println!("{}", text);
                        }
                    } else {
                        print_quic_stats(&data.result);
                    }
                }
            }
        }
        NetCmd::GossipStatus { url, json } => {
            let client = RpcClient::from_env();
            #[derive(serde::Serialize)]
            struct Payload {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
            }
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "net.gossip_status",
                params: json!({}),
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    if json {
                        if let Ok(val) = json_from_str::<Value>(&text) {
                            let out = val.get("result").cloned().unwrap_or(val);
                            if let Ok(pretty) = json_to_string_pretty(&out) {
                                println!("{}", pretty);
                            } else {
                                println!("{}", text);
                            }
                        } else {
                            println!("{}", text);
                        }
                    } else if let Ok(val) = json_from_str::<Value>(&text) {
                        let result = val.get("result").cloned().unwrap_or(val);
                        print_gossip_status(&result);
                    } else {
                        println!("{}", text);
                    }
                }
            }
        }
        NetCmd::OverlayStatus { url, json, format } => {
            let client = RpcClient::from_env();
            #[derive(serde::Serialize)]
            struct Payload {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
            }
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "net.overlay_status",
                params: serde_json::Value::Null,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    let output = format.unwrap_or_else(|| {
                        if json {
                            OverlayOutputFormat::Json
                        } else {
                            OverlayOutputFormat::Plain
                        }
                    });

                    if output == OverlayOutputFormat::Json {
                        if let Ok(val) = json_from_str::<Value>(&text) {
                            let out = val.get("result").cloned().unwrap_or(val);
                            if let Ok(pretty) = json_to_string_pretty(&out) {
                                println!("{}", pretty);
                            } else {
                                println!("{}", text);
                            }
                        } else {
                            println!("{}", text);
                        }
                    } else {
                        match json_from_str::<RpcEnvelope<OverlayStatusView>>(&text) {
                            Ok(env) => print_overlay_status(&env.result),
                            Err(_) => println!("{}", text),
                        }
                    }
                }
            }
        }
        NetCmd::RotateCert { url } => {
            let client = RpcClient::from_env();
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "net.rotate_cert",
                params: serde_json::Value::Null,
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        NetCmd::Rebate { action } => match action {
            RebateCmd::Claim {
                peer,
                threshold,
                epoch,
                reward,
                url,
            } => {
                let client = RpcClient::from_env();
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    jsonrpc: &'static str,
                    id: u32,
                    method: &'static str,
                    params: serde_json::Value,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auth: Option<&'a str>,
                }
                let payload = Payload {
                    jsonrpc: "2.0",
                    id: 1,
                    method: "peer.rebate_claim",
                    params: json!({"peer": peer, "threshold": threshold, "epoch": epoch, "reward": reward}),
                    auth: None,
                };
                if let Ok(resp) = client.call(&url, &payload) {
                    if let Ok(text) = resp.text() {
                        println!("{}", text);
                    }
                }
            }
        },
    }
}

fn print_overlay_status(status: &OverlayStatusView) {
    println!("Active overlay backend: {}", status.backend);
    println!("Peers observed by uptime tracker: {}", status.active_peers);
    println!("Persisted peer entries: {}", status.persisted_peers);
    match &status.database_path {
        Some(path) => println!("Peer database: {}", path),
        None => println!("Peer database: (in-memory)"),
    }
}

fn print_quic_stats(entries: &[QuicStatsEntry]) {
    if entries.is_empty() {
        println!("no active QUIC peers");
        return;
    }
    println!(
        "{:<66} {:<16} {:<66} {:>12} {:>14} {:>10} {:>12}",
        "Peer", "Provider", "Fingerprint", "Latency(ms)", "Retransmits", "Reuse", "Failures"
    );
    for entry in entries {
        let latency = entry
            .latency_ms
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<66} {:<16} {:<66} {:>12} {:>14} {:>10} {:>12}",
            entry.peer_id,
            entry.provider.as_deref().unwrap_or("-"),
            entry.fingerprint.as_deref().unwrap_or("-"),
            latency,
            entry.retransmits,
            entry.endpoint_reuse,
            entry.handshake_failures
        );
    }
}

fn print_quic_cert_history(entries: &[PeerCertHistoryEntry]) {
    if entries.is_empty() {
        println!("no cached QUIC certificates");
        return;
    }
    for entry in entries {
        println!(
            "Peer {} via {} (rotations: {})",
            entry.peer, entry.provider, entry.rotations
        );
        let current = &entry.current;
        println!(
            "  current: {} (age: {}s, cert: {}, updated_at: {})",
            current.fingerprint,
            current.age_secs,
            if current.has_certificate { "yes" } else { "no" },
            current.updated_at
        );
        if entry.history.is_empty() {
            println!("  history: <empty>");
        } else {
            println!("  history:");
            for hist in &entry.history {
                println!(
                    "    - {} (age: {}s, cert: {}, updated_at: {})",
                    hist.fingerprint,
                    hist.age_secs,
                    if hist.has_certificate { "yes" } else { "no" },
                    hist.updated_at
                );
            }
        }
    }
}

fn print_gossip_status(value: &Value) {
    let ttl = value.get("ttl_ms").and_then(Value::as_u64).unwrap_or(0);
    let dedup_size = value.get("dedup_size").and_then(Value::as_u64).unwrap_or(0);
    let dedup_capacity = value
        .get("dedup_capacity")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    println!("TTL: {} ms", ttl);
    println!("Dedup cache: {}/{} entries", dedup_size, dedup_capacity);
    if let Some(fanout) = value.get("fanout") {
        let min = fanout.get("min").and_then(Value::as_u64).unwrap_or(0);
        let base = fanout.get("base").and_then(Value::as_u64).unwrap_or(0);
        let max = fanout.get("max").and_then(Value::as_u64).unwrap_or(0);
        println!("Fanout(min/base/max): {}/{}/{}", min, base, max);
        if let Some(last) = fanout.get("last").and_then(Value::as_u64) {
            let cand = fanout
                .get("candidates")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let score = fanout
                .get("avg_score")
                .and_then(Value::as_f64)
                .map(|s| format!("{:.2}", s))
                .unwrap_or_else(|| "n/a".to_string());
            println!(
                "Last selection: {} peers from {} candidates (avg score {})",
                last, cand, score
            );
        }
        if let Some(selected) = fanout.get("selected_peers").and_then(Value::as_array) {
            if !selected.is_empty() {
                println!("  Selected peers:");
                for peer in selected {
                    if let Some(id) = peer.as_str() {
                        println!("    - {}", id);
                    }
                }
            }
        }
    }
    if let Some(partition) = value.get("partition") {
        let active = partition
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if active {
            let marker = partition.get("marker").and_then(Value::as_u64).unwrap_or(0);
            println!("Partition: active (marker {})", marker);
        } else {
            println!("Partition: inactive");
        }
        if let Some(list) = partition.get("isolated_peers").and_then(Value::as_array) {
            if !list.is_empty() {
                println!("  Isolated peers:");
                for peer in list {
                    if let Some(s) = peer.as_str() {
                        println!("    - {}", s);
                    }
                }
            }
        }
    }
    if let Some(shards) = value.get("shard_affinity").and_then(Value::as_array) {
        if !shards.is_empty() {
            println!("Shard affinity:");
            for entry in shards {
                let shard = entry.get("shard").and_then(Value::as_u64).unwrap_or(0);
                let peers = entry
                    .get("peers")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if peers.is_empty() {
                    println!("  shard {}: <none>", shard);
                } else {
                    let list: Vec<String> = peers
                        .into_iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    println!("  shard {}: {}", shard, list.join(", "));
                }
            }
        }
    }
}
