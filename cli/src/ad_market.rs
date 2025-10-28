use crate::{json_helpers::json_rpc_request, parse_utils::take_string, rpc::RpcClient};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::{self, Value};
use std::fs;

pub enum AdMarketCmd {
    Inventory {
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    List {
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    Distribution {
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    Register {
        url: String,
        auth: Option<String>,
        campaign_path: String,
    },
}

impl AdMarketCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market"),
            "ad-market",
            "Ad marketplace operations",
        )
        .subcommand(Self::inventory_command())
        .subcommand(Self::list_command())
        .subcommand(Self::distribution_command())
        .subcommand(Self::register_command())
        .build()
    }

    fn inventory_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.inventory"),
            "inventory",
            "Show registered campaigns and remaining budgets",
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
        .build()
    }

    fn list_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.list_campaigns"),
            "list",
            "List registered advertising campaigns",
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
        .build()
    }

    fn distribution_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.distribution"),
            "distribution",
            "Show active advertising distribution policy",
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
        .build()
    }

    fn register_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.register"),
            "register",
            "Register a new advertising campaign",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "auth",
            "auth",
            "Bearer token or basic auth",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("campaign", "campaign", "Path to campaign JSON").required(true),
        ))
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'ad-market'".to_string())?;
        match name {
            "inventory" => Ok(Self::Inventory {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                pretty: sub_matches.get_flag("pretty"),
            }),
            "list" => Ok(Self::List {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                pretty: sub_matches.get_flag("pretty"),
            }),
            "distribution" => Ok(Self::Distribution {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                pretty: sub_matches.get_flag("pretty"),
            }),
            "register" => Ok(Self::Register {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                campaign_path: take_string(sub_matches, "campaign")
                    .ok_or_else(|| "missing '--campaign' path".to_string())?,
            }),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn handle(cmd: AdMarketCmd) {
    match cmd {
        AdMarketCmd::Inventory { url, auth, pretty } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("ad_market.inventory", Value::Null);
            print_rpc_response(&client, &url, payload, auth.as_deref(), pretty);
        }
        AdMarketCmd::List { url, auth, pretty } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("ad_market.list_campaigns", Value::Null);
            print_rpc_response(&client, &url, payload, auth.as_deref(), pretty);
        }
        AdMarketCmd::Distribution { url, auth, pretty } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("ad_market.distribution", Value::Null);
            print_rpc_response(&client, &url, payload, auth.as_deref(), pretty);
        }
        AdMarketCmd::Register {
            url,
            auth,
            campaign_path,
        } => {
            let client = RpcClient::from_env();
            match fs::read(&campaign_path) {
                Ok(bytes) => match json::value_from_slice(&bytes) {
                    Ok(value) => {
                        let payload = json_rpc_request("ad_market.register_campaign", value);
                        print_rpc_response(&client, &url, payload, auth.as_deref(), true);
                    }
                    Err(err) => {
                        eprintln!("failed to parse campaign JSON: {err}");
                    }
                },
                Err(err) => eprintln!("failed to read campaign file: {err}"),
            }
        }
    }
}

fn print_rpc_response(
    client: &RpcClient,
    url: &str,
    payload: Value,
    auth: Option<&str>,
    pretty: bool,
) {
    match client.call_with_auth(url, &payload, auth) {
        Ok(resp) => match resp.text() {
            Ok(body) => {
                if pretty {
                    match json::value_from_slice(body.as_bytes()) {
                        Ok(value) => match json::to_string_pretty(&value) {
                            Ok(text) => println!("{}", text),
                            Err(err) => {
                                eprintln!("failed to format response: {err}");
                                println!("{}", body);
                            }
                        },
                        Err(err) => {
                            eprintln!("failed to decode response: {err}");
                            println!("{}", body);
                        }
                    }
                } else {
                    println!("{}", body);
                }
            }
            Err(err) => eprintln!("failed to read response: {err}"),
        },
        Err(err) => eprintln!("RPC call failed: {err}"),
    }
}
