use crate::{
    codec_helpers::{json_from_str, json_to_string_pretty},
    rpc::RpcClient,
};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum GatewayCmd {
    /// Inspect or manage the mobile RPC cache
    MobileCache {
        #[command(subcommand)]
        action: MobileCacheAction,
    },
}

#[derive(Subcommand)]
pub enum MobileCacheAction {
    /// Show mobile cache status and queue metrics
    Status {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        auth: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
    /// Flush cached responses and offline queue state
    Flush {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        auth: Option<String>,
    },
}

pub fn handle(cmd: GatewayCmd) {
    match cmd {
        GatewayCmd::MobileCache { action } => {
            let client = RpcClient::from_env();
            match action {
                MobileCacheAction::Status { url, auth, pretty } => {
                    let payload = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "gateway.mobile_cache_status",
                        "params": serde_json::Value::Null,
                    });
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => match resp.text() {
                            Ok(body) => {
                                if pretty {
                                    match json_from_str::<serde_json::Value>(&body) {
                                        Ok(value) => {
                                            if let Ok(text) = json_to_string_pretty(&value) {
                                                println!("{}", text);
                                            }
                                        }
                                        Err(err) => {
                                            eprintln!("failed to decode status response: {err}");
                                            println!("{}", body);
                                        }
                                    }
                                } else {
                                    println!("{}", body);
                                }
                            }
                            Err(err) => {
                                eprintln!("failed to read status response: {err}");
                            }
                        },
                        Err(err) => {
                            eprintln!("mobile cache status failed: {err}");
                        }
                    }
                }
                MobileCacheAction::Flush { url, auth } => {
                    let payload = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "gateway.mobile_cache_flush",
                        "params": serde_json::Value::Null,
                    });
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("mobile cache flush failed: {err}");
                        }
                    }
                }
            }
        }
    }
}
