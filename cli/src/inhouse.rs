use std::{fmt, path::PathBuf};

use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};

use crate::{
    config,
    logs::{
        run_correlate_metric, run_rotate_key, run_search, CorrelateMetricOptions, RotateKeyOptions,
        SearchOptions,
    },
    version,
};

#[derive(Debug)]
pub enum Dispatch {
    Handled,
    HelpDisplayed,
    Unhandled,
    Error(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    ConfigReload {
        url: String,
    },
    ConfigShow {
        file: Option<String>,
        key: Option<String>,
    },
    VersionProvenance,
    LogsSearch(SearchOptions),
    LogsRotateKey(RotateKeyOptions),
    LogsCorrelateMetric(CorrelateMetricOptions),
}

#[derive(Debug)]
pub enum ActionError {
    Parse(ParseError),
    MissingSubcommand(&'static str),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::Parse(err) => write!(f, "{}", err),
            ActionError::MissingSubcommand(cmd) => {
                write!(f, "missing subcommand for '{cmd}'")
            }
        }
    }
}

impl std::error::Error for ActionError {}

pub fn dispatch(args: &[String]) -> Dispatch {
    let root = build_root_command();

    if maybe_print_help(&root, args) {
        return Dispatch::HelpDisplayed;
    }

    match parse_action(&root, args) {
        Ok(Some(action)) => match execute(action) {
            Ok(()) => Dispatch::Handled,
            Err(err) => Dispatch::Error(err),
        },
        Ok(None) => Dispatch::Unhandled,
        Err(ActionError::Parse(ParseError::UnknownSubcommand(_))) => Dispatch::Unhandled,
        Err(ActionError::Parse(ParseError::UnknownOption(option))) => {
            if args.first().map(|s| s.as_str()) == Some("config")
                || args.first().map(|s| s.as_str()) == Some("version")
            {
                Dispatch::Error(format!("unknown option '--{option}'"))
            } else {
                Dispatch::Unhandled
            }
        }
        Err(ActionError::Parse(ParseError::MissingValue(option))) => {
            Dispatch::Error(format!("missing value for '--{option}'"))
        }
        Err(ActionError::Parse(ParseError::MissingOption(name))) => {
            Dispatch::Error(format!("missing required option '--{name}'"))
        }
        Err(ActionError::Parse(ParseError::MissingPositional(name))) => {
            Dispatch::Error(format!("missing positional argument '{name}'"))
        }
        Err(ActionError::MissingSubcommand(cmd)) => {
            Dispatch::Error(format!("missing subcommand for '{cmd}'"))
        }
        Err(ActionError::Parse(err)) => Dispatch::Error(err.to_string()),
    }
}

