use crate::{
    codec_helpers::{json_from_str, json_to_string_pretty},
    rpc::RpcClient,
};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::{Map as JsonMap, Number, Value};

use crate::parse_utils::{require_positional, take_string};

fn json_map_from(pairs: Vec<(String, Value)>) -> JsonMap {
    let mut map = JsonMap::new();
    for (key, value) in pairs {
        map.insert(key, value);
    }
    map
}

fn json_object_from(pairs: Vec<(String, Value)>) -> Value {
    Value::Object(json_map_from(pairs))
}

pub enum GatewayCmd {
    /// Inspect or manage the mobile RPC cache
    MobileCache { action: MobileCacheAction },
    /// Manage premium domain auctions and sales
    Domain { action: DomainAction },
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
            .subcommand(DomainAction::command())
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
            "domain" => Ok(GatewayCmd::Domain {
                action: DomainAction::from_matches(sub_matches)?,
            }),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub enum DomainAction {
    List {
        domain: String,
        min_bid: u64,
        stake_requirement: Option<u64>,
        duration_secs: Option<u64>,
        seller: Option<String>,
        seller_stake: Option<String>,
        protocol_fee_bps: Option<u64>,
        royalty_bps: Option<u64>,
        url: String,
        auth: Option<String>,
    },
    Bid {
        domain: String,
        bidder: String,
        bid: u64,
        stake_reference: Option<String>,
        url: String,
        auth: Option<String>,
    },
    Complete {
        domain: String,
        force: bool,
        url: String,
        auth: Option<String>,
    },
    Cancel {
        domain: String,
        seller: String,
        url: String,
        auth: Option<String>,
    },
    StakeRegister {
        reference: String,
        owner: String,
        deposit: u64,
        url: String,
        auth: Option<String>,
    },
    StakeWithdraw {
        reference: String,
        owner: String,
        withdraw: u64,
        url: String,
        auth: Option<String>,
    },
    StakeStatus {
        reference: String,
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
    Status {
        domain: Option<String>,
        url: String,
        auth: Option<String>,
        pretty: bool,
    },
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

impl DomainAction {
    fn command() -> Command {
        CommandBuilder::new(
            CommandId("gateway.domain"),
            "domain",
            "Manage premium domain auctions",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.list"),
                "list",
                "List a domain for auction",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "domain",
                "Domain name",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "min_bid",
                "Minimum bid (BLOCK)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "stake-requirement",
                "stake-requirement",
                "Stake requirement (BLOCK)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "duration",
                "duration",
                "Auction duration (seconds)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "seller",
                "seller",
                "Seller account identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "seller-stake",
                "seller-stake",
                "Seller stake reference",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "protocol-fee-bps",
                "protocol-fee-bps",
                "Protocol fee in basis points",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "royalty-bps",
                "royalty-bps",
                "Royalty share in basis points",
            )))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.bid"),
                "bid",
                "Place a bid on an active auction",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "domain",
                "Domain name",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "bid",
                "Bid amount (BLOCK)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "bidder",
                "bidder",
                "Bidder account identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "stake-ref",
                "stake-ref",
                "Stake reference for the bid",
            )))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.complete"),
                "complete",
                "Complete an auction and transfer ownership",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "domain",
                "Domain name",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "force",
                "force",
                "Complete even if the auction window has not elapsed",
            )))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.cancel"),
                "cancel",
                "Cancel an active auction before settlement",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "domain",
                "Domain name",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "seller",
                "seller",
                "Seller account identifier",
            )))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.stake_register"),
                "stake-register",
                "Deposit BLOCK into a stake reference",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "reference",
                "Stake reference",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "deposit",
                "Deposit amount (BLOCK)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "owner",
                "owner",
                "Stake owner account",
            )))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.stake_withdraw"),
                "stake-withdraw",
                "Withdraw BLOCK from an unlocked stake reference",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "reference",
                "Stake reference",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Withdrawal amount (BLOCK)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "owner",
                "owner",
                "Stake owner account",
            )))
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
        .subcommand(
            CommandBuilder::new(
                CommandId("gateway.domain.stake_status"),
                "stake-status",
                "Inspect a stake reference",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "reference",
                "Stake reference",
            )))
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
                CommandId("gateway.domain.status"),
                "status",
                "Show auction status and sale history",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "domain",
                "domain",
                "Domain filter",
            )))
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
        .build()
    }

    fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'gateway domain'".to_string())?;

        match name {
            "list" => {
                let domain = require_positional(sub_matches, "domain")?;
                let min_bid_raw = require_positional(sub_matches, "min_bid")?;
                let min_bid = min_bid_raw.parse::<u64>().map_err(|_| {
                    format!("invalid value '{min_bid_raw}' for 'min_bid': expected integer")
                })?;
                let stake_requirement = match take_string(sub_matches, "stake-requirement") {
                    Some(raw) => Some(raw.parse::<u64>().map_err(|_| {
                        format!("invalid value '{raw}' for '--stake-requirement': expected integer")
                    })?),
                    None => None,
                };
                let duration_secs = match take_string(sub_matches, "duration") {
                    Some(raw) => Some(raw.parse::<u64>().map_err(|_| {
                        format!("invalid value '{raw}' for '--duration': expected integer")
                    })?),
                    None => None,
                };
                let protocol_fee_bps = match take_string(sub_matches, "protocol-fee-bps") {
                    Some(raw) => Some(raw.parse::<u64>().map_err(|_| {
                        format!("invalid value '{raw}' for '--protocol-fee-bps': expected integer")
                    })?),
                    None => None,
                };
                let royalty_bps = match take_string(sub_matches, "royalty-bps") {
                    Some(raw) => Some(raw.parse::<u64>().map_err(|_| {
                        format!("invalid value '{raw}' for '--royalty-bps': expected integer")
                    })?),
                    None => None,
                };
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::List {
                    domain,
                    min_bid,
                    stake_requirement,
                    duration_secs,
                    seller: take_string(sub_matches, "seller"),
                    seller_stake: take_string(sub_matches, "seller-stake"),
                    protocol_fee_bps,
                    royalty_bps,
                    url,
                    auth: take_string(sub_matches, "auth"),
                })
            }
            "bid" => {
                let domain = require_positional(sub_matches, "domain")?;
                let bid_raw = require_positional(sub_matches, "bid")?;
                let bid = bid_raw.parse::<u64>().map_err(|_| {
                    format!("invalid value '{bid_raw}' for 'bid': expected integer")
                })?;
                let bidder = take_string(sub_matches, "bidder")
                    .ok_or_else(|| "missing required '--bidder' option".to_string())?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::Bid {
                    domain,
                    bidder,
                    bid,
                    stake_reference: take_string(sub_matches, "stake-ref"),
                    url,
                    auth: take_string(sub_matches, "auth"),
                })
            }
            "complete" => {
                let domain = require_positional(sub_matches, "domain")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::Complete {
                    domain,
                    force: sub_matches.get_flag("force"),
                    url,
                    auth: take_string(sub_matches, "auth"),
                })
            }
            "cancel" => {
                let domain = require_positional(sub_matches, "domain")?;
                let seller = take_string(sub_matches, "seller")
                    .ok_or_else(|| "missing required '--seller' option".to_string())?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::Cancel {
                    domain,
                    seller,
                    url,
                    auth: take_string(sub_matches, "auth"),
                })
            }
            "stake-register" => {
                let reference = require_positional(sub_matches, "reference")?;
                let deposit_raw = require_positional(sub_matches, "deposit")?;
                let deposit = deposit_raw.parse::<u64>().map_err(|_| {
                    format!("invalid value '{deposit_raw}' for 'deposit': expected integer")
                })?;
                let owner = take_string(sub_matches, "owner")
                    .ok_or_else(|| "missing required '--owner' option".to_string())?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::StakeRegister {
                    reference,
                    owner,
                    deposit,
                    url,
                    auth: take_string(sub_matches, "auth"),
                })
            }
            "stake-withdraw" => {
                let reference = require_positional(sub_matches, "reference")?;
                let amount_raw = require_positional(sub_matches, "amount")?;
                let withdraw = amount_raw.parse::<u64>().map_err(|_| {
                    format!("invalid value '{amount_raw}' for 'amount': expected integer")
                })?;
                let owner = take_string(sub_matches, "owner")
                    .ok_or_else(|| "missing required '--owner' option".to_string())?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::StakeWithdraw {
                    reference,
                    owner,
                    withdraw,
                    url,
                    auth: take_string(sub_matches, "auth"),
                })
            }
            "stake-status" => {
                let reference = require_positional(sub_matches, "reference")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::StakeStatus {
                    reference,
                    url,
                    auth: take_string(sub_matches, "auth"),
                    pretty: sub_matches.get_flag("pretty"),
                })
            }
            "status" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(DomainAction::Status {
                    domain: take_string(sub_matches, "domain"),
                    url,
                    auth: take_string(sub_matches, "auth"),
                    pretty: sub_matches.get_flag("pretty"),
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'gateway domain'")),
        }
    }
}

