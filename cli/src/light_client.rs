use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::codec_helpers::{json_from_str, json_to_string, json_to_string_pretty};
use crate::json_helpers::{
    empty_object, json_array_from, json_bool, json_f64, json_null, json_object_from,
    json_rpc_request, json_string, json_u64,
};
use crate::parse_utils::{
    optional_path, parse_bool, parse_u64, parse_u64_required, parse_usize, parse_usize_required,
    require_positional, take_string,
};
use crate::rpc::RpcClient;
use crate::tx::{TxDidAnchor, TxDidAnchorAttestation};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::signatures::ed25519::SigningKey;
use diagnostics::{anyhow, Context, Result};
use foundation_serialization::json::{Map as JsonMap, Value};
use foundation_serialization::{Deserialize, Serialize};
use light_client::{self, SyncOptions};

const MAX_DID_DOC_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub enum LightClientCmd {
    /// Show current proof rebate balance
    RebateStatus { url: String },
    /// Inspect historical proof rebate claims
    RebateHistory(RebateHistoryArgs),
    /// Inspect recent micro-shard root bundles
    RootBundles(RootBundlesArgs),
    /// Interact with the decentralized identifier registry
    Did { action: DidCmd },
    /// Inspect or configure device-aware sync policy
    Device { action: DeviceCmd },
}

#[derive(Debug)]
pub enum DidCmd {
    /// Anchor a DID document on-chain
    Anchor(DidAnchorArgs),
    /// Resolve the latest DID document for an address
    Resolve(DidResolveArgs),
}

#[derive(Debug)]
pub enum DeviceCmd {
    /// Inspect current device probes and gating decision
    Status {
        /// Emit JSON instead of human-readable text
        json: bool,
    },
    /// Persist an override that skips the charging requirement
    IgnoreCharging {
        /// Enable (`true`) or disable (`false`) the override
        enable: bool,
    },
    /// Remove all persisted overrides
    ClearOverrides,
}

#[derive(Debug, Clone)]
pub struct DidAnchorArgs {
    /// Path to the DID document JSON file
    pub file: PathBuf,
    /// Override the address used for anchoring (defaults to the public key hex)
    pub address: Option<String>,
    /// Nonce for replay protection
    pub nonce: u64,
    /// Hex-encoded owner secret key
    pub secret: Option<String>,
    /// File containing the owner secret key (hex)
    pub secret_file: Option<PathBuf>,
    /// Optional remote signer material (JSON or raw hex secret)
    pub remote_signer: Option<PathBuf>,
    /// JSON-RPC endpoint
    pub rpc: String,
    /// Skip submission and emit the signed payload for offline broadcast
    pub sign_only: bool,
}

#[derive(Debug, Clone)]
pub struct DidResolveArgs {
    /// Address whose DID should be resolved
    pub address: String,
    /// JSON-RPC endpoint
    pub rpc: String,
    /// Emit JSON instead of human-readable output
    pub json: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AnchorKeyMaterial {
    pub address: Option<String>,
    pub nonce: u64,
    pub owner_secret: Vec<u8>,
    pub remote_secret: Option<Vec<u8>>,
    pub remote_signer_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnchorRemoteAttestation {
    pub signer: String,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct RebateHistoryArgs {
    pub url: String,
    /// Hex-encoded relayer identifier to filter receipts
    pub relayer: Option<String>,
    /// Resume listing before this block height
    pub cursor: Option<u64>,
    /// Maximum number of receipts to fetch
    pub limit: usize,
    /// Emit JSON instead of human-readable output
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct RootBundlesArgs {
    pub url: String,
    pub limit: usize,
    pub json: bool,
}

impl LightClientCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("light-client"),
            "light-client",
            "Light-client utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.rebate-status"),
                "rebate-status",
                "Show current proof rebate balance",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "JSON-RPC endpoint")
                    .default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.rebate-history"),
                "rebate-history",
                "Inspect historical proof rebate claims",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "JSON-RPC endpoint")
                    .default("http://localhost:26658"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "relayer",
                "relayer",
                "Hex-encoded relayer identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "cursor",
                "cursor",
                "Resume listing before this block height",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Maximum number of receipts").default("25"),
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
                CommandId("light-client.root-bundles"),
                "root-bundles",
                "Inspect recent micro-shard root bundles",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "JSON-RPC endpoint")
                    .default("http://localhost:26658"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Number of bundles").default("5"),
            ))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "json",
                "json",
                "Emit JSON instead of human-readable output",
            )))
            .build(),
        )
        .subcommand(DidCmd::command())
        .subcommand(DeviceCmd::command())
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'light-client'".to_string())?;

        match name {
            "rebate-status" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(LightClientCmd::RebateStatus { url })
            }
            "rebate-history" => {
                let args = RebateHistoryArgs::from_matches(sub_matches)?;
                Ok(LightClientCmd::RebateHistory(args))
            }
            "root-bundles" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?
                    .unwrap_or(5);
                let json = sub_matches.get_flag("json");
                Ok(LightClientCmd::RootBundles(RootBundlesArgs { url, limit, json }))
            }
            "did" => {
                let action = DidCmd::from_matches(sub_matches)?;
                Ok(LightClientCmd::Did { action })
            }
            "device" => {
                let action = DeviceCmd::from_matches(sub_matches)?;
                Ok(LightClientCmd::Device { action })
            }
            other => Err(format!("unknown subcommand '{other}' for 'light-client'")),
        }
    }
}

