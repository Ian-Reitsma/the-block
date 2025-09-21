use crate::rpc::RpcClient;
use bridges::{header::PowHeader, light_client::Proof, RelayerProof};
use clap::Subcommand;
use serde_json::json;
use std::fs;

#[derive(Subcommand)]
pub enum BridgeCmd {
    /// Submit a light-client deposit proof via RPC
    Deposit {
        asset: String,
        user: String,
        amount: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        relayers: Vec<String>,
        #[arg(long, default_value = "header.json")]
        header: String,
        #[arg(long, default_value = "proof.json")]
        proof: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Request a withdrawal guarded by a challenge window
    Withdraw {
        asset: String,
        user: String,
        amount: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        relayers: Vec<String>,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Challenge a pending withdrawal commitment
    Challenge {
        asset: String,
        commitment: String,
        #[arg(long, default_value = "challenger")]
        challenger: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Inspect pending withdrawals for an asset
    Pending {
        #[arg(long)]
        asset: Option<String>,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// List active bridge challenges
    Challenges {
        #[arg(long)]
        asset: Option<String>,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Display relayer quorum composition
    Relayers {
        asset: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Paginate deposit receipts for auditing
    History {
        asset: String,
        #[arg(long)]
        cursor: Option<u64>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Review slashing events for relayers
    SlashLog {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Top up collateral for a relayer account
    Bond {
        relayer: String,
        amount: u64,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

fn load_header(path: &str) -> PowHeader {
    let contents = fs::read_to_string(path).expect("read header");
    serde_json::from_str(&contents).expect("decode header")
}

fn load_proof(path: &str) -> Proof {
    let contents = fs::read_to_string(path).expect("read proof");
    serde_json::from_str(&contents).expect("decode proof")
}

fn relayer_proofs(relayers: &[String], user: &str, amount: u64) -> Vec<RelayerProof> {
    relayers
        .iter()
        .map(|id| RelayerProof::new(id, user, amount))
        .collect()
}

fn print_response(resp: reqwest::blocking::Response) {
    if let Ok(text) = resp.text() {
        println!("{}", text);
    }
}

pub fn handle(action: BridgeCmd) {
    let client = RpcClient::from_env();
    match action {
        BridgeCmd::Deposit {
            asset,
            user,
            amount,
            relayers,
            header,
            proof,
            url,
        } => {
            if relayers.is_empty() {
                eprintln!("at least one relayer must be provided");
                return;
            }
            let header = load_header(&header);
            let proof = load_proof(&proof);
            let proofs = relayer_proofs(&relayers, &user, amount);
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let primary = relayers.first().cloned().unwrap_or_default();
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "bridge.verify_deposit",
                params: json!({
                    "asset": asset,
                    "relayer": primary,
                    "user": user,
                    "amount": amount,
                    "header": header,
                    "proof": proof,
                    "relayer_proofs": proofs,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Withdraw {
            asset,
            user,
            amount,
            relayers,
            url,
        } => {
            if relayers.is_empty() {
                eprintln!("at least one relayer must be provided");
                return;
            }
            let proofs = relayer_proofs(&relayers, &user, amount);
            let primary = relayers.first().cloned().unwrap_or_default();
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
                method: "bridge.request_withdrawal",
                params: json!({
                    "asset": asset,
                    "relayer": primary,
                    "user": user,
                    "amount": amount,
                    "relayer_proofs": proofs,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Challenge {
            asset,
            commitment,
            challenger,
            url,
        } => {
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
                method: "bridge.challenge_withdrawal",
                params: json!({
                    "asset": asset,
                    "commitment": commitment,
                    "challenger": challenger,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Pending { asset, url } => {
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
                method: "bridge.pending_withdrawals",
                params: json!({
                    "asset": asset,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Challenges { asset, url } => {
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
                method: "bridge.active_challenges",
                params: json!({
                    "asset": asset,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Relayers { asset, url } => {
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
                method: "bridge.relayer_quorum",
                params: json!({
                    "asset": asset,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::History {
            asset,
            cursor,
            limit,
            url,
        } => {
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
                method: "bridge.deposit_history",
                params: json!({
                    "asset": asset,
                    "cursor": cursor,
                    "limit": limit,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::SlashLog { url } => {
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
                method: "bridge.slash_log",
                params: json!({}),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Bond {
            relayer,
            amount,
            url,
        } => {
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
                method: "bridge.bond_relayer",
                params: json!({
                    "relayer": relayer,
                    "amount": amount,
                }),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
    }
}
