use crate::{
    codec_helpers::{json_to_string, json_to_string_pretty},
    parse_utils::{parse_u64, take_string},
    rpc::RpcClient,
};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::{self, from_value, Value};
use foundation_serialization::{Deserialize, Serialize};

const DEFAULT_RPC: &str = "http://127.0.0.1:26657";
const DEFAULT_LIMIT: u64 = 20;

#[derive(Debug)]
pub enum GovernorCmd {
    Status {
        rpc: String,
        json: bool,
    },
    Intents {
        rpc: String,
        gate: Option<String>,
        limit: u64,
        json: bool,
    },
}

impl GovernorCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("governor"),
            "governor",
            "Launch governor diagnostics and automation status",
        )
        .subcommand(Self::status_command())
        .subcommand(Self::intents_command())
        .build()
    }

    fn status_command() -> Command {
        CommandBuilder::new(
            CommandId("governor.status"),
            "status",
            "Show governor gate states, streaks, and economics snapshot",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("rpc", "rpc", "JSON-RPC endpoint").default(DEFAULT_RPC),
        ))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "json",
            "json",
            "Only output the raw governor.status JSON",
        )))
        .build()
    }

    fn intents_command() -> Command {
        CommandBuilder::new(
            CommandId("governor.intents"),
            "intents",
            "Inspect recent governor intents",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("rpc", "rpc", "JSON-RPC endpoint").default(DEFAULT_RPC),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "gate",
            "gate",
            "Gate name to filter (e.g. economics)",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "limit",
            "limit",
            "Limit the number of records (default 20)",
        )))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "json",
            "json",
            "Only emit the raw governor.decisions JSON",
        )))
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'governor'".to_string())?;
        match name {
            "status" => {
                let rpc = take_string(sub_matches, "rpc").unwrap_or_else(|| DEFAULT_RPC.into());
                let json = sub_matches.get_flag("json");
                Ok(GovernorCmd::Status { rpc, json })
            }
            "intents" => {
                let rpc = take_string(sub_matches, "rpc").unwrap_or_else(|| DEFAULT_RPC.into());
                let gate = take_string(sub_matches, "gate");
                let limit =
                    parse_u64(take_string(sub_matches, "limit"), "limit")?.unwrap_or(DEFAULT_LIMIT);
                let json = sub_matches.get_flag("json");
                Ok(GovernorCmd::Intents {
                    rpc,
                    gate,
                    limit,
                    json,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'governor'")),
        }
    }
}

pub fn handle(cmd: GovernorCmd) -> Result<(), String> {
    match cmd {
        GovernorCmd::Status { rpc, json } => handle_status(&rpc, json),
        GovernorCmd::Intents {
            rpc,
            gate,
            limit,
            json,
        } => handle_intents(&rpc, gate.as_deref(), limit, json),
    }
}

fn handle_status(rpc: &str, json_only: bool) -> Result<(), String> {
    let client = RpcClient::from_env();
    let status_value = client
        .governor_status(rpc)
        .map_err(|err| format!("governor.status RPC failed: {err}"))?;
    print_json(&status_value)?;
    if json_only {
        return Ok(());
    }
    let view: GovernorStatusView = from_value(status_value.clone())
        .map_err(|err| format!("failed to decode governor.status payload: {err}"))?;
    print_status_summary(&view)?;
    Ok(())
}

fn handle_intents(
    rpc: &str,
    gate_filter: Option<&str>,
    limit: u64,
    json_only: bool,
) -> Result<(), String> {
    let client = RpcClient::from_env();
    let decisions_value = client
        .governor_decisions(rpc, limit)
        .map_err(|err| format!("governor.decisions RPC failed: {err}"))?;
    print_json(&decisions_value)?;
    if json_only {
        return Ok(());
    }
    let entries: Vec<IntentSummaryView> = from_value(decisions_value.clone())
        .map_err(|err| format!("failed to decode governor.decisions payload: {err}"))?;
    let filtered = entries
        .into_iter()
        .filter(|entry| {
            gate_filter
                .map(|gate| gate.eq_ignore_ascii_case(&entry.gate))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        println!("no matching intents");
        return Ok(());
    }
    println!("showing {} intent(s):", filtered.len());
    for entry in filtered {
        println!(
            "* {} | gate={} action={} epoch={} state={}",
            entry.id, entry.gate, entry.action, entry.epoch_apply, entry.state
        );
        println!("  reason: {}", entry.reason);
        if !entry.metrics.is_null() {
            let metrics = json_to_string_pretty(&entry.metrics).unwrap_or_else(|_| {
                json_to_string(&entry.metrics).unwrap_or_else(|_| "<invalid metrics>".into())
            });
            for line in metrics.lines() {
                println!("  metrics: {line}");
            }
        }
    }
    Ok(())
}

fn print_json(value: &Value) -> Result<(), String> {
    if let Ok(pretty) = json_to_string_pretty(value) {
        println!("{pretty}");
    } else {
        println!(
            "{}",
            json_to_string(value).map_err(|err| format!("failed to serialize JSON: {err}"))?
        );
    }
    Ok(())
}

fn print_status_summary(view: &GovernorStatusView) -> Result<(), String> {
    println!(
        "enabled={} epoch={} window={} schema={} autopilot={}",
        view.enabled, view.epoch, view.window_secs, view.schema_version, view.autopilot_enabled
    );
    println!("gates:");
    for gate in &view.gates {
        println!(
            "  {:<12} state={} enter={} exit={} required={} reason={}",
            gate.name,
            gate.state,
            gate.enter_streak,
            gate.exit_streak,
            gate.streak_required,
            gate.last_reason
        );
    }
    if !view.economics_sample.is_null() {
        println!("economics sample:");
        let sample: EconomicsSampleView = from_value(view.economics_sample.clone())
            .map_err(|err| format!("failed to parse economics sample: {err}"))?;
        println!(
            "  tx_count={} volume={} treasury={} reward={}",
            sample.epoch_tx_count,
            sample.epoch_tx_volume,
            sample.epoch_treasury_inflow,
            sample.block_reward
        );
        if !sample.market_metrics.is_empty() {
            println!("  persisted market metrics (ppm):");
            for metric in sample.market_metrics {
                println!(
                    "    {:<8} util_ppm={} margin_ppm={}",
                    metric.market, metric.utilization_ppm, metric.provider_margin_ppm
                );
            }
        }
        if !view.economics_prev_market_metrics.is_empty() {
            println!("  telemetry gauges (ppm) read from governor:");
            for metric in &view.economics_prev_market_metrics {
                println!(
                    "    {:<8} util_ppm={} margin_ppm={}",
                    metric.market, metric.utilization_ppm, metric.provider_margin_ppm
                );
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GovernorStatusView {
    enabled: bool,
    epoch: u64,
    window_secs: u64,
    gates: Vec<GateSnapshotView>,
    pending: Vec<IntentSummaryView>,
    economics_sample: Value,
    #[serde(default)]
    economics_prev_market_metrics: Vec<EconomicsPrevMetricView>,
    autopilot_enabled: bool,
    schema_version: u64,
}

#[derive(Debug, Deserialize)]
struct GateSnapshotView {
    name: String,
    state: String,
    enter_streak: u64,
    exit_streak: u64,
    streak_required: u64,
    last_reason: String,
}

#[derive(Debug, Deserialize)]
struct EconomicsSampleView {
    #[serde(default)]
    epoch_tx_count: u64,
    #[serde(default)]
    epoch_tx_volume: u64,
    #[serde(default)]
    epoch_treasury_inflow: u64,
    #[serde(default)]
    block_reward: u64,
    #[serde(default)]
    storage_util: Option<f64>,
    #[serde(default)]
    storage_margin: Option<f64>,
    #[serde(default)]
    compute_util: Option<f64>,
    #[serde(default)]
    compute_margin: Option<f64>,
    #[serde(default)]
    energy_util: Option<f64>,
    #[serde(default)]
    energy_margin: Option<f64>,
    #[serde(default)]
    ad_util: Option<f64>,
    #[serde(default)]
    ad_margin: Option<f64>,
    #[serde(default)]
    market_metrics: Vec<EconomicsPrevMetricView>,
}

#[derive(Debug, Deserialize)]
struct EconomicsPrevMetricView {
    market: String,
    utilization_ppm: i64,
    provider_margin_ppm: i64,
}

#[derive(Debug, Deserialize)]
struct IntentSummaryView {
    id: String,
    gate: String,
    action: String,
    epoch_apply: u64,
    state: String,
    params_patch: Value,
    metrics: Value,
    reason: String,
}
