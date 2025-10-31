use crate::parse_utils::{
    parse_optional, parse_positional_u64, parse_u64, parse_u64_required, require_positional,
    take_string,
};
use crate::rpc::{RpcClient, RpcClientError};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::binary;
use foundation_serialization::json::Value;
use foundation_serialization::{json, Deserialize, Serialize};
use governance::{
    controller, encode_runtime_backend_policy, encode_storage_engine_policy,
    encode_transport_provider_policy, registry, GovStore, ParamKey, Proposal, ProposalStatus,
    ReleaseAttestation as GovReleaseAttestation, ReleaseBallot, ReleaseVerifier, ReleaseVote,
    TreasuryBalanceSnapshot, TreasuryDisbursement, TreasuryExecutorSnapshot, Vote, VoteChoice,
};
use httpd::ClientError;
use std::io::{self, Write};
use std::time::SystemTime;
use the_block::{governance::release::ReleaseAttestation as NodeReleaseAttestation, provenance};

struct CliReleaseVerifier;

impl ReleaseVerifier for CliReleaseVerifier {
    fn configured_signers(&self) -> Vec<String> {
        provenance::release_signer_hexes()
    }

    fn verify(&self, build_hash: &str, signer_hex: &str, signature_hex: &str) -> bool {
        provenance::parse_signer_hex(signer_hex)
            .and_then(|vk| {
                if provenance::verify_release_attestation(build_hash, &vk, signature_hex) {
                    Some(())
                } else {
                    None
                }
            })
            .is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteTreasuryStatus {
    Scheduled,
    Executed,
    Cancelled,
}

impl RemoteTreasuryStatus {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "scheduled" => Some(Self::Scheduled),
            "executed" => Some(Self::Executed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Executed => "executed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct JsonRpcRequest<'a, P> {
    jsonrpc: &'static str,
    id: u32,
    method: &'a str,
    params: P,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct RpcEnvelope<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

fn report_rpc_failure(rpc: &str, method: &str, err: &RpcClientError) {
    match err {
        RpcClientError::Transport(inner) => match inner {
            ClientError::Timeout => {
                eprintln!("error: rpc call '{method}' to {rpc} timed out.",);
                eprintln!(
                    "hint: ensure the node RPC endpoint is reachable or raise TB_RPC_TIMEOUT_MS."
                );
            }
            ClientError::InvalidUrl(url) => {
                eprintln!("error: rpc endpoint {rpc} is not a valid URL ({url}).",);
            }
            ClientError::UnsupportedScheme(scheme) => {
                eprintln!("error: rpc endpoint {rpc} uses unsupported scheme '{scheme}'.",);
            }
            ClientError::Io(io_err) => {
                eprintln!("error: rpc call '{method}' to {rpc} failed: {io_err}",);
                match io_err.kind() {
                    io::ErrorKind::ConnectionRefused => {
                        eprintln!(
                                "hint: the node rejected the connection; verify it is running and TB_RPC_ENDPOINT matches its listen address."
                            );
                    }
                    io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe => {
                        eprintln!(
                                "hint: the connection dropped mid-request; check network stability or TLS configuration."
                            );
                    }
                    _ => {}
                }
            }
            other => {
                eprintln!("error: rpc call '{method}' to {rpc} failed: {other}",);
            }
        },
        RpcClientError::InjectedFault => {
            eprintln!(
                "error: rpc call '{method}' aborted due to TB_RPC_FAULT_RATE fault injection."
            );
        }
    }
}

fn call_rpc_envelope<T, P>(
    client: &RpcClient,
    rpc: &str,
    method: &'static str,
    params: P,
) -> io::Result<RpcEnvelope<T>>
where
    T: for<'de> Deserialize<'de>,
    P: Serialize,
{
    let payload = JsonRpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method,
        params,
    };
    let response = client.call(rpc, &payload).map_err(|err| {
        report_rpc_failure(rpc, method, &err);
        io::Error::new(io::ErrorKind::Other, err.to_string())
    })?;
    response.json().map_err(|err| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("failed to decode {method} response: {err}"),
        )
    })
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RpcTreasuryDisbursementsResult {
    pub disbursements: Vec<TreasuryDisbursement>,
    #[serde(default)]
    pub next_cursor: Option<u64>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RpcTreasuryBalanceResult {
    pub balance_ct: u64,
    #[serde(default)]
    pub balance_it: u64,
    #[serde(default)]
    pub last_snapshot: Option<TreasuryBalanceSnapshot>,
    #[serde(default)]
    pub executor: Option<TreasuryExecutorSnapshot>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RpcTreasuryHistoryResult {
    pub snapshots: Vec<TreasuryBalanceSnapshot>,
    #[serde(default)]
    pub next_cursor: Option<u64>,
    #[allow(dead_code)]
    pub current_balance_ct: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub current_balance_it: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryFetchOutput {
    pub disbursements: Vec<TreasuryDisbursement>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub next_cursor: Option<u64>,
    pub balance_ct: u64,
    pub balance_it: u64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub last_snapshot: Option<TreasuryBalanceSnapshot>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub balance_history: Option<Vec<TreasuryBalanceSnapshot>>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub balance_next_cursor: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub executor: Option<TreasuryExecutorSnapshot>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
struct ExecutorDependency {
    disbursement_id: u64,
    dependencies: Vec<u64>,
    memo: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
struct ExecutorStatusReport {
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    snapshot: Option<TreasuryExecutorSnapshot>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    lease_seconds_remaining: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    lease_last_nonce: Option<u64>,
    lease_released: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    staged_intents: Vec<SignedExecutionIntent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependency_blocks: Vec<ExecutorDependency>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DisbursementList<'a> {
    disbursements: &'a [TreasuryDisbursement],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreasuryDisbursementQuery {
    pub status: Option<RemoteTreasuryStatus>,
    pub after_id: Option<u64>,
    pub limit: Option<usize>,
    pub destination: Option<String>,
    pub min_epoch: Option<u64>,
    pub max_epoch: Option<u64>,
    pub min_amount_ct: Option<u64>,
    pub max_amount_ct: Option<u64>,
    pub min_amount_it: Option<u64>,
    pub max_amount_it: Option<u64>,
    pub min_created_at: Option<u64>,
    pub max_created_at: Option<u64>,
    pub min_status_ts: Option<u64>,
    pub max_status_ts: Option<u64>,
}

pub fn treasury_disbursement_params(query: &TreasuryDisbursementQuery) -> json::Value {
    let mut params = json::Map::new();
    if let Some(filter) = query.status {
        params.insert("status".into(), json::Value::String(filter.as_str().into()));
    }
    if let Some(cursor) = query.after_id {
        params.insert("after_id".into(), json::Value::from(cursor));
    }
    if let Some(max) = query.limit {
        params.insert("limit".into(), json::Value::from(max as u64));
    }
    if let Some(destination) = &query.destination {
        params.insert(
            "destination".into(),
            json::Value::String(destination.clone()),
        );
    }
    if let Some(min_epoch) = query.min_epoch {
        params.insert("min_epoch".into(), json::Value::from(min_epoch));
    }
    if let Some(max_epoch) = query.max_epoch {
        params.insert("max_epoch".into(), json::Value::from(max_epoch));
    }
    if let Some(min_amount_ct) = query.min_amount_ct {
        params.insert("min_amount_ct".into(), json::Value::from(min_amount_ct));
    }
    if let Some(max_amount_ct) = query.max_amount_ct {
        params.insert("max_amount_ct".into(), json::Value::from(max_amount_ct));
    }
    if let Some(min_amount_it) = query.min_amount_it {
        params.insert("min_amount_it".into(), json::Value::from(min_amount_it));
    }
    if let Some(max_amount_it) = query.max_amount_it {
        params.insert("max_amount_it".into(), json::Value::from(max_amount_it));
    }
    if let Some(min_created_at) = query.min_created_at {
        params.insert("min_created_at".into(), json::Value::from(min_created_at));
    }
    if let Some(max_created_at) = query.max_created_at {
        params.insert("max_created_at".into(), json::Value::from(max_created_at));
    }
    if let Some(min_status_ts) = query.min_status_ts {
        params.insert("min_status_ts".into(), json::Value::from(min_status_ts));
    }
    if let Some(max_status_ts) = query.max_status_ts {
        params.insert("max_status_ts".into(), json::Value::from(max_status_ts));
    }
    json::Value::Object(params)
}

pub fn treasury_history_params(after_id: Option<u64>, limit: Option<usize>) -> json::Value {
    let mut params = json::Map::new();
    if let Some(cursor) = after_id {
        params.insert("after_id".into(), json::Value::from(cursor));
    }
    if let Some(max) = limit {
        params.insert("limit".into(), json::Value::from(max as u64));
    }
    json::Value::Object(params)
}

pub fn combine_treasury_fetch_results(
    disbursement: RpcTreasuryDisbursementsResult,
    balance: RpcTreasuryBalanceResult,
    history: Option<RpcTreasuryHistoryResult>,
) -> TreasuryFetchOutput {
    let mut output = TreasuryFetchOutput {
        disbursements: disbursement.disbursements,
        next_cursor: disbursement.next_cursor,
        balance_ct: balance.balance_ct,
        balance_it: balance.balance_it,
        last_snapshot: balance.last_snapshot,
        balance_history: None,
        balance_next_cursor: None,
        executor: balance.executor,
    };

    if let Some(history_result) = history {
        output.balance_history = Some(history_result.snapshots);
        output.balance_next_cursor = history_result.next_cursor;
    }

    output
}
fn unwrap_rpc_result<T>(envelope: RpcEnvelope<T>) -> io::Result<T> {
    if let Some(error) = envelope.error {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("rpc error {}: {}", error.code, error.message),
        ));
    }
    envelope
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "rpc response missing result"))
}

pub enum GovCmd {
    /// List all proposals
    List { state: String },
    /// Vote on a proposal
    Vote {
        id: u64,
        choice: String,
        state: String,
    },
    /// Show proposal status
    Status { id: u64, state: String },
    /// Submit a release vote referencing a signed hash
    ProposeRelease {
        hash: String,
        signatures: Vec<String>,
        signers: Vec<String>,
        threshold: Option<u32>,
        state: String,
        deadline: u64,
    },
    /// Cast a vote in favour of a release proposal
    ApproveRelease { id: u64, state: String, weight: u64 },
    /// Download and verify an approved release artifact
    FetchRelease {
        hash: String,
        signatures: Vec<String>,
        signers: Vec<String>,
        dest: Option<String>,
        install: bool,
    },
    /// Parameter management helpers
    Param { action: GovParamCmd },
    /// Treasury disbursement helpers
    Treasury { action: GovTreasuryCmd },
}

pub enum GovParamCmd {
    /// Submit a governance proposal to update a parameter
    Update {
        /// Parameter key (e.g. `mempool.fee_floor_window`)
        key: String,
        /// Proposed new value
        value: String,
        state: String,
        epoch: u64,
        vote_deadline: Option<u64>,
        proposer: Option<String>,
    },
}

pub enum GovTreasuryCmd {
    Schedule {
        destination: String,
        amount: u64,
        amount_it: u64,
        memo: Option<String>,
        epoch: u64,
        state: String,
    },
    Execute {
        id: u64,
        tx_hash: String,
        state: String,
    },
    Cancel {
        id: u64,
        reason: String,
        state: String,
    },
    List {
        state: String,
    },
    Fetch {
        rpc: String,
        query: TreasuryDisbursementQuery,
        include_history: bool,
        history_after_id: Option<u64>,
        history_limit: Option<usize>,
    },
    Executor {
        state: String,
        json: bool,
    },
}

impl GovCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("gov"), "gov", "Governance utilities")
            .subcommand(
                CommandBuilder::new(CommandId("gov.list"), "list", "List all proposals")
                    .arg(ArgSpec::Option(
                        OptionSpec::new("state", "state", "Path to governance state store")
                            .default("gov.db"),
                    ))
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("gov.vote"), "vote", "Vote on a proposal")
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "id",
                        "Proposal identifier",
                    )))
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "choice",
                        "Vote choice (yes/no/abstain)",
                    )))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("state", "state", "Path to governance state store")
                            .default("gov.db"),
                    ))
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("gov.status"), "status", "Show proposal status")
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "id",
                        "Proposal identifier",
                    )))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("state", "state", "Path to governance state store")
                            .default("gov.db"),
                    ))
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("gov.propose_release"),
                    "propose-release",
                    "Submit a release vote referencing a signed hash",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "hash",
                    "Release build hash",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("signature", "signature", "Release signature").multiple(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("signer", "signer", "Signer public key").multiple(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "threshold",
                    "threshold",
                    "Override release threshold",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("state", "state", "Path to governance state store")
                        .default("gov.db"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("deadline", "deadline", "Voting deadline block").default("32"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("gov.approve_release"),
                    "approve-release",
                    "Cast a vote in favour of a release proposal",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "id",
                    "Release proposal identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("state", "state", "Path to governance state store")
                        .default("gov.db"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("weight", "weight", "Voting weight").default("1"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("gov.fetch_release"),
                    "fetch-release",
                    "Download and verify an approved release artifact",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "hash",
                    "Release build hash",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("signature", "signature", "Release signature").multiple(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("signer", "signer", "Signer public key").multiple(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "dest",
                    "dest",
                    "Destination directory for the artifact",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "install",
                    "install",
                    "Install the release after verification",
                )))
                .build(),
            )
            .subcommand(GovTreasuryCmd::command())
            .subcommand(GovParamCmd::command())
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'gov'".to_string())?;

        match name {
            "list" => {
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovCmd::List { state })
            }
            "vote" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let choice = require_positional(sub_matches, "choice")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovCmd::Vote { id, choice, state })
            }
            "status" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovCmd::Status { id, state })
            }
            "propose-release" => {
                let hash = require_positional(sub_matches, "hash")?;
                let signatures = sub_matches.get_strings("signature");
                let signers = sub_matches.get_strings("signer");
                let threshold = parse_optional(take_string(sub_matches, "threshold"), "threshold")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                let deadline =
                    parse_u64_required(take_string(sub_matches, "deadline"), "deadline")?;
                Ok(GovCmd::ProposeRelease {
                    hash,
                    signatures,
                    signers,
                    threshold,
                    state,
                    deadline,
                })
            }
            "approve-release" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                let weight = parse_u64_required(take_string(sub_matches, "weight"), "weight")?;
                Ok(GovCmd::ApproveRelease { id, state, weight })
            }
            "fetch-release" => {
                let hash = require_positional(sub_matches, "hash")?;
                let signatures = sub_matches.get_strings("signature");
                let signers = sub_matches.get_strings("signer");
                let dest = take_string(sub_matches, "dest");
                let install = sub_matches.get_flag("install");
                Ok(GovCmd::FetchRelease {
                    hash,
                    signatures,
                    signers,
                    dest,
                    install,
                })
            }
            "param" => {
                let cmd = GovParamCmd::from_matches(sub_matches)?;
                Ok(GovCmd::Param { action: cmd })
            }
            "treasury" => {
                let cmd = GovTreasuryCmd::from_matches(sub_matches)?;
                Ok(GovCmd::Treasury { action: cmd })
            }
            other => Err(format!("unknown subcommand '{other}' for 'gov'")),
        }
    }
}

