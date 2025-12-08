use std::fs::File;
use std::path::Path;

use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use log_index::{
    ingest_with_seek_and_observer, search_logs_in_store, IndexOptions, LogEntry, LogFilter,
    LogIndexError, LogStore, StoredEntry,
};

pub type LogIndexerError = LogIndexError;
pub type Result<T, E = LogIndexerError> = std::result::Result<T, E>;

/// Index JSON log lines into the in-house log store using default options.
pub fn index_logs(log_path: &Path, db_path: &Path) -> Result<()> {
    index_logs_with_options(log_path, db_path, IndexOptions::default())
}

/// Index JSON log lines with explicit options such as encryption.
pub fn index_logs_with_options(log_path: &Path, db_path: &Path, opts: IndexOptions) -> Result<()> {
    let store = LogStore::open(db_path)?;
    let mut file = File::open(log_path)?;
    let source = canonical_source_key(log_path);
    ingest_with_seek_and_observer(&mut file, &source, &opts, &store, |entry: &StoredEntry| {
        increment_indexed_metric(&entry.correlation_id)
    })
}

fn canonical_source_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

// Telemetry removed - module doesn't exist in log-indexer-cli
#[cfg(feature = "telemetry")]
fn increment_indexed_metric(_correlation_id: &str) {
    // TODO: wire telemetry when module exists
}

#[cfg(not(feature = "telemetry"))]
fn increment_indexed_metric(_correlation_id: &str) {}

/// Search indexed logs with optional filters.
pub fn search_logs(db_path: &Path, filter: &LogFilter) -> Result<Vec<LogEntry>> {
    let store = LogStore::open(db_path)?;
    let results = search_logs_in_store(&store, filter)?;

    // Telemetry removed - module doesn't exist
    #[cfg(feature = "telemetry")]
    {
        if filter
            .correlation
            .as_ref()
            .map(|c| !c.is_empty())
            .unwrap_or(false)
            && results.is_empty()
        {
            // TODO: wire telemetry when module exists
            // crate::telemetry::record_log_correlation_fail();
        }
    }

    Ok(results)
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Failure(LogIndexerError),
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
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
}

fn run_cli() -> Result<(), CliError> {
    let mut argv = std::env::args();
    let _bin = argv.next().unwrap_or_else(|| "log-indexer".into());
    let command = build_command();
    let parser = Parser::new(&command);
    let matches = match parser.parse(&argv.collect::<Vec<_>>()) {
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
        .get(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| CliError::Usage(err.to_string()))
        })
        .transpose()
}

fn parse_option_usize(matches: &Matches, name: &str) -> Result<Option<usize>, CliError> {
    matches
        .get(name)
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|err| CliError::Usage(err.to_string()))
        })
        .transpose()
}

#[allow(dead_code)]
fn print_root_help(command: &CliCommand, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details on a command.");
}

fn print_help_for_path(root: &CliCommand, path: &str) {
    let segments: Vec<&str> = path.split_whitespace().collect();
    if let Some(cmd) = find_command(root, &segments) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(root: &'a CliCommand, path: &[&str]) -> Option<&'a CliCommand> {
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
