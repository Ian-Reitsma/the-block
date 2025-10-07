use crate::{
    codec_helpers::{json_from_str, json_to_string_pretty},
    rpc::RpcClient,
};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};

use crate::parse_utils::take_string;

pub enum GatewayCmd {
    /// Inspect or manage the mobile RPC cache
    MobileCache { action: MobileCacheAction },
}

pub enum MobileCacheAction {
    /// Show mobile cache status and queue metrics
    Status {
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    /// Flush cached responses and offline queue state
    Flush { url: String, auth: Option<String> },
}

impl GatewayCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("gateway"), "gateway", "Gateway operations")
            .subcommand(MobileCacheAction::command())
            .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'gateway'".to_string())?;

        match name {
            "mobile-cache" => Ok(GatewayCmd::MobileCache {
                action: MobileCacheAction::from_matches(sub_matches)?,
            }),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

impl MobileCacheAction {
    fn command() -> Command {
        CommandBuilder::new(
            CommandId("gateway.mobile_cache"),
            "mobile-cache",
            "Inspect or manage the mobile RPC cache",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.mobile_cache.status"),
                "status",
                "Show mobile cache status and queue metrics",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "auth",
                "auth",
                "Bearer token or basic auth",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "pretty",
                "pretty",
                "Pretty-print JSON response",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.mobile_cache.flush"),
                "flush",
                "Flush cached responses and offline queue state",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "auth",
                "auth",
                "Bearer token or basic auth",
            )))
            .build(),
        )
        .build()
    }

    fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'gateway mobile-cache'".to_string())?;

        match name {
            "status" => Ok(MobileCacheAction::Status {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                pretty: sub_matches.get_flag("pretty"),
            }),
            "flush" => Ok(MobileCacheAction::Flush {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
            }),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn handle(cmd: GatewayCmd) {
    match cmd {
        GatewayCmd::MobileCache { action } => {
            let client = RpcClient::from_env();
            match action {
                MobileCacheAction::Status { url, auth, pretty } => {
                    let payload = foundation_serialization::json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "gateway.mobile_cache_status",
                        "params": foundation_serialization::json::Value::Null,
                    });
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => match resp.text() {
                            Ok(body) => {
                                if pretty {
                                    match json_from_str::<foundation_serialization::json::Value>(
                                        &body,
                                    ) {
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
                    let payload = foundation_serialization::json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "gateway.mobile_cache_flush",
                        "params": foundation_serialization::json::Value::Null,
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