impl GovParamCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("gov.param"),
            "param",
            "Parameter management helpers",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.param.update"),
                "update",
                "Submit a governance proposal to update a parameter",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "key",
                "Parameter key (e.g. mempool.fee_floor_window)",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "value",
                "Proposed value",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Path to governance state store")
                    .default("gov.db"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("epoch", "epoch", "Epoch number").default("0"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "vote-deadline",
                "vote-deadline",
                "Optional voting deadline block",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "proposer",
                "proposer",
                "Override proposer identifier",
            )))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'gov param'".to_string())?;

        match name {
            "update" => {
                let key = require_positional(sub_matches, "key")?;
                let value = require_positional(sub_matches, "value")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                let epoch = parse_u64_required(take_string(sub_matches, "epoch"), "epoch")?;
                let vote_deadline =
                    parse_u64(take_string(sub_matches, "vote-deadline"), "vote-deadline")?;
                let proposer = take_string(sub_matches, "proposer");
                Ok(GovParamCmd::Update {
                    key,
                    value,
                    state,
                    epoch,
                    vote_deadline,
                    proposer,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'gov param'")),
        }
    }
}

impl GovTreasuryCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("gov.treasury"),
            "treasury",
            "Treasury disbursement helpers",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.treasury.schedule"),
                "schedule",
                "Queue a treasury disbursement",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "destination",
                "Destination address for the disbursement",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Amount (in CT) to disburse",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "amount-it",
                    "amount-it",
                    "Industrial-token amount to disburse",
                )
                .default("0"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "memo",
                "memo",
                "Optional memo recorded alongside the disbursement",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("epoch", "epoch", "Epoch to execute the disbursement").default("0"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Path to governance state store")
                    .default("gov.db"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.treasury.execute"),
                "execute",
                "Mark a queued disbursement as executed",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "id",
                "Disbursement identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "tx-hash",
                "Transaction hash authorising the disbursement",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Path to governance state store")
                    .default("gov.db"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.treasury.cancel"),
                "cancel",
                "Cancel a queued disbursement",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "id",
                "Disbursement identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "reason",
                "Reason for cancelling the disbursement",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Path to governance state store")
                    .default("gov.db"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.treasury.list"),
                "list",
                "List scheduled and completed disbursements",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Path to governance state store")
                    .default("gov.db"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.treasury.fetch"),
                "fetch",
                "Query treasury state via RPC",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("rpc", "rpc", "JSON-RPC endpoint")
                    .default("http://127.0.0.1:26658"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "status",
                "status",
                "Filter by status (scheduled/executed/cancelled)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "after-id",
                "after-id",
                "Only return disbursements with an id greater than this value",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "limit",
                "limit",
                "Maximum number of disbursements to return",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "destination",
                "destination",
                "Filter by destination account",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "min-epoch",
                "min-epoch",
                "Only include disbursements scheduled at or after this epoch",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "max-epoch",
                "max-epoch",
                "Only include disbursements scheduled at or before this epoch",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "min-amount-ct",
                "min-amount-ct",
                "Minimum CT amount",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "max-amount-ct",
                "max-amount-ct",
                "Maximum CT amount",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "min-amount-it",
                "min-amount-it",
                "Minimum IT amount",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "max-amount-it",
                "max-amount-it",
                "Maximum IT amount",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "min-created-at",
                "min-created-at",
                "Only include disbursements created at or after this timestamp",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "max-created-at",
                "max-created-at",
                "Only include disbursements created at or before this timestamp",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "min-status-ts",
                "min-status-ts",
                "Filter by minimum status timestamp (executed/cancelled)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "max-status-ts",
                "max-status-ts",
                "Filter by maximum status timestamp (executed/cancelled)",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "history",
                "history",
                "Include treasury balance history snapshots",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "history-after-id",
                "history-after-id",
                "Only return balance snapshots with an id greater than this value",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "history-limit",
                "history-limit",
                "Maximum number of balance snapshots to return",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.treasury.executor"),
                "executor",
                "Show automated treasury executor status",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Path to governance state store")
                    .default("gov.db"),
            ))
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
            .ok_or_else(|| "missing subcommand for 'gov treasury'".to_string())?;

        match name {
            "schedule" => {
                let destination = require_positional(sub_matches, "destination")?;
                let amount = parse_positional_u64(sub_matches, "amount")?;
                let amount_it =
                    parse_u64(take_string(sub_matches, "amount-it"), "amount-it")?.unwrap_or(0);
                let memo = take_string(sub_matches, "memo");
                let epoch = parse_u64_required(take_string(sub_matches, "epoch"), "epoch")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovTreasuryCmd::Schedule {
                    destination,
                    amount,
                    amount_it,
                    memo,
                    epoch,
                    state,
                })
            }
            "execute" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let tx_hash = require_positional(sub_matches, "tx-hash")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovTreasuryCmd::Execute { id, tx_hash, state })
            }
            "cancel" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let reason = require_positional(sub_matches, "reason")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovTreasuryCmd::Cancel { id, reason, state })
            }
            "list" => {
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                Ok(GovTreasuryCmd::List { state })
            }
            "fetch" => {
                let rpc = take_string(sub_matches, "rpc")
                    .unwrap_or_else(|| "http://127.0.0.1:26658".to_string());
                let mut query = TreasuryDisbursementQuery::default();
                if let Some(raw) = take_string(sub_matches, "status") {
                    query.status = Some(
                        RemoteTreasuryStatus::parse(&raw)
                            .ok_or_else(|| format!("invalid status filter: {raw}"))?,
                    );
                }
                query.after_id = parse_u64(take_string(sub_matches, "after-id"), "after-id")?;
                query.limit = parse_u64(take_string(sub_matches, "limit"), "limit")?
                    .map(|value| {
                        usize::try_from(value)
                            .map_err(|_| format!("limit {value} exceeds usize range"))
                    })
                    .transpose()?;
                query.destination = take_string(sub_matches, "destination");
                query.min_epoch = parse_u64(take_string(sub_matches, "min-epoch"), "min-epoch")?;
                query.max_epoch = parse_u64(take_string(sub_matches, "max-epoch"), "max-epoch")?;
                query.min_amount_ct =
                    parse_u64(take_string(sub_matches, "min-amount-ct"), "min-amount-ct")?;
                query.max_amount_ct =
                    parse_u64(take_string(sub_matches, "max-amount-ct"), "max-amount-ct")?;
                query.min_amount_it =
                    parse_u64(take_string(sub_matches, "min-amount-it"), "min-amount-it")?;
                query.max_amount_it =
                    parse_u64(take_string(sub_matches, "max-amount-it"), "max-amount-it")?;
                query.min_created_at =
                    parse_u64(take_string(sub_matches, "min-created-at"), "min-created-at")?;
                query.max_created_at =
                    parse_u64(take_string(sub_matches, "max-created-at"), "max-created-at")?;
                query.min_status_ts =
                    parse_u64(take_string(sub_matches, "min-status-ts"), "min-status-ts")?;
                query.max_status_ts =
                    parse_u64(take_string(sub_matches, "max-status-ts"), "max-status-ts")?;
                let include_history = sub_matches.get_flag("history");
                let history_after_id = parse_u64(
                    take_string(sub_matches, "history-after-id"),
                    "history-after-id",
                )?;
                let history_limit =
                    parse_u64(take_string(sub_matches, "history-limit"), "history-limit")?
                        .map(|value| {
                            usize::try_from(value)
                                .map_err(|_| format!("history-limit {value} exceeds usize range"))
                        })
                        .transpose()?;
                Ok(GovTreasuryCmd::Fetch {
                    rpc,
                    query,
                    include_history,
                    history_after_id,
                    history_limit,
                })
            }
            "executor" => {
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "gov.db".to_string());
                let json = sub_matches.get_flag("json");
                Ok(GovTreasuryCmd::Executor { state, json })
            }
            other => Err(format!("unknown subcommand '{other}' for 'gov treasury'")),
        }
    }
}

