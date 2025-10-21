use crate::codec_helpers::json_from_str;
use crate::json_helpers::{
    empty_object, json_null, json_object_from, json_option_string, json_rpc_request, json_string,
    json_u64,
};
use crate::parse_utils::{
    parse_positional_u64, parse_usize_required, require_positional, take_string,
};
use crate::rpc::RpcClient;
use bridges::{header::PowHeader, light_client::Proof, RelayerProof};
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json;
use httpd::ClientResponse;
use std::fs;

pub enum BridgeCmd {
    /// Submit a light-client deposit proof via RPC
    Deposit {
        asset: String,
        user: String,
        amount: u64,
        relayers: Vec<String>,
        header: String,
        proof: String,
        url: String,
    },
    /// Request a withdrawal guarded by a challenge window
    Withdraw {
        asset: String,
        user: String,
        amount: u64,
        relayers: Vec<String>,
        url: String,
    },
    /// Challenge a pending withdrawal commitment
    Challenge {
        asset: String,
        commitment: String,
        challenger: String,
        url: String,
    },
    /// Inspect pending withdrawals for an asset
    Pending { asset: Option<String>, url: String },
    /// List active bridge challenges
    Challenges { asset: Option<String>, url: String },
    /// Display relayer quorum composition
    Relayers { asset: String, url: String },
    /// Paginate deposit receipts for auditing
    History {
        asset: String,
        cursor: Option<u64>,
        limit: usize,
        url: String,
    },
    /// Review slashing events for relayers
    SlashLog { url: String },
    /// Top up collateral for a relayer account
    Bond {
        relayer: String,
        amount: u64,
        url: String,
    },
}

impl BridgeCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("bridge"),
            "bridge",
            "Bridge deposit and withdrawal utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.deposit"),
                "deposit",
                "Submit a light-client deposit proof via RPC",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "user",
                "Recipient account identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Deposit amount",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "relayers",
                    "relayers",
                    "Comma separated relayer identifiers",
                )
                .required(true)
                .multiple(true)
                .value_delimiter(','),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("header", "header", "Path to the deposit header JSON file")
                    .default("header.json"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("proof", "proof", "Path to the deposit proof JSON file")
                    .default("proof.json"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.withdraw"),
                "withdraw",
                "Request a withdrawal guarded by a challenge window",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "user",
                "Recipient account identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Withdrawal amount",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "relayers",
                    "relayers",
                    "Comma separated relayer identifiers",
                )
                .required(true)
                .multiple(true)
                .value_delimiter(','),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.challenge"),
                "challenge",
                "Challenge a pending withdrawal commitment",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "commitment",
                "Commitment hash",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("challenger", "challenger", "Challenger identifier")
                    .default("challenger"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.pending"),
                "pending",
                "Inspect pending withdrawals for an asset",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "asset",
                "asset",
                "Optional asset filter",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.challenges"),
                "challenges",
                "List active bridge challenges",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "asset",
                "asset",
                "Optional asset filter",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.relayers"),
                "relayers",
                "Display relayer quorum composition",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.history"),
                "history",
                "Paginate deposit receipts for auditing",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "cursor",
                "cursor",
                "Pagination cursor",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Page size").default("50"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.slash_log"),
                "slash-log",
                "Review slashing events for relayers",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.bond"),
                "bond",
                "Top up collateral for a relayer account",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "relayer",
                "Relayer identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Collateral amount",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'bridge'".to_string())?;

        match name {
            "deposit" => {
                let asset = require_positional(sub_matches, "asset")?;
                let user = require_positional(sub_matches, "user")?;
                let amount = parse_positional_u64(sub_matches, "amount")?;
                let relayers = sub_matches.get_strings("relayers");
                if relayers.is_empty() {
                    return Err("at least one --relayers entry is required".to_string());
                }
                let header =
                    take_string(sub_matches, "header").unwrap_or_else(|| "header.json".to_string());
                let proof =
                    take_string(sub_matches, "proof").unwrap_or_else(|| "proof.json".to_string());
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Deposit {
                    asset,
                    user,
                    amount,
                    relayers,
                    header,
                    proof,
                    url,
                })
            }
            "withdraw" => {
                let asset = require_positional(sub_matches, "asset")?;
                let user = require_positional(sub_matches, "user")?;
                let amount = parse_positional_u64(sub_matches, "amount")?;
                let relayers = sub_matches.get_strings("relayers");
                if relayers.is_empty() {
                    return Err("at least one --relayers entry is required".to_string());
                }
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Withdraw {
                    asset,
                    user,
                    amount,
                    relayers,
                    url,
                })
            }
            "challenge" => {
                let asset = require_positional(sub_matches, "asset")?;
                let commitment = require_positional(sub_matches, "commitment")?;
                let challenger = take_string(sub_matches, "challenger")
                    .unwrap_or_else(|| "challenger".to_string());
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Challenge {
                    asset,
                    commitment,
                    challenger,
                    url,
                })
            }
            "pending" => {
                let asset = take_string(sub_matches, "asset");
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Pending { asset, url })
            }
            "challenges" => {
                let asset = take_string(sub_matches, "asset");
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Challenges { asset, url })
            }
            "relayers" => {
                let asset = require_positional(sub_matches, "asset")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Relayers { asset, url })
            }
            "history" => {
                let asset = require_positional(sub_matches, "asset")?;
                let cursor =
                    crate::parse_utils::parse_u64(take_string(sub_matches, "cursor"), "cursor")?;
                let limit = parse_usize_required(take_string(sub_matches, "limit"), "limit")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::History {
                    asset,
                    cursor,
                    limit,
                    url,
                })
            }
            "slash-log" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::SlashLog { url })
            }
            "bond" => {
                let relayer = require_positional(sub_matches, "relayer")?;
                let amount = parse_positional_u64(sub_matches, "amount")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Bond {
                    relayer,
                    amount,
                    url,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'bridge'")),
        }
    }
}

