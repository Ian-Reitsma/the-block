use std::{error::Error, io, process};

use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::json::{self, Value};
use foundation_telemetry::TelemetrySummary;
use httpd::Method;
use the_block::http_client;
use std::collections::VecDeque;

mod cli_support;
use cli_support::{collect_args, parse_matches};

#[derive(Debug)]
struct Cli {
    db: String,
    cmd: Command,
}

#[derive(Debug)]
enum Command {
    Prune { before: u64 },
    Telemetry { url: String },
}

fn main() -> Result<(), Box<dyn Error>> {
    let command = build_command();
    let (bin, args) = collect_args("aggregator");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return Ok(()),
    };

    let cli = match build_cli(matches) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };

    let db = sled::open(&cli.db)?;
    match cli.cmd {
        Command::Prune { before } => {
            let mut total = 0u64;
            for item in db.iter() {
                let (k, v) = item?;
                let mut deque: VecDeque<(u64, Value)> = json::from_slice(&v).unwrap_or_default();
                let before_len = deque.len();
                deque.retain(|(ts, _)| *ts >= before);
                let after_len = deque.len();
                total += (before_len - after_len) as u64;
                if after_len == 0 {
                    db.remove(&k)?;
                } else {
                    db.insert(&k, json::to_vec(&deque)?)?;
                }
            }
            db.flush()?;
            println!("pruned {total}");
        }
        Command::Telemetry { url } => {
            let client = http_client::blocking_client();
            let resp = client
                .request(Method::Get, &format!("{}/telemetry", url))?
                .send()?;
            if !resp.status().is_success() {
                eprintln!("telemetry request failed: {}", resp.status().as_u16());
            }
            let body = resp.text()?;
            let payload = json::value_from_str(&body)?;
            if let Err(err) = validate_telemetry_payload(&payload) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("telemetry payload failed schema validation: {err}"),
                )
                .into());
            }
            println!("{}", body);
        }
    }
    Ok(())
}

fn build_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("aggregator"),
        "aggregator",
        "Metrics aggregator administration",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("db", "db", "Path to aggregator database").default("peer_metrics.db"),
    ))
    .subcommand(
        CommandBuilder::new(
            CommandId("aggregator.prune"),
            "prune",
            "Remove samples before the given UNIX timestamp",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("before", "before", "UNIX timestamp cutoff").required(true),
        ))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("aggregator.telemetry"),
            "telemetry",
            "Fetch latest telemetry summaries from the aggregator HTTP API",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("url", "url", "Aggregator base URL").default("http://localhost:8080"),
        ))
        .build(),
    )
    .build()
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let db = matches
        .get_string("db")
        .unwrap_or_else(|| "peer_metrics.db".to_string());
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let cmd = match sub {
        "prune" => {
            let before_str = sub_matches
                .get_string("before")
                .ok_or_else(|| "missing --before".to_string())?;
            let before = before_str
                .parse::<u64>()
                .map_err(|err| format!("invalid before value: {err}"))?;
            Command::Prune { before }
        }
        "telemetry" => {
            let url = sub_matches
                .get_string("url")
                .unwrap_or_else(|| "http://localhost:8080".to_string());
            Command::Telemetry { url }
        }
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(Cli { db, cmd })
}

fn validate_telemetry_payload(payload: &Value) -> Result<(), String> {
    let map = payload
        .as_object()
        .ok_or_else(|| "telemetry payload must be a JSON object".to_string())?;
    let mut errors = Vec::new();
    for (node, summary_value) in map {
        if let Err(err) = TelemetrySummary::validate_value(summary_value) {
            errors.push(format!("{node}: {} ({})", err.message(), err.path()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_payload_accepts_well_formed_summary() {
        let value = json::value_from_str(
            r#"{
                "node-a": {
                    "node_id": "node-a",
                    "seq": 1,
                    "timestamp": 1700000000,
                    "sample_rate_ppm": 500000,
                    "compaction_secs": 30,
                    "memory": {
                        "mempool": {"latest": 1, "p50": 1, "p90": 1, "p99": 1}
                    }
                }
            }"#,
        )
        .unwrap();
        assert!(validate_telemetry_payload(&value).is_ok());
    }

    #[test]
    fn validate_payload_reports_schema_errors() {
        let value = json::value_from_str(
            r#"{
                "node-a": {
                    "node_id": "node-a",
                    "seq": 1,
                    "timestamp": 1700000000,
                    "sample_rate_ppm": 500000,
                    "compaction_secs": 30
                }
            }"#,
        )
        .unwrap();
        let err = validate_telemetry_payload(&value).expect_err("schema drift should be reported");
        assert!(err.contains("/memory"));
    }
}