impl DidCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("light-client.did"),
            "did",
            "Decentralized identifier registry operations",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.did.anchor"),
                "anchor",
                "Anchor a DID document on-chain",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "file",
                "Path to the DID document JSON file",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "address",
                "address",
                "Override the address used for anchoring",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("nonce", "nonce", "Nonce for replay protection").required(true),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "secret",
                "secret",
                "Hex-encoded owner secret key",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "secret-file",
                "secret-file",
                "File containing the owner secret key (hex)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "remote-signer",
                "remote-signer",
                "Optional remote signer material (JSON or raw hex)",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("rpc", "rpc", "JSON-RPC endpoint")
                    .default("http://127.0.0.1:26658"),
            ))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "sign_only",
                "sign-only",
                "Skip submission and emit the signed payload",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.did.resolve"),
                "resolve",
                "Resolve the latest DID document for an address",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "address",
                "Address whose DID should be resolved",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("rpc", "rpc", "JSON-RPC endpoint")
                    .default("http://127.0.0.1:26658"),
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

    pub fn from_matches(matches: &Matches) -> Result<DidCmd, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'light-client did'".to_string())?;

        match name {
            "anchor" => {
                let args = DidAnchorArgs::from_matches(sub_matches)?;
                Ok(DidCmd::Anchor(args))
            }
            "resolve" => {
                let args = DidResolveArgs::from_matches(sub_matches)?;
                Ok(DidCmd::Resolve(args))
            }
            other => Err(format!(
                "unknown subcommand '{other}' for 'light-client did'"
            )),
        }
    }
}

impl DeviceCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("light-client.device"),
            "device",
            "Inspect or configure device-aware sync policy",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.device.status"),
                "status",
                "Inspect current device probes and gating decision",
            )
            .arg(ArgSpec::Flag(FlagSpec::new(
                "json",
                "json",
                "Emit JSON instead of human-readable text",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.device.ignore_charging"),
                "ignore-charging",
                "Persist an override that skips the charging requirement",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "enable",
                    "enable",
                    "Enable (true) or disable (false) the override",
                )
                .required(true),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("light-client.device.clear_overrides"),
                "clear-overrides",
                "Remove all persisted overrides",
            )
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<DeviceCmd, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'light-client device'".to_string())?;

        match name {
            "status" => Ok(DeviceCmd::Status {
                json: sub_matches.get_flag("json"),
            }),
            "ignore-charging" => {
                let enable = parse_bool(take_string(sub_matches, "enable"), false, "enable")?;
                Ok(DeviceCmd::IgnoreCharging { enable })
            }
            "clear-overrides" => Ok(DeviceCmd::ClearOverrides),
            other => Err(format!(
                "unknown subcommand '{other}' for 'light-client device'"
            )),
        }
    }
}

impl DidAnchorArgs {
    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let file = PathBuf::from(require_positional(matches, "file")?);
        let address = take_string(matches, "address");
        let nonce = parse_u64_required(take_string(matches, "nonce"), "nonce")?;
        let secret = take_string(matches, "secret");
        let secret_file = optional_path(matches, "secret-file");
        let remote_signer = optional_path(matches, "remote-signer");
        if secret.is_none() && secret_file.is_none() {
            return Err("either --secret or --secret-file must be provided".to_string());
        }
        let rpc =
            take_string(matches, "rpc").unwrap_or_else(|| "http://127.0.0.1:26658".to_string());
        let sign_only = matches.get_flag("sign_only");
        Ok(DidAnchorArgs {
            file,
            address,
            nonce,
            secret,
            secret_file,
            remote_signer,
            rpc,
            sign_only,
        })
    }
}

