use crate::{
    codec_helpers::json_from_str,
    json_helpers::{
        json_null, json_object_from, json_rpc_request, json_rpc_request_with_id, json_string,
        json_u64,
    },
    parse_utils::take_string,
    rpc::RpcClient,
};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::hex;
use foundation_serialization::json::Value as JsonValue;
use std::convert::TryInto;
use std::io::{self, Write};
use the_block::compute_market::settlement::{SlaResolution, SlaResolutionKind};
use the_block::compute_market::snark::{CircuitArtifact, ProofBundle, SnarkBackend};
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
    /// List recent SLA resolutions and attached SNARK proofs
    Proofs { url: String, limit: usize },
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
        .subcommand(
            CommandBuilder::new(
                CommandId("compute.proofs"),
                "proofs",
                "Show recent SLA resolutions with SNARK proofs",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Number of SLA entries to display").default("10"),
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
            "proofs" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let limit = take_string(sub_matches, "limit")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(10);
                Ok(ComputeCmd::Proofs { url, limit })
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

pub fn write_sla_history_from_str(text: &str, out: &mut dyn Write) -> io::Result<()> {
    if let Ok(val) = json_from_str::<JsonValue>(text) {
        if let Some(entries) = val.get("result").and_then(|v| v.as_array()) {
            for entry in entries {
                let job = entry.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
                let provider = entry.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                let outcome = entry.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
                let reason = entry
                    .get("outcome_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let burned = entry.get("burned").and_then(|v| v.as_u64()).unwrap_or(0);
                let refunded = entry.get("refunded").and_then(|v| v.as_u64()).unwrap_or(0);
                let deadline = entry.get("deadline").and_then(|v| v.as_u64()).unwrap_or(0);
                let resolved_at = entry
                    .get("resolved_at")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                writeln!(
                    out,
                    "job {job} provider {provider} outcome {outcome} reason {reason} burned {burned} refunded {refunded} deadline {deadline} resolved_at {resolved_at}"
                )?;
                if let Some(proofs) = entry.get("proofs").and_then(|v| v.as_array()) {
                    for proof in proofs {
                        let fingerprint = proof
                            .get("fingerprint")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let backend = proof.get("backend").and_then(|v| v.as_str()).unwrap_or("");
                        let circuit = proof
                            .get("circuit_hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let program = proof
                            .get("program_commitment")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let witness = proof
                            .get("witness_commitment")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let latency = proof
                            .get("latency_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let verified = proof
                            .get("verified")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let generated_at = proof
                            .get("artifact")
                            .and_then(|v| v.get("generated_at"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        writeln!(
                            out,
                            "  proof {fingerprint} backend {backend} circuit {circuit} wasm {program} witness {witness} latency_ms {latency} verified {verified} generated_at {generated_at}"
                        )?;
                    }
                }
            }
        } else {
            writeln!(out, "{}", text)?;
        }
    }
    Ok(())
}

pub fn parse_sla_history_from_str(text: &str) -> Result<Vec<SlaResolution>, String> {
    let val = json_from_str::<JsonValue>(text)
        .map_err(|err| format!("parse sla history response: {err}"))?;
    let entries = val
        .get("result")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| "missing result array in sla history response".to_string())?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let job_id = expect_str(entry, "job_id")?.to_string();
        let provider = expect_str(entry, "provider")?.to_string();
        let buyer = expect_str(entry, "buyer")?.to_string();
        let burned = expect_u64(entry, "burned")?;
        let refunded = expect_u64(entry, "refunded")?;
        let deadline = expect_u64(entry, "deadline")?;
        let resolved_at = expect_u64(entry, "resolved_at")?;
        let outcome = parse_outcome(entry)?;
        let proofs = parse_proofs(entry.get("proofs"))?;
        out.push(SlaResolution {
            job_id,
            provider,
            buyer,
            outcome,
            burned,
            refunded,
            deadline,
            resolved_at,
            proofs,
        });
    }
    Ok(out)
}

fn parse_outcome(entry: &JsonValue) -> Result<SlaResolutionKind, String> {
    let outcome = expect_str(entry, "outcome")?;
    match outcome {
        "completed" => Ok(SlaResolutionKind::Completed),
        "cancelled" => {
            let reason = entry
                .get("outcome_reason")
                .and_then(JsonValue::as_str)
                .unwrap_or("unspecified")
                .to_string();
            Ok(SlaResolutionKind::Cancelled { reason })
        }
        "violated" => {
            let reason = entry
                .get("outcome_reason")
                .and_then(JsonValue::as_str)
                .unwrap_or("unspecified")
                .to_string();
            Ok(SlaResolutionKind::Violated { reason })
        }
        other => Err(format!("unknown SLA outcome '{other}'")),
    }
}

fn parse_proofs(value: Option<&JsonValue>) -> Result<Vec<ProofBundle>, String> {
    let Some(array) = value.and_then(JsonValue::as_array) else {
        return Ok(Vec::new());
    };
    let mut proofs = Vec::with_capacity(array.len());
    for proof in array {
        let backend = parse_backend(expect_str(proof, "backend")?)?;
        let circuit_hash = decode_hex_array(expect_str(proof, "circuit_hash")?)?;
        let program_commitment = decode_hex_array(expect_str(proof, "program_commitment")?)?;
        let output_commitment = decode_hex_array(expect_str(proof, "output_commitment")?)?;
        let witness_commitment = decode_hex_array(expect_str(proof, "witness_commitment")?)?;
        let latency_ms = expect_u64(proof, "latency_ms")?;
        let proof_bytes = hex::decode(expect_str(proof, "proof")?)
            .map_err(|err| format!("invalid proof hex: {err}"))?;
        let artifact_value = proof
            .get("artifact")
            .ok_or_else(|| "missing artifact in proof entry".to_string())?;
        let artifact = parse_artifact(artifact_value)?;
        let bundle = ProofBundle::from_encoded_parts(
            backend,
            circuit_hash,
            program_commitment,
            output_commitment,
            witness_commitment,
            proof_bytes,
            latency_ms,
            artifact,
        )
        .map_err(|err| format!("failed to rebuild proof bundle: {err}"))?;
        proofs.push(bundle);
    }
    Ok(proofs)
}

fn parse_artifact(value: &JsonValue) -> Result<CircuitArtifact, String> {
    let circuit_hash = decode_hex_array(expect_str(value, "circuit_hash")?)?;
    let wasm_hash = decode_hex_array(expect_str(value, "wasm_hash")?)?;
    let generated_at = expect_u64(value, "generated_at")?;
    Ok(CircuitArtifact {
        circuit_hash,
        wasm_hash,
        generated_at,
    })
}

fn parse_backend(label: &str) -> Result<SnarkBackend, String> {
    match label.to_ascii_lowercase().as_str() {
        "cpu" => Ok(SnarkBackend::Cpu),
        "gpu" => Ok(SnarkBackend::Gpu),
        other => Err(format!("unknown prover backend '{other}'")),
    }
}

fn expect_str<'a>(entry: &'a JsonValue, field: &str) -> Result<&'a str, String> {
    entry
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing field '{field}' in sla history entry"))
}

fn expect_u64(entry: &JsonValue, field: &str) -> Result<u64, String> {
    entry
        .get(field)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| format!("missing numeric field '{field}'"))
}

fn decode_hex_array(hex_value: &str) -> Result<[u8; 32], String> {
    let bytes =
        hex::decode(hex_value).map_err(|err| format!("invalid hex field '{hex_value}': {err}"))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| format!("expected 32-byte array, got {} bytes", bytes.len()))
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
        ComputeCmd::Proofs { url, limit } => {
            let client = RpcClient::from_env();
            let params = json_object_from([("limit", json_u64(limit as u64))]);
            let payload = json_rpc_request("compute_market.sla_history", params);
            eprintln!("[CLI] Making RPC call to: {}", url);
            match client.call(&url, &payload) {
                Ok(resp) => {
                    eprintln!("[CLI] RPC call succeeded, parsing response...");
                    if let Ok(text) = resp.text() {
                        write_sla_history_from_str(&text, out)?;
                    } else {
                        eprintln!("[CLI] Failed to get response text");
                    }
                }
                Err(e) => {
                    eprintln!("[CLI] RPC call FAILED: {:?}", e);
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