fn load_header(path: &str) -> PowHeader {
    let contents = fs::read_to_string(path).expect("read header");
    json_from_str(&contents).expect("decode header")
}

fn load_proof(path: &str) -> Proof {
    let contents = fs::read_to_string(path).expect("read proof");
    json_from_str(&contents).expect("decode proof")
}

fn relayer_proofs(relayers: &[String], user: &str, amount: u64) -> Vec<RelayerProof> {
    relayers
        .iter()
        .map(|id| RelayerProof::new(id, user, amount))
        .collect()
}

fn print_response(resp: ClientResponse) {
    if let Ok(text) = resp.text() {
        println!("{}", text);
    }
}

fn to_json_value<T>(value: &T) -> json::Value
where
    T: foundation_serialization::Serialize,
{
    json::to_value(value).expect("serialize bridge payload component")
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
            let primary = relayers.first().cloned().unwrap_or_default();
            let params = json_object_from([
                ("asset", json_string(asset)),
                ("relayer", json_string(primary)),
                ("user", json_string(user)),
                ("amount", json_u64(amount)),
                ("header", to_json_value(&header)),
                ("proof", to_json_value(&proof)),
                ("relayer_proofs", to_json_value(&proofs)),
            ]);
            let payload = json_rpc_request("bridge.verify_deposit", params);
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
            let params = json_object_from([
                ("asset", json_string(asset)),
                ("relayer", json_string(primary)),
                ("user", json_string(user)),
                ("amount", json_u64(amount)),
                ("relayer_proofs", to_json_value(&proofs)),
            ]);
            let payload = json_rpc_request("bridge.request_withdrawal", params);
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
            let params = json_object_from([
                ("asset", json_string(asset)),
                ("commitment", json_string(commitment)),
                ("challenger", json_string(challenger)),
            ]);
            let payload = json_rpc_request("bridge.challenge_withdrawal", params);
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Pending { asset, url } => {
            let params = json_object_from([("asset", json_option_string(asset))]);
            let payload = json_rpc_request("bridge.pending_withdrawals", params);
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Challenges { asset, url } => {
            let params = json_object_from([("asset", json_option_string(asset))]);
            let payload = json_rpc_request("bridge.active_challenges", params);
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Relayers { asset, url } => {
            let params = json_object_from([("asset", json_string(asset))]);
            let payload = json_rpc_request("bridge.relayer_quorum", params);
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
            let params = json_object_from([
                ("asset", json_string(asset)),
                ("cursor", cursor.map(json_u64).unwrap_or_else(json_null)),
                ("limit", json_u64(limit as u64)),
            ]);
            let payload = json_rpc_request("bridge.deposit_history", params);
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::SlashLog { url } => {
            let payload = json_rpc_request("bridge.slash_log", empty_object());
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
        BridgeCmd::Bond {
            relayer,
            amount,
            url,
        } => {
            let params = json_object_from([
                ("relayer", json_string(relayer)),
                ("amount", json_u64(amount)),
            ]);
            let payload = json_rpc_request("bridge.bond_relayer", params);
            if let Ok(resp) = client.call(&url, &payload) {
                print_response(resp);
            }
        }
    }
}