fn parse_param_key(name: &str) -> Option<ParamKey> {
    match name {
        "mempool.fee_floor_window" | "FeeFloorWindow" => Some(ParamKey::FeeFloorWindow),
        "mempool.fee_floor_percentile" | "FeeFloorPercentile" => Some(ParamKey::FeeFloorPercentile),
        "runtime.backend" | "RuntimeBackend" => Some(ParamKey::RuntimeBackend),
        "transport.provider" | "TransportProvider" => Some(ParamKey::TransportProvider),
        "storage.engine_policy" | "StorageEnginePolicy" => Some(ParamKey::StorageEnginePolicy),
        "treasury.percent_ct" | "TreasuryPercentCt" => Some(ParamKey::TreasuryPercentCt),
        _ => None,
    }
}

pub fn handle(cmd: GovCmd) {
    match cmd {
        GovCmd::Treasury { action } => {
            let mut stdout = io::stdout();
            if let Err(err) = handle_treasury(action, &mut stdout) {
                eprintln!("treasury command failed: {err}");
            }
        }
        GovCmd::List { state } => {
            let store = GovStore::open(state);
            for item in store.proposals().iter() {
                if let Ok((_, raw)) = item {
                    if let Ok(p) = binary::decode::<Proposal>(&raw) {
                        println!("{} {:?}", p.id, p.status);
                    }
                }
            }
        }
        GovCmd::Vote { id, choice, state } => {
            let store = GovStore::open(state);
            let c = match choice.as_str() {
                "yes" => VoteChoice::Yes,
                "no" => VoteChoice::No,
                _ => VoteChoice::Abstain,
            };
            let v = Vote {
                proposal_id: id,
                voter: "cli".into(),
                choice: c,
                weight: 1,
                received_at: 0,
            };
            if store.vote(id, v, 0).is_err() {
                eprintln!("vote failed");
            }
        }
        GovCmd::Status { id, state } => {
            let store = GovStore::open(state);
            let key = binary::encode(&id).expect("serialize proposal id");
            if let Ok(Some(raw)) = store.proposals().get(key) {
                if let Ok(p) = binary::decode::<Proposal>(&raw) {
                    println!("{:?}", p.status);
                }
            }
        }
        GovCmd::ProposeRelease {
            hash,
            mut signatures,
            mut signers,
            threshold,
            state,
            deadline,
        } => {
            let store = GovStore::open(state);
            if !signatures.is_empty() && !signers.is_empty() && signers.len() != signatures.len() {
                eprintln!("--signer count must match --signature count");
                return;
            }
            if signers.is_empty() && !signatures.is_empty() {
                let configured = the_block::provenance::release_signer_hexes();
                if signatures.len() == 1 && configured.len() == 1 {
                    signers = configured;
                } else {
                    eprintln!("--signer must be supplied for each signature");
                    return;
                }
            }
            let attestations: Vec<GovReleaseAttestation> = signatures
                .drain(..)
                .zip(signers.drain(..))
                .map(|(signature, signer)| GovReleaseAttestation { signer, signature })
                .collect();
            let threshold_value = threshold.unwrap_or_default();
            let proposal = ReleaseVote::new(
                hash,
                attestations,
                threshold_value,
                "cli".into(),
                0,
                deadline,
            );
            match controller::submit_release(&store, proposal, Some(&CliReleaseVerifier)) {
                Ok(id) => println!("release proposal {id} submitted"),
                Err(e) => eprintln!("submit failed: {e}"),
            }
        }
        GovCmd::ApproveRelease { id, state, weight } => {
            let store = GovStore::open(state);
            let ballot = ReleaseBallot {
                proposal_id: id,
                voter: "cli".into(),
                choice: VoteChoice::Yes,
                weight,
                received_at: 0,
            };
            if let Err(e) = controller::vote_release(&store, ballot) {
                eprintln!("vote failed: {e}");
            }
            if let Ok(status) = controller::tally_release(&store, id, 0) {
                println!("release status: {:?}", status);
            }
        }
        GovCmd::FetchRelease {
            hash,
            mut signatures,
            mut signers,
            dest,
            install,
        } => {
            if !signatures.is_empty() && !signers.is_empty() && signatures.len() != signers.len() {
                eprintln!("--signer count must match --signature count");
                return;
            }
            if signers.is_empty() && !signatures.is_empty() {
                let configured = the_block::provenance::release_signer_hexes();
                if signatures.len() == 1 && configured.len() == 1 {
                    signers = configured;
                } else {
                    eprintln!("--signer must be supplied for each signature");
                    return;
                }
            }
            let attestations: Vec<NodeReleaseAttestation> = signatures
                .drain(..)
                .zip(signers.drain(..))
                .map(|(signature, signer)| NodeReleaseAttestation { signer, signature })
                .collect();
            let dest_path = dest.as_deref().map(std::path::Path::new);
            match the_block::update::fetch_release(&hash, &attestations, dest_path) {
                Ok(download) => {
                    println!(
                        "downloaded {} to {}",
                        download.hash,
                        download.path.display()
                    );
                    if install {
                        if let Err(err) = the_block::update::install_release(&download.hash) {
                            eprintln!("install guard failed: {err}");
                        } else {
                            println!("install recorded for {}", download.hash);
                        }
                    }
                }
                Err(err) => eprintln!("fetch failed: {err}"),
            }
        }
        GovCmd::Param { action } => match action {
            GovParamCmd::Update {
                key,
                value,
                state,
                epoch,
                vote_deadline,
                proposer,
            } => {
                let Some(param_key) = parse_param_key(&key) else {
                    eprintln!("unknown parameter key: {key}");
                    return;
                };
                let Some(spec) = registry().iter().find(|s| s.key == param_key) else {
                    eprintln!("parameter not governable: {key}");
                    return;
                };
                let new_value = match param_key {
                    ParamKey::RuntimeBackend => {
                        let entries: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_ascii_lowercase())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        match encode_runtime_backend_policy(entries.iter().map(|s| s.as_str())) {
                            Ok(mask) => mask,
                            Err(err) => {
                                eprintln!("invalid runtime backend policy: {err}");
                                return;
                            }
                        }
                    }
                    ParamKey::TransportProvider => {
                        let entries: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_ascii_lowercase())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        match encode_transport_provider_policy(entries.iter().map(|s| s.as_str())) {
                            Ok(mask) => mask,
                            Err(err) => {
                                eprintln!("invalid transport provider policy: {err}");
                                return;
                            }
                        }
                    }
                    ParamKey::StorageEnginePolicy => {
                        let entries: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_ascii_lowercase())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        match encode_storage_engine_policy(entries.iter().map(|s| s.as_str())) {
                            Ok(mask) => mask,
                            Err(err) => {
                                eprintln!("invalid storage engine policy: {err}");
                                return;
                            }
                        }
                    }
                    _ => match value.parse::<i64>() {
                        Ok(v) => v,
                        Err(err) => {
                            eprintln!("failed to parse value `{value}`: {err}");
                            return;
                        }
                    },
                };

                if new_value < spec.min || new_value > spec.max {
                    eprintln!(
                        "value {} out of bounds (min {} max {})",
                        new_value, spec.min, spec.max
                    );
                    return;
                }
                let store = GovStore::open(&state);
                let proposal = Proposal {
                    id: 0,
                    key: param_key,
                    new_value,
                    min: spec.min,
                    max: spec.max,
                    proposer: proposer.unwrap_or_else(|| "cli".into()),
                    created_epoch: epoch,
                    vote_deadline_epoch: vote_deadline.unwrap_or(epoch),
                    activation_epoch: None,
                    status: ProposalStatus::Open,
                    deps: Vec::new(),
                };
                match controller::submit_proposal(&store, proposal) {
                    Ok(id) => println!("parameter proposal {id} submitted"),
                    Err(e) => eprintln!("submit failed: {e}"),
                }
            }
        },
    }
}