pub fn parse_action(root: &Command, args: &[String]) -> Result<Option<Action>, ActionError> {
    if args.is_empty() {
        return Ok(None);
    }

    let parser = Parser::new(root);
    let matches = parser.parse(args).map_err(ActionError::Parse)?;

    let (sub_name, sub_matches) = match matches.subcommand() {
        Some(value) => value,
        None => return Ok(None),
    };

    match sub_name {
        "config" => {
            let (action_name, action_matches) = sub_matches
                .subcommand()
                .ok_or(ActionError::MissingSubcommand("config"))?;
            match action_name {
                "reload" => {
                    let url = action_matches
                        .get_string("url")
                        .unwrap_or_else(|| "http://localhost:26658".to_string());
                    Ok(Some(Action::ConfigReload { url }))
                }
                "show" => Ok(Some(Action::ConfigShow {
                    file: action_matches.get_string("file"),
                    key: action_matches.get_string("key"),
                })),
                _ => Err(ActionError::Parse(ParseError::UnknownSubcommand(
                    action_name.to_string(),
                ))),
            }
        }
        "version" => {
            let (action_name, _) = sub_matches
                .subcommand()
                .ok_or(ActionError::MissingSubcommand("version"))?;
            match action_name {
                "provenance" => Ok(Some(Action::VersionProvenance)),
                _ => Err(ActionError::Parse(ParseError::UnknownSubcommand(
                    action_name.to_string(),
                ))),
            }
        }
        "logs" => {
            let (action_name, action_matches) = sub_matches
                .subcommand()
                .ok_or(ActionError::MissingSubcommand("logs"))?;
            match action_name {
                "search" => {
                    let db = action_matches
                        .get_positional("db")
                        .and_then(|values| values.first().cloned())
                        .ok_or_else(|| {
                            ActionError::Parse(ParseError::MissingPositional("db".into()))
                        })?;
                    let block = parse_optional_u64(action_matches.get_string("block"), "block")?;
                    let since = parse_optional_u64(action_matches.get_string("since"), "since")?;
                    let until = parse_optional_u64(action_matches.get_string("until"), "until")?;
                    let after_id =
                        parse_optional_u64(action_matches.get_string("after_id"), "after-id")?;
                    let limit = parse_optional_usize(action_matches.get_string("limit"), "limit")?;
                    Ok(Some(Action::LogsSearch(SearchOptions {
                        db,
                        peer: action_matches.get_string("peer"),
                        tx: action_matches.get_string("tx"),
                        block,
                        correlation: action_matches.get_string("correlation"),
                        level: action_matches.get_string("level"),
                        since,
                        until,
                        after_id,
                        passphrase: action_matches.get_string("passphrase"),
                        limit,
                    })))
                }
                "rotate-key" => Ok(Some(Action::LogsRotateKey(RotateKeyOptions {
                    db: action_matches
                        .get_positional("db")
                        .and_then(|values| values.first().cloned())
                        .ok_or_else(|| {
                            ActionError::Parse(ParseError::MissingPositional("db".into()))
                        })?,
                    old_passphrase: action_matches.get_string("old_passphrase"),
                    new_passphrase: action_matches.get_string("new_passphrase"),
                }))),
                "correlate-metric" => {
                    let aggregator = action_matches
                        .get_string("aggregator")
                        .unwrap_or_else(|| "http://localhost:9000".to_string());
                    let metric = action_matches.get_string("metric").ok_or_else(|| {
                        ActionError::Parse(ParseError::MissingOption("metric".into()))
                    })?;
                    let max_correlations = parse_optional_usize(
                        action_matches.get_string("max_correlations"),
                        "max-correlations",
                    )?
                    .unwrap_or(1);
                    let rows = parse_optional_usize(action_matches.get_string("rows"), "rows")?
                        .unwrap_or(20);
                    Ok(Some(Action::LogsCorrelateMetric(CorrelateMetricOptions {
                        aggregator,
                        metric,
                        db: action_matches.get_string("db"),
                        max_correlations,
                        rows,
                        passphrase: action_matches.get_string("passphrase"),
                    })))
                }
                _ => Err(ActionError::Parse(ParseError::UnknownSubcommand(
                    action_name.to_string(),
                ))),
            }
        }
        _ => Ok(None),
    }
}

fn execute(action: Action) -> Result<(), String> {
    match action {
        Action::ConfigReload { url } => {
            config::reload(url);
            Ok(())
        }
        Action::ConfigShow { file, key } => {
            config::show(file.map(PathBuf::from), key).map_err(|err| err.to_string())
        }
        Action::VersionProvenance => {
            version::provenance();
            Ok(())
        }
        Action::LogsSearch(options) => {
            run_search(options);
            Ok(())
        }
        Action::LogsRotateKey(options) => {
            run_rotate_key(options);
            Ok(())
        }
        Action::LogsCorrelateMetric(options) => {
            run_correlate_metric(options);
            Ok(())
        }
    }
}

fn maybe_print_help(root: &Command, args: &[String]) -> bool {
    if !args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return false;
    }

    let path = command_path_from_args(args);
    if let Some(command) = find_command(root, &path) {
        let generator = HelpGenerator::new(command);
        println!("{}", generator.render());
        return true;
    }

    false
}

fn command_path_from_args<'a>(args: &'a [String]) -> Vec<&'a str> {
    let mut path = Vec::new();
    for token in args {
        if token.starts_with('-') {
            break;
        }
        path.push(token.as_str());
    }
    path
}

