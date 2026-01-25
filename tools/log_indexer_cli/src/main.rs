#![allow(clippy::all)]
#![allow(clippy::pedantic)]

mod lib {
    include!("../../log_indexer.rs");
}

use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use lib::{index_logs_with_options, search_logs, LogIndexerError};
use log_index::{IndexOptions, LogFilter};
use std::path::Path;

#[derive(Debug)]
enum CliError {
    Usage(String),
    Failure(LogIndexerError),
}

fn main() {
    if let Err(err) = run_cli() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            CliError::Failure(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
}

fn run_cli() -> Result<(), CliError> {
    let mut argv = std::env::args();
    let _bin = argv.next().unwrap_or_else(|| "log-indexer".into());
    let args = argv.collect::<Vec<_>>();
    run_cli_with_args(&args)
}

fn run_cli_with_args(args: &[String]) -> Result<(), CliError> {
    let command = build_command();
    let parser = Parser::new(&command);
    let matches = match parser.parse(args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(&command, &path);
            return Ok(());
        }
        Err(err) => return Err(CliError::Usage(err.to_string())),
    };

    match matches
        .subcommand()
        .ok_or_else(|| CliError::Usage("missing subcommand".into()))?
    {
        ("index", sub_matches) => handle_index(sub_matches),
        ("search", sub_matches) => handle_search(sub_matches),
        (other, _) => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
    }
}

fn build_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("log-indexer"),
        "log-indexer",
        "Index and query structured logs",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("log-indexer.index"),
            "index",
            "Index a JSON log file into the in-house log store",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "log",
            "Path to the JSON log file",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "db",
            "Destination directory for the log store",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "passphrase",
            "passphrase",
            "Optional passphrase for encrypting log messages at rest",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("log-indexer.search"),
            "search",
            "Query previously indexed logs",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "db",
            "Log store directory produced by 'index'",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "peer",
            "peer",
            "Filter by peer identifier",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "tx",
            "tx",
            "Filter by transaction identifier",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "block",
            "block",
            "Filter by block height",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "correlation",
            "correlation",
            "Filter by correlation identifier",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "level",
            "level",
            "Filter by log level",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "since",
            "since",
            "Only include entries after this timestamp",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "until",
            "until",
            "Only include entries before this timestamp",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "after-id",
            "after-id",
            "Only include entries after this database id",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "passphrase",
            "passphrase",
            "Passphrase required to decrypt encrypted log messages",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "limit",
            "limit",
            "Maximum number of rows to return",
        )))
        .build(),
    )
    .build()
}

fn handle_index(matches: &Matches) -> Result<(), CliError> {
    let log = positional(matches, "log")?;
    let db = positional(matches, "db")?;
    let passphrase = matches.get_string("passphrase");
    let opts = IndexOptions { passphrase };
    index_logs_with_options(Path::new(&log), Path::new(&db), opts).map_err(CliError::Failure)
}

fn handle_search(matches: &Matches) -> Result<(), CliError> {
    let db = positional(matches, "db")?;
    let filter = LogFilter {
        peer: matches.get_string("peer"),
        tx: matches.get_string("tx"),
        block: parse_option_u64(matches, "block")?,
        correlation: matches.get_string("correlation"),
        level: matches.get_string("level"),
        since: parse_option_u64(matches, "since")?,
        until: parse_option_u64(matches, "until")?,
        after_id: parse_option_u64(matches, "after-id")?,
        limit: parse_option_usize(matches, "limit")?,
        passphrase: matches.get_string("passphrase"),
    };

    match search_logs(Path::new(&db), &filter) {
        Ok(results) => {
            for entry in results {
                println!(
                    "{} [{}] {} :: {}",
                    entry.timestamp, entry.level, entry.correlation_id, entry.message
                );
            }
            Ok(())
        }
        Err(err) => Err(CliError::Failure(err)),
    }
}

fn positional(matches: &Matches, name: &str) -> Result<String, CliError> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| CliError::Usage(format!("missing '{name}' argument")))
}

fn parse_option_u64(matches: &Matches, name: &str) -> Result<Option<u64>, CliError> {
    matches
        .get_string(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| CliError::Usage(format!("invalid value '{value}' for {name}")))
        })
        .transpose()
}

fn parse_option_usize(matches: &Matches, name: &str) -> Result<Option<usize>, CliError> {
    matches
        .get_string(name)
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| CliError::Usage(format!("invalid value '{value}' for {name}")))
        })
        .transpose()
}

fn print_help_for_path(root: &CliCommand, path: &str) {
    let help_gen = HelpGenerator::new(root);
    let rendered = if path.is_empty() {
        help_gen.render()
    } else {
        help_gen.render()
    };
    println!("{rendered}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn run_args(args: &[&str]) -> Result<(), CliError> {
        let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        run_cli_with_args(&owned)
    }

    fn unique_paths() -> (PathBuf, PathBuf) {
        let mut base = std::env::temp_dir();
        base.push(format!(
            "log_indexer_cli_test_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::create_dir_all(&base);
        let log = base.join("input.log");
        let db = base.join("store");
        (log, db)
    }

    #[test]
    fn run_cli_requires_subcommand() {
        let args: Vec<String> = Vec::new();
        match run_cli_with_args(&args) {
            Err(CliError::Usage(msg)) => assert!(msg.contains("missing subcommand")),
            other => panic!("expected usage error, got {other:?}"),
        }
    }

    #[test]
    fn index_command_propagates_errors() {
        let (log, db) = unique_paths();
        match run_args(&[
            "index",
            log.to_string_lossy().as_ref(),
            db.to_string_lossy().as_ref(),
        ]) {
            Err(CliError::Failure(err)) => {
                let _ = err.to_string();
            }
            other => panic!("expected indexing failure, got {other:?}"),
        }
    }

    #[test]
    fn search_command_propagates_errors() {
        let (_, db) = unique_paths();
        let db_str = db.to_string_lossy().to_string();
        match run_args(&["search", db_str.as_str(), "--limit", "not-a-number"]) {
            Err(CliError::Usage(msg)) => assert!(!msg.is_empty()),
            other => panic!("expected usage error for invalid limit, got {other:?}"),
        }
    }
}