pub fn handle(cmd: GatewayCmd) {
    match cmd {
        GatewayCmd::MobileCache { action } => {
            let client = RpcClient::from_env();
            match action {
                MobileCacheAction::Status { url, auth, pretty } => {
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("gateway.mobile_cache_status".to_owned()),
                        ),
                        ("params".to_owned(), Value::Null),
                    ]);
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
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("gateway.mobile_cache_flush".to_owned()),
                        ),
                        ("params".to_owned(), Value::Null),
                    ]);
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
        GatewayCmd::Domain { action } => {
            let client = RpcClient::from_env();
            match action {
                DomainAction::List {
                    domain,
                    min_bid,
                    stake_requirement,
                    duration_secs,
                    seller,
                    seller_stake,
                    protocol_fee_bps,
                    royalty_bps,
                    url,
                    auth,
                } => {
                    let mut params = vec![
                        ("domain".to_owned(), Value::String(domain.clone())),
                        ("min_bid".to_owned(), Value::Number(Number::from(min_bid))),
                    ];
                    if let Some(value) = stake_requirement {
                        params.push((
                            "stake_requirement".to_owned(),
                            Value::Number(Number::from(value)),
                        ));
                    }
                    if let Some(value) = duration_secs {
                        params.push((
                            "duration_secs".to_owned(),
                            Value::Number(Number::from(value)),
                        ));
                    }
                    if let Some(value) = seller {
                        params.push(("seller_account".to_owned(), Value::String(value)));
                    }
                    if let Some(value) = seller_stake {
                        params.push(("seller_stake".to_owned(), Value::String(value)));
                    }
                    if let Some(value) = protocol_fee_bps {
                        params.push((
                            "protocol_fee_bps".to_owned(),
                            Value::Number(Number::from(value)),
                        ));
                    }
                    if let Some(value) = royalty_bps {
                        params.push(("royalty_bps".to_owned(), Value::Number(Number::from(value))));
                    }
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.list_for_sale".to_owned()),
                        ),
                        ("params".to_owned(), Value::Object(json_map_from(params))),
                    ]);
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("list auction failed: {err}");
                        }
                    }
                }
                DomainAction::Bid {
                    domain,
                    bidder,
                    bid,
                    stake_reference,
                    url,
                    auth,
                } => {
                    let mut params = vec![
                        ("domain".to_owned(), Value::String(domain)),
                        ("bidder_account".to_owned(), Value::String(bidder)),
                        ("bid".to_owned(), Value::Number(Number::from(bid))),
                    ];
                    if let Some(value) = stake_reference {
                        params.push(("stake_reference".to_owned(), Value::String(value)));
                    }
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.place_bid".to_owned()),
                        ),
                        ("params".to_owned(), Value::Object(json_map_from(params))),
                    ]);
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("bid submission failed: {err}");
                        }
                    }
                }
                DomainAction::Complete {
                    domain,
                    force,
                    url,
                    auth,
                } => {
                    let mut params = vec![("domain".to_owned(), Value::String(domain))];
                    if force {
                        params.push(("force".to_owned(), Value::Bool(true)));
                    }
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.complete_sale".to_owned()),
                        ),
                        ("params".to_owned(), Value::Object(json_map_from(params))),
                    ]);
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("complete auction failed: {err}");
                        }
                    }
                }
                DomainAction::Cancel {
                    domain,
                    seller,
                    url,
                    auth,
                } => {
                    let params = json_map_from(vec![
                        ("domain".to_owned(), Value::String(domain)),
                        ("seller_account".to_owned(), Value::String(seller)),
                    ]);
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.cancel_sale".to_owned()),
                        ),
                        ("params".to_owned(), Value::Object(params)),
                    ]);
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("cancel auction failed: {err}");
                        }
                    }
                }
                DomainAction::StakeRegister {
                    reference,
                    owner,
                    deposit,
                    url,
                    auth,
                } => {
                    let params = json_map_from(vec![
                        ("reference".to_owned(), Value::String(reference)),
                        ("owner_account".to_owned(), Value::String(owner)),
                        ("deposit".to_owned(), Value::Number(Number::from(deposit))),
                    ]);
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.register_stake".to_owned()),
                        ),
                        ("params".to_owned(), Value::Object(params)),
                    ]);
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("stake register failed: {err}");
                        }
                    }
                }
                DomainAction::StakeWithdraw {
                    reference,
                    owner,
                    withdraw,
                    url,
                    auth,
                } => {
                    let params = json_map_from(vec![
                        ("reference".to_owned(), Value::String(reference)),
                        ("owner_account".to_owned(), Value::String(owner)),
                        ("withdraw".to_owned(), Value::Number(Number::from(withdraw))),
                    ]);
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.withdraw_stake".to_owned()),
                        ),
                        ("params".to_owned(), Value::Object(params)),
                    ]);
                    match client.call_with_auth(&url, &payload, auth.as_deref()) {
                        Ok(resp) => {
                            if let Ok(text) = resp.text() {
                                println!("{}", text);
                            }
                        }
                        Err(err) => {
                            eprintln!("stake withdraw failed: {err}");
                        }
                    }
                }
                DomainAction::StakeStatus {
                    reference,
                    url,
                    auth,
                    pretty,
                } => {
                    let params = Value::Object(json_map_from(vec![(
                        "reference".to_owned(),
                        Value::String(reference),
                    )]));
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.stake_status".to_owned()),
                        ),
                        ("params".to_owned(), params),
                    ]);
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
                                            eprintln!("failed to decode stake status: {err}");
                                            println!("{}", body);
                                        }
                                    }
                                } else {
                                    println!("{}", body);
                                }
                            }
                            Err(err) => {
                                eprintln!("failed to read stake status response: {err}");
                            }
                        },
                        Err(err) => {
                            eprintln!("stake status failed: {err}");
                        }
                    }
                }
                DomainAction::Status {
                    domain,
                    url,
                    auth,
                    pretty,
                } => {
                    let params = match domain {
                        Some(value) => Value::Object(json_map_from(vec![(
                            "domain".to_owned(),
                            Value::String(value),
                        )])),
                        None => Value::Null,
                    };
                    let payload = json_object_from(vec![
                        ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
                        ("id".to_owned(), Value::from(1u32)),
                        (
                            "method".to_owned(),
                            Value::String("dns.auctions".to_owned()),
                        ),
                        ("params".to_owned(), params),
                    ]);
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
                            eprintln!("auction status failed: {err}");
                        }
                    }
                }
            }
        }
    }
}
