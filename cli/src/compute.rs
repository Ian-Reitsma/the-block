use crate::rpc::RpcClient;
use clap::Subcommand;
use serde_json::json;
use std::io::{self, Write};

#[derive(Subcommand)]
pub enum ComputeCmd {
    /// Cancel an in-flight compute job
    Cancel {
        job_id: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// List job cancellations
    List {
        #[arg(long)]
        preempted: bool,
    },
    /// Show compute market stats
    Stats {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        accelerator: Option<String>,
    },
    /// Show scheduler queue with aged priorities
    Queue {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Show status for a job
    Status {
        job_id: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
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
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let params = json!({"job_id": job_id});
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "compute.job_cancel",
                params,
                auth: None,
            };
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
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let params = accelerator
                .as_ref()
                .map(|acc| serde_json::json!({"accelerator": acc}))
                .unwrap_or(serde_json::Value::Null);
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "compute_market.stats",
                params,
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(res) = val.get("result") {
                            let backlog = res
                                .get("industrial_backlog")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            let util = res
                                .get("industrial_utilization")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            let units = res
                                .get("industrial_units_total")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            let price = res
                                .get("industrial_price_per_unit")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            writeln!(
                                out,
                                "backlog: {backlog} util: {util}% units: {units} price: {price}"
                            )?;
                            if let Some(base) =
                                res.get("industrial_price_base").and_then(|v| v.as_u64())
                            {
                                let weighted = res
                                    .get("industrial_price_weighted")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(base);
                                writeln!(out, "median base: {base} weighted: {weighted}")?;
                            }
                        } else {
                            writeln!(out, "{}", text)?;
                        }
                    }
                }
            }

            let balance_payload = Payload {
                jsonrpc: "2.0",
                id: 2,
                method: "compute_market.provider_balances",
                params: serde_json::Value::Null,
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &balance_payload) {
                if let Ok(text) = resp.text() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(providers) = val
                            .get("result")
                            .and_then(|res| res.get("providers"))
                            .and_then(|v| v.as_array())
                        {
                            for entry in providers {
                                let provider =
                                    entry.get("provider").and_then(|v| v.as_str()).unwrap_or("");
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
                }
            }
        }
        ComputeCmd::Queue { url } => {
            let client = RpcClient::from_env();
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
                method: "compute_market.stats",
                params: serde_json::Value::Null,
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
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
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let params = json!({"job_id": job_id});
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "compute.job_status",
                params,
                auth: None,
            };
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
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".the_block")
                .join("cancellations.log")
        })
}
