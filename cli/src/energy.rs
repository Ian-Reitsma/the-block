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
        } => {
            let hash = meter_hash.ok_or_else(|| "missing --meter-hash".to_string())?;
            let mut pairs = vec![
                ("provider_id", json_string(provider_id)),
                ("kwh_consumed", json_u64(kwh_consumed)),
                ("meter_hash", json_string(hash)),
                ("buyer", json_string(buyer)),
            ];
            let payload = json_rpc_request("energy.settle", json_object_from(pairs));
            dispatch(&client, &url, payload).map(|text| println!("{text}"))
        }
        EnergyCmd::SubmitReading { reading, url } => {
            let payload = json_rpc_request("energy.submit_reading", reading);
            dispatch(&client, &url, payload).map(|text| println!("{text}"))
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
