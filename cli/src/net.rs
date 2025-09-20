use crate::rpc::RpcClient;
use clap::Subcommand;
use hex;
use serde::Deserialize;
use serde_json::json;
use the_block::net::QuicStatsEntry;

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
        "{:<66} {:>12} {:>14} {:>10} {:>12}",
        "Peer", "Latency(ms)", "Retransmits", "Reuse", "Failures"
    );
    for entry in entries {
        let latency = entry
            .latency_ms
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<66} {:>12} {:>14} {:>10} {:>12}",
            entry.peer_id,
            latency,
            entry.retransmits,
            entry.endpoint_reuse,
            entry.handshake_failures
        );
    }
}
