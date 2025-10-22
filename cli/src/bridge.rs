use crate::codec_helpers::json_from_str;
use crate::json_helpers::{
    empty_object, json_bool, json_null, json_object_from, json_option_string, json_rpc_request,
    json_string, json_u64,
};
use crate::parse_utils::{
    parse_bool, parse_bool_option, parse_positional_u64, parse_u64, parse_usize,
    parse_usize_required, require_positional, take_string,
};
use crate::rpc::RpcClient;
use bridges::{header::PowHeader, light_client::Proof, RelayerProof};
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json;
use std::fs;
use std::io::{self, Write};

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
    /// Inspect relayer accounting snapshots
    Accounting {
        asset: Option<String>,
        relayer: Option<String>,
        url: String,
    },
    /// Review recorded duty assignments and outcomes
    Duties {
        asset: Option<String>,
        relayer: Option<String>,
        limit: usize,
        url: String,
    },
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
    /// Claim accrued bridge rewards with governance approval
    Claim {
        relayer: String,
        amount: u64,
        approval_key: String,
        url: String,
    },
    /// Submit an external settlement proof for a pending withdrawal
    Settlement {
        asset: String,
        relayer: String,
        commitment: String,
        settlement_chain: String,
        proof_hash: String,
        settlement_height: u64,
        url: String,
    },
    /// Inspect reward claim history for relayers
    RewardClaims {
        relayer: Option<String>,
        cursor: Option<u64>,
        limit: usize,
        url: String,
    },
    /// Inspect settlement submissions
    SettlementLog {
        asset: Option<String>,
        cursor: Option<u64>,
        limit: usize,
        url: String,
    },
    /// Render dispute audit summaries
    DisputeAudit {
        asset: Option<String>,
        cursor: Option<u64>,
        limit: usize,
        url: String,
    },
    /// List configured bridge assets
    Assets { url: String },
    /// Configure a bridge asset channel
    ConfigureAsset {
        asset: String,
        confirm_depth: Option<u64>,
        fee_per_byte: Option<u64>,
        challenge_period_secs: Option<u64>,
        relayer_quorum: Option<usize>,
        headers_dir: Option<String>,
        requires_settlement_proof: Option<bool>,
        settlement_chain: Option<String>,
        clear_settlement_chain: bool,
        url: String,
    },
}

