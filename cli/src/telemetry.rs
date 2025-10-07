use crate::parse_utils::{parse_optional, parse_u64, parse_u64_required, take_string};
use crate::rpc::RpcClient;
use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::json;

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
            let payload = json!({
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
    }
}