impl DidResolveArgs {
    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let address = require_positional(matches, "address")?;
        let rpc =
            take_string(matches, "rpc").unwrap_or_else(|| "http://127.0.0.1:26658".to_string());
        let json = matches.get_flag("json");
        Ok(DidResolveArgs { address, rpc, json })
    }
}

impl RebateHistoryArgs {
    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let url =
            take_string(matches, "url").unwrap_or_else(|| "http://localhost:26658".to_string());
        let relayer = take_string(matches, "relayer");
        let cursor = parse_u64(take_string(matches, "cursor"), "cursor")?;
        let limit = parse_usize_required(take_string(matches, "limit"), "limit")?;
        let json = matches.get_flag("json");
        Ok(RebateHistoryArgs {
            url,
            relayer,
            cursor,
            limit,
            json,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AnchorRecord {
    pub address: String,
    pub document: Value,
    pub hash: String,
    pub nonce: u64,
    pub updated_at: u64,
    pub public_key: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_attestation: Option<AnchorRemoteAttestation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedDid {
    pub address: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub document: Option<Value>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub nonce: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_attestation: Option<AnchorRemoteAttestation>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcEnvelope<T> {
    #[serde(default = "foundation_serialization::defaults::default")]
    result: Option<T>,
    #[serde(default = "foundation_serialization::defaults::default")]
    error: Option<RpcErrorBody>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RebateHistoryResult {
    receipts: Vec<RebateHistoryReceipt>,
    #[serde(default = "foundation_serialization::defaults::default")]
    next: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct RebateHistoryReceipt {
    height: u64,
    amount: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    relayers: Vec<RebateHistoryRelayer>,
}

#[derive(Debug, Clone, Deserialize)]
struct RebateHistoryRelayer {
    id: String,
    amount: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LightHeader {
    pub height: u64,
    pub hash: String,
    pub difficulty: u64,
}

pub fn handle(cmd: LightClientCmd) {
    match cmd {
        LightClientCmd::RebateStatus { url } => {
            let client = RpcClient::from_env();
            if let Err(err) = query_rebate_status(&client, &url) {
                eprintln!("{}", err);
            }
        }
        LightClientCmd::RebateHistory(args) => {
            let client = RpcClient::from_env();
            if let Err(err) = query_rebate_history(&client, &args) {
                eprintln!("{}", err);
            }
        }
        LightClientCmd::RootBundles(args) => {
            let client = RpcClient::from_env();
            if let Err(err) = query_root_bundles(&client, &args) {
                eprintln!("{}", err);
            }
        }
        LightClientCmd::Did { action } => match action {
            DidCmd::Anchor(args) => {
                if let Err(err) = run_anchor_command(args) {
                    eprintln!("{}", err);
                }
            }
            DidCmd::Resolve(args) => {
                if let Err(err) = run_resolve_command(args) {
                    eprintln!("{}", err);
                }
            }
        },
        LightClientCmd::Device { action } => match action {
            DeviceCmd::Status { json } => {
                if let Err(err) = run_device_status(json) {
                    eprintln!("{}", err);
                }
            }
            DeviceCmd::IgnoreCharging { enable } => {
                if let Err(err) = toggle_charging_override(enable) {
                    eprintln!("{}", err);
                }
            }
            DeviceCmd::ClearOverrides => {
                if let Err(err) = clear_device_overrides() {
                    eprintln!("{}", err);
                }
            }
        },
    }
}

fn query_rebate_status(client: &RpcClient, url: &str) -> Result<()> {
    let payload = json_rpc_request("light_client.rebate_status", empty_object());
    let response = client
        .call(url, &payload)
        .context("rebate status RPC call failed")?;
    let text = response
        .text()
        .context("failed to read rebate status response")?;
    println!("{}", text);
    Ok(())
}

fn query_rebate_history(client: &RpcClient, args: &RebateHistoryArgs) -> Result<()> {
    let mut params = JsonMap::new();
    if let Some(relayer) = &args.relayer {
        params.insert("relayer".to_string(), json_string(relayer));
    }
    if let Some(cursor) = args.cursor {
        params.insert("cursor".to_string(), json_u64(cursor));
    }
    params.insert("limit".to_string(), json_u64(args.limit as u64));

    let payload = json_rpc_request("light_client.rebate_history", Value::Object(params));
    let response = client
        .call(&args.url, &payload)
        .context("rebate history RPC call failed")?;
    let text = response
        .text()
        .context("failed to read rebate history response")?;
    let value: Value = json_from_str(&text).context("failed to parse rebate history response")?;
    let envelope: RpcEnvelope<RebateHistoryResult> =
        foundation_serialization::json::from_value(value.clone())
            .context("invalid rebate history envelope")?;
    if let Some(err) = envelope.error {
        anyhow::bail!("{} (code {})", err.message, err.code);
    }
    let result = envelope.result.unwrap_or_default();
    if args.json {
        println!("{}", json_to_string_pretty(&value)?);
        return Ok(());
    }
    if result.receipts.is_empty() {
        println!("No rebate receipts found.");
    } else {
        for receipt in &result.receipts {
            println!("Block {} – {} BLOCK", receipt.height, receipt.amount);
            for relayer in &receipt.relayers {
                println!("  {}: {}", relayer.id, relayer.amount);
            }
        }
    }
    if let Some(next) = result.next {
        println!("Next cursor: {}", next);
    }
    Ok(())
}

fn query_root_bundles(client: &RpcClient, args: &RootBundlesArgs) -> Result<()> {
    let mut params = JsonMap::new();
    params.insert("n".to_string(), json_u64(args.limit as u64));

    let payload = json_rpc_request("microshard.roots.last", Value::Object(params));
    let response = client
        .call(&args.url, &payload)
        .context("root bundle RPC call failed")?;
    let text = response
        .text()
        .context("failed to read root bundle response")?;
    let value: Value = json_from_str(&text).context("failed to parse root bundle response")?;
    let envelope: RpcEnvelope<Vec<RootBundleSummaryJson>> =
        foundation_serialization::json::from_value(value.clone())
            .context("invalid microshard.roots.last response")?;
    if let Some(err) = envelope.error {
        anyhow::bail!("{} (code {})", err.message, err.code);
    }
    let summaries = envelope.result.unwrap_or_default();
    if args.json {
        println!("{}", json_to_string_pretty(&value)?);
        return Ok(());
    }
    if summaries.is_empty() {
        println!("No micro-shard root bundles recorded.");
    } else {
        for summary in summaries {
            println!(
                "Slot {} ({}) – hash {} entries {} available_until {}",
                summary.slot,
                summary.size_class,
                summary.bundle_hash,
                summary.entry_count,
                summary.available_until,
            );
        }
    }
    Ok(())
}

fn run_device_status(json: bool) -> Result<()> {
    let cfg = light_client::load_user_config().unwrap_or_default();
    let opts = SyncOptions::default().apply_config(&cfg);
    let probe = match light_client::default_probe() {
        Ok(p) => p,
        Err(err) => {
            if json {
                let gating = opts
                    .gating_reason(&light_client::DeviceStatus::from(opts.fallback))
                    .map(|g| json_string(g.as_str()))
                    .unwrap_or_else(json_null);
                let payload =
                    json_object_from([("error", json_string(err.to_string())), ("gating", gating)]);
                println!("{}", json_to_string_pretty(&payload)?);
            } else {
                println!("device probe unavailable: {}", err);
            }
            return Ok(());
        }
    };
    let watcher = light_client::DeviceStatusWatcher::new(probe, opts.fallback, opts.stale_after);
    let snapshot = runtime::block_on(async { watcher.poll().await });
    let gating = opts.gating_reason(&snapshot.status);
    if json {
        let observed_ms = snapshot
            .observed_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let observed_ms = u64::try_from(observed_ms).unwrap_or(u64::MAX);
        let stale_ms = u64::try_from(snapshot.stale_for.as_millis()).unwrap_or(u64::MAX);
        let gating_value = gating
            .map(|g| json_string(g.as_str()))
            .unwrap_or_else(json_null);
        let payload = json_object_from([
            ("wifi", json_bool(snapshot.status.on_wifi)),
            ("charging", json_bool(snapshot.status.is_charging)),
            (
                "battery",
                json_f64(f64::from(snapshot.status.battery_level)),
            ),
            (
                "freshness",
                json_string(snapshot.freshness.as_label().to_owned()),
            ),
            ("observed_at_millis", json_u64(observed_ms)),
            ("stale_for_millis", json_u64(stale_ms)),
            ("gating", gating_value),
        ]);
        println!("{}", json_to_string_pretty(&payload)?);
    } else {
        println!(
            "Wi-Fi: {} (freshness: {:?})",
            if snapshot.status.on_wifi {
                "available"
            } else {
                "offline"
            },
            snapshot.freshness
        );
        println!(
            "Charging: {}",
            if snapshot.status.is_charging {
                "yes"
            } else {
                "no"
            }
        );
        println!(
            "Battery level: {:.0}%",
            snapshot.status.battery_level * 100.0
        );
        match gating {
            Some(reason) => println!("Sync gating: {}", reason.as_str()),
            None => println!("Sync gating: clear"),
        }
    }
    Ok(())
}

fn toggle_charging_override(enable: bool) -> Result<()> {
    let mut cfg = light_client::load_user_config().unwrap_or_default();
    cfg.ignore_charging_requirement = enable;
    light_client::save_user_config(&cfg)?;
    if enable {
        println!("Charging requirement disabled for background sync");
    } else {
        println!("Charging requirement restored to default");
    }
    Ok(())
}

fn clear_device_overrides() -> Result<()> {
    light_client::save_user_config(&light_client::LightClientConfig::default())?;
    println!("Cleared light-client device overrides");
    Ok(())
}

fn run_anchor_command(args: DidAnchorArgs) -> Result<()> {
    let (document, material) = prepare_anchor_inputs(&args)?;
    let tx = build_anchor_transaction(&document, &material)?;
    if args.sign_only {
        let payload =
            foundation_serialization::json::to_value(&tx).context("serialize anchor payload")?;
        println!(
            "{}",
            json_to_string_pretty(&payload).context("pretty-print anchor payload")?
        );
        return Ok(());
    }
    let client = RpcClient::from_env();
    let record = submit_anchor(&client, &args.rpc, &tx)?;
    let header = latest_header(&client, &args.rpc)?;
    println!(
        "Anchored DID {} with hash {} at height {}",
        record.address, record.hash, header.height
    );
    println!("Nonce: {}", record.nonce);
    if let Some(att) = &record.remote_attestation {
        println!("Remote signer: {}", att.signer);
    }
    Ok(())
}

fn run_resolve_command(args: DidResolveArgs) -> Result<()> {
    let client = RpcClient::from_env();
    let record = resolve_did_record(&client, &args.rpc, &args.address)?;
    if args.json {
        println!(
            "{}",
            json_to_string_pretty(&record).context("serialize resolve output")?
        );
        return Ok(());
    }
    println!("Address: {}", record.address);
    match &record.hash {
        Some(hash) => println!("Hash: {}", hash),
        None => println!("Hash: <none>"),
    }
    match record.nonce {
        Some(nonce) => println!("Nonce: {}", nonce),
        None => println!("Nonce: <none>"),
    }
    match record.updated_at {
        Some(ts) => println!("Updated at: {}", ts),
        None => println!("Updated at: <none>"),
    }
    match &record.document {
        Some(doc) => {
            println!(
                "Document:\n{}",
                json_to_string_pretty(doc)
                    .or_else(|_| json_to_string(doc))
                    .unwrap_or_else(|_| format!("{doc:?}"))
            );
        }
        None => println!("Document: <none>"),
    }
    if let Some(att) = &record.remote_attestation {
        println!("Remote signer: {}", att.signer);
    }
    Ok(())
}

fn prepare_anchor_inputs(args: &DidAnchorArgs) -> Result<(Value, AnchorKeyMaterial)> {
    let contents = fs::read_to_string(&args.file)
        .with_context(|| format!("failed to read DID document from {}", args.file.display()))?;
    let document: Value = json_from_str(&contents)
        .with_context(|| format!("DID document {} is not valid JSON", args.file.display()))?;

    let owner_secret = if let Some(secret) = &args.secret {
        decode_secret(secret)
    } else if let Some(path) = &args.secret_file {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read secret key from {}", path.display()))?;
        decode_secret(&text)
    } else {
        Err(anyhow!("missing owner secret key"))
    }?;

    let mut material = AnchorKeyMaterial {
        address: args.address.clone(),
        nonce: args.nonce,
        owner_secret,
        remote_secret: None,
        remote_signer_hex: None,
    };

    if let Some(path) = &args.remote_signer {
        let (secret, signer) = load_remote_signer(path)?;
        material.remote_secret = Some(secret);
        material.remote_signer_hex = signer.map(|s| s.to_lowercase());
    }

    Ok((document, material))
}

fn decode_secret(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.trim();
    let normalized = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = crypto_suite::hex::decode(normalized).context("secret key must be hex encoded")?;
    if bytes.len() != 32 && bytes.len() != 64 {
        return Err(anyhow!("secret key must be 32 or 64 bytes"));
    }
    Ok(bytes)
}

fn load_remote_signer(path: &Path) -> Result<(Vec<u8>, Option<String>)> {
    let raw = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read remote signer material from {}",
            path.display()
        )
    })?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("remote signer material is empty"));
    }
    if trimmed.starts_with('{') {
        #[derive(Deserialize)]
        struct RemoteSignerFile {
            secret: String,
            #[serde(default = "foundation_serialization::defaults::default")]
            signer: Option<String>,
        }
        let parsed: RemoteSignerFile =
            json_from_str(trimmed).context("remote signer file must be JSON with 'secret'")?;
        let secret = decode_secret(&parsed.secret)?;
        Ok((secret, parsed.signer))
    } else {
        let secret = decode_secret(trimmed)?;
        Ok((secret, None))
    }
}

fn key_from_bytes(bytes: &[u8]) -> Result<SigningKey> {
    match bytes.len() {
        32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            Ok(SigningKey::from_bytes(&arr))
        }
        64 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            Ok(SigningKey::from_bytes(&arr))
        }
        _ => Err(anyhow!("ed25519 private key must be 32 or 64 bytes")),
    }
}

pub fn build_anchor_transaction(doc: &Value, material: &AnchorKeyMaterial) -> Result<TxDidAnchor> {
    let canonical = json_to_string(doc).context("canonicalize DID document")?;
    if canonical.as_bytes().len() > MAX_DID_DOC_BYTES {
        return Err(anyhow!("DID document exceeds {} bytes", MAX_DID_DOC_BYTES));
    }
    let owner_key = key_from_bytes(&material.owner_secret)?;
    let owner_public = owner_key.verifying_key().to_bytes();
    let address = material
        .address
        .clone()
        .unwrap_or_else(|| crypto_suite::hex::encode(owner_public));

    let mut tx = TxDidAnchor {
        address,
        public_key: owner_public.to_vec(),
        document: canonical,
        nonce: material.nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let owner_sig = owner_key.sign(tx.owner_digest().as_ref());
    tx.signature = owner_sig.to_bytes().to_vec();

    if let Some(remote_secret) = &material.remote_secret {
        let remote_key = key_from_bytes(remote_secret)?;
        let derived = crypto_suite::hex::encode(remote_key.verifying_key().to_bytes());
        let signer_hex = material
            .remote_signer_hex
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| derived.clone());
        if signer_hex != derived {
            return Err(anyhow!("remote signer hex does not match provided secret"));
        }
        let att_sig = remote_key.sign(tx.remote_digest().as_ref());
        tx.remote_attestation = Some(TxDidAnchorAttestation {
            signer: signer_hex,
            signature: crypto_suite::hex::encode(att_sig.to_bytes()),
        });
    }

    Ok(tx)
}

fn json_byte_array(bytes: &[u8]) -> Value {
    json_array_from(bytes.iter().copied().map(|b| json_u64(u64::from(b))))
}

fn remote_attestation_value(attestation: &Option<TxDidAnchorAttestation>) -> Value {
    match attestation {
        Some(att) => json_object_from([
            ("signer", json_string(&att.signer)),
            ("signature", json_string(&att.signature)),
        ]),
        None => json_null(),
    }
}

pub fn anchor_request_params(tx: &TxDidAnchor) -> Value {
    let mut map = JsonMap::new();
    map.insert("address".to_string(), json_string(&tx.address));
    map.insert("public_key".to_string(), json_byte_array(&tx.public_key));
    map.insert("document".to_string(), json_string(&tx.document));
    map.insert("nonce".to_string(), json_u64(tx.nonce));
    map.insert("signature".to_string(), json_byte_array(&tx.signature));
    map.insert(
        "remote_attestation".to_string(),
        remote_attestation_value(&tx.remote_attestation),
    );
    Value::Object(map)
}

pub fn anchor_record_from_value(result: Value) -> Result<AnchorRecord> {
    parse_anchor_record(&result).context("unexpected identity.anchor response format")
}

fn anchor_envelope_to_record(envelope: RpcEnvelope<Value>) -> Result<AnchorRecord> {
    if let Some(err) = envelope.error {
        return Err(anyhow!(
            "identity.anchor error {} (code {})",
            err.message,
            err.code
        ));
    }
    let result = envelope
        .result
        .ok_or_else(|| anyhow!("missing identity.anchor result"))?;
    if let Some(code) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("identity.anchor rejected request: {}", code));
    }
    anchor_record_from_value(result)
}