pub trait BridgeRpcTransport {
    fn call(&self, url: &str, payload: &json::Value) -> io::Result<String>;
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
                CommandId("bridge.accounting"),
                "accounting",
                "Inspect relayer accounting snapshots",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "asset",
                "asset",
                "Optional asset filter",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "relayer",
                "relayer",
                "Optional relayer identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.duties"),
                "duties",
                "Inspect bridge duty assignments",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "asset",
                "asset",
                "Optional asset filter",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "relayer",
                "relayer",
                "Optional relayer identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Maximum duty entries").default("50"),
            ))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.claim"),
                "claim",
                "Claim accrued rewards with a governance approval key",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "relayer",
                "Relayer identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Amount to claim",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "approval-key",
                "Governance approval key",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.settlement"),
                "settlement",
                "Submit an external settlement proof",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "relayer",
                "Relayer identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "commitment",
                "Withdrawal commitment",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "settlement-chain",
                "Destination chain identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "proof-hash",
                "Settlement proof hash",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "height",
                "Settlement height",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.reward_claims"),
                "reward-claims",
                "Inspect recorded reward claims",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "relayer",
                "relayer",
                "Optional relayer identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "cursor",
                "cursor",
                "Pagination cursor",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Maximum records to return").default("50"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.settlement_log"),
                "settlement-log",
                "Inspect settlement submissions",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "asset",
                "asset",
                "Optional asset filter",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "cursor",
                "cursor",
                "Pagination cursor",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Maximum records to return").default("50"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.dispute_audit"),
                "dispute-audit",
                "Render dispute audit summaries",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "asset",
                "asset",
                "Optional asset filter",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "cursor",
                "cursor",
                "Pagination cursor",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Maximum records to return").default("50"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.assets"),
                "assets",
                "List configured bridge assets",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("bridge.configure"),
                "configure",
                "Configure a bridge asset channel",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "asset",
                "Asset identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "confirm-depth",
                "confirm-depth",
                "Confirmation depth",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "fee-per-byte",
                "fee-per-byte",
                "Fee per byte",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "challenge-period",
                "challenge-period",
                "Challenge period seconds",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "relayer-quorum",
                "relayer-quorum",
                "Relayer quorum",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "headers-dir",
                "headers-dir",
                "Headers directory",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "requires-settlement-proof",
                "requires-settlement-proof",
                "Require settlement proof (true/false)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "settlement-chain",
                "settlement-chain",
                "Default settlement chain",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "clear-settlement-chain",
                "clear-settlement-chain",
                "Clear the configured settlement chain (true/false)",
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
            "accounting" => {
                let asset = take_string(sub_matches, "asset");
                let relayer = take_string(sub_matches, "relayer");
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Accounting {
                    asset,
                    relayer,
                    url,
                })
            }
            "duties" => {
                let asset = take_string(sub_matches, "asset");
                let relayer = take_string(sub_matches, "relayer");
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?.unwrap_or(50);
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Duties {
                    asset,
                    relayer,
                    limit,
                    url,
                })
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
            "claim" => {
                let relayer = require_positional(sub_matches, "relayer")?;
                let amount = parse_positional_u64(sub_matches, "amount")?;
                let approval_key = require_positional(sub_matches, "approval-key")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Claim {
                    relayer,
                    amount,
                    approval_key,
                    url,
                })
            }
            "settlement" => {
                let asset = require_positional(sub_matches, "asset")?;
                let relayer = require_positional(sub_matches, "relayer")?;
                let commitment = require_positional(sub_matches, "commitment")?;
                let settlement_chain = require_positional(sub_matches, "settlement-chain")?;
                let proof_hash = require_positional(sub_matches, "proof-hash")?;
                let settlement_height = parse_positional_u64(sub_matches, "height")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Settlement {
                    asset,
                    relayer,
                    commitment,
                    settlement_chain,
                    proof_hash,
                    settlement_height,
                    url,
                })
            }
            "reward-claims" => {
                let relayer = take_string(sub_matches, "relayer");
                let cursor =
                    crate::parse_utils::parse_u64(take_string(sub_matches, "cursor"), "cursor")?;
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?.unwrap_or(50);
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::RewardClaims {
                    relayer,
                    cursor,
                    limit,
                    url,
                })
            }
            "settlement-log" => {
                let asset = take_string(sub_matches, "asset");
                let cursor =
                    crate::parse_utils::parse_u64(take_string(sub_matches, "cursor"), "cursor")?;
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?.unwrap_or(50);
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::SettlementLog {
                    asset,
                    cursor,
                    limit,
                    url,
                })
            }
            "dispute-audit" => {
                let asset = take_string(sub_matches, "asset");
                let cursor =
                    crate::parse_utils::parse_u64(take_string(sub_matches, "cursor"), "cursor")?;
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?.unwrap_or(50);
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::DisputeAudit {
                    asset,
                    cursor,
                    limit,
                    url,
                })
            }
            "assets" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::Assets { url })
            }
            "configure" => {
                let asset = require_positional(sub_matches, "asset")?;
                let confirm_depth =
                    parse_u64(take_string(sub_matches, "confirm-depth"), "confirm-depth")?;
                let fee_per_byte =
                    parse_u64(take_string(sub_matches, "fee-per-byte"), "fee-per-byte")?;
                let challenge_period_secs = parse_u64(
                    take_string(sub_matches, "challenge-period"),
                    "challenge-period",
                )?;
                let relayer_quorum =
                    parse_usize(take_string(sub_matches, "relayer-quorum"), "relayer-quorum")?;
                let headers_dir = take_string(sub_matches, "headers-dir");
                let requires_settlement_proof = parse_bool_option(
                    take_string(sub_matches, "requires-settlement-proof"),
                    "requires-settlement-proof",
                )?;
                let settlement_chain = take_string(sub_matches, "settlement-chain");
                let clear_settlement_chain = parse_bool(
                    take_string(sub_matches, "clear-settlement-chain"),
                    false,
                    "clear-settlement-chain",
                )?;
                if clear_settlement_chain && settlement_chain.is_some() {
                    return Err("cannot set and clear settlement chain simultaneously".to_string());
                }
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(BridgeCmd::ConfigureAsset {
                    asset,
                    confirm_depth,
                    fee_per_byte,
                    challenge_period_secs,
                    relayer_quorum,
                    headers_dir,
                    requires_settlement_proof,
                    settlement_chain,
                    clear_settlement_chain,
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

fn to_json_value<T>(value: &T) -> json::Value
where
    T: foundation_serialization::Serialize,
{
    json::to_value(value).expect("serialize bridge payload component")
}

pub fn handle(action: BridgeCmd) {
    let client = RpcClient::from_env();
    let transport = RpcClientTransport { client: &client };
    let mut stdout = io::stdout();
    if let Err(err) = handle_with_transport(action, &transport, &mut stdout) {
        eprintln!("{err}");
    }
}

#[allow(dead_code)]
pub fn handle_with_writer(action: BridgeCmd, out: &mut dyn Write) -> io::Result<()> {
    let client = RpcClient::from_env();
    let transport = RpcClientTransport { client: &client };
    handle_with_transport(action, &transport, out)
}

pub fn handle_with_transport(
    action: BridgeCmd,
    transport: &dyn BridgeRpcTransport,
    out: &mut dyn Write,
) -> io::Result<()> {
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
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "at least one relayer must be provided",
                ));
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
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Withdraw {
            asset,
            user,
            amount,
            relayers,
            url,
        } => {
            if relayers.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "at least one relayer must be provided",
                ));
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
            send_rpc(transport, &url, &payload, out)?;
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
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Pending { asset, url } => {
            let params = json_object_from([("asset", json_option_string(asset))]);
            let payload = json_rpc_request("bridge.pending_withdrawals", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Challenges { asset, url } => {
            let params = json_object_from([("asset", json_option_string(asset))]);
            let payload = json_rpc_request("bridge.active_challenges", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Relayers { asset, url } => {
            let params = json_object_from([("asset", json_string(asset))]);
            let payload = json_rpc_request("bridge.relayer_quorum", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Accounting {
            asset,
            relayer,
            url,
        } => {
            let params = json_object_from([
                ("asset", json_option_string(asset)),
                ("relayer", json_option_string(relayer)),
            ]);
            let payload = json_rpc_request("bridge.relayer_accounting", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Duties {
            asset,
            relayer,
            limit,
            url,
        } => {
            let params = json_object_from([
                ("asset", json_option_string(asset)),
                ("relayer", json_option_string(relayer)),
                ("limit", json_u64(limit as u64)),
            ]);
            let payload = json_rpc_request("bridge.duty_log", params);
            send_rpc(transport, &url, &payload, out)?;
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
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::SlashLog { url } => {
            let payload = json_rpc_request("bridge.slash_log", empty_object());
            send_rpc(transport, &url, &payload, out)?;
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
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Claim {
            relayer,
            amount,
            approval_key,
            url,
        } => {
            let params = json_object_from([
                ("relayer", json_string(relayer)),
                ("amount", json_u64(amount)),
                ("approval_key", json_string(approval_key)),
            ]);
            let payload = json_rpc_request("bridge.claim_rewards", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Settlement {
            asset,
            relayer,
            commitment,
            settlement_chain,
            proof_hash,
            settlement_height,
            url,
        } => {
            let params = json_object_from([
                ("asset", json_string(asset)),
                ("relayer", json_string(relayer)),
                ("commitment", json_string(commitment)),
                ("settlement_chain", json_string(settlement_chain)),
                ("proof_hash", json_string(proof_hash)),
                ("settlement_height", json_u64(settlement_height)),
            ]);
            let payload = json_rpc_request("bridge.submit_settlement", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::RewardClaims {
            relayer,
            cursor,
            limit,
            url,
        } => {
            let params = json_object_from([
                ("relayer", json_option_string(relayer)),
                ("cursor", cursor.map(json_u64).unwrap_or_else(json_null)),
                ("limit", json_u64(limit as u64)),
            ]);
            let payload = json_rpc_request("bridge.reward_claims", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::SettlementLog {
            asset,
            cursor,
            limit,
            url,
        } => {
            let params = json_object_from([
                ("asset", json_option_string(asset)),
                ("cursor", cursor.map(json_u64).unwrap_or_else(json_null)),
                ("limit", json_u64(limit as u64)),
            ]);
            let payload = json_rpc_request("bridge.settlement_log", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::DisputeAudit {
            asset,
            cursor,
            limit,
            url,
        } => {
            let params = json_object_from([
                ("asset", json_option_string(asset)),
                ("cursor", cursor.map(json_u64).unwrap_or_else(json_null)),
                ("limit", json_u64(limit as u64)),
            ]);
            let payload = json_rpc_request("bridge.dispute_audit", params);
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::Assets { url } => {
            let payload = json_rpc_request("bridge.assets", empty_object());
            send_rpc(transport, &url, &payload, out)?;
        }
        BridgeCmd::ConfigureAsset {
            asset,
            confirm_depth,
            fee_per_byte,
            challenge_period_secs,
            relayer_quorum,
            headers_dir,
            requires_settlement_proof,
            settlement_chain,
            clear_settlement_chain,
            url,
        } => {
            let mut entries: Vec<(&'static str, json::Value)> = vec![("asset", json_string(asset))];
            if let Some(depth) = confirm_depth {
                entries.push(("confirm_depth", json_u64(depth)));
            }
            if let Some(fee) = fee_per_byte {
                entries.push(("fee_per_byte", json_u64(fee)));
            }
            if let Some(window) = challenge_period_secs {
                entries.push(("challenge_period_secs", json_u64(window)));
            }
            if let Some(quorum) = relayer_quorum {
                entries.push(("relayer_quorum", json_u64(quorum as u64)));
            }
            if let Some(dir) = headers_dir {
                entries.push(("headers_dir", json_string(dir)));
            }
            if let Some(flag) = requires_settlement_proof {
                entries.push(("requires_settlement_proof", json_bool(flag)));
            }
            if clear_settlement_chain {
                entries.push(("settlement_chain", json_null()));
            } else if let Some(chain) = settlement_chain {
                entries.push(("settlement_chain", json_string(chain)));
            }
            let params = json_object_from(entries);
            let payload = json_rpc_request("bridge.configure_asset", params);
            send_rpc(transport, &url, &payload, out)?;
        }
    }
    Ok(())
}

fn send_rpc(
    transport: &dyn BridgeRpcTransport,
    url: &str,
    payload: &json::Value,
    out: &mut dyn Write,
) -> io::Result<()> {
    let text = transport.call(url, payload)?;
    writeln!(out, "{text}")?;
    Ok(())
}

struct RpcClientTransport<'a> {
    client: &'a RpcClient,
}

impl<'a> BridgeRpcTransport for RpcClientTransport<'a> {
    fn call(&self, url: &str, payload: &json::Value) -> io::Result<String> {
        let resp = self
            .client
            .call(url, payload)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        resp.text()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("read response: {err}")))
    }
}
