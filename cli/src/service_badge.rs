use crate::json_helpers::{empty_object, json_object_from, json_rpc_request, json_string};
use crate::parse_utils::{require_positional, take_string};
use crate::rpc::RpcClient;
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::Value;

pub enum ServiceBadgeCmd {
    /// Verify a badge token via RPC
    Verify {
        badge: String,
        url: String,
    },
    /// Issue a new badge via RPC
    Issue {
        url: String,
    },
    /// Revoke the current badge via RPC
    Revoke {
        url: String,
    },
    VenueRegister {
        url: String,
        venue: String,
    },
    VenueRotate {
        url: String,
        venue: String,
    },
    VenueStatus {
        url: String,
        venue: String,
    },
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
        .subcommand(
            CommandBuilder::new(
                CommandId("service-badge.venue"),
                "venue",
                "Venue management",
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("service-badge.venue.register"),
                    "register",
                    "Register a venue and issue a presence token",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "venue",
                    "Venue identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("service-badge.venue.rotate"),
                    "rotate",
                    "Rotate a venue presence token",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "venue",
                    "Venue identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("service-badge.venue.status"),
                    "status",
                    "Show last recorded venue crowd status",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "venue",
                    "Venue identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
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
            "venue" => {
                let (venue_cmd, venue_matches) = sub_matches
                    .subcommand()
                    .ok_or_else(|| "missing subcommand for 'service-badge venue'".to_string())?;
                match venue_cmd {
                    "register" => Ok(ServiceBadgeCmd::VenueRegister {
                        venue: require_positional(venue_matches, "venue")?,
                        url: take_string(venue_matches, "url")
                            .unwrap_or_else(|| "http://localhost:26658".to_string()),
                    }),
                    "rotate" => Ok(ServiceBadgeCmd::VenueRotate {
                        venue: require_positional(venue_matches, "venue")?,
                        url: take_string(venue_matches, "url")
                            .unwrap_or_else(|| "http://localhost:26658".to_string()),
                    }),
                    "status" => Ok(ServiceBadgeCmd::VenueStatus {
                        venue: require_positional(venue_matches, "venue")?,
                        url: take_string(venue_matches, "url")
                            .unwrap_or_else(|| "http://localhost:26658".to_string()),
                    }),
                    other => Err(format!(
                        "unknown subcommand '{other}' for 'service-badge venue'"
                    )),
                }
            }
            other => Err(format!("unknown subcommand '{other}' for 'service-badge'")),
        }
    }
}

fn verify_request(badge: &str) -> Value {
    let params = json_object_from([("badge", json_string(badge))]);
    json_rpc_request("service_badge_verify", params)
}

fn issue_request() -> Value {
    json_rpc_request("service_badge_issue", empty_object())
}

fn revoke_request() -> Value {
    json_rpc_request("service_badge_revoke", empty_object())
}

pub fn handle(cmd: ServiceBadgeCmd) {
    match cmd {
        ServiceBadgeCmd::Verify { badge, url } => {
            let client = RpcClient::from_env();
            let payload = verify_request(&badge);
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::Issue { url } => {
            let client = RpcClient::from_env();
            let payload = issue_request();
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::Revoke { url } => {
            let client = RpcClient::from_env();
            let payload = revoke_request();
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::VenueRegister { url, venue } => {
            let client = RpcClient::from_env();
            let payload = json_object_from([("venue_id", json_string(&venue))]);
            let envelope = json_rpc_request("gateway.venue_register", payload);
            if let Ok(resp) = client.call(&url, &envelope) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::VenueRotate { url, venue } => {
            let client = RpcClient::from_env();
            let payload = json_object_from([("venue_id", json_string(&venue))]);
            let envelope = json_rpc_request("gateway.venue_rotate", payload);
            if let Ok(resp) = client.call(&url, &envelope) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
        ServiceBadgeCmd::VenueStatus { url, venue } => {
            let client = RpcClient::from_env();
            let payload = json_object_from([("venue_id", json_string(&venue))]);
            let envelope = json_rpc_request("gateway.venue_status", payload);
            if let Ok(resp) = client.call(&url, &envelope) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::{Map as JsonMap, Number, Value};

    fn jsonrpc_baseline(method: &str, params: Value) -> Value {
        let mut map = JsonMap::new();
        map.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
        map.insert("id".to_string(), Value::Number(Number::from(1)));
        map.insert("method".to_string(), Value::String(method.to_string()));
        map.insert("params".to_string(), params);
        Value::Object(map)
    }

    #[test]
    fn verify_request_wraps_badge_param() {
        let payload = verify_request("badge-token");
        let mut params = JsonMap::new();
        params.insert(
            "badge".to_string(),
            Value::String("badge-token".to_string()),
        );
        let expected = jsonrpc_baseline("service_badge_verify", Value::Object(params));
        assert_eq!(payload, expected);
    }

    #[test]
    fn issue_request_uses_empty_object() {
        let payload = issue_request();
        let expected = jsonrpc_baseline("service_badge_issue", Value::Object(JsonMap::new()));
        assert_eq!(payload, expected);
    }

    #[test]
    fn revoke_request_uses_empty_object() {
        let payload = revoke_request();
        let expected = jsonrpc_baseline("service_badge_revoke", Value::Object(JsonMap::new()));
        assert_eq!(payload, expected);
    }
}