#[allow(dead_code)]
pub fn anchor_envelope_value_to_record(value: Value) -> Result<AnchorRecord> {
    let envelope = parse_value_envelope(&value, "identity.anchor")
        .context("invalid identity.anchor envelope")?;
    anchor_envelope_to_record(envelope)
}

fn latest_header_from_envelope(envelope: RpcEnvelope<LightHeader>) -> Result<LightHeader> {
    if let Some(err) = envelope.error {
        return Err(anyhow!(
            "light.latest_header error {} (code {})",
            err.message,
            err.code
        ));
    }
    envelope
        .result
        .ok_or_else(|| anyhow!("missing light.latest_header result"))
}

#[allow(dead_code)]
pub fn latest_header_value_to_header(value: Value) -> Result<LightHeader> {
    let envelope = parse_header_envelope(&value).context("invalid light.latest_header envelope")?;
    latest_header_from_envelope(envelope)
}

pub fn resolved_did_from_value(result: Value) -> Result<ResolvedDid> {
    parse_resolved_did(&result).context("unexpected identity.resolve response format")
}

fn resolved_did_envelope_to_record(envelope: RpcEnvelope<Value>) -> Result<ResolvedDid> {
    if let Some(err) = envelope.error {
        return Err(anyhow!(
            "identity.resolve error {} (code {})",
            err.message,
            err.code
        ));
    }
    let result = envelope
        .result
        .ok_or_else(|| anyhow!("missing identity.resolve result"))?;
    if let Some(code) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!(
            "identity.resolve returned application error: {}",
            code
        ));
    }
    resolved_did_from_value(result)
}

