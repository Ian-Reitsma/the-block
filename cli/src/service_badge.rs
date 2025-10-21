use crate::json_helpers::{empty_object, json_object_from, json_rpc_request, json_string};
use crate::parse_utils::{require_positional, take_string};
use crate::rpc::RpcClient;
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};

pub enum ServiceBadgeCmd {
    /// Verify a badge token via RPC
    Verify { badge: String, url: String },
    /// Issue a new badge via RPC
    Issue { url: String },
    /// Revoke the current badge via RPC
    Revoke { url: String },
}

impl ServiceBadgeCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("service-badge"),
            "service-badge",
            "Service badge utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("service-badge.verify"),
                "verify",
                "Verify a badge token via RPC",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "badge",
                "Badge token to verify",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("service-badge.issue"),
                "issue",
                "Issue a new badge via RPC",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("service-badge.revoke"),
                "revoke",
                "Revoke the current badge via RPC",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
            ))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'service-badge'".to_string())?;

        match name {
            "verify" => {
                let badge = require_positional(sub_matches, "badge")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(ServiceBadgeCmd::Verify { badge, url })
            }
            "issue" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(ServiceBadgeCmd::Issue { url })
            }
            "revoke" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(ServiceBadgeCmd::Revoke { url })
            }
            other => Err(format!("unknown subcommand '{other}' for 'service-badge'")),
        }
    }
}

pub fn handle(cmd: ServiceBadgeCmd) {
    match cmd {
        ServiceBadgeCmd::Verify { badge, url } => {
            let client = RpcClient::from_env();
            let params = json_object_from([("badge", json_string(badge))]);
            let payload = json_rpc_request("service_badge_verify", params);
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::Issue { url } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("service_badge_issue", empty_object());
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::Revoke { url } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("service_badge_revoke", empty_object());
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
    }
}
