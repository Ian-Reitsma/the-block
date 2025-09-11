use clap::Subcommand;
use serde_json::json;
use the_block::rpc::client::RpcClient;

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
                        println!("{}", text);
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
                            println!("{job} {reason}");
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
            let params = if let Some(acc) = accelerator {
                serde_json::json!({"accelerator": acc})
            } else {
                serde_json::Value::Null
            };
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
                            let base = res
                                .get("industrial_price_base")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            let weighted = res
                                .get("industrial_price_weighted")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            println!("base: {base} weighted: {weighted}");
                        } else {
                            println!("{}", text);
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
                                    println!("{id} {eff:.3}");
                                }
                            }
                        } else {
                            println!("{}", text);
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
                    println!("{}", text);
                }
            }
        }
    }
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