#[allow(dead_code)]
pub fn resolved_did_value_to_record(value: Value) -> Result<ResolvedDid> {
    let envelope = parse_value_envelope(&value, "identity.resolve")
        .context("invalid identity.resolve envelope")?;
    resolved_did_envelope_to_record(envelope)
}

pub fn submit_anchor(client: &RpcClient, url: &str, tx: &TxDidAnchor) -> Result<AnchorRecord> {
    let params = anchor_request_params(tx);
    let payload = json_rpc_request("identity.anchor", params);
    let envelope = client
        .call(url, &payload)
        .context("identity.anchor RPC call failed")?
        .json::<RpcEnvelope<Value>>()
        .context("failed to decode identity.anchor response")?;
    anchor_envelope_to_record(envelope)
}

pub fn latest_header(client: &RpcClient, url: &str) -> Result<LightHeader> {
    let payload = json_rpc_request("light.latest_header", json_null());
    let envelope = client
        .call(url, &payload)
        .context("light.latest_header RPC call failed")?
        .json::<RpcEnvelope<LightHeader>>()
        .context("failed to decode light.latest_header response")?;
    latest_header_from_envelope(envelope)
}

pub fn resolve_did_record(client: &RpcClient, url: &str, address: &str) -> Result<ResolvedDid> {
    let params = json_object_from([("address", json_string(address))]);
    let payload = json_rpc_request("identity.resolve", params);
    let envelope = client
        .call(url, &payload)
        .context("identity.resolve RPC call failed")?
        .json::<RpcEnvelope<Value>>()
        .context("failed to decode identity.resolve response")?;
    resolved_did_envelope_to_record(envelope)
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn parse_string_field(map: &JsonMap, field: &str, context: &str) -> Result<String> {
    map.get(field)
        .ok_or_else(|| anyhow!("{context} missing field '{field}'"))?
        .as_str()
        .map(|value| value.to_owned())
        .ok_or_else(|| anyhow!("{context} field '{field}' must be a string"))
}

fn parse_optional_string_field(
    map: &JsonMap,
    field: &str,
    context: &str,
) -> Result<Option<String>> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| anyhow!("{context} field '{field}' must be a string when present")),
    }
}

