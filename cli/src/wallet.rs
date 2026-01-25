#![deny(warnings)]

use crate::codec_helpers::{json_to_string, json_to_string_pretty};
use crate::parse_utils::{parse_required, parse_u64_required, require_string, take_string};
use crate::rpc::{RpcClient, WalletQosError, WalletQosEvent};
use crate::tx::{generate_keypair, sign_tx, FeeLane, RawTxPayload};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto::session::SessionKey;
use diagnostics::{anyhow, Context, Result};
use foundation_lazy::sync::Lazy;
use foundation_serialization::json::{Map as JsonMap, Value};
use foundation_serialization::{binary, Serialize};
use std::collections::HashMap;
#[cfg(feature = "quantum")]
use std::fs::File;
use std::io::{self, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

const FEE_FLOOR_CACHE_TTL: Duration = Duration::from_secs(10);

static FEE_FLOOR_CACHE: Lazy<Mutex<HashMap<String, CacheEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct CacheEntry {
    floor: u64,
    fetched_at: Instant,
}

pub enum WalletCmd {
    /// Generate Ed25519 and Dilithium keys in parallel and export keystore
    Gen { out: String },
    /// Show available wallet commands
    Help,
    /// List balances for all known tokens
    Balances,
    /// Send tokens to an address with optional ephemeral source
    Send {
        to: String,
        amount: u64,
        fee: u64,
        nonce: u64,
        pct: u8,
        memo: Option<String>,
        lane: String,
        rpc: String,
        from: Option<String>,
        ephemeral: bool,
        auto_bump: bool,
        force: bool,
        json: bool,
        lang: Option<String>,
    },
    /// Generate a session key with specified TTL in seconds
    Session { ttl: u64 },
    /// Broadcast a meta-transaction signed by a session key
    MetaSend {
        to: String,
        amount: u64,
        session_sk: String,
    },
}

impl WalletCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("wallet"), "wallet", "Wallet utilities")
            .subcommand(
                CommandBuilder::new(
                    CommandId("wallet.gen"),
                    "gen",
                    "Generate Ed25519 and Dilithium keys and export keystore",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("out", "out", "Keystore output path").default("keystore.json"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("wallet.help"),
                    "help",
                    "Show available wallet commands",
                )
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("wallet.balances"),
                    "balances",
                    "List balances for all known tokens",
                )
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("wallet.send"),
                    "send",
                    "Send tokens to an address",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("to", "to", "Recipient address").required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("amount", "amount", "Amount to send").required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("fee", "fee", "Fee to pay").default("0"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("nonce", "nonce", "Transaction nonce").default("0"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "pct",
                        "pct",
                        "Percent of the fee routed through the consumer lane",
                    )
                    .default("100"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "memo",
                    "memo",
                    "Optional memo field",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("lane", "lane", "Fee lane to use").default("consumer"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("rpc", "rpc", "Wallet RPC endpoint")
                        .default("http://127.0.0.1:26658"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "from",
                    "from",
                    "Override sender address",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "ephemeral",
                    "ephemeral",
                    "Use an ephemeral key for the transaction",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "auto_bump",
                    "auto-bump",
                    "Automatically bump fee if below floor",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "force",
                    "force",
                    "Force submission even if below floor",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "lang",
                    "lang",
                    "Localization language override",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("wallet.session"),
                    "session",
                    "Generate a session key with specified TTL",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("ttl", "ttl", "Session TTL in seconds").default("3600"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("wallet.metasend"),
                    "meta-send",
                    "Broadcast a meta-transaction signed by a session key",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("to", "to", "Recipient address").required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("amount", "amount", "Amount to send").required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("session_sk", "session-sk", "Session secret key")
                        .required(true),
                ))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'wallet'".to_string())?;

        match name {
            "gen" => {
                let out =
                    take_string(sub_matches, "out").unwrap_or_else(|| "keystore.json".to_string());
                Ok(WalletCmd::Gen { out })
            }
            "help" => Ok(WalletCmd::Help),
            "balances" => Ok(WalletCmd::Balances),
            "send" => {
                let to = require_string(sub_matches, "to")?;
                let amount = parse_u64_required(take_string(sub_matches, "amount"), "amount")?;
                let fee = parse_u64_required(take_string(sub_matches, "fee"), "fee")?;
                let nonce = parse_u64_required(take_string(sub_matches, "nonce"), "nonce")?;
                let pct = parse_required::<u8>(take_string(sub_matches, "pct"), "pct")?;
                let memo = take_string(sub_matches, "memo");
                let lane =
                    take_string(sub_matches, "lane").unwrap_or_else(|| "consumer".to_string());
                let rpc = take_string(sub_matches, "rpc")
                    .unwrap_or_else(|| "http://127.0.0.1:26658".to_string());
                let from = take_string(sub_matches, "from");
                let ephemeral = sub_matches.get_flag("ephemeral");
                let auto_bump = sub_matches.get_flag("auto_bump");
                let force = sub_matches.get_flag("force");
                let json = sub_matches.get_flag("json");
                let lang = take_string(sub_matches, "lang");
                Ok(WalletCmd::Send {
                    to,
                    amount,
                    fee,
                    nonce,
                    pct,
                    memo,
                    lane,
                    rpc,
                    from,
                    ephemeral,
                    auto_bump,
                    force,
                    json,
                    lang,
                })
            }
            "session" => {
                let ttl = parse_u64_required(take_string(sub_matches, "ttl"), "ttl")?;
                Ok(WalletCmd::Session { ttl })
            }
            "meta-send" => {
                let to = require_string(sub_matches, "to")?;
                let amount = parse_u64_required(take_string(sub_matches, "amount"), "amount")?;
                let session_sk = require_string(sub_matches, "session_sk")?;
                Ok(WalletCmd::MetaSend {
                    to,
                    amount,
                    session_sk,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'wallet'")),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum BuildTxStatus {
    Ready,
    NeedsConfirmation,
    Cancelled,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum SignerSource {
    Local,
    Ephemeral,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct SignerMetadata {
    pub signer: String,
    pub source: SignerSource,
}

impl SignerMetadata {
    fn new(signer: impl Into<String>, source: SignerSource) -> Self {
        Self {
            signer: signer.into(),
            source,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BuildTxReport {
    pub status: BuildTxStatus,
    pub user_fee: u64,
    pub effective_fee: u64,
    pub fee_floor: u64,
    pub lane: String,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub payload: Option<RawTxPayload>,
    pub auto_bumped: bool,
    pub forced: bool,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub signer_metadata: Option<Vec<SignerMetadata>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletTelemetryEvent {
    pub kind: &'static str,
    pub lane: FeeLane,
    pub fee: u64,
    pub floor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeFloorEvaluation {
    pub warnings: Vec<String>,
    pub status: BuildTxStatus,
    pub effective_fee: u64,
    pub auto_bumped: bool,
    pub forced: bool,
    pub event: Option<WalletTelemetryEvent>,
    pub early_return: bool,
    pub prompt_required: bool,
}

enum WalletEventKind {
    Warning,
    Override,
}

impl WalletEventKind {
    fn as_str(&self) -> &'static str {
        match self {
            WalletEventKind::Warning => "warning",
            WalletEventKind::Override => "override",
        }
    }
}

pub fn handle(cmd: WalletCmd) {
    match cmd {
        WalletCmd::Gen { out } => {
            #[cfg(feature = "quantum")]
            {
                use std::thread;
                use wallet::pq::generate as pq_generate;
                use wallet::{Wallet, WalletSigner};
                let ed_handle = thread::spawn(|| Wallet::generate());
                let pq_handle = thread::spawn(|| pq_generate());
                let ed = ed_handle.join().expect("ed25519");
                let (pq_pk, pq_sk) = pq_handle.join().expect("dilithium");
                let mut f = File::create(&out).expect("write");
                let json = json_object_from(vec![
                    (
                        "ed25519_pub".to_owned(),
                        Value::String(crypto_suite::hex::encode(ed.public_key().to_bytes())),
                    ),
                    (
                        "dilithium_pub".to_owned(),
                        Value::String(crypto_suite::hex::encode(pq_pk.as_bytes())),
                    ),
                    (
                        "dilithium_sk".to_owned(),
                        Value::String(crypto_suite::hex::encode(pq_sk.as_bytes())),
                    ),
                ]);
                f.write_all(json.to_string().as_bytes()).expect("write");
                println!("exported keystore to {}", out);
            }
            #[cfg(not(feature = "quantum"))]
            {
                println!(
                    "quantum feature not enabled; cannot export keystore to {}",
                    out
                );
            }
        }
        WalletCmd::Help => {
            println!(
                "wallet commands:
  gen --out <FILE>    Generate key material
  help                Show this message"
            );
        }
        WalletCmd::Balances => {
            println!(
                "token balances:
  BLOCK: 0"
            );
        }
        WalletCmd::Send {
            to,
            amount,
            fee,
            nonce,
            pct,
            memo,
            lane,
            rpc,
            from,
            ephemeral,
            auto_bump,
            force,
            json,
            lang,
        } => {
            if auto_bump && force {
                eprintln!("--auto-bump and --force cannot be combined");
                return;
            }
            let language = Language::detect(&lang);
            let localizer = Localizer::new(language);
            let memo_bytes = memo.unwrap_or_default().into_bytes();
            let lane = match parse_lane(&lane) {
                Ok(lane) => lane,
                Err(err) => {
                    if json {
                        let payload = json_object_from(vec![
                            ("status".to_owned(), Value::String("error".to_owned())),
                            ("message".to_owned(), Value::String(err.to_string())),
                        ]);
                        match json_to_string_pretty(&payload).or_else(|_| json_to_string(&payload))
                        {
                            Ok(text) => println!("{}", text),
                            Err(err) => eprintln!("failed to encode json payload: {err}"),
                        }
                    } else {
                        eprintln!("{}", err);
                    }
                    return;
                }
            };
            let mut from_addr = from.unwrap_or_else(|| "wallet".to_string());
            let mut ephemeral_notice = None;
            if ephemeral {
                let (_, pk_bytes) = generate_keypair();
                from_addr = crypto_suite::hex::encode(&pk_bytes);
                if !json {
                    ephemeral_notice = Some(localizer.ephemeral_notice(&from_addr, amount, &to));
                }
            }
            let signer_source = if ephemeral {
                SignerSource::Ephemeral
            } else {
                SignerSource::Local
            };
            let client = RpcClient::from_env();
            match build_tx(
                &client,
                &rpc,
                lane,
                &from_addr,
                &to,
                amount,
                fee,
                pct,
                nonce,
                &memo_bytes,
                auto_bump,
                force,
                json,
                &localizer,
                signer_source,
            ) {
                Ok(report) => {
                    if json {
                        match json_to_string_pretty(&report) {
                            Ok(text) => println!("{}", text),
                            Err(err) => eprintln!("failed to encode json: {err}"),
                        }
                        return;
                    }
                    for warning in &report.warnings {
                        println!("{}", warning);
                    }
                    if let Some(message) = ephemeral_notice.take() {
                        println!("{}", message);
                    }
                    match report.status {
                        BuildTxStatus::Ready => {
                            if let Some(payload) = report.payload {
                                println!(
                                    "{}",
                                    localizer.success_message(
                                        &from_addr,
                                        &to,
                                        amount,
                                        report.effective_fee,
                                        report.fee_floor,
                                        lane,
                                        report.auto_bumped,
                                        report.forced,
                                    )
                                );
                                println!(
                                    "{}",
                                    json_to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
                                );
                            }
                        }
                        BuildTxStatus::NeedsConfirmation => {
                            println!("{}", localizer.needs_confirmation());
                        }
                        BuildTxStatus::Cancelled => {
                            println!("{}", localizer.cancelled());
                        }
                    }
                }
                Err(err) => {
                    if json {
                        let payload = json_object_from(vec![
                            ("status".to_owned(), Value::String("error".to_owned())),
                            ("message".to_owned(), Value::String(err.to_string())),
                        ]);
                        match json_to_string_pretty(&payload).or_else(|_| json_to_string(&payload))
                        {
                            Ok(text) => println!("{}", text),
                            Err(err) => eprintln!("failed to encode json payload: {err}"),
                        }
                    } else {
                        eprintln!("wallet send failed: {err}");
                    }
                }
            }
        }
        WalletCmd::Session { ttl } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let sk = match SessionKey::generate(now + ttl) {
                Ok(sk) => sk,
                Err(err) => {
                    eprintln!("failed to generate session key: {err}");
                    return;
                }
            };
            println!(
                "session key issued pk={} expires_at={}",
                crypto_suite::hex::encode(&sk.public_key),
                sk.expires_at
            );
            println!("secret={}", crypto_suite::hex::encode(sk.secret.to_bytes()));
        }
        WalletCmd::MetaSend {
            to,
            amount,
            session_sk,
        } => {
            let sk_bytes = crypto_suite::hex::decode(session_sk).expect("hex");
            let payload = RawTxPayload {
                from_: "meta".into(),
                to,
                amount_consumer: amount,
                amount_industrial: 0,
                fee: 0,
                pct: 100,
                nonce: 0,
                memo: Vec::new(),
            };
            if let Some(tx) = sign_tx(&sk_bytes, &payload) {
                println!(
                    "meta-tx signed {}",
                    crypto_suite::hex::encode(binary::encode(&tx).expect("serialize tx"))
                );
            } else {
                println!("invalid session key");
            }
        }
    }
}

enum PromptDecision {
    Auto,
    Force,
    Cancel,
}

fn compute_signer_metadata(signer: &str, source: SignerSource) -> Option<Vec<SignerMetadata>> {
    if signer.is_empty() {
        None
    } else {
        Some(vec![SignerMetadata::new(signer.to_string(), source)])
    }
}

pub fn build_tx(
    client: &RpcClient,
    rpc: &str,
    lane: FeeLane,
    from: &str,
    to: &str,
    amount: u64,
    fee: u64,
    pct: u8,
    nonce: u64,
    memo: &[u8],
    auto_bump: bool,
    force: bool,
    json: bool,
    localizer: &Localizer,
    signer_source: SignerSource,
) -> Result<BuildTxReport> {
    let floor = cached_fee_floor(client, rpc, lane)
        .with_context(|| format!("fetching mempool stats from {}", rpc))?;
    let lane_label = lane.as_str().to_string();
    let mut evaluation = evaluate_fee_floor(lane, fee, floor, auto_bump, force, json, localizer);
    let mut auto_bump_printed = false;
    let mut force_printed = false;

    if evaluation.prompt_required {
        if let Some(message) = evaluation.warnings.last() {
            println!("{}", message);
        }
        let decision = prompt_user(localizer)?;
        match decision {
            PromptDecision::Auto => {
                if !json {
                    println!("{}", localizer.auto_bump_confirmation(floor));
                    auto_bump_printed = true;
                }
                evaluation = evaluate_fee_floor(lane, fee, floor, true, false, json, localizer);
            }
            PromptDecision::Force => {
                if !json {
                    println!("{}", localizer.force_confirmation(fee, floor));
                    force_printed = true;
                }
                evaluation = evaluate_fee_floor(lane, fee, floor, false, true, json, localizer);
            }
            PromptDecision::Cancel => {
                record_wallet_event(
                    client,
                    rpc,
                    WalletEventKind::Warning,
                    lane,
                    fee,
                    floor,
                    json,
                );
                return Ok(BuildTxReport {
                    status: BuildTxStatus::Cancelled,
                    user_fee: fee,
                    effective_fee: fee,
                    fee_floor: floor,
                    lane: lane_label,
                    warnings: evaluation.warnings,
                    payload: None,
                    auto_bumped: false,
                    forced: false,
                    signer_metadata: None,
                });
            }
        }
    }

    if evaluation.auto_bumped && !json && !auto_bump_printed {
        println!("{}", localizer.auto_bump_confirmation(floor));
    }
    if evaluation.forced && !json && !force_printed {
        println!("{}", localizer.force_confirmation(fee, floor));
    }

    if let Some(event) = &evaluation.event {
        let kind = match event.kind {
            "warning" => WalletEventKind::Warning,
            "override" => WalletEventKind::Override,
            other => {
                eprintln!("unknown wallet telemetry event kind '{other}'");
                WalletEventKind::Warning
            }
        };
        record_wallet_event(client, rpc, kind, event.lane, event.fee, event.floor, json);
    }

    if evaluation.early_return {
        return Ok(BuildTxReport {
            status: evaluation.status,
            user_fee: fee,
            effective_fee: evaluation.effective_fee,
            fee_floor: floor,
            lane: lane_label,
            warnings: evaluation.warnings,
            payload: None,
            auto_bumped: evaluation.auto_bumped,
            forced: evaluation.forced,
            signer_metadata: None,
        });
    }

    let pct = pct.min(100);
    let (amount_consumer, amount_industrial) = match lane {
        FeeLane::Consumer => (amount, 0),
        FeeLane::Industrial => (0, amount),
    };
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer,
        amount_industrial,
        fee: evaluation.effective_fee,
        pct,
        nonce,
        memo: memo.to_vec(),
    };

    Ok(BuildTxReport {
        status: evaluation.status,
        user_fee: fee,
        effective_fee: evaluation.effective_fee,
        fee_floor: floor,
        lane: lane_label,
        warnings: evaluation.warnings,
        payload: Some(payload),
        auto_bumped: evaluation.auto_bumped,
        forced: evaluation.forced,
        signer_metadata: compute_signer_metadata(from, signer_source),
    })
}

pub fn evaluate_fee_floor(
    lane: FeeLane,
    fee: u64,
    floor: u64,
    auto_bump: bool,
    force: bool,
    json: bool,
    localizer: &Localizer,
) -> FeeFloorEvaluation {
    let mut warnings = Vec::new();
    let mut status = BuildTxStatus::Ready;
    let mut effective_fee = fee;
    let mut auto_bumped = false;
    let mut forced = false;
    let mut event: Option<WalletTelemetryEvent> = None;
    let mut early_return = false;
    let mut prompt_required = false;

    if fee < floor {
        warnings.push(localizer.warning_message(lane, fee, floor));
        if force {
            forced = true;
            event = Some(WalletTelemetryEvent {
                kind: "override",
                lane,
                fee,
                floor,
            });
        } else if auto_bump {
            auto_bumped = true;
            effective_fee = floor;
            event = Some(WalletTelemetryEvent {
                kind: "warning",
                lane,
                fee: effective_fee,
                floor,
            });
        } else if json {
            status = BuildTxStatus::NeedsConfirmation;
            event = Some(WalletTelemetryEvent {
                kind: "warning",
                lane,
                fee,
                floor,
            });
            early_return = true;
        } else {
            prompt_required = true;
        }
    }

    FeeFloorEvaluation {
        warnings,
        status,
        effective_fee,
        auto_bumped,
        forced,
        event,
        early_return,
        prompt_required,
    }
}

fn cached_fee_floor(client: &RpcClient, rpc: &str, lane: FeeLane) -> Result<u64> {
    let key = format!("{}::{}", rpc, lane.as_str());
    if let Some(floor) = {
        let cache = FEE_FLOOR_CACHE.lock().unwrap();
        cache.get(&key).and_then(|entry| {
            if entry.fetched_at.elapsed() < FEE_FLOOR_CACHE_TTL {
                Some(entry.floor)
            } else {
                None
            }
        })
    } {
        return Ok(floor);
    }
    let stats = client
        .mempool_stats(rpc, lane)
        .with_context(|| format!("rpc mempool.stats for lane {}", lane.as_str()))?;
    let floor = stats.fee_floor;
    // Touch additional stats so dead-code lints capture the full response
    let _ = (
        stats.size,
        stats.age_p50,
        stats.age_p95,
        stats.fee_p50,
        stats.fee_p90,
    );
    let mut cache = FEE_FLOOR_CACHE.lock().unwrap();
    cache.insert(
        key,
        CacheEntry {
            floor,
            fetched_at: Instant::now(),
        },
    );
    Ok(floor)
}

#[cfg(test)]
mod tests {}

fn record_wallet_event(
    client: &RpcClient,
    rpc: &str,
    kind: WalletEventKind,
    lane: FeeLane,
    fee: u64,
    floor: u64,
    json: bool,
) {
    let event = WalletQosEvent {
        event: kind.as_str(),
        lane: lane.as_str(),
        fee,
        floor,
    };
    if let Err(err) = client.record_wallet_qos_event(rpc, event) {
        if !json {
            let msg = match &err {
                WalletQosError::Transport(_) => {
                    format!("failed to record wallet telemetry: {err}")
                }
                WalletQosError::Rpc { code, message } => {
                    format!("wallet telemetry rejected by node (code {code}): {message}")
                }
                WalletQosError::MissingStatus => {
                    "wallet telemetry response missing status field".to_string()
                }
                WalletQosError::InvalidStatus(status) => {
                    format!("wallet telemetry response returned status '{status}'")
                }
            };
            eprintln!("{msg}");
        }
    }
}

fn prompt_user(localizer: &Localizer) -> Result<PromptDecision> {
    loop {
        print!("{}", localizer.prompt());
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();
        match trimmed.as_str() {
            "a" | "auto" => return Ok(PromptDecision::Auto),
            "f" | "force" => return Ok(PromptDecision::Force),
            "c" | "cancel" | "q" | "quit" => return Ok(PromptDecision::Cancel),
            _ => println!("{}", localizer.invalid_choice()),
        }
    }
}

fn parse_lane(lane: &str) -> Result<FeeLane> {
    match lane.to_ascii_lowercase().as_str() {
        "consumer" => Ok(FeeLane::Consumer),
        "industrial" => Ok(FeeLane::Industrial),
        other => Err(anyhow!(
            "unknown lane '{other}', expected consumer or industrial"
        )),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Language {
    En,
    Es,
    Fr,
    De,
    Pt,
    Zh,
}

impl Language {
    pub fn detect(explicit: &Option<String>) -> Self {
        if let Some(code) = explicit {
            return Self::from_code(code);
        }
        if let Ok(code) = std::env::var("TB_LANG") {
            if !code.is_empty() {
                return Self::from_code(&code);
            }
        }
        if let Ok(code) = std::env::var("LANG") {
            if !code.is_empty() {
                return Self::from_code(&code);
            }
        }
        Language::En
    }

    pub fn from_code(code: &str) -> Self {
        let lower = code.to_ascii_lowercase();
        if lower.starts_with("es") {
            Language::Es
        } else if lower.starts_with("fr") {
            Language::Fr
        } else if lower.starts_with("de") {
            Language::De
        } else if lower.starts_with("pt") {
            Language::Pt
        } else if lower.starts_with("zh") {
            Language::Zh
        } else {
            Language::En
        }
    }
}

pub struct Localizer {
    lang: Language,
}

impl Localizer {
    pub fn new(lang: Language) -> Self {
        Self { lang }
    }

    fn lane_label(&self, lane: FeeLane) -> &'static str {
        match (self.lang, lane) {
            (Language::Es, FeeLane::Consumer) => "consumo",
            (Language::Es, FeeLane::Industrial) => "industrial",
            (Language::Fr, FeeLane::Consumer) => "consommateur",
            (Language::Fr, FeeLane::Industrial) => "industriel",
            (Language::De, FeeLane::Consumer) => "verbraucher",
            (Language::De, FeeLane::Industrial) => "industrie",
            (Language::Pt, FeeLane::Consumer) => "consumo",
            (Language::Pt, FeeLane::Industrial) => "industrial",
            (Language::Zh, FeeLane::Consumer) => "消费",
            (Language::Zh, FeeLane::Industrial) => "工业",
            (_, FeeLane::Consumer) => "consumer",
            (_, FeeLane::Industrial) => "industrial",
        }
    }

    fn warning_message(&self, lane: FeeLane, fee: u64, floor: u64) -> String {
        match self.lang {
            Language::Es => format!(
                "Advertencia: la tarifa {fee} está por debajo del piso {} ({floor}).",
                self.lane_label(lane)
            ),
            Language::Fr => format!(
                "Avertissement : les frais {fee} sont inférieurs au plancher {} ({floor}).",
                self.lane_label(lane)
            ),
            Language::De => format!(
                "Warnung: Gebühr {fee} liegt unter dem {}-Gebührenboden ({floor}).",
                self.lane_label(lane)
            ),
            Language::Pt => format!(
                "Aviso: a taxa {fee} está abaixo do piso {} ({floor}).",
                self.lane_label(lane)
            ),
            Language::Zh => format!(
                "警告：费用 {fee} 低于 {} 费用下限 ({floor})。",
                self.lane_label(lane)
            ),
            Language::En => format!(
                "Warning: fee {fee} is below the {} fee floor ({floor}).",
                self.lane_label(lane)
            ),
        }
    }

    fn auto_bump_confirmation(&self, floor: u64) -> String {
        match self.lang {
            Language::Es => format!("Ajustando automáticamente la tarifa al piso {floor}."),
            Language::Fr => format!("Ajustement automatique des frais au plancher {floor}."),
            Language::De => format!("Automatisches Anheben der Gebühr auf den Boden {floor}."),
            Language::Pt => format!("Aumentando automaticamente a taxa para o piso {floor}."),
            Language::Zh => format!("自动将费用提升至下限 {floor}。"),
            Language::En => format!("Auto-bumping fee to floor {floor}."),
        }
    }

    fn force_confirmation(&self, fee: u64, floor: u64) -> String {
        match self.lang {
            Language::Es => {
                format!("Forzando el envío con tarifa {fee} por debajo del piso {floor}.")
            }
            Language::Fr => {
                format!("Envoi forcé avec des frais {fee} en dessous du plancher {floor}.")
            }
            Language::De => {
                format!("Senden wird mit Gebühr {fee} unter dem Gebührenboden {floor} erzwungen.")
            }
            Language::Pt => format!("Forçando o envio com taxa {fee} abaixo do piso {floor}."),
            Language::Zh => format!("强制以费用 {fee}（低于下限 {floor}）发送。"),
            Language::En => format!("Forcing send with fee {fee} below floor {floor}."),
        }
    }

    fn cancelled(&self) -> String {
        match self.lang {
            Language::Es => "Operación cancelada.".to_string(),
            Language::Fr => "Opération annulée.".to_string(),
            Language::De => "Vorgang abgebrochen.".to_string(),
            Language::Pt => "Operação cancelada.".to_string(),
            Language::Zh => "操作已取消。".to_string(),
            Language::En => "Transaction cancelled.".to_string(),
        }
    }

    fn needs_confirmation(&self) -> String {
        match self.lang {
            Language::Es => "La tarifa está por debajo del piso. Ejecute de nuevo con --auto-bump o --force para continuar.".to_string(),
            Language::Fr => "Les frais sont sous le plancher. Relancez avec --auto-bump ou --force pour continuer.".to_string(),
            Language::De => "Gebühr unter dem Minimum. Erneut mit --auto-bump oder --force ausführen.".to_string(),
            Language::Pt => "A taxa está abaixo do piso. Reexecute com --auto-bump ou --force para continuar.".to_string(),
            Language::Zh => "费用低于阈值。请使用 --auto-bump 或 --force 重新运行以继续。".to_string(),
            Language::En => "Fee is below the mempool floor; re-run with --auto-bump or --force to continue.".to_string(),
        }
    }

    fn prompt(&self) -> String {
        match self.lang {
            Language::Es => "Elegir [a]uto, [f]orzar o [c]ancelar: ".to_string(),
            Language::Fr => "Choisir [a]uto, [f]orcer ou [c]annuler : ".to_string(),
            Language::De => "[a]utomatisch, [f]erzwingen oder [c]abbrechen wählen: ".to_string(),
            Language::Pt => "Escolha [a]uto, [f]orçar ou [c]ancelar: ".to_string(),
            Language::Zh => "选择 [a] 自动调整、[f] 强制或 [c] 取消：".to_string(),
            Language::En => "Choose [a]uto bump, [f]orce, or [c]ancel: ".to_string(),
        }
    }

    fn invalid_choice(&self) -> String {
        match self.lang {
            Language::Es => "Opción no válida, inténtelo de nuevo.".to_string(),
            Language::Fr => "Choix invalide, veuillez réessayer.".to_string(),
            Language::De => "Ungültige Auswahl, bitte erneut versuchen.".to_string(),
            Language::Pt => "Opção inválida, tente novamente.".to_string(),
            Language::Zh => "无效选项，请重试。".to_string(),
            Language::En => "Unrecognised option, please try again.".to_string(),
        }
    }

    fn success_message(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        fee: u64,
        floor: u64,
        lane: FeeLane,
        auto_bumped: bool,
        forced: bool,
    ) -> String {
        let lane_label = self.lane_label(lane);
        let adjustment = if forced {
            match self.lang {
                Language::Es => " (forzado por debajo del piso)".to_string(),
                Language::Fr => " (forcé sous le plancher)".to_string(),
                Language::De => " (erzwungen unter dem Minimum)".to_string(),
                Language::Pt => " (forçado abaixo do piso)".to_string(),
                Language::Zh => "（低于下限强制发送）".to_string(),
                Language::En => " (forced below floor)".to_string(),
            }
        } else if auto_bumped {
            match self.lang {
                Language::Es => " (ajuste automático)".to_string(),
                Language::Fr => " (ajustement automatique)".to_string(),
                Language::De => " (automatisch angepasst)".to_string(),
                Language::Pt => " (ajuste automático)".to_string(),
                Language::Zh => "（已自动调整）".to_string(),
                Language::En => " (auto-bumped)".to_string(),
            }
        } else {
            String::new()
        };
        match self.lang {
            Language::Es => format!(
                "Transacción preparada de {from} a {to} por {amount} en la vía {lane_label} con tarifa {fee} (piso {floor}){adjustment}."
            ),
            Language::Fr => format!(
                "Transaction préparée de {from} vers {to} pour {amount} sur la voie {lane_label} avec des frais {fee} (plancher {floor}){adjustment}."
            ),
            Language::De => format!(
                "Transaktion von {from} an {to} über {amount} im Kanal {lane_label} mit Gebühr {fee} (Grenze {floor}){adjustment} vorbereitet."
            ),
            Language::Pt => format!(
                "Transação preparada de {from} para {to} por {amount} na fila {lane_label} com taxa {fee} (piso {floor}){adjustment}."
            ),
            Language::Zh => format!(
                "已为 {from} 向 {to} 准备金额 {amount} 的 {lane_label} 交易，费用 {fee}（下限 {floor}）{adjustment}。"
            ),
            Language::En => format!(
                "Transaction prepared from {from} to {to} for {amount} on the {lane_label} lane at fee {fee} (floor {floor}){adjustment}."
            ),
        }
    }

    fn ephemeral_notice(&self, addr: &str, amount: u64, to: &str) -> String {
        match self.lang {
            Language::Es => {
                format!("Se usa la dirección efímera {addr} para transferir {amount} a {to}")
            }
            Language::Fr => {
                format!("Adresse éphémère {addr} utilisée pour transférer {amount} à {to}")
            }
            Language::De => format!(
                "Ephemere Adresse {addr} wird für die Überweisung von {amount} an {to} verwendet"
            ),
            Language::Pt => {
                format!("Endereço efêmero {addr} usado para transferir {amount} para {to}")
            }
            Language::Zh => format!("使用临时地址 {addr} 向 {to} 转账 {amount}"),
            Language::En => {
                format!("ephemeral address {addr} used for transfer of {amount} to {to}")
            }
        }
    }
}
