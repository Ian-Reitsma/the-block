use crate::rpc::RpcClient;
use clap::Subcommand;
use hex;
use serde::Deserialize;
use serde_json::{json, Value};
use the_block::net::{PeerCertHistoryEntry, QuicStatsEntry};

#[derive(Subcommand)]
pub enum NetCmd {
    /// Reputation operations
    Reputation {
        #[command(subcommand)]
        action: ReputationCmd,
    },
    /// DNS operations
    Dns {
        #[command(subcommand)]
        action: DnsCmd,
    },
    /// Rotate a peer's public key
    RotateKey {
        peer_id: String,
        new_key: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Rotate the local QUIC certificate
    RotateCert {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Rebate operations
    Rebate {
        #[command(subcommand)]
        action: RebateCmd,
    },
    /// QUIC diagnostics
    Quic {
        #[command(subcommand)]
        action: QuicCmd,
    },
    /// Display QUIC peer statistics
    QuicStats {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show gossip relay configuration and shard affinity
    GossipStatus {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ReputationCmd {
    /// Show reputation for a peer
    Show {
        peer: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

#[derive(Subcommand)]
pub enum DnsCmd {
    /// Verify DNS TXT record for a domain
    Verify {
        domain: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

#[derive(Subcommand)]
pub enum QuicCmd {
    /// Show recent handshake failures
    Failures {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Inspect cached QUIC certificate history
    History {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        json: bool,
    },
    /// Reload the QUIC certificate cache from disk
    Refresh {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

#[derive(Subcommand)]
pub enum RebateCmd {
    /// Claim rebate voucher for a peer
    Claim {
        peer: String,
        threshold: u64,
        epoch: u64,
        reward: u64,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
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
            use ed25519_dalek::Signer;
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
                            if let Ok(text) = serde_json::to_string_pretty(&data.result) {
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
                        if let Ok(text) = serde_json::to_string_pretty(&data.result) {
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
                        if let Ok(val) = serde_json::from_str::<Value>(&text) {
                            let out = val.get("result").cloned().unwrap_or(val);
                            if let Ok(pretty) = serde_json::to_string_pretty(&out) {
                                println!("{}", pretty);
                            } else {
                                println!("{}", text);
                            }
                        } else {
                            println!("{}", text);
                        }
                    } else if let Ok(val) = serde_json::from_str::<Value>(&text) {
                        let result = val.get("result").cloned().unwrap_or(val);
                        print_gossip_status(&result);
                    } else {
                        println!("{}", text);
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

fn print_quic_stats(entries: &[QuicStatsEntry]) {
    if entries.is_empty() {
        println!("no active QUIC peers");
        return;
    }
    println!(
        "{:<66} {:<66} {:>12} {:>14} {:>10} {:>12}",
        "Peer", "Fingerprint", "Latency(ms)", "Retransmits", "Reuse", "Failures"
    );
    for entry in entries {
        let latency = entry
            .latency_ms
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<66} {:<66} {:>12} {:>14} {:>10} {:>12}",
            entry.peer_id,
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
        println!("Peer {} (rotations: {})", entry.peer, entry.rotations);
        let current = &entry.current;
        println!(
            "  current: {} (age: {}s, cert: {})",
            current.fingerprint,
            current.age_secs,
            if current.has_certificate { "yes" } else { "no" }
        );
        if entry.history.is_empty() {
            println!("  history: <empty>");
        } else {
            println!("  history:");
            for hist in &entry.history {
                println!(
                    "    - {} (age: {}s, cert: {})",
                    hist.fingerprint,
                    hist.age_secs,
                    if hist.has_certificate { "yes" } else { "no" }
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