fn parse_u64_field(map: &JsonMap, field: &str, context: &str) -> Result<u64> {
    map.get(field)
        .ok_or_else(|| anyhow!("{context} missing field '{field}'"))?
        .as_u64()
        .ok_or_else(|| anyhow!("{context} field '{field}' must be an unsigned integer"))
}

fn parse_optional_u64_field(map: &JsonMap, field: &str, context: &str) -> Result<Option<u64>> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(|| {
            anyhow!("{context} field '{field}' must be an unsigned integer when present")
        }),
    }
}

fn parse_remote_attestation(
    map: &JsonMap,
    context: &str,
) -> Result<Option<AnchorRemoteAttestation>> {
    match map.get("remote_attestation") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(object)) => {
            let att_context = format!("{context} remote_attestation");
            let signer = parse_string_field(object, "signer", &att_context)?;
            let signature = parse_string_field(object, "signature", &att_context)?;
            Ok(Some(AnchorRemoteAttestation { signer, signature }))
        }
        Some(other) => Err(anyhow!(
            "{context} field 'remote_attestation' must be an object or null, got {}",
            value_kind(other)
        )),
    }
}

fn parse_anchor_record(value: &Value) -> Result<AnchorRecord> {
    let context = "identity.anchor result";
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("{context} must be an object, got {}", value_kind(value)))?;

    let address = parse_string_field(map, "address", context)?;
    let document_value = map
        .get("document")
        .ok_or_else(|| anyhow!("{context} missing field 'document'"))?;
    let document = match document_value {
        Value::String(raw) => json_from_str(raw).unwrap_or_else(|_| Value::String(raw.clone())),
        other => other.clone(),
    };
    let hash = parse_string_field(map, "hash", context)?;
    let nonce = parse_u64_field(map, "nonce", context)?;
    let updated_at = parse_u64_field(map, "updated_at", context)?;
    let public_key = parse_string_field(map, "public_key", context)?;
    let remote_attestation = parse_remote_attestation(map, context)?;

    Ok(AnchorRecord {
        address,
        document,
        hash,
        nonce,
        updated_at,
        public_key,
        remote_attestation,
    })
}

