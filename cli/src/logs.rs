use crate::parse_utils::{
    parse_u64, parse_usize, parse_usize_required, require_positional, require_string, take_string,
};
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub db: String,
    pub peer: Option<String>,
    pub tx: Option<String>,
    pub block: Option<u64>,
    pub correlation: Option<String>,
    pub level: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub after_id: Option<u64>,
    pub passphrase: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RotateKeyOptions {
    pub db: String,
    pub old_passphrase: Option<String>,
    pub new_passphrase: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CorrelateMetricOptions {
    pub aggregator: String,
    pub metric: String,
    pub db: Option<String>,
    pub max_correlations: usize,
    pub rows: usize,
    pub passphrase: Option<String>,
}

#[derive(Debug)]
pub enum LogCmd {
    /// Search indexed logs stored in the in-house log store.
    Search {
        /// Log store directory produced by `log indexer`.
        db: String,
        /// Filter by peer identifier.
        peer: Option<String>,
        /// Filter by transaction hash correlation id.
        tx: Option<String>,
        /// Filter by block height.
        block: Option<u64>,
        /// Filter by raw correlation id value.
        correlation: Option<String>,
        /// Filter by severity level.
        level: Option<String>,
        /// Filter by minimum timestamp (inclusive).
        since: Option<u64>,
        /// Filter by maximum timestamp (inclusive).
        until: Option<u64>,
        /// Filter by internal row id greater than the provided value.
        after_id: Option<u64>,
        /// Optional passphrase to decrypt encrypted log messages.
        passphrase: Option<String>,
        /// Maximum rows to return.
        limit: Option<usize>,
    },
    /// Re-encrypt stored messages with a new passphrase.
    RotateKey {
        /// Log store directory produced by `log indexer`.
        db: String,
        /// Existing passphrase protecting log messages.
        old_passphrase: Option<String>,
        /// New passphrase to apply.
        new_passphrase: Option<String>,
    },
    /// Fetch correlations from the metrics aggregator and stream matching logs.
    CorrelateMetric {
        /// Metrics aggregator base URL.
        aggregator: String,
        /// Metric name to correlate (e.g. quic_handshake_fail_total).
        metric: String,
        /// Optional override for the log database path.
        db: Option<String>,
        /// Maximum correlated metric entries to inspect.
        max_correlations: usize,
        /// Limit log rows returned per correlation.
        rows: usize,
        /// Optional passphrase to decrypt log messages.
        passphrase: Option<String>,
    },
}

impl LogCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("logs"),
            "logs",
            "Log search and correlation utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("logs.search"),
                "search",
                "Search indexed logs stored in the log store",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "Log store directory produced by log indexer",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "peer",
                "peer",
                "Filter by peer identifier",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "tx",
                "tx",
                "Filter by transaction hash correlation id",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "block",
                "block",
                "Filter by block height",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "correlation",
                "correlation",
                "Filter by raw correlation id value",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "level",
                "level",
                "Filter by severity level",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "since",
                "since",
                "Filter by minimum timestamp (inclusive)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "until",
                "until",
                "Filter by maximum timestamp (inclusive)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "after_id",
                "after-id",
                "Filter by internal row id greater than the provided value",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "passphrase",
                "passphrase",
                "Passphrase to decrypt encrypted log messages",
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
                CommandId("logs.rotate_key"),
                "rotate-key",
                "Re-encrypt stored messages with a new passphrase",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "Log store directory produced by log indexer",
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
                CommandId("logs.correlate_metric"),
                "correlate-metric",
                "Fetch correlations from the metrics aggregator and stream matching logs",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("aggregator", "aggregator", "Metrics aggregator base URL")
                    .default("http://localhost:9000"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("metric", "metric", "Metric name to correlate").required(true),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "db",
                "db",
                "Optional override for the log database path",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "max_correlations",
                    "max-correlations",
                    "Maximum correlated metric entries to inspect",
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
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'logs'".to_string())?;

        match name {
            "search" => {
                let db = require_positional(sub_matches, "db")?;
                let peer = take_string(sub_matches, "peer");
                let tx = take_string(sub_matches, "tx");
                let block = parse_u64(take_string(sub_matches, "block"), "block")?;
                let correlation = take_string(sub_matches, "correlation");
                let level = take_string(sub_matches, "level");
                let since = parse_u64(take_string(sub_matches, "since"), "since")?;
                let until = parse_u64(take_string(sub_matches, "until"), "until")?;
                let after_id = parse_u64(take_string(sub_matches, "after_id"), "after-id")?;
                let passphrase = take_string(sub_matches, "passphrase");
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?;
                Ok(LogCmd::Search {
                    db,
                    peer,
                    tx,
                    block,
                    correlation,
                    level,
                    since,
                    until,
                    after_id,
                    passphrase,
                    limit,
                })
            }
            "rotate-key" => {
                let db = require_positional(sub_matches, "db")?;
                let old_passphrase = take_string(sub_matches, "old_passphrase");
                let new_passphrase = take_string(sub_matches, "new_passphrase");
                Ok(LogCmd::RotateKey {
                    db,
                    old_passphrase,
                    new_passphrase,
                })
            }
            "correlate-metric" => {
                let aggregator = take_string(sub_matches, "aggregator")
                    .unwrap_or_else(|| "http://localhost:9000".to_string());
                let metric = require_string(sub_matches, "metric")?;
                let db = take_string(sub_matches, "db");
                let max_correlations = parse_usize_required(
                    take_string(sub_matches, "max_correlations"),
                    "max-correlations",
                )?;
                let rows = parse_usize_required(take_string(sub_matches, "rows"), "rows")?;
                let passphrase = take_string(sub_matches, "passphrase");
                Ok(LogCmd::CorrelateMetric {
                    aggregator,
                    metric,
                    db,
                    max_correlations,
                    rows,
                    passphrase,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'logs'")),
        }
    }
}

pub fn run_search(options: SearchOptions) {
    log_store::run_search(options);
}

pub fn run_rotate_key(options: RotateKeyOptions) {
    log_store::run_rotate_key(options);
}

pub fn run_correlate_metric(options: CorrelateMetricOptions) {
    log_store::run_correlate_metric(options);
}

mod log_store {
    use super::{CorrelateMetricOptions, RotateKeyOptions, SearchOptions};
    use crate::http_client;
    use foundation_serialization::Deserialize;
    use foundation_tui::prompt;
    use httpd::Method;
    use log_index::{
        rotate_key, search_logs_in_store, LogEntry, LogFilter, LogIndexError, LogStore,
    };
    use std::env;
    use std::path::Path;

    #[derive(Debug, Deserialize)]
    struct AggregatorCorrelation {
        metric: String,
        correlation_id: String,
        peer_id: String,
        value: Option<f64>,
        timestamp: u64,
    }

    pub fn run_search(options: SearchOptions) {
        let SearchOptions {
            db,
            peer,
            tx,
            block,
            correlation,
            level,
            since,
            until,
            after_id,
            passphrase,
            limit,
        } = options;

        let passphrase =
            prompt_optional_passphrase(passphrase, "Log passphrase (leave blank for none): ");
        let filter = LogFilter {
            peer,
            tx,
            block,
            correlation,
            level,
            since,
            until,
            after_id,
            limit,
            passphrase,
        };

        match search(&db, filter) {
            Ok(entries) => print_entries(&entries),
            Err(err) => exit_with_error("log search failed", err),
        }
    }

    pub fn run_rotate_key(options: RotateKeyOptions) {
        let RotateKeyOptions {
            db,
            old_passphrase,
            new_passphrase,
        } = options;

        let old = prompt_optional_passphrase(
            old_passphrase,
            "Current passphrase (leave blank for none): ",
        );
        let new_pass = prompt_required_passphrase(
            new_passphrase,
            "New passphrase (required): ",
            "new passphrase required",
        );

        match rotate(&db, old.as_deref(), &new_pass) {
            Ok(()) => {}
            Err(err) => exit_with_error("rotate failed", err),
        }
    }

    pub fn run_correlate_metric(options: CorrelateMetricOptions) {
        let CorrelateMetricOptions {
            aggregator,
            metric,
            db,
            max_correlations,
            rows,
            passphrase,
        } = options;

        let client = http_client::blocking_client();
        let url = format!(
            "{}/correlations/{}",
            aggregator.trim_end_matches('/'),
            metric
        );
        let response = match client
            .request(Method::Get, &url)
            .and_then(|builder| builder.send())
        {
            Ok(resp) => resp,
            Err(err) => {
                eprintln!("failed to query aggregator: {err}");
                return;
            }
        };
        if !response.status().is_success() {
            eprintln!(
                "aggregator responded with status {}",
                response.status().as_u16()
            );
            return;
        }
        let mut records: Vec<AggregatorCorrelation> = match response.json() {
            Ok(records) => records,
            Err(err) => {
                eprintln!("failed to decode aggregator response: {err}");
                return;
            }
        };
        if records.is_empty() {
            eprintln!("no correlations recorded for metric {metric}");
            return;
        }
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        let db_path = match resolve_store_path(db) {
            Ok(path) => path,
            Err(msg) => {
                eprintln!("{msg}");
                return;
            }
        };

        let passphrase =
            prompt_optional_passphrase(passphrase, "Log passphrase (leave blank for none): ");
        let limit = max_correlations.max(1);

        let store = match LogStore::open(Path::new(&db_path)) {
            Ok(store) => store,
            Err(err) => {
                exit_with_error("failed to open log store", err);
            }
        };

        for record in records.into_iter().take(limit) {
            let value_str = record
                .value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".into());
            println!(
                "\nmetric={} correlation={} peer={} value={} timestamp={}",
                record.metric, record.correlation_id, record.peer_id, value_str, record.timestamp
            );

            let filter = LogFilter {
                peer: None,
                tx: None,
                block: None,
                correlation: Some(record.correlation_id.clone()),
                level: None,
                since: None,
                until: None,
                after_id: None,
                limit: Some(rows),
                passphrase: passphrase.clone(),
            };

            match search_in_store(&store, filter) {
                Ok(entries) => print_entries(&entries),
                Err(err) => {
                    eprintln!("log search failed: {}", format_error(&err));
                }
            }
        }
    }

    fn search(db: &str, filter: LogFilter) -> Result<Vec<LogEntry>, LogIndexError> {
        let store = LogStore::open(Path::new(db))?;
        search_logs_in_store(&store, &filter)
    }

    fn search_in_store(
        store: &LogStore,
        filter: LogFilter,
    ) -> Result<Vec<LogEntry>, LogIndexError> {
        search_logs_in_store(store, &filter)
    }

    fn rotate(db: &str, current: Option<&str>, new_passphrase: &str) -> Result<(), LogIndexError> {
        let store = LogStore::open(Path::new(db))?;
        rotate_key(&store, current, new_passphrase)
    }

    fn prompt_optional_passphrase(existing: Option<String>, prompt: &str) -> Option<String> {
        match existing {
            Some(p) => Some(p),
            None => match prompt::optional_passphrase(prompt) {
                Ok(pass) => pass
                    .map(|value| value.trim().to_string())
                    .filter(|s| !s.is_empty()),
                Err(err) => {
                    eprintln!("failed to read passphrase: {err}");
                    None
                }
            },
        }
    }

    fn prompt_required_passphrase(
        existing: Option<String>,
        prompt: &str,
        error_msg: &str,
    ) -> String {
        match prompt_optional_passphrase(existing, prompt) {
            Some(pass) => pass,
            None => {
                eprintln!("{error_msg}");
                std::process::exit(1);
            }
        }
    }

    fn print_entries(entries: &[LogEntry]) {
        for entry in entries {
            println!(
                "{} [{}] {} :: {}",
                entry.timestamp, entry.level, entry.correlation_id, entry.message
            );
        }
    }

    fn resolve_store_path(db: Option<String>) -> Result<String, String> {
        if let Some(path) = db {
            return Ok(path);
        }
        if let Ok(path) = env::var("TB_LOG_STORE_PATH") {
            return Ok(path);
        }
        if let Ok(path) = env::var("TB_LOG_DB_PATH") {
            return Ok(path);
        }
        Err("--db must be provided or TB_LOG_STORE_PATH set".to_string())
    }

    fn exit_with_error(context: &str, err: LogIndexError) -> ! {
        eprintln!("{}", format_error_with_context(context, &err));
        std::process::exit(1);
    }

    fn format_error(err: &LogIndexError) -> String {
        match err {
            LogIndexError::MigrationRequired(path) => format!(
                "migration required for legacy SQLite database at {}. Rebuild contract-cli with `--features sqlite-storage` to enable migration support",
                path.display()
            ),
            other => other.to_string(),
        }
    }

    fn format_error_with_context(context: &str, err: &LogIndexError) -> String {
        format!("{}: {}", context, format_error(err))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use foundation_tui::prompt::testing::with_passphrase_override;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        #[test]
        fn prompt_optional_skips_when_existing() {
            let invoked = Arc::new(AtomicBool::new(false));
            let captured = Arc::clone(&invoked);
            let result = with_passphrase_override(
                move |_| {
                    captured.store(true, Ordering::SeqCst);
                    Ok("ignored".to_string())
                },
                || prompt_optional_passphrase(Some("existing".into()), "prompt"),
            );

            assert_eq!(result, Some("existing".to_string()));
            assert!(!invoked.load(Ordering::SeqCst));
        }

        #[test]
        fn prompt_optional_prompts_and_trims() {
            let value = with_passphrase_override(
                |_| Ok("  secret  ".to_string()),
                || prompt_optional_passphrase(None, "prompt"),
            );

            assert_eq!(value, Some("secret".to_string()));
        }

        #[test]
        fn prompt_optional_filters_empty() {
            let value = with_passphrase_override(
                |_| Ok("   ".to_string()),
                || prompt_optional_passphrase(None, "prompt"),
            );

            assert_eq!(value, None);
        }

        #[test]
        fn prompt_required_returns_existing() {
            let result = with_passphrase_override(
                |_| Ok("should-not-run".to_string()),
                || prompt_required_passphrase(Some("value".into()), "prompt", "error"),
            );

            assert_eq!(result, "value".to_string());
        }

        #[test]
        fn prompt_required_reads_when_missing() {
            let result = with_passphrase_override(
                |_| Ok("new-pass".to_string()),
                || prompt_required_passphrase(None, "prompt", "error"),
            );

            assert_eq!(result, "new-pass".to_string());
        }
    }
}

pub fn handle(cmd: LogCmd) {
    match cmd {
        LogCmd::Search {
            db,
            peer,
            tx,
            block,
            correlation,
            level,
            since,
            until,
            after_id,
            passphrase,
            limit,
        } => {
            run_search(SearchOptions {
                db,
                peer,
                tx,
                block,
                correlation,
                level,
                since,
                until,
                after_id,
                passphrase,
                limit,
            });
        }
        LogCmd::RotateKey {
            db,
            old_passphrase,
            new_passphrase,
        } => {
            run_rotate_key(RotateKeyOptions {
                db,
                old_passphrase,
                new_passphrase,
            });
        }
        LogCmd::CorrelateMetric {
            aggregator,
            metric,
            db,
            max_correlations,
            rows,
            passphrase,
        } => {
            run_correlate_metric(CorrelateMetricOptions {
                aggregator,
                metric,
                db,
                max_correlations,
                rows,
                passphrase,
            });
        }
    }
}
