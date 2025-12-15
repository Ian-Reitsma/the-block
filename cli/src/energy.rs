use crate::codec_helpers::json_from_str;
use crate::json_helpers::{json_null, json_object_from, json_rpc_request, json_string, json_u64};
use crate::parse_utils::{
    parse_positional_u64, parse_u64_required, require_positional, take_string,
};
use crate::rpc::RpcClient;
use cli_core::arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec};
use cli_core::command::{Command, CommandBuilder, CommandId};
use cli_core::parse::Matches;
use foundation_serialization::json::Value as JsonValue;

const DEFAULT_RPC_URL: &str = "http://localhost:26658";

pub enum EnergyCmd {
    Register {
        capacity_kwh: u64,
        price_per_kwh: u64,
        meter_address: String,
        jurisdiction: String,
        stake: u64,
        owner: String,
        url: String,
    },
    Market {
        provider_id: Option<String>,
        verbose: bool,
        url: String,
    },
    Settle {
        provider_id: String,
        kwh_consumed: u64,
        meter_hash: Option<String>,
        buyer: String,
        url: String,
    },
    SubmitReading {
        reading: JsonValue,
        url: String,
    },
    Disputes {
        provider_id: Option<String>,
        status: Option<String>,
        meter_hash: Option<String>,
        page: u64,
        page_size: u64,
        url: String,
        json: bool,
    },
    FlagDispute {
        meter_hash: String,
        reason: String,
        reporter: String,
        url: String,
    },
    ResolveDispute {
        dispute_id: u64,
        resolution_note: Option<String>,
        resolver: String,
        url: String,
    },
    Receipts {
        provider_id: Option<String>,
        page: u64,
        page_size: u64,
        url: String,
        json: bool,
    },
    Credits {
        provider_id: Option<String>,
        page: u64,
        page_size: u64,
        url: String,
        json: bool,
    },
}