fn parse_optional_error(value: Option<&Value>) -> Result<Option<RpcErrorBody>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => {
            let context = "RPC error";
            let code_value = map
                .get("code")
                .ok_or_else(|| anyhow!("{context} missing field 'code'"))?;
            let code = if let Some(value) = code_value.as_i64() {
                value
            } else if let Some(value) = code_value.as_u64() {
                if value <= i64::MAX as u64 {
                    value as i64
                } else {
                    return Err(anyhow!(
                        "{context} field 'code' exceeds supported range for signed integers"
                    ));
                }
            } else {
                return Err(anyhow!("{context} field 'code' must be a signed integer"));
            };
            let message = map
                .get("message")
                .ok_or_else(|| anyhow!("{context} missing field 'message'"))?
                .as_str()
                .ok_or_else(|| anyhow!("{context} field 'message' must be a string"))?
                .to_owned();
            Ok(Some(RpcErrorBody { code, message }))
        }
        Some(other) => Err(anyhow!(
            "RPC error body must be an object or null, got {}",
            value_kind(other)
        )),
    }
}

fn parse_value_envelope(value: &Value, method: &str) -> Result<RpcEnvelope<Value>> {
    let context = format!("{method} envelope");
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("{context} must be an object, got {}", value_kind(value)))?;
    let error = parse_optional_error(map.get("error"))?;
    let result = match map.get("result") {
        None | Some(Value::Null) => None,
        Some(other) => Some(other.clone()),
    };
    Ok(RpcEnvelope { result, error })
}

