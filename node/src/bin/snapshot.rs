#![allow(clippy::expect_used)]

use std::process;

use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use the_block::{Blockchain, SnapshotManager};

mod cli_support;
use cli_support::{collect_args, parse_matches};

#[derive(Debug)]
struct Cli {
    cmd: Command,
}

#[derive(Debug)]
enum Command {
    Create {
        data_dir: String,
        db_path: Option<String>,
    },
    Apply {
        data_dir: String,
        db_path: Option<String>,
    },
    List {
        data_dir: String,
    },
}

fn main() {
    let command = build_command();
    let (bin, args) = collect_args("snapshot");
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

    match cli.cmd {
        Command::Create { data_dir, db_path } => {
            let dbp = db_path.unwrap_or_else(|| data_dir.clone());
            let bc = Blockchain::open_with_db(&data_dir, &dbp).expect("open chain");
            let mgr = SnapshotManager::new(data_dir, bc.snapshot.interval);
            let _ = mgr
                .write_snapshot(bc.block_height, bc.accounts())
                .expect("snapshot");
        }
        Command::Apply {
            data_dir,
            db_path: _,
        } => {
            let mgr = SnapshotManager::new(data_dir, 0);
            if let Ok(Some((h, _, root))) = mgr.load_latest() {
                println!("{h}:{root}");
            }
        }
        Command::List { data_dir } => {
            let mgr = SnapshotManager::new(data_dir, 0);
            if let Ok(list) = mgr.list() {
                for h in list {
                    println!("{h}");
                }
            }
        }
    }
}

fn build_command() -> CliCommand {
    CommandBuilder::new(CommandId("snapshot"), "snapshot", "Manage chain snapshots")
        .subcommand(
            CommandBuilder::new(
                CommandId("snapshot.create"),
                "create",
                "Create a snapshot from the given data directory",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "data_dir",
                "Data directory",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "db_path",
                "db-path",
                "Custom database path",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("snapshot.apply"),
                "apply",
                "Apply the latest snapshot and print its root",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "data_dir",
                "Data directory",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "db_path",
                "db-path",
                "Custom database path",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("snapshot.list"),
                "list",
                "List available snapshot heights",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "data_dir",
                "Data directory",
            )))
            .build(),
        )
        .build()
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let cmd = match sub {
        "create" => Command::Create {
            data_dir: require_positional(sub_matches, "data_dir")?,
            db_path: sub_matches.get_string("db_path"),
        },
        "apply" => Command::Apply {
            data_dir: require_positional(sub_matches, "data_dir")?,
            db_path: sub_matches.get_string("db_path"),
        },
        "list" => Command::List {
            data_dir: require_positional(sub_matches, "data_dir")?,
        },
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
