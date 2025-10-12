use crate::parse_utils::{parse_optional, parse_u64, parse_u64_required, take_string};
use crate::rpc::RpcClient;
use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
#[cfg(feature = "telemetry")]
use foundation_serialization::json;
#[cfg(feature = "telemetry")]
use std::collections::BTreeMap;
#[cfg(feature = "telemetry")]
use std::time::{SystemTime, UNIX_EPOCH};

pub enum TelemetryCmd {
    /// Dump current telemetry allocation in bytes
    Dump,
    /// Continuously print telemetry allocation every second
    Tail { interval: u64 },
    /// Configure telemetry sampling and compaction intervals
    Configure {
        sample_rate: Option<f64>,
        compaction: Option<u64>,
        url: String,
        token: Option<String>,
    },
    /// Display local TLS environment warning snapshots
    TlsWarnings {
        prefix: Option<String>,
        code: Option<String>,
        json: bool,
        probe_detail: Option<String>,
        probe_variables: Option<Vec<String>>,
    },
}

#[cfg(feature = "telemetry")]
fn format_tls_warning_fingerprint(value: i64) -> String {
    let unsigned = u64::from_le_bytes(value.to_le_bytes());
    format!("{unsigned:016x}")
}

#[cfg(feature = "telemetry")]
fn format_tls_warning_fingerprint_counts(map: &BTreeMap<String, u64>) -> String {
    if map.is_empty() {
        return "-".to_string();
    }
    map.iter()
        .map(|(fingerprint, count)| format!("{fingerprint}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

impl TelemetryCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("telemetry"), "telemetry", "Telemetry diagnostics")
            .subcommand(
                CommandBuilder::new(CommandId("telemetry.dump"), "dump", "Dump telemetry usage")
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("telemetry.tail"),
                    "tail",
                    "Continuously print telemetry allocation every second",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("interval", "interval", "Sampling interval in seconds")
                        .default("1"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("telemetry.configure"),
                    "configure",
                    "Configure telemetry sampling and compaction intervals",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "sample_rate",
                    "sample-rate",
                    "Sampling rate (0.0-1.0)",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "compaction",
                    "compaction",
                    "Compaction interval in seconds",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "Telemetry RPC endpoint")
                        .default("http://localhost:26658"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "token",
                    "token",
                    "Bearer token for authorization",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("telemetry.tls-warnings"),
                    "tls-warnings",
                    "Show local TLS environment warning snapshots",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "prefix",
                    "prefix",
                    "Filter warnings by environment prefix",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "code",
                    "code",
                    "Filter warnings by warning code",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "probe_detail",
                    "probe-detail",
                    "Compute the fingerprint for the provided detail payload",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "probe_variables",
                    "probe-variables",
                    "Compute the fingerprint for a comma-separated variable list",
                )))
                .arg(ArgSpec::Flag(cli_core::arg::FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON output",
                )))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'telemetry'".to_string())?;

        match name {
            "dump" => Ok(TelemetryCmd::Dump),
            "tail" => {
                let interval =
                    parse_u64_required(take_string(sub_matches, "interval"), "interval")?;
                Ok(TelemetryCmd::Tail { interval })
            }
            "configure" => {
                let sample_rate =
                    parse_optional(take_string(sub_matches, "sample_rate"), "sample-rate")?;
                let compaction = parse_u64(take_string(sub_matches, "compaction"), "compaction")?;
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                let token = take_string(sub_matches, "token");
                Ok(TelemetryCmd::Configure {
                    sample_rate,
                    compaction,
                    url,
                    token,
                })
            }
            "tls-warnings" => {
                let prefix = take_string(sub_matches, "prefix");
                let code = take_string(sub_matches, "code");
                let json = sub_matches.get_flag("json");
                let probe_detail = take_string(sub_matches, "probe_detail");
                let probe_variables =
                    take_string(sub_matches, "probe_variables").and_then(|value| {
                        let values: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if values.is_empty() {
                            None
                        } else {
                            Some(values)
                        }
                    });
                Ok(TelemetryCmd::TlsWarnings {
                    prefix,
                    code,
                    json,
                    probe_detail,
                    probe_variables,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'telemetry'")),
        }
    }
}

pub fn handle(cmd: TelemetryCmd) {
    match cmd {
        TelemetryCmd::Dump => {
            #[cfg(feature = "telemetry")]
            println!("{}", the_block::telemetry::current_alloc_bytes());
            #[cfg(not(feature = "telemetry"))]
            println!("telemetry disabled");
        }
        TelemetryCmd::Tail { interval } => {
            #[cfg(feature = "telemetry")]
            {
                use std::thread::sleep;
                use std::time::Duration;
                loop {
                    println!("{}", the_block::telemetry::current_alloc_bytes());
                    sleep(Duration::from_secs(interval));
                }
            }
            #[cfg(not(feature = "telemetry"))]
            {
                let _ = interval;
                println!("telemetry disabled");
            }
        }
        TelemetryCmd::Configure {
            sample_rate,
            compaction,
            url,
            token,
        } => {
            if sample_rate.is_none() && compaction.is_none() {
                eprintln!("no parameters provided");
                return;
            }
            let client = RpcClient::from_env();
            let payload = foundation_serialization::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "telemetry.configure",
                "params": {
                    "sample_rate": sample_rate,
                    "compaction_secs": compaction,
                },
            });
            let auth = token.as_ref().map(|t| format!("Bearer {}", t));
            match client.call_with_auth(&url, &payload, auth.as_deref()) {
                Ok(resp) => match resp.text() {
                    Ok(body) => println!("{}", body),
                    Err(err) => eprintln!("telemetry.configure response error: {err}"),
                },
                Err(err) => eprintln!("telemetry.configure failed: {err}"),
            }
        }
        TelemetryCmd::TlsWarnings {
            prefix,
            code,
            json,
            probe_detail,
            probe_variables,
        } => {
            #[cfg(feature = "telemetry")]
            {
                let mut fingerprint_emitted = false;
                if let Some(detail) = probe_detail.as_ref() {
                    let fingerprint =
                        the_block::telemetry::tls_env_warning_detail_fingerprint(detail);
                    let formatted = format_tls_warning_fingerprint(fingerprint);
                    if json {
                        eprintln!("detail fingerprint: {formatted}");
                    } else {
                        println!("detail fingerprint: {formatted}");
                    }
                    fingerprint_emitted = true;
                }
                if let Some(vars) = probe_variables.as_ref() {
                    if let Some(fingerprint) =
                        the_block::telemetry::tls_env_warning_variables_fingerprint(vars)
                    {
                        let formatted = format_tls_warning_fingerprint(fingerprint);
                        if json {
                            eprintln!("variables fingerprint: {formatted}");
                        } else {
                            println!("variables fingerprint: {formatted}");
                        }
                    } else if json {
                        eprintln!(
                            "variables fingerprint: unavailable (provide at least one variable)"
                        );
                    } else {
                        println!(
                            "variables fingerprint: unavailable (provide at least one variable)"
                        );
                    }
                    fingerprint_emitted = true;
                }
                if fingerprint_emitted && !json {
                    println!();
                }
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let mut snapshots: Vec<_> = the_block::telemetry::tls_env_warning_snapshots()
                    .into_iter()
                    .filter(|snapshot| {
                        prefix
                            .as_ref()
                            .map(|p| snapshot.prefix.eq_ignore_ascii_case(p))
                            .unwrap_or(true)
                            && code
                                .as_ref()
                                .map(|c| snapshot.code.eq_ignore_ascii_case(c))
                                .unwrap_or(true)
                    })
                    .collect();
                snapshots.sort_by(|a, b| a.prefix.cmp(&b.prefix).then(a.code.cmp(&b.code)));

                if json {
                    let payload = json!(snapshots);
                    println!("{}", payload);
                } else if snapshots.is_empty() {
                    println!("no TLS environment warnings recorded");
                } else {
                    println!(
                        "{:<32} {:<32} {:<12} {:>8} {:>8} {:<}",
                        "PREFIX", "CODE", "ORIGIN", "TOTAL", "AGE_S", "DETAIL"
                    );
                    for snapshot in snapshots {
                        let age = now.saturating_sub(snapshot.last_seen);
                        let detail = snapshot
                            .detail
                            .as_deref()
                            .map(|detail| detail.to_string())
                            .unwrap_or_else(|| "-".to_string());
                        println!(
                            "{:<32} {:<32} {:<12} {:>8} {:>8} {}",
                            snapshot.prefix,
                            snapshot.code,
                            snapshot.origin.as_str(),
                            snapshot.total,
                            age,
                            detail
                        );
                        if !snapshot.variables.is_empty() {
                            println!("{:>77} vars: {}", "", snapshot.variables.join(","));
                        }
                        if snapshot.detail_fingerprint.is_some()
                            || snapshot.variables_fingerprint.is_some()
                        {
                            let detail_hex = snapshot
                                .detail_fingerprint
                                .map(format_tls_warning_fingerprint)
                                .unwrap_or_else(|| "-".to_string());
                            let vars_hex = snapshot
                                .variables_fingerprint
                                .map(format_tls_warning_fingerprint)
                                .unwrap_or_else(|| "-".to_string());
                            println!(
                                "{:>77} fingerprints: detail={} vars={}",
                                "", detail_hex, vars_hex
                            );
                        }
                        let detail_counts = format_tls_warning_fingerprint_counts(
                            &snapshot.detail_fingerprint_counts,
                        );
                        let vars_counts = format_tls_warning_fingerprint_counts(
                            &snapshot.variables_fingerprint_counts,
                        );
                        if detail_counts != "-" || vars_counts != "-" {
                            println!(
                                "{:>77} fingerprint_counts: detail={} vars={}",
                                "", detail_counts, vars_counts
                            );
                        }
                    }
                }
            }
            #[cfg(not(feature = "telemetry"))]
            {
                let _ = (prefix, code, json, probe_detail, probe_variables);
                println!("telemetry disabled");
            }
        }
    }
}