fn find_command<'a>(command: &'a Command, path: &[&str]) -> Option<&'a Command> {
    if path.is_empty() {
        return Some(command);
    }

    let mut current = command;
    for segment in path {
        if let Some(next) = current.subcommands.iter().find(|cmd| cmd.name == *segment) {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
}

fn parse_optional_u64(
    value: Option<String>,
    option: &'static str,
) -> Result<Option<u64>, ActionError> {
    match value {
        Some(raw) => raw.parse::<u64>().map(Some).map_err(|_| {
            ActionError::Parse(ParseError::InvalidValue {
                option: option.to_string(),
                value: raw,
                expected: "an unsigned integer",
            })
        }),
        None => Ok(None),
    }
}

fn parse_optional_usize(
    value: Option<String>,
    option: &'static str,
) -> Result<Option<usize>, ActionError> {
    match value {
        Some(raw) => raw.parse::<usize>().map(Some).map_err(|_| {
            ActionError::Parse(ParseError::InvalidValue {
                option: option.to_string(),
                value: raw,
                expected: "a non-negative integer",
            })
        }),
        None => Ok(None),
    }
}

fn build_root_command() -> Command {
    CommandBuilder::new(CommandId("contract"), "contract", "Contract management CLI")
        .allow_external_subcommands(true)
        .subcommand(
            CommandBuilder::new(CommandId("contract.config"), "config", "Config utilities")
                .subcommand(
                    CommandBuilder::new(
                        CommandId("contract.config.reload"),
                        "reload",
                        "Trigger config reload",
                    )
                    .arg(ArgSpec::Option(
                        OptionSpec::new("url", "url", "Config service URL")
                            .default("http://localhost:26658"),
                    ))
                    .build(),
                )
                .subcommand(
                    CommandBuilder::new(
                        CommandId("contract.config.show"),
                        "show",
                        "Display parsed configuration values",
                    )
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "file",
                        "file",
                        "Configuration file path",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "key",
                        "key",
                        "Restrict output to a single key",
                    )))
                    .build(),
                )
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("contract.version"),
                "version",
                "Version and build info",
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("contract.version.provenance"),
                    "provenance",
                    "Display build provenance info",
                )
                .build(),
            )
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("contract.logs"), "logs", "Log utilities")
                .subcommand(
                    CommandBuilder::new(
                        CommandId("contract.logs.search"),
                        "search",
                        "Search indexed logs stored in SQLite",
                    )
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "db",
                        "SQLite database produced by the log indexer",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "peer",
                        "peer",
                        "Filter by peer identifier",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "tx",
                        "tx",
                        "Filter by transaction hash",
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
                        "Filter by severity level",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "since",
                        "since",
                        "Minimum timestamp (inclusive)",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "until",
                        "until",
                        "Maximum timestamp (inclusive)",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "after_id",
                        "after-id",
                        "Filter by row id greater than the provided value",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "passphrase",
                        "passphrase",
                        "Passphrase to decrypt log messages",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "limit",
                        "limit",
                        "Maximum rows to return",
                    )))
                    .build(),
                )
                .subcommand(
                    CommandBuilder::new(
                        CommandId("contract.logs.rotate_key"),
                        "rotate-key",
                        "Re-encrypt stored messages with a new passphrase",
                    )
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "db",
                        "SQLite database produced by the log indexer",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "old_passphrase",
                        "old-passphrase",
                        "Existing passphrase protecting log messages",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "new_passphrase",
                        "new-passphrase",
                        "New passphrase to apply",
                    )))
                    .build(),
                )
                .subcommand(
                    CommandBuilder::new(
                        CommandId("contract.logs.correlate_metric"),
                        "correlate-metric",
                        "Fetch correlations and stream matching logs",
                    )
                    .arg(ArgSpec::Option(
                        OptionSpec::new("aggregator", "aggregator", "Metrics aggregator base URL")
                            .default("http://localhost:9000"),
                    ))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("metric", "metric", "Metric name to correlate")
                            .required(true),
                    ))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "db",
                        "db",
                        "Override for the log database path",
                    )))
                    .arg(ArgSpec::Option(
                        OptionSpec::new(
                            "max_correlations",
                            "max-correlations",
                            "Maximum correlated entries to inspect",
                        )
                        .default("1"),
                    ))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("rows", "rows", "Limit log rows returned per correlation")
                            .default("20"),
                    ))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "passphrase",
                        "passphrase",
                        "Passphrase to decrypt log messages",
                    )))
                    .build(),
                )
                .build(),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Action>, ActionError> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let root = build_root_command();
        parse_action(&root, &owned)
    }

    #[test]
    fn parses_config_reload_default() {
        let action = parse(&["config", "reload"]).unwrap();
        assert_eq!(
            action,
            Some(Action::ConfigReload {
                url: "http://localhost:26658".to_string()
            })
        );
    }

    #[test]
    fn parses_config_reload_custom_url() {
        let action = parse(&["config", "reload", "--url", "https://node.example"]).unwrap();
        assert_eq!(
            action,
            Some(Action::ConfigReload {
                url: "https://node.example".to_string()
            })
        );
    }

    #[test]
    fn parses_config_show_defaults() {
        let action = parse(&["config", "show"]).unwrap();
        assert_eq!(
            action,
            Some(Action::ConfigShow {
                file: None,
                key: None
            })
        );
    }

    #[test]
    fn parses_config_show_with_options() {
        let action =
            parse(&["config", "show", "--file", "custom.cfg", "--key", "rpc.url"]).unwrap();
        assert_eq!(
            action,
            Some(Action::ConfigShow {
                file: Some("custom.cfg".to_string()),
                key: Some("rpc.url".to_string())
            })
        );
    }

    #[test]
    fn parses_version_provenance() {
        let action = parse(&["version", "provenance"]).unwrap();
        assert_eq!(action, Some(Action::VersionProvenance));
    }

    #[test]
    fn unknown_command_is_none() {
        assert!(parse(&["bridge"]).unwrap().is_none());
    }

    #[test]
    fn missing_subcommand_errors() {
        let err = parse(&["config"]).unwrap_err();
        assert!(matches!(err, ActionError::MissingSubcommand("config")));
    }

    #[test]
    fn parses_logs_search_with_defaults() {
        let action = parse(&["logs", "search", "logs.db"]).unwrap();
        match action {
            Some(Action::LogsSearch(opts)) => {
                assert_eq!(opts.db, "logs.db");
                assert!(opts.peer.is_none());
                assert!(opts.block.is_none());
                assert!(opts.limit.is_none());
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn parses_logs_search_with_filters() {
        let action = parse(&[
            "logs",
            "search",
            "logs.db",
            "--peer",
            "peer-1",
            "--block",
            "42",
            "--limit",
            "10",
            "--since",
            "5",
            "--after-id",
            "7",
        ])
        .unwrap();
        match action {
            Some(Action::LogsSearch(opts)) => {
                assert_eq!(opts.peer.as_deref(), Some("peer-1"));
                assert_eq!(opts.block, Some(42));
                assert_eq!(opts.limit, Some(10));
                assert_eq!(opts.since, Some(5));
                assert_eq!(opts.after_id, Some(7));
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn logs_search_invalid_block_reports_error() {
        let err = parse(&["logs", "search", "logs.db", "--block", "not-a-number"]).unwrap_err();
        assert!(matches!(
            err,
            ActionError::Parse(ParseError::InvalidValue { ref option, .. }) if option == "block"
        ));
    }

    #[test]
    fn parses_logs_rotate_key() {
        let action = parse(&[
            "logs",
            "rotate-key",
            "logs.db",
            "--old-passphrase",
            "old",
            "--new-passphrase",
            "new",
        ])
        .unwrap();
        match action {
            Some(Action::LogsRotateKey(opts)) => {
                assert_eq!(opts.db, "logs.db");
                assert_eq!(opts.old_passphrase.as_deref(), Some("old"));
                assert_eq!(opts.new_passphrase.as_deref(), Some("new"));
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn parses_logs_correlate_metric_defaults() {
        let action = parse(&[
            "logs",
            "correlate-metric",
            "--metric",
            "quic_handshake_fail_total",
        ])
        .unwrap();
        match action {
            Some(Action::LogsCorrelateMetric(opts)) => {
                assert_eq!(opts.aggregator, "http://localhost:9000");
                assert_eq!(opts.metric, "quic_handshake_fail_total");
                assert_eq!(opts.max_correlations, 1);
                assert_eq!(opts.rows, 20);
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }
}
