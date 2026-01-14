use crate::{json_helpers::json_rpc_request, parse_utils::take_string, rpc::RpcClient};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::{
    encoding::hex,
    hashing::blake3,
    signatures::ed25519::{Signature as EdSignature, VerifyingKey},
};
use foundation_serialization::json::{self, Value};
use std::fs;
use std::path::Path;
use std::process;

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
    Budget {
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    Register {
        url: String,
        auth: Option<String>,
        campaign_path: String,
    },
    Readiness {
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    PolicyVerify {
        data_dir: String,
        epoch: u64,
    },
    PresenceList {
        url: String,
        auth: Option<String>,
        pretty: bool,
        region: Option<String>,
        domain_tier: Option<String>,
        min_confidence_bps: Option<u64>,
        kind: Option<String>,
        limit: Option<u64>,
    },
    PresenceReserve {
        url: String,
        auth: Option<String>,
        campaign_id: String,
        bucket_id: String,
        slot_count: u64,
    },
    ClaimRoutes {
        url: String,
        auth: Option<String>,
        pretty: bool,
        domain: String,
        provider: Option<String>,
        domain_tier: Option<String>,
        presence_bucket_id: Option<String>,
        interest_tags: Option<String>,
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
        .subcommand(Self::budget_command())
        .subcommand(Self::register_command())
        .subcommand(Self::readiness_command())
        .subcommand(Self::policy_command())
        .subcommand(Self::presence_command())
        .subcommand(Self::claim_routes_command())
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

    fn budget_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.budget"),
            "budget",
            "Show budget broker snapshot",
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

    fn readiness_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.readiness"),
            "readiness",
            "Show readiness snapshot (thresholds, dynamic config, rehearsal status)",
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

    fn policy_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.policy"),
            "policy",
            "Advertising policy tooling",
        )
        .subcommand(Self::policy_verify_command())
        .build()
    }

    fn policy_verify_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.policy.verify"),
            "verify",
            "Verify local policy snapshot signature",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("data_dir", "data-dir", "Node data directory").default("node-data"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("epoch", "epoch", "Epoch number").required(true),
        ))
        .build()
    }

    fn presence_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.presence"),
            "presence",
            "Presence cohort management",
        )
        .subcommand(Self::presence_list_command())
        .subcommand(Self::presence_reserve_command())
        .build()
    }

    fn presence_list_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.presence.list"),
            "list",
            "List available presence cohorts for targeting",
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
        .arg(ArgSpec::Option(OptionSpec::new(
            "region",
            "region",
            "Filter by ISO 3166-1 alpha-2 region code",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "domain_tier",
            "domain-tier",
            "Filter by domain tier (premium|reserved|community|unverified)",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "min_confidence",
            "min-confidence",
            "Minimum presence confidence in basis points (0-10000)",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "kind",
            "kind",
            "Filter by presence source (localnet|range_boost)",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "limit",
            "limit",
            "Maximum number of cohorts to return",
        )))
        .build()
    }

    fn presence_reserve_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.presence.reserve"),
            "reserve",
            "Reserve presence slots for a campaign",
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
            OptionSpec::new("campaign", "campaign", "Campaign ID").required(true),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("bucket", "bucket", "Presence bucket ID").required(true),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("slots", "slots", "Number of slots to reserve").required(true),
        ))
        .build()
    }

    fn claim_routes_command() -> Command {
        CommandBuilder::new(
            CommandId("ad_market.claim_routes"),
            "claim-routes",
            "Inspect payout claim routes for a domain/cohort",
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
        .arg(ArgSpec::Option(
            OptionSpec::new("domain", "domain", "Domain to query").required(true),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "provider",
            "provider",
            "Optional provider hint",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "domain_tier",
            "domain-tier",
            "Domain tier hint (premium|reserved|community|unverified)",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "presence_bucket_id",
            "presence-bucket-id",
            "Presence bucket id if applicable",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "interest_tags",
            "interest-tags",
            "Comma-separated interest tags",
        )))
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
            "budget" => Ok(Self::Budget {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                pretty: sub_matches.get_flag("pretty"),
            }),
            "readiness" => Ok(Self::Readiness {
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
            "policy" => {
                let (policy_sub, policy_matches) = sub_matches
                    .subcommand()
                    .ok_or_else(|| "missing subcommand for 'ad-market policy'".to_string())?;
                match policy_sub {
                    "verify" => {
                        let epoch_str = take_string(policy_matches, "epoch")
                            .ok_or_else(|| "missing '--epoch'".to_string())?;
                        let epoch = epoch_str
                            .parse::<u64>()
                            .map_err(|_| "invalid '--epoch' value".to_string())?;
                        let data_dir = take_string(policy_matches, "data_dir")
                            .unwrap_or_else(|| "node-data".to_string());
                        Ok(Self::PolicyVerify { data_dir, epoch })
                    }
                    other => Err(format!("unknown ad-market policy subcommand '{other}'")),
                }
            }
            "presence" => {
                let (presence_sub, presence_matches) = sub_matches
                    .subcommand()
                    .ok_or_else(|| "missing subcommand for 'ad-market presence'".to_string())?;
                match presence_sub {
                    "list" => Ok(Self::PresenceList {
                        url: take_string(presence_matches, "url")
                            .unwrap_or_else(|| "http://localhost:26658".to_string()),
                        auth: take_string(presence_matches, "auth"),
                        pretty: presence_matches.get_flag("pretty"),
                        region: take_string(presence_matches, "region"),
                        domain_tier: take_string(presence_matches, "domain_tier"),
                        min_confidence_bps: take_string(presence_matches, "min_confidence")
                            .and_then(|s| s.parse::<u64>().ok()),
                        kind: take_string(presence_matches, "kind"),
                        limit: take_string(presence_matches, "limit")
                            .and_then(|s| s.parse::<u64>().ok()),
                    }),
                    "reserve" => {
                        let campaign_id = take_string(presence_matches, "campaign")
                            .ok_or_else(|| "missing '--campaign'".to_string())?;
                        let bucket_id = take_string(presence_matches, "bucket")
                            .ok_or_else(|| "missing '--bucket'".to_string())?;
                        let slots_str = take_string(presence_matches, "slots")
                            .ok_or_else(|| "missing '--slots'".to_string())?;
                        let slot_count = slots_str
                            .parse::<u64>()
                            .map_err(|_| "invalid '--slots' value".to_string())?;
                        Ok(Self::PresenceReserve {
                            url: take_string(presence_matches, "url")
                                .unwrap_or_else(|| "http://localhost:26658".to_string()),
                            auth: take_string(presence_matches, "auth"),
                            campaign_id,
                            bucket_id,
                            slot_count,
                        })
                    }
                    other => Err(format!("unknown ad-market presence subcommand '{other}'")),
                }
            }
            "claim-routes" => Ok(Self::ClaimRoutes {
                url: take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string()),
                auth: take_string(sub_matches, "auth"),
                pretty: sub_matches.get_flag("pretty"),
                domain: take_string(sub_matches, "domain")
                    .ok_or_else(|| "missing '--domain'".to_string())?,
                provider: take_string(sub_matches, "provider"),
                domain_tier: take_string(sub_matches, "domain_tier")
                    .or_else(|| take_string(sub_matches, "domain-tier")),
                presence_bucket_id: take_string(sub_matches, "presence_bucket_id")
                    .or_else(|| take_string(sub_matches, "presence-bucket-id")),
                interest_tags: take_string(sub_matches, "interest_tags")
                    .or_else(|| take_string(sub_matches, "interest-tags")),
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
        AdMarketCmd::Budget { url, auth, pretty } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("ad_market.budget", Value::Null);
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
        AdMarketCmd::Readiness { url, auth, pretty } => {
            let client = RpcClient::from_env();
            let payload = json_rpc_request("ad_market.readiness", Value::Null);
            print_rpc_response(&client, &url, payload, auth.as_deref(), pretty);
        }
        AdMarketCmd::PolicyVerify { data_dir, epoch } => {
            match verify_policy_snapshot(&data_dir, epoch) {
                Ok(()) => {
                    println!(
                        "snapshot {epoch} under {} verified successfully",
                        Path::new(&data_dir).join("ad_policy").display()
                    );
                }
                Err(err) => {
                    eprintln!("policy snapshot verification failed: {err}");
                    process::exit(1);
                }
            }
        }
        AdMarketCmd::PresenceList {
            url,
            auth,
            pretty,
            region,
            domain_tier,
            min_confidence_bps,
            kind,
            limit,
        } => {
            let client = RpcClient::from_env();
            let mut params = json::Map::new();
            if let Some(r) = region {
                params.insert("region".into(), Value::String(r));
            }
            if let Some(dt) = domain_tier {
                params.insert("domain_tier".into(), Value::String(dt));
            }
            if let Some(mc) = min_confidence_bps {
                params.insert(
                    "min_confidence_bps".into(),
                    Value::Number(json::Number::from(mc)),
                );
            }
            if let Some(k) = kind {
                params.insert("kind".into(), Value::String(k));
            }
            if let Some(l) = limit {
                params.insert("limit".into(), Value::Number(json::Number::from(l)));
            }
            let payload =
                json_rpc_request("ad_market.list_presence_cohorts", Value::Object(params));
            print_rpc_response(&client, &url, payload, auth.as_deref(), pretty);
        }
        AdMarketCmd::PresenceReserve {
            url,
            auth,
            campaign_id,
            bucket_id,
            slot_count,
        } => {
            let client = RpcClient::from_env();
            let mut params = json::Map::new();
            params.insert("campaign_id".into(), Value::String(campaign_id));
            params.insert("presence_bucket_id".into(), Value::String(bucket_id));
            params.insert(
                "slot_count".into(),
                Value::Number(json::Number::from(slot_count)),
            );
            let payload = json_rpc_request("ad_market.reserve_presence", Value::Object(params));
            print_rpc_response(&client, &url, payload, auth.as_deref(), true);
        }
        AdMarketCmd::ClaimRoutes {
            url,
            auth,
            pretty,
            domain,
            provider,
            domain_tier,
            presence_bucket_id,
            interest_tags,
        } => {
            let client = RpcClient::from_env();
            let mut params = json::Map::new();
            params.insert("domain".into(), Value::String(domain));
            if let Some(p) = provider {
                params.insert("provider".into(), Value::String(p));
            }
            if let Some(dt) = domain_tier {
                params.insert("domain_tier".into(), Value::String(dt));
            }
            if let Some(pb) = presence_bucket_id {
                params.insert("presence_bucket_id".into(), Value::String(pb));
            }
            if let Some(tags) = interest_tags {
                let arr: Vec<Value> = tags
                    .split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| Value::String(s.trim().to_string()))
                    .collect();
                params.insert("interest_tags".into(), Value::Array(arr));
            }
            let payload = json_rpc_request("ad_market.claim_routes", Value::Object(params));
            print_rpc_response(&client, &url, payload, auth.as_deref(), pretty);
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

fn verify_policy_snapshot(data_dir: &str, epoch: u64) -> Result<(), String> {
    let policy_dir = Path::new(data_dir).join("ad_policy");
    let json_path = policy_dir.join(format!("{epoch}.json"));
    let sig_path = policy_dir.join(format!("{epoch}.sig"));
    let payload = fs::read(&json_path)
        .map_err(|err| format!("read snapshot {}: {err}", json_path.display()))?;
    let digest = blake3::hash(&payload);
    let sig_bytes =
        fs::read(&sig_path).map_err(|err| format!("read sidecar {}: {err}", sig_path.display()))?;
    let sidecar: Value = json::from_slice(&sig_bytes)
        .map_err(|err| format!("decode sidecar {}: {err}", sig_path.display()))?;
    let obj = sidecar
        .as_object()
        .ok_or_else(|| "sidecar payload is not an object".to_string())?;
    let pub_hex = obj
        .get("pubkey_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| "sidecar missing pubkey_hex".to_string())?;
    let sig_hex = obj
        .get("signature_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| "sidecar missing signature_hex".to_string())?;
    let hash_hex = obj
        .get("payload_hash_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| "sidecar missing payload_hash_hex".to_string())?;
    let digest_hex = digest.to_hex().to_string();
    if hash_hex != digest_hex {
        return Err("payload hash mismatch".into());
    }
    let pub_bytes_vec = hex::decode(pub_hex).map_err(|err| format!("decode pubkey: {err}"))?;
    let pub_bytes: [u8; 32] = pub_bytes_vec
        .try_into()
        .map_err(|_| "pubkey has invalid length".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&pub_bytes).map_err(|err| format!("invalid pubkey: {err}"))?;
    let sig_bytes_vec = hex::decode(sig_hex).map_err(|err| format!("decode signature: {err}"))?;
    let sig_bytes: [u8; 64] = sig_bytes_vec
        .try_into()
        .map_err(|_| "signature has invalid length".to_string())?;
    let signature = EdSignature::from_bytes(&sig_bytes);
    verifying_key
        .verify(digest.as_bytes(), &signature)
        .map_err(|err| format!("signature verification failed: {err}"))?;
    Ok(())
}
