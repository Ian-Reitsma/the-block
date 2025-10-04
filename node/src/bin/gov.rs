#![allow(clippy::expect_used)]

use std::{
    fs,
    path::Path,
    process,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use cli_core::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use the_block::{governance::House, Governance};

mod cli_support;
use cli_support::{collect_args, parse_matches};

#[derive(Debug)]
struct Cli {
    cmd: Command,
}

#[derive(Debug)]
enum Command {
    Submit { file: String },
    Vote { id: u64, house: HouseArg },
    Exec { id: u64 },
    Status { id: u64 },
    List,
}

#[derive(Clone, Debug)]
enum HouseArg {
    Ops,
    Builders,
}

impl From<HouseArg> for House {
    fn from(h: HouseArg) -> Self {
        match h {
            HouseArg::Ops => House::Operators,
            HouseArg::Builders => House::Builders,
        }
    }
}

impl FromStr for HouseArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "ops" => Ok(HouseArg::Ops),
            "builders" => Ok(HouseArg::Builders),
            other => Err(format!("invalid house '{other}'")),
        }
    }
}

fn main() {
    let command = build_command();
    let (bin, args) = collect_args("gov");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return,
    };

    let cli = match build_cli(matches) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };

    let db_path = "examples/governance/proposals.db";
    if let Some(parent) = Path::new(db_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut gov = Governance::load(db_path, 1, 1, 0);
    match cli.cmd {
        Command::Submit { file } => {
            let text = fs::read_to_string(file).expect("read");
            let v: serde_json::Value = serde_json::from_str(&text).expect("json");
            let start = v["start"].as_u64().unwrap_or(0);
            let end = v["end"].as_u64().unwrap_or(0);
            let id = gov.submit(start, end);
            println!("submitted {id}");
            gov.persist(db_path).expect("persist");
        }
        Command::Vote { id, house } => {
            gov.vote(id, house.into(), true).expect("vote");
            println!("voted {id}");
            gov.persist(db_path).expect("persist");
        }
        Command::Exec { id } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            gov.execute(id, now).expect("exec");
            println!("executed {id}");
            gov.persist(db_path).expect("persist");
        }
        Command::Status { id } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let (p, remaining) = gov.status(id, now).expect("status");
            println!(
                "id={} ops_for={} builders_for={} executed={} timelock_remaining={}s",
                p.id, p.ops_for, p.builders_for, p.executed, remaining
            );
        }
        Command::List => {
            for p in gov.list() {
                println!(
                    "id={} start={} end={} ops_for={} builders_for={} executed={}",
                    p.id, p.start, p.end, p.ops_for, p.builders_for, p.executed
                );
            }
        }
    }
}

fn build_command() -> CliCommand {
    CommandBuilder::new(CommandId("gov"), "gov", "Governance helpers")
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.submit"),
                "submit",
                "Submit a proposal JSON file",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "file",
                "Proposal JSON file",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("gov.vote"), "vote", "Vote for a proposal")
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "id",
                    "Proposal id",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "house",
                    "House (ops|builders)",
                )))
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.exec"),
                "exec",
                "Execute a proposal after quorum and timelock",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "id",
                "Proposal id",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("gov.status"), "status", "Show proposal status")
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "id",
                    "Proposal id",
                )))
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("gov.list"),
                "list",
                "List all proposals and their current status",
            )
            .build(),
        )
        .build()
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let cmd = match sub {
        "submit" => {
            let file = require_positional(sub_matches, "file")?;
            Command::Submit { file }
        }
        "vote" => {
            let id_str = require_positional(sub_matches, "id")?;
            let id = id_str
                .parse::<u64>()
                .map_err(|err| format!("invalid id: {err}"))?;
            let house_str = require_positional(sub_matches, "house")?;
            let house = HouseArg::from_str(&house_str)?;
            Command::Vote { id, house }
        }
        "exec" => {
            let id = parse_u64(sub_matches, "id")?;
            Command::Exec { id }
        }
        "status" => {
            let id = parse_u64(sub_matches, "id")?;
            Command::Status { id }
        }
        "list" => Command::List,
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(Cli { cmd })
}

fn require_positional(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| format!("missing argument '{name}'"))
}

fn parse_u64(matches: &Matches, name: &str) -> Result<u64, String> {
    let raw = require_positional(matches, name)?;
    raw.parse::<u64>()
        .map_err(|err| format!("invalid {name}: {err}"))
}