#[allow(dead_code)]
pub fn handle_with_writer(cmd: GovCmd, out: &mut dyn Write) -> io::Result<()> {
    match cmd {
        GovCmd::Treasury { action } => handle_treasury(action, out),
        other => {
            handle(other);
            Ok(())
        }
    }
}

fn handle_treasury(action: GovTreasuryCmd, out: &mut dyn Write) -> io::Result<()> {
    match action {
        GovTreasuryCmd::Schedule {
            destination,
            amount,
            amount_it,
            memo,
            epoch,
            state,
        } => {
            let store = GovStore::open(state);
            let memo_value = memo.unwrap_or_default();
            match store.queue_disbursement(&destination, amount, amount_it, &memo_value, epoch) {
                Ok(record) => match foundation_serialization::json::to_string_pretty(&record) {
                    Ok(serialized) => writeln!(out, "{serialized}")?,
                    Err(_) => writeln!(out, "queued disbursement {}", record.id)?,
                },
                Err(err) => eprintln!("queue failed: {err}"),
            }
        }
        GovTreasuryCmd::Execute { id, tx_hash, state } => {
            let store = GovStore::open(state);
            match store.execute_disbursement(id, &tx_hash) {
                Ok(record) => match foundation_serialization::json::to_string_pretty(&record) {
                    Ok(serialized) => writeln!(out, "{serialized}")?,
                    Err(_) => writeln!(out, "executed disbursement {id}")?,
                },
                Err(err) => eprintln!("execute failed: {err}"),
            }
        }
        GovTreasuryCmd::Cancel { id, reason, state } => {
            let store = GovStore::open(state);
            match store.cancel_disbursement(id, &reason) {
                Ok(record) => match foundation_serialization::json::to_string_pretty(&record) {
                    Ok(serialized) => writeln!(out, "{serialized}")?,
                    Err(_) => writeln!(out, "cancelled disbursement {id}")?,
                },
                Err(err) => eprintln!("cancel failed: {err}"),
            }
        }
        GovTreasuryCmd::List { state } => {
            let store = GovStore::open(state);
            match store.disbursements() {
                Ok(records) => {
                    match foundation_serialization::json::to_string_pretty(&DisbursementList {
                        disbursements: &records,
                    }) {
                        Ok(serialized) => writeln!(out, "{serialized}")?,
                        Err(err) => eprintln!("format failed: {err}"),
                    }
                }
                Err(err) => eprintln!("list failed: {err}"),
            }
        }
        GovTreasuryCmd::Fetch {
            rpc,
            query,
            include_history,
            history_after_id,
            history_limit,
        } => {
            let client = RpcClient::from_env();
            let disb_params = treasury_disbursement_params(&query);
            let disb_envelope: RpcEnvelope<RpcTreasuryDisbursementsResult> = call_rpc_envelope(
                &client,
                &rpc,
                "gov.treasury.disbursements",
                disb_params.clone(),
            )?;
            let disbursement_result = unwrap_rpc_result(disb_envelope)?;

            let balance_envelope: RpcEnvelope<RpcTreasuryBalanceResult> = call_rpc_envelope(
                &client,
                &rpc,
                "gov.treasury.balance",
                json::Value::Object(json::Map::new()),
            )?;
            let balance_result = unwrap_rpc_result(balance_envelope)?;

            let history_result = if include_history {
                let history_params = treasury_history_params(history_after_id, history_limit);
                let history_envelope: RpcEnvelope<RpcTreasuryHistoryResult> = call_rpc_envelope(
                    &client,
                    &rpc,
                    "gov.treasury.balance_history",
                    history_params,
                )?;
                Some(unwrap_rpc_result(history_envelope)?)
            } else {
                None
            };

            let output =
                combine_treasury_fetch_results(disbursement_result, balance_result, history_result);

            match json::to_string_pretty(&output) {
                Ok(serialized) => writeln!(out, "{serialized}")?,
                Err(err) => eprintln!("format failed: {err}"),
            }
        }
        GovTreasuryCmd::Executor { state, json } => {
            let store = GovStore::open(state);
            let snapshot = store.executor_snapshot().map_err(|err| {
                io::Error::new(io::ErrorKind::Other, format!("executor snapshot: {err}"))
            })?;
            let intents = store.execution_intents().map_err(|err| {
                io::Error::new(io::ErrorKind::Other, format!("execution intents: {err}"))
            })?;
            let disbursements = store.disbursements().map_err(|err| {
                io::Error::new(io::ErrorKind::Other, format!("load disbursements: {err}"))
            })?;
            let blocked: Vec<ExecutorDependency> = disbursements
                .into_iter()
                .filter(|d| matches!(d.status, DisbursementStatus::Scheduled))
                .filter_map(|d| {
                    let deps = parse_dependency_list(&d.memo);
                    if deps.is_empty() {
                        None
                    } else {
                        Some(ExecutorDependency {
                            disbursement_id: d.id,
                            dependencies: deps,
                            memo: d.memo,
                        })
                    }
                })
                .collect();
            let now_secs = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let lease_seconds_remaining = snapshot
                .as_ref()
                .and_then(|snap| snap.lease_expires_at)
                .and_then(|expires| expires.checked_sub(now_secs));
            let lease_last_nonce = snapshot.as_ref().and_then(|snap| snap.lease_last_nonce);
            let lease_released = snapshot
                .as_ref()
                .map(|snap| snap.lease_released)
                .unwrap_or(false);
            let staged_ids: Vec<u64> = intents
                .iter()
                .map(|intent| intent.disbursement_id)
                .collect();
            let report = ExecutorStatusReport {
                snapshot,
                lease_seconds_remaining,
                lease_last_nonce,
                lease_released,
                staged_intents: intents,
                dependency_blocks: blocked,
            };
            if json {
                let serialized = json::to_string_pretty(&report)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
                writeln!(out, "{serialized}")?;
            } else {
                if let Some(snap) = &report.snapshot {
                    writeln!(
                        out,
                        "Lease holder: {}",
                        snap.lease_holder.clone().unwrap_or_else(|| "(none)".into())
                    )?;
                    if let Some(expires) = snap.lease_expires_at {
                        let remaining = lease_seconds_remaining
                            .map(|secs| format!(" ({secs}s remaining)"))
                            .unwrap_or_default();
                        writeln!(out, "Lease expires at: {expires}{remaining}")?;
                    } else {
                        writeln!(out, "Lease expires at: (unknown)")?;
                    }
                    if let Some(renewed) = snap.lease_renewed_at {
                        writeln!(out, "Lease renewed at: {renewed}")?;
                    }
                    if let Some(nonce) = snap.last_submitted_nonce {
                        writeln!(out, "Last submitted nonce: {nonce}")?;
                    }
                    if let Some(watermark) = report.lease_last_nonce {
                        writeln!(out, "Lease nonce watermark: {watermark}")?;
                    }
                    if report.lease_released {
                        writeln!(out, "Lease status: released (awaiting new holder)")?;
                    }
                    writeln!(
                        out,
                        "Pending matured disbursements: {}",
                        snap.pending_matured
                    )?;
                    writeln!(out, "Staged intents: {}", snap.staged_intents)?;
                    writeln!(out, "Last tick at: {}", snap.last_tick_at)?;
                    if let Some(ts) = snap.last_success_at {
                        writeln!(out, "Last success at: {ts}")?;
                    }
                    if let Some(err_ts) = snap.last_error_at {
                        writeln!(out, "Last error at: {err_ts}")?;
                    }
                    if let Some(err) = snap.last_error.as_ref() {
                        writeln!(out, "Last error: {err}")?;
                    }
                } else {
                    writeln!(
                        out,
                        "executor snapshot unavailable; automation not yet initialised"
                    )?;
                }
                if !staged_ids.is_empty() {
                    writeln!(out, "Staged disbursement intents: {staged_ids:?}")?;
                }
                if !report.dependency_blocks.is_empty() {
                    writeln!(out, "Dependency blockers:")?;
                    for block in &report.dependency_blocks {
                        writeln!(
                            out,
                            "  disbursement #{}, waiting on {:?}",
                            block.disbursement_id, block.dependencies
                        )?;
                    }
                }
                writeln!(
                    out,
                    "Provisioning guidance: launch nodes with --treasury-executor --treasury-key <KEY_ID> \n  optional flags: --treasury-executor-id <IDENTITY>, --treasury-executor-lease <seconds>."
                )?;
            }
        }
    }
    Ok(())
}

fn parse_dependency_list(memo: &str) -> Vec<u64> {
    let trimmed = memo.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(Value::Object(map)) = json::from_str::<Value>(trimmed) {
        if let Some(Value::Array(items)) = map.get("depends_on") {
            return items
                .iter()
                .filter_map(|item| match item {
                    Value::Number(num) => num.as_u64(),
                    Value::String(text) => text.trim().parse::<u64>().ok(),
                    _ => None,
                })
                .collect();
        }
    }
    if let Some(rest) = trimmed
        .strip_prefix("depends_on=")
        .or_else(|| trimmed.strip_prefix("depends_on:"))
    {
        return rest
            .split(',')
            .filter_map(|entry| entry.trim().parse::<u64>().ok())
            .collect();
    }
    Vec::new()
}
