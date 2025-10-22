use crate::{
    codec_helpers::json_from_str,
    json_helpers::{
        json_null, json_object_from, json_rpc_request, json_rpc_request_with_id, json_string,
    },
    parse_utils::take_string,
    rpc::RpcClient,
};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::Value as JsonValue;
use std::io::{self, Write};
use the_block::simple_db::EngineKind;

pub enum ComputeCmd {
    /// Cancel an in-flight compute job
    Cancel { job_id: String, url: String },
    /// List job cancellations
    List { preempted: bool },
    /// Show compute market stats
    Stats {
        url: String,
        accelerator: Option<String>,
    },
    /// Show scheduler queue with aged priorities
    Queue { url: String },
    /// Show status for a job
    Status { job_id: String, url: String },
}

impl ComputeCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("compute"),
            "compute",
            "Compute marketplace utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("compute.cancel"),
                "cancel",
                "Cancel an in-flight compute job",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "job_id",
                "Job identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("compute.list"), "list", "List job cancellations")
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "preempted",
                    "preempted",
                    "Only show preempted jobs",
                )))
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("compute.stats"),
                "stats",
                "Show compute market stats",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "accelerator",
                "accelerator",
                "Filter by accelerator",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("compute.queue"), "queue", "Show scheduler queue")
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("compute.status"),
                "status",
                "Show status for a job",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "job_id",
                "Job identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'compute'".to_string())?;

        match name {
            "cancel" => {
                let job_id = sub_matches
                    .get_positional("job_id")
                    .and_then(|vals| vals.first().cloned())
                    .ok_or_else(|| "missing positional argument 'job_id'".to_string())?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(ComputeCmd::Cancel { job_id, url })
            }
            "list" => Ok(ComputeCmd::List {
                preempted: sub_matches.get_flag("preempted"),
            }),
            "stats" => Ok(ComputeCmd::Stats {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                accelerator: take_string(sub_matches, "accelerator"),
            }),
            "queue" => Ok(ComputeCmd::Queue {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
            }),
            "status" => {
                let job_id = sub_matches
                    .get_positional("job_id")
                    .and_then(|vals| vals.first().cloned())
                    .ok_or_else(|| "missing positional argument 'job_id'".to_string())?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(ComputeCmd::Status { job_id, url })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn stats_request_payload(accelerator: Option<&str>) -> foundation_serialization::json::Value {
    let params = accelerator
        .map(|acc| json_object_from([("accelerator", json_string(acc))]))
        .unwrap_or_else(json_null);
    json_rpc_request("compute_market.stats", params)
}

pub fn provider_balances_payload() -> foundation_serialization::json::Value {
    json_rpc_request_with_id("compute_market.provider_balances", json_null(), 2)
}

pub fn write_stats_from_str(text: &str, out: &mut dyn Write) -> io::Result<()> {
    if let Ok(val) = json_from_str::<JsonValue>(text) {
        if let Some(res) = val.get("result") {
            if let Some(engine) = res.get("settlement_engine").and_then(|v| v.as_object()) {
                let engine_label = engine.get("engine").and_then(|v| v.as_str()).unwrap_or("-");
                writeln!(out, "settlement engine: {engine_label}")?;
                let recommended = EngineKind::default_for_build().label();
                if engine_label != recommended {
                    writeln!(
                        out,
                        "warning: recommended settlement engine is {recommended}"
                    )?;
                }
                if engine
                    .get("legacy_mode")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    writeln!(out, "warning: settlement engine running in legacy mode")?;
                }
            }
            if let Some(backlog) = res.get("industrial_backlog").and_then(|v| v.as_u64()) {
                writeln!(out, "industrial backlog: {backlog}")?;
            }
            if let Some(utilization) = res.get("industrial_utilization").and_then(|v| v.as_u64()) {
                writeln!(out, "industrial utilization: {utilization}%")?;
            }
            if let Some(total) = res.get("industrial_units_total").and_then(|v| v.as_u64()) {
                writeln!(out, "industrial units total: {total}")?;
            }
            if let Some(price) = res
                .get("industrial_price_per_unit")
                .and_then(|v| v.as_u64())
            {
                writeln!(out, "industrial price per unit: {price}")?;
            }
            if let Some(lanes) = res.get("lanes").and_then(|v| v.as_array()) {
                for lane in lanes {
                    let lane_name = lane.get("lane").and_then(|v| v.as_str()).unwrap_or("-");
                    let pending = lane.get("pending").and_then(|v| v.as_u64()).unwrap_or(0);
                    let admitted = lane.get("admitted").and_then(|v| v.as_u64()).unwrap_or(0);
                    writeln!(
                        out,
                        "lane {lane_name}: pending {pending} admitted {admitted}"
                    )?;
                    if let Some(recent) = lane.get("recent").and_then(|v| v.as_array()) {
                        for entry in recent {
                            let job = entry.get("job").and_then(|v| v.as_str()).unwrap_or("");
                            let provider =
                                entry.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                            let price = entry.get("price").and_then(|v| v.as_u64()).unwrap_or(0);
                            let issued =
                                entry.get("issued_at").and_then(|v| v.as_u64()).unwrap_or(0);
                            writeln!(
                                out,
                                "recent lane {lane_name} job {job} provider {provider} price {price} issued_at {issued}"
                            )?;
                        }
                    }
                }
            }
            if let Some(recent_matches) = res.get("recent_matches").and_then(|v| v.as_object()) {
                for (lane_name, entries) in recent_matches {
                    if let Some(array) = entries.as_array() {
                        for entry in array {
                            let job = entry.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
                            let provider =
                                entry.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                            let price = entry.get("price").and_then(|v| v.as_u64()).unwrap_or(0);
                            let issued =
                                entry.get("issued_at").and_then(|v| v.as_u64()).unwrap_or(0);
                            writeln!(
                                out,
                                "recent lane {lane_name} job {job} provider {provider} price {price} issued_at {issued}"
                            )?;
                        }
                    }
                }
            }
            if let Some(lane_stats) = res.get("lane_stats").and_then(|v| v.as_array()) {
                for entry in lane_stats {
                    let name = entry.get("lane").and_then(|v| v.as_str()).unwrap_or("");
                    let bids = entry.get("bids").and_then(|v| v.as_u64()).unwrap_or(0);
                    let asks = entry.get("asks").and_then(|v| v.as_u64()).unwrap_or(0);
                    let oldest_bid = entry
                        .get("oldest_bid_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let oldest_ask = entry
                        .get("oldest_ask_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    writeln!(
                        out,
                        "lane {name} bids: {bids} asks: {asks} oldest_bid_ms: {oldest_bid} oldest_ask_ms: {oldest_ask}"
                    )?;
                }
            }
            if let Some(warnings) = res.get("lane_starvation").and_then(|v| v.as_array()) {
                for warning in warnings {
                    let name = warning.get("lane").and_then(|v| v.as_str()).unwrap_or("");
                    let job = warning.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
                    let waited = warning
                        .get("waited_for_secs")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    writeln!(
                        out,
                        "starvation lane {name} job {job} waited_secs: {waited}"
                    )?;
                }
            }
        } else {
            writeln!(out, "{}", text)?;
        }
    }
    Ok(())
}

pub fn write_provider_balances_from_str(text: &str, out: &mut dyn Write) -> io::Result<()> {
    if let Ok(val) = json_from_str::<JsonValue>(text) {
        if let Some(providers) = val
            .get("result")
            .and_then(|res| res.get("providers"))
            .and_then(|v| v.as_array())
        {
            for entry in providers {
                let provider = entry.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                let ct = entry.get("ct").and_then(|v| v.as_u64()).unwrap_or(0);
                let industrial = entry
                    .get("industrial")
                    .or_else(|| entry.get("it"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                writeln!(out, "provider: {provider} ct: {ct} it: {industrial}")?;
            }
        }
    }
    Ok(())
}

pub fn handle(cmd: ComputeCmd) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = handle_with_writer(cmd, &mut handle);
}

pub fn handle_with_writer(cmd: ComputeCmd, out: &mut dyn Write) -> io::Result<()> {
    match cmd {
        ComputeCmd::Cancel { job_id, url } => {
            let client = RpcClient::from_env();
            let params = json_object_from([("job_id", json_string(job_id))]);
            let payload = json_rpc_request("compute.job_cancel", params);
            match client.call(&url, &payload) {
                Ok(resp) => {
                    if let Ok(text) = resp.text() {
                        writeln!(out, "{}", text)?;
                    }
                }
                Err(e) => eprintln!("{e}"),
            }
        }
        ComputeCmd::List { preempted } => {
            let path = cancel_log_path();
            if let Ok(contents) = std::fs::read_to_string(path) {
                for line in contents.lines() {
                    let mut parts = line.split_whitespace();
                    if let (Some(job), Some(reason)) = (parts.next(), parts.next()) {
                        if !preempted || reason == "preempted" {
                            writeln!(out, "{job} {reason}")?;
                        }
                    }
                }
            }
        }
        ComputeCmd::Stats { url, accelerator } => {
            let client = RpcClient::from_env();
            let payload = stats_request_payload(accelerator.as_deref());
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    write_stats_from_str(&text, out)?;
                }
            }

            let balance_payload = provider_balances_payload();
            if let Ok(resp) = client.call(&url, &balance_payload) {
                if let Ok(text) = resp.text() {
                    write_provider_balances_from_str(&text, out)?;
                }
            }
        }
        ComputeCmd::Queue { url } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("compute_market.stats", json_null());
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    if let Ok(val) = json_from_str::<foundation_serialization::json::Value>(&text) {
                        if let Some(res) = val.get("result") {
                            if let Some(pending) = res.get("pending").and_then(|v| v.as_array()) {
                                for job in pending {
                                    let id =
                                        job.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
                                    let eff = job
                                        .get("effective_priority")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0);
                                    writeln!(out, "{id} {eff:.3}")?;
                                }
                            }
                        } else {
                            writeln!(out, "{}", text)?;
                        }
                    }
                }
            }
        }
        ComputeCmd::Status { job_id, url } => {
            let client = RpcClient::from_env();
            let params = json_object_from([("job_id", json_string(job_id))]);
            let payload = json_rpc_request("compute.job_status", params);
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    writeln!(out, "{}", text)?;
                }
            }
        }
    }
    Ok(())
}

fn cancel_log_path() -> std::path::PathBuf {
    std::env::var("TB_CANCEL_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            sys::paths::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".the_block")
                .join("cancellations.log")
        })
}
