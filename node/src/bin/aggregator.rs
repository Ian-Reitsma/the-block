use std::{error::Error, process};

use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use httpd::{BlockingClient, Method};
use serde_json::Value;
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
                let mut deque: VecDeque<(u64, Value)> =
                    serde_json::from_slice(&v).unwrap_or_default();
                let before_len = deque.len();
                deque.retain(|(ts, _)| *ts >= before);
                let after_len = deque.len();
                total += (before_len - after_len) as u64;
                if after_len == 0 {
                    db.remove(&k)?;
                } else {
                    db.insert(&k, serde_json::to_vec(&deque)?)?;
                }
            }
            db.flush()?;
            println!("pruned {total}");
        }
        Command::Telemetry { url } => {
            let client = BlockingClient::default();
            let resp = client
                .request(Method::Get, &format!("{}/telemetry", url))?
                .send()?;
            if !resp.status().is_success() {
                eprintln!("telemetry request failed: {}", resp.status().as_u16());
            }
            let body = resp.text()?;
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