impl EnergyCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("energy"), "energy", "Energy marketplace tooling")
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.register"),
                    "register",
                    "Register as an energy provider",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "capacity_kwh",
                    "Advertised capacity in kWh",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "price_per_kwh",
                    "Price per kWh (in CT microunits)",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "meter_address",
                        "meter-address",
                        "Registered meter identifier",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "jurisdiction",
                        "jurisdiction",
                        "Jurisdiction pack (e.g. US_CA)",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "owner",
                    "owner",
                    "Owner account identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("stake", "stake", "Stake/bond amount in CT").default("1000"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.market"),
                    "market",
                    "Query current energy market state",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "provider_id",
                    "provider-id",
                    "Filter by provider identifier",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "verbose",
                    "verbose",
                    "Print raw JSON response",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.settle"),
                    "settle",
                    "Settle delivered energy",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "provider_id",
                    "Provider identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "kwh_consumed",
                    "kWh consumed",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "meter_hash",
                    "meter-hash",
                    "Meter reading hash to consume",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "buyer",
                    "buyer",
                    "Buyer account identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.submit_reading"),
                    "submit-reading",
                    "Submit a signed meter reading JSON blob",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "reading_json",
                        "reading-json",
                        "JSON payload describing the meter reading",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.disputes"),
                    "disputes",
                    "List or filter energy disputes",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "provider_id",
                    "provider-id",
                    "Filter by provider identifier",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "status",
                    "status",
                    "Filter by status (open|resolved)",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "meter_hash",
                    "meter-hash",
                    "Filter by meter hash",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page", "page", "Page index").default("0"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page_size", "page-size", "Page size").default("25"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit raw JSON response",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.receipts"),
                    "receipts",
                    "List settled energy receipts",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "provider_id",
                    "provider-id",
                    "Filter by provider identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page", "page", "Page index").default("0"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page_size", "page-size", "Page size").default("25"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit raw JSON response",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.credits"),
                    "credits",
                    "List pending energy meter credits",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "provider_id",
                    "provider-id",
                    "Filter by provider identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page", "page", "Page index").default("0"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page_size", "page-size", "Page size").default("25"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit raw JSON response",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.flag_dispute"),
                    "flag-dispute",
                    "Flag a meter reading or receipt for dispute review",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("meter_hash", "meter-hash", "Meter hash to dispute")
                        .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("reason", "reason", "Dispute reason").required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("reporter", "reporter", "Reporter identifier")
                        .default("anonymous"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("energy.resolve_dispute"),
                    "resolve-dispute",
                    "Resolve an existing dispute",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("dispute_id", "dispute-id", "Dispute identifier")
                        .required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "resolution_note",
                    "resolution-note",
                    "Optional resolution note",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("resolver", "resolver", "Resolver identifier")
                        .default("system"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default(DEFAULT_RPC_URL),
                ))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'energy'".to_string())?;
        match name {
            "register" => {
                let capacity_kwh = parse_positional_u64(sub_matches, "capacity_kwh")?;
                let price_per_kwh = parse_positional_u64(sub_matches, "price_per_kwh")?;
                let meter_address = take_string(sub_matches, "meter_address")
                    .ok_or_else(|| "missing --meter-address".to_string())?;
                let jurisdiction = take_string(sub_matches, "jurisdiction")
                    .ok_or_else(|| "missing --jurisdiction".to_string())?;
                let stake = parse_u64_required(take_string(sub_matches, "stake"), "stake")?;
                let owner =
                    take_string(sub_matches, "owner").unwrap_or_else(|| "anonymous".to_string());
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                Ok(EnergyCmd::Register {
                    capacity_kwh,
                    price_per_kwh,
                    meter_address,
                    jurisdiction,
                    stake,
                    owner,
                    url,
                })
            }
            "market" => Ok(EnergyCmd::Market {
                provider_id: take_string(sub_matches, "provider_id"),
                verbose: sub_matches.get_flag("verbose"),
                url: take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into()),
            }),
            "settle" => {
                let provider_id = require_positional(sub_matches, "provider_id")?.to_string();
                let kwh_consumed = parse_positional_u64(sub_matches, "kwh_consumed")?;
                let meter_hash = take_string(sub_matches, "meter_hash");
                let buyer = take_string(sub_matches, "buyer").unwrap_or_else(|| "self".into());
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                Ok(EnergyCmd::Settle {
                    provider_id,
                    kwh_consumed,
                    meter_hash,
                    buyer,
                    url,
                })
            }
            "submit-reading" => {
                let raw = take_string(sub_matches, "reading_json")
                    .ok_or_else(|| "missing --reading-json".to_string())?;
                let reading = json_from_str::<JsonValue>(&raw)
                    .map_err(|err| format!("invalid JSON payload: {err}"))?;
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                Ok(EnergyCmd::SubmitReading { reading, url })
            }
            "disputes" => {
                let provider_id = take_string(sub_matches, "provider_id");
                let status =
                    take_string(sub_matches, "status").map(|value| value.to_ascii_lowercase());
                let meter_hash = take_string(sub_matches, "meter_hash");
                let page = parse_u64_required(take_string(sub_matches, "page"), "page")?;
                let page_size =
                    parse_u64_required(take_string(sub_matches, "page_size"), "page_size")?;
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                let json = sub_matches.get_flag("json");
                Ok(EnergyCmd::Disputes {
                    provider_id,
                    status,
                    meter_hash,
                    page,
                    page_size,
                    url,
                    json,
                })
            }
            "flag-dispute" => {
                let meter_hash = take_string(sub_matches, "meter_hash")
                    .ok_or_else(|| "missing --meter-hash".to_string())?;
                let reason = take_string(sub_matches, "reason")
                    .ok_or_else(|| "missing --reason".to_string())?;
                let reporter =
                    take_string(sub_matches, "reporter").unwrap_or_else(|| "anonymous".into());
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                Ok(EnergyCmd::FlagDispute {
                    meter_hash,
                    reason,
                    reporter,
                    url,
                })
            }
            "resolve-dispute" => {
                let dispute_id =
                    parse_u64_required(take_string(sub_matches, "dispute_id"), "dispute_id")?;
                let resolution_note = take_string(sub_matches, "resolution_note");
                let resolver =
                    take_string(sub_matches, "resolver").unwrap_or_else(|| "system".into());
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                Ok(EnergyCmd::ResolveDispute {
                    dispute_id,
                    resolution_note,
                    resolver,
                    url,
                })
            }
            "receipts" => {
                let provider_id = take_string(sub_matches, "provider_id");
                let page = parse_u64_required(take_string(sub_matches, "page"), "page")?;
                let page_size =
                    parse_u64_required(take_string(sub_matches, "page_size"), "page_size")?;
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                let json = sub_matches.get_flag("json");
                Ok(EnergyCmd::Receipts {
                    provider_id,
                    page,
                    page_size,
                    url,
                    json,
                })
            }
            "credits" => {
                let provider_id = take_string(sub_matches, "provider_id");
                let page = parse_u64_required(take_string(sub_matches, "page"), "page")?;
                let page_size =
                    parse_u64_required(take_string(sub_matches, "page_size"), "page_size")?;
                let url = take_string(sub_matches, "url").unwrap_or_else(|| DEFAULT_RPC_URL.into());
                let json = sub_matches.get_flag("json");
                Ok(EnergyCmd::Credits {
                    provider_id,
                    page,
                    page_size,
                    url,
                    json,
                })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn handle(cmd: EnergyCmd) {
    let client = RpcClient::from_env();
    let result: Result<(), String> = match cmd {
        EnergyCmd::Register {
            capacity_kwh,
            price_per_kwh,
            meter_address,
            jurisdiction,
            stake,
            owner,
            url,
        } => {
            let params = json_object_from([
                ("capacity_kwh", json_u64(capacity_kwh)),
                ("price_per_kwh", json_u64(price_per_kwh)),
                ("meter_address", json_string(meter_address)),
                ("jurisdiction", json_string(jurisdiction)),
                ("stake", json_u64(stake)),
                ("owner", json_string(owner)),
            ]);
            let payload = json_rpc_request("energy.register_provider", params);
            dispatch(&client, &url, payload).map(|text| println!("{text}"))
        }
        EnergyCmd::Market {
            provider_id,
            verbose,
            url,
        } => {
            let params = provider_id
                .map(|provider| json_object_from([("provider_id", json_string(provider))]))
                .unwrap_or_else(json_null);
            let payload = json_rpc_request("energy.market_state", params);
            dispatch(&client, &url, payload).map(|text| {
                if verbose {
                    println!("{text}");
                } else if let Ok(value) = json_from_str::<JsonValue>(&text) {
                    println!("energy_market => {:#}", value);
                } else {
                    println!("{text}");
                }
            })
        }
        EnergyCmd::Settle {
            provider_id,
            kwh_consumed,
            meter_hash,
            buyer,
            url,
        } => match meter_hash {
            Some(hash) => {
                let pairs = vec![
                    ("provider_id", json_string(provider_id)),
                    ("kwh_consumed", json_u64(kwh_consumed)),
                    ("meter_hash", json_string(hash)),
                    ("buyer", json_string(buyer)),
                ];
                let payload = json_rpc_request("energy.settle", json_object_from(pairs));
                dispatch(&client, &url, payload).map(|text| println!("{text}"))
            }
            None => Err("missing --meter-hash".to_string()),
        },
        EnergyCmd::SubmitReading { reading, url } => {
            let payload = json_rpc_request("energy.submit_reading", reading);
            dispatch(&client, &url, payload).map(|text| println!("{text}"))
        }
        EnergyCmd::Disputes {
            provider_id,
            status,
            meter_hash,
            page,
            page_size,
            url,
            json,
        } => {
            let mut pairs = vec![("page", json_u64(page)), ("page_size", json_u64(page_size))];
            if let Some(provider) = provider_id {
                pairs.push(("provider_id", json_string(provider)));
            }
            if let Some(state) = status {
                pairs.push(("status", json_string(state)));
            }
            if let Some(hash) = meter_hash {
                pairs.push(("meter_hash", json_string(hash)));
            }
            let payload = json_rpc_request("energy.disputes", json_object_from(pairs));
            dispatch(&client, &url, payload).map(|text| print_disputes_response(&text, json))
        }
        EnergyCmd::FlagDispute {
            meter_hash,
            reason,
            reporter,
            url,
        } => {
            let payload = json_rpc_request(
                "energy.flag_dispute",
                json_object_from(vec![
                    ("meter_hash", json_string(meter_hash)),
                    ("reason", json_string(reason)),
                    ("reporter", json_string(reporter)),
                ]),
            );
            dispatch(&client, &url, payload).map(|text| print_single_dispute_response(&text, false))
        }
        EnergyCmd::ResolveDispute {
            dispute_id,
            resolution_note,
            resolver,
            url,
        } => {
            let mut pairs = vec![
                ("dispute_id", json_u64(dispute_id)),
                ("resolver", json_string(resolver)),
            ];
            if let Some(note) = resolution_note {
                pairs.push(("resolution_note", json_string(note)));
            }
            let payload = json_rpc_request("energy.resolve_dispute", json_object_from(pairs));
            dispatch(&client, &url, payload).map(|text| print_single_dispute_response(&text, false))
        }
        EnergyCmd::Receipts {
            provider_id,
            page,
            page_size,
            url,
            json,
        } => {
            let mut pairs = vec![("page", json_u64(page)), ("page_size", json_u64(page_size))];
            if let Some(provider) = provider_id {
                pairs.push(("provider_id", json_string(provider)));
            }
            let payload = json_rpc_request("energy.receipts", json_object_from(pairs));
            dispatch(&client, &url, payload).map(|text| print_receipts_response(&text, json))
        }
        EnergyCmd::Credits {
            provider_id,
            page,
            page_size,
            url,
            json,
        } => {
            let mut pairs = vec![("page", json_u64(page)), ("page_size", json_u64(page_size))];
            if let Some(provider) = provider_id {
                pairs.push(("provider_id", json_string(provider)));
            }
            let payload = json_rpc_request("energy.credits", json_object_from(pairs));
            dispatch(&client, &url, payload).map(|text| print_credits_response(&text, json))
        }
    };
    if let Err(err) = result {
        eprintln!("energy command failed: {err}");
    }
}

fn dispatch(client: &RpcClient, url: &str, payload: JsonValue) -> Result<String, String> {
    let response = client
        .call(url, &payload)
        .map_err(|err| format!("rpc error: {err}"))?;
    response
        .text()
        .map_err(|err| format!("failed to read response body: {err}"))
}

fn print_paginated_list<F>(body: &str, json: bool, list_key: &str, heading: &str, fmt: F)
where
    F: Fn(&JsonValue) -> String,
{
    if json {
        println!("{body}");
        return;
    }
    match json_from_str::<JsonValue>(body) {
        Ok(value) => {
            if value.get("error").is_some() {
                println!("{body}");
                return;
            }
            let page = value.get("page").and_then(|v| v.as_u64()).unwrap_or(0);
            let page_size = value.get("page_size").and_then(|v| v.as_u64()).unwrap_or(0);
            let total = value.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("{heading} page {page} size {page_size} (total {total})");
            if let Some(items) = value.get(list_key).and_then(|v| v.as_array()) {
                for item in items {
                    println!(" - {}", fmt(item));
                }
            }
        }
        Err(_) => println!("{body}"),
    }
}

fn print_disputes_response(body: &str, json: bool) {
    print_paginated_list(body, json, "disputes", "energy disputes", |dispute| {
        let id = dispute.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let provider = dispute
            .get("provider_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let status = dispute
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let reason = dispute
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        format!("#{id} provider={provider} status={status} reason={reason}")
    });
}

fn print_single_dispute_response(body: &str, json: bool) {
    if json {
        println!("{body}");
        return;
    }
    match json_from_str::<JsonValue>(body) {
        Ok(value) => {
            if value.get("error").is_some() {
                println!("{body}");
                return;
            }
            if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                let provider = value
                    .get("provider_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("-");
                println!("dispute #{id} provider={provider} status={status}");
            } else {
                println!("{body}");
            }
        }
        Err(_) => println!("{body}"),
    }
}

fn print_receipts_response(body: &str, json: bool) {
    print_paginated_list(body, json, "receipts", "energy receipts", |receipt| {
        let buyer = receipt.get("buyer").and_then(|v| v.as_str()).unwrap_or("-");
        let seller = receipt
            .get("seller")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let kwh = receipt
            .get("kwh_delivered")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let price = receipt
            .get("price_paid")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        format!("{seller} -> {buyer} {kwh}kWh price={price}ct")
    });
}

fn print_credits_response(body: &str, json: bool) {
    print_paginated_list(body, json, "credits", "energy credits", |credit| {
        let provider = credit
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let amount = credit
            .get("amount_kwh")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let timestamp = credit
            .get("timestamp")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        format!("{provider} pending={amount}kWh recorded_at={timestamp}")
    });
}
