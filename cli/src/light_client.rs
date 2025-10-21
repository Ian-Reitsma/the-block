use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::codec_helpers::{json_from_str, json_to_string, json_to_string_pretty};
use crate::parse_utils::{
    optional_path, parse_bool, parse_u64, parse_u64_required, parse_usize_required,
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

fn json_map_from(pairs: Vec<(String, Value)>) -> JsonMap {
    let mut map = JsonMap::new();
    for (key, value) in pairs {
        map.insert(key, value);
    }
    map
}

fn json_object_from(pairs: Vec<(String, Value)>) -> Value {
    Value::Object(json_map_from(pairs))
}

#[derive(Debug)]
pub enum LightClientCmd {
    /// Show current proof rebate balance
    RebateStatus { url: String },
    /// Inspect historical proof rebate claims
    RebateHistory(RebateHistoryArgs),
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
struct AnchorRecordWire {
    address: String,
    document: String,
    hash: String,
    nonce: u64,
    updated_at: u64,
    public_key: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    remote_attestation: Option<AnchorRemoteAttestation>,
}

impl AnchorRecordWire {
    fn into_record(self) -> AnchorRecord {
        let doc =
            json_from_str(&self.document).unwrap_or_else(|_| Value::String(self.document.clone()));
        AnchorRecord {
            address: self.address,
            document: doc,
            hash: self.hash,
            nonce: self.nonce,
            updated_at: self.updated_at,
            public_key: self.public_key,
            remote_attestation: self.remote_attestation,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ResolvedDidWire {
    address: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    document: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    hash: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    nonce: Option<u64>,
    #[serde(default = "foundation_serialization::defaults::default")]
    updated_at: Option<u64>,
    #[serde(default = "foundation_serialization::defaults::default")]
    public_key: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    remote_attestation: Option<AnchorRemoteAttestation>,
}

impl ResolvedDidWire {
    fn into_record(self) -> ResolvedDid {
        let document = self.document.and_then(|doc| {
            json_from_str(&doc)
                .map(Some)
                .unwrap_or_else(|_| Some(Value::String(doc)))
        });
        ResolvedDid {
            address: self.address,
            document,
            hash: self.hash,
            nonce: self.nonce,
            updated_at: self.updated_at,
            public_key: self.public_key,
            remote_attestation: self.remote_attestation,
        }
    }
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

#[derive(Serialize)]
struct Payload<'a> {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: Value,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    auth: Option<&'a str>,
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
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "light_client.rebate_status",
        params: Value::Object(JsonMap::new()),
        auth: None,
    };
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
    let mut params = foundation_serialization::json::Map::new();
    if let Some(relayer) = &args.relayer {
        params.insert("relayer".to_string(), Value::String(relayer.clone()));
    }
    if let Some(cursor) = args.cursor {
        params.insert("cursor".to_string(), Value::Number(cursor.into()));
    }
    params.insert(
        "limit".to_string(),
        Value::Number(foundation_serialization::json::Number::from(
            args.limit as u64,
        )),
    );

    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "light_client.rebate_history",
        params: Value::Object(params),
        auth: None,
    };
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
            println!("Block {} – {} CT", receipt.height, receipt.amount);
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

fn run_device_status(json: bool) -> Result<()> {
    let cfg = light_client::load_user_config().unwrap_or_default();
    let opts = SyncOptions::default().apply_config(&cfg);
    let probe = match light_client::default_probe() {
        Ok(p) => p,
        Err(err) => {
            if json {
                let gating = opts
                    .gating_reason(&light_client::DeviceStatus::from(opts.fallback))
                    .map(|g| Value::String(g.as_str().to_owned()))
                    .unwrap_or(Value::Null);
                let payload = json_object_from(vec![
                    ("error".to_owned(), Value::String(err.to_string())),
                    ("gating".to_owned(), gating),
                ]);
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
            .map(|g| Value::String(g.as_str().to_owned()))
            .unwrap_or(Value::Null);
        let payload = json_object_from(vec![
            ("wifi".to_owned(), Value::Bool(snapshot.status.on_wifi)),
            (
                "charging".to_owned(),
                Value::Bool(snapshot.status.is_charging),
            ),
            (
                "battery".to_owned(),
                Value::from(snapshot.status.battery_level),
            ),
            (
                "freshness".to_owned(),
                Value::String(snapshot.freshness.as_label().to_owned()),
            ),
            ("observed_at_millis".to_owned(), Value::from(observed_ms)),
            ("stale_for_millis".to_owned(), Value::from(stale_ms)),
            ("gating".to_owned(), gating_value),
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

pub fn submit_anchor(client: &RpcClient, url: &str, tx: &TxDidAnchor) -> Result<AnchorRecord> {
    let params =
        foundation_serialization::json::to_value(tx).context("serialize anchor request")?;
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "identity.anchor",
        params,
        auth: None,
    };
    let resp = client
        .call(url, &payload)
        .context("identity.anchor RPC call failed")?
        .json::<RpcEnvelope<Value>>()
        .context("failed to decode identity.anchor response")?;
    if let Some(err) = resp.error {
        return Err(anyhow!(
            "identity.anchor error {} (code {})",
            err.message,
            err.code
        ));
    }
    let result = resp
        .result
        .ok_or_else(|| anyhow!("missing identity.anchor result"))?;
    if let Some(code) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("identity.anchor rejected request: {}", code));
    }
    let wire: AnchorRecordWire = foundation_serialization::json::from_value(result)
        .context("unexpected identity.anchor response format")?;
    Ok(wire.into_record())
}

pub fn latest_header(client: &RpcClient, url: &str) -> Result<LightHeader> {
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "light.latest_header",
        params: Value::Null,
        auth: None,
    };
    let resp = client
        .call(url, &payload)
        .context("light.latest_header RPC call failed")?
        .json::<RpcEnvelope<LightHeader>>()
        .context("failed to decode light.latest_header response")?;
    if let Some(err) = resp.error {
        return Err(anyhow!(
            "light.latest_header error {} (code {})",
            err.message,
            err.code
        ));
    }
    resp.result
        .ok_or_else(|| anyhow!("missing light.latest_header result"))
}

pub fn resolve_did_record(client: &RpcClient, url: &str, address: &str) -> Result<ResolvedDid> {
    let params = json_object_from(vec![(
        "address".to_owned(),
        Value::String(address.to_string()),
    )]);
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "identity.resolve",
        params,
        auth: None,
    };
    let resp = client
        .call(url, &payload)
        .context("identity.resolve RPC call failed")?
        .json::<RpcEnvelope<Value>>()
        .context("failed to decode identity.resolve response")?;
    if let Some(err) = resp.error {
        return Err(anyhow!(
            "identity.resolve error {} (code {})",
            err.message,
            err.code
        ));
    }
    let result = resp
        .result
        .ok_or_else(|| anyhow!("missing identity.resolve result"))?;
    if let Some(code) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!(
            "identity.resolve returned application error: {}",
            code
        ));
    }
    let wire: ResolvedDidWire = foundation_serialization::json::from_value(result)
        .context("unexpected identity.resolve response format")?;
    Ok(wire.into_record())
}
