use std::path::Path;

use anyhow::Result;
use cli_core::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
/// Create a hybrid-compressed snapshot of the legacy RocksDB database.
///
/// The RocksDB backend has been fully retired; call sites should migrate to the
/// first-party in-house storage exporter.  This helper now surfaces a clear
/// error so automation fails fast instead of silently linking the native
/// dependency again.
pub fn create_snapshot(_db_path: &Path, _out_path: &Path) -> Result<()> {
    anyhow::bail!(
        "legacy RocksDB snapshots are no longer supported; use the in-house \
         storage exporter instead"
    )
}

/// Restore a snapshot into a RocksDB directory.
pub fn restore_snapshot(_archive_path: &Path, _db_path: &Path) -> Result<()> {
    anyhow::bail!(
        "legacy RocksDB snapshots are no longer supported; use the in-house \
         storage importer instead"
    )
}

enum CliError {
    Usage(String),
    Failure(anyhow::Error),
}

#[cfg(not(test))]
fn main() {
    if let Err(err) = run_cli() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            CliError::Failure(err) => {
                #[cfg(feature = "telemetry")]
                foundation_metrics::increment_counter!("snapshot_restore_fail_total");
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
}

fn run_cli() -> Result<(), CliError> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "snapshot".to_string());
    let args: Vec<String> = argv.collect();

    let command = build_command();
    if args.is_empty() {
        print_root_help(&command, &bin);
        return Ok(());
    }

    let parser = Parser::new(&command);
    let matches = match parser.parse(&args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(&command, &path);
            return Ok(());
        }
        Err(err) => return Err(CliError::Usage(err.to_string())),
    };

    match matches
        .subcommand()
        .ok_or_else(|| CliError::Usage("missing subcommand".into()))? {
        ("create", sub_matches) => {
            let db = positional(sub_matches, "db")?;
            let out = positional(sub_matches, "out")?;
            create_snapshot(Path::new(&db), Path::new(&out)).map_err(CliError::Failure)
        }
        ("restore", sub_matches) => {
            let archive = positional(sub_matches, "archive")?;
            let dst = positional(sub_matches, "dst")?;
            restore_snapshot(Path::new(&archive), Path::new(&dst)).map_err(CliError::Failure)
        }
        (other, _) => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
    }
}

fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("snapshot"),
        "snapshot",
        "Snapshot utilities",
    )
    .subcommand(
        CommandBuilder::new(CommandId("snapshot.create"), "create", "Create a snapshot")
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "Path to the RocksDB directory",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "out",
                "Destination snapshot path",
            )))
            .build(),
    )
    .subcommand(
        CommandBuilder::new(CommandId("snapshot.restore"), "restore", "Restore a snapshot")
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "archive",
                "Snapshot archive path",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "dst",
                "Destination RocksDB directory",
            )))
            .build(),
    )
    .build()
}

fn positional(matches: &Matches, name: &str) -> Result<String, CliError> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| CliError::Usage(format!("missing '{name}' argument")))
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for more details.");
}

fn print_help_for_path(root: &Command, path: &str) {
    let segments: Vec<&str> = path.split_whitespace().collect();
    if let Some(cmd) = find_command(root, &segments) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(root: &'a Command, path: &[&str]) -> Option<&'a Command> {
    if path.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in path.iter().skip(1) {
        if let Some(next) = current
            .subcommands
            .iter()
            .find(|command| command.name == *segment)
        {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
}