fn parse_light_header(value: &Value) -> Result<LightHeader> {
    let context = "light.latest_header result";
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("{context} must be an object, got {}", value_kind(value)))?;
    let height = parse_u64_field(map, "height", context)?;
    let hash = parse_string_field(map, "hash", context)?;
    let difficulty = parse_u64_field(map, "difficulty", context)?;
    Ok(LightHeader {
        height,
        hash,
        difficulty,
    })
}

fn parse_header_envelope(value: &Value) -> Result<RpcEnvelope<LightHeader>> {
    let context = "light.latest_header envelope";
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("{context} must be an object, got {}", value_kind(value)))?;
    let error = parse_optional_error(map.get("error"))?;
    let result = match map.get("result") {
        None | Some(Value::Null) => None,
        Some(other) => {
            Some(parse_light_header(other).context("invalid light.latest_header result payload")?)
        }
    };
    Ok(RpcEnvelope { result, error })
}

fn parse_resolved_did(value: &Value) -> Result<ResolvedDid> {
    let context = "identity.resolve result";
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("{context} must be an object, got {}", value_kind(value)))?;

    let address = parse_string_field(map, "address", context)?;
    let document = match map.get("document") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => {
            Some(json_from_str(raw).unwrap_or_else(|_| Value::String(raw.clone())))
        }
        Some(other) => Some(other.clone()),
    };
    let hash = parse_optional_string_field(map, "hash", context)?;
    let nonce = parse_optional_u64_field(map, "nonce", context)?;
    let updated_at = parse_optional_u64_field(map, "updated_at", context)?;
    let public_key = parse_optional_string_field(map, "public_key", context)?;
    let remote_attestation = parse_remote_attestation(map, context)?;

    Ok(ResolvedDid {
        address,
        document,
        hash,
        nonce,
        updated_at,
        public_key,
        remote_attestation,
    })
}
