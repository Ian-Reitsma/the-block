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
    /// Search indexed logs stored in SQLite.
    Search {
        /// SQLite database produced by `log indexer`.
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
        /// SQLite database produced by `log indexer`.
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
                "Search indexed logs stored in SQLite",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "SQLite database produced by log indexer",
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
                "SQLite database produced by log indexer",
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
    run_search_impl(options);
}

pub fn run_rotate_key(options: RotateKeyOptions) {
    run_rotate_key_impl(options);
}

pub fn run_correlate_metric(options: CorrelateMetricOptions) {
    run_correlate_metric_impl(options);
}

#[cfg(feature = "sqlite-storage")]
fn run_search_impl(options: SearchOptions) {
    sqlite::run_search(options);
}

#[cfg(not(feature = "sqlite-storage"))]
fn run_search_impl(_options: SearchOptions) {
    emit_missing_sqlite_feature();
}

#[cfg(feature = "sqlite-storage")]
fn run_rotate_key_impl(options: RotateKeyOptions) {
    sqlite::run_rotate_key(options);
}

#[cfg(not(feature = "sqlite-storage"))]
fn run_rotate_key_impl(_options: RotateKeyOptions) {
    emit_missing_sqlite_feature();
}

#[cfg(feature = "sqlite-storage")]
fn run_correlate_metric_impl(options: CorrelateMetricOptions) {
    sqlite::run_correlate_metric(options);
}

#[cfg(not(feature = "sqlite-storage"))]
fn run_correlate_metric_impl(_options: CorrelateMetricOptions) {
    emit_missing_sqlite_feature();
}

#[cfg(not(feature = "sqlite-storage"))]
fn emit_missing_sqlite_feature() {
    eprintln!(
        "log database commands require the `sqlite-storage` feature. Rebuild contract-cli with `--features sqlite-storage` or `--features full`.",
    );
    std::process::exit(1);
}

#[cfg(feature = "sqlite-storage")]
mod sqlite {
    use super::{CorrelateMetricOptions, RotateKeyOptions, SearchOptions};
    use crate::http_client;
    use base64_fp::{decode_standard, encode_standard};
    use coding::Encryptor;
    use coding::{
        decrypt_xchacha20_poly1305, encrypt_xchacha20_poly1305, ChaCha20Poly1305Encryptor,
        CHACHA20_POLY1305_KEY_LEN, CHACHA20_POLY1305_NONCE_LEN, XCHACHA20_POLY1305_NONCE_LEN,
    };
    use crypto_suite::hashing::blake3::derive_key;
    use diagnostics::anyhow::{anyhow, Result as AnyResult};
    use foundation_serialization::Deserialize;
    use foundation_sqlite::{params, params_from_iter, Connection, Row, Value};
    use httpd::Method;
    use rpassword::prompt_password;
    use std::env;

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
        if let Err(e) = search(
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
        ) {
            eprintln!("log search failed: {e}");
            std::process::exit(1);
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
        let new_pass =
            match prompt_optional_passphrase(new_passphrase, "New passphrase (required): ") {
                Some(p) => p,
                None => {
                    eprintln!("new passphrase required");
                    std::process::exit(1);
                }
            };
        if let Err(e) = rotate_key(db, old, new_pass) {
            eprintln!("rotate failed: {e}");
            std::process::exit(1);
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
        let db_path = match db.or_else(|| env::var("TB_LOG_DB_PATH").ok()) {
            Some(path) => path,
            None => {
                eprintln!("--db must be provided or TB_LOG_DB_PATH set");
                return;
            }
        };
        let passphrase =
            prompt_optional_passphrase(passphrase, "Log passphrase (leave blank for none): ");
        let limit = max_correlations.max(1);
        for record in records.into_iter().take(limit) {
            let value_str = record
                .value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".into());
            println!(
                "\nmetric={} correlation={} peer={} value={} timestamp={}",
                record.metric, record.correlation_id, record.peer_id, value_str, record.timestamp
            );
            if let Err(err) = search(
                db_path.clone(),
                None,
                None,
                None,
                Some(record.correlation_id.clone()),
                None,
                None,
                None,
                None,
                passphrase.clone(),
                Some(rows),
            ) {
                eprintln!("log search failed: {err}");
            }
        }
    }

    fn search(
        db: String,
        peer: Option<String>,
        tx: Option<String>,
        block: Option<u64>,
        correlation: Option<String>,
        level: Option<String>,
        since: Option<u64>,
        until: Option<u64>,
        after_id: Option<u64>,
        passphrase: Option<String>,
        limit: Option<usize>,
    ) -> foundation_sqlite::Result<()> {
        let conn = Connection::open(db)?;
        let mut clauses = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        if let Some(peer) = peer {
            clauses.push("peer = ?".to_string());
            params.push(peer.into());
        }
        if let Some(tx) = tx {
            clauses.push("tx = ?".to_string());
            params.push(tx.into());
        }
        if let Some(block) = block {
            clauses.push("block = ?".to_string());
            params.push((block as i64).into());
        }
        if let Some(corr) = correlation {
            clauses.push("correlation_id = ?".to_string());
            params.push(corr.into());
        }
        if let Some(level) = level {
            clauses.push("level = ?".to_string());
            params.push(level.into());
        }
        if let Some(since) = since {
            clauses.push("timestamp >= ?".to_string());
            params.push((since as i64).into());
        }
        if let Some(until) = until {
            clauses.push("timestamp <= ?".to_string());
            params.push((until as i64).into());
        }
        if let Some(after_id) = after_id {
            clauses.push("id > ?".to_string());
            params.push((after_id as i64).into());
        }
        let mut sql = String::from(
            "SELECT id, timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce FROM logs",
        );
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY timestamp DESC");
        if let Some(limit) = limit {
            sql.push_str(" LIMIT ?");
            params.push((limit as i64).into());
        }
        let mut stmt = conn.prepare(&sql)?;
        let key = passphrase.as_ref().map(|p| derive_key_bytes(p));
        let key_ref = key.as_ref();
        let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
            decode_row(row, key_ref)
        })?;
        for row in rows {
            let entry = row?;
            println!(
                "#{} {} [{}] {} :: {}",
                entry.id.unwrap_or(0),
                entry.timestamp,
                entry.level,
                entry.correlation_id,
                entry.message
            );
        }
        Ok(())
    }

    struct QueryRow {
        id: Option<u64>,
        timestamp: i64,
        level: String,
        message: String,
        correlation_id: String,
    }

    fn decode_row(
        row: &Row<'_>,
        key: Option<&[u8; CHACHA20_POLY1305_KEY_LEN]>,
    ) -> foundation_sqlite::Result<QueryRow> {
        let encrypted: i64 = row.get("encrypted")?;
        let stored_msg: String = row.get("message")?;
        let nonce: Option<Vec<u8>> = row.get("nonce")?;
        let message = if encrypted == 1 {
            if let (Some(key), Some(nonce)) = (key, nonce.as_ref()) {
                decrypt_message(key, &stored_msg, nonce)
                    .unwrap_or_else(|| "<decrypt-failed>".into())
            } else {
                "<encrypted>".into()
            }
        } else {
            stored_msg
        };
        Ok(QueryRow {
            id: row.get::<_, Option<i64>>("id")?.map(|v| v.max(0) as u64),
            timestamp: row.get("timestamp")?,
            level: row.get("level")?,
            message,
            correlation_id: row.get("correlation_id")?,
        })
    }

    fn rotate_key(db: String, current: Option<String>, new_pass: String) -> AnyResult<()> {
        let mut conn = Connection::open(db)?;
        let old_key = current.as_deref().map(derive_key_bytes);
        let new_key = derive_key_bytes(&new_pass);
        let tx = conn.transaction()?;
        let select_rows = tx
            .prepare("SELECT id, message, nonce, encrypted FROM logs")?
            .query_map(params![], |row| {
                Ok((
                    row.get::<_, i64>("id")?,
                    row.get::<_, Option<Vec<u8>>>("nonce")?,
                    row.get::<_, String>("message")?,
                    row.get::<_, i64>("encrypted")?,
                ))
            })?;
        let mut updates = Vec::new();
        for row in select_rows {
            let (id, nonce, message, encrypted_flag) = row?;
            let plain = if encrypted_flag == 1 {
                let key = old_key
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing current passphrase"))?;
                let nonce_bytes = nonce.as_deref().ok_or_else(|| anyhow!("missing nonce"))?;
                decrypt_message(key, &message, nonce_bytes)
                    .ok_or_else(|| anyhow!("decrypt failed"))?
            } else {
                message.clone()
            };
            let (cipher, nonce_bytes) = encrypt_message(&new_key, &plain)?;
            updates.push((id, cipher, nonce_bytes));
        }
        {
            let mut update_stmt = tx
                .prepare("UPDATE logs SET message = ?1, nonce = ?2, encrypted = 1 WHERE id = ?3")?;
            for (id, message, nonce) in updates {
                update_stmt.execute(params![message, nonce, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn derive_key_bytes(passphrase: &str) -> [u8; CHACHA20_POLY1305_KEY_LEN] {
        derive_key("the-block-log-indexer", passphrase.as_bytes())
    }

    fn encrypt_message(
        key: &[u8; CHACHA20_POLY1305_KEY_LEN],
        message: &str,
    ) -> AnyResult<(String, Vec<u8>)> {
        let payload = encrypt_xchacha20_poly1305(key, message.as_bytes())
            .map_err(|e| anyhow!("encrypt: {e}"))?;
        let (nonce, body) = payload.split_at(XCHACHA20_POLY1305_NONCE_LEN);
        Ok((encode_standard(body), nonce.to_vec()))
    }

    fn decrypt_message(
        key: &[u8; CHACHA20_POLY1305_KEY_LEN],
        data: &str,
        nonce: &[u8],
    ) -> Option<String> {
        let body = decode_standard(data).ok()?;
        if nonce.is_empty() {
            return decrypt_xchacha20_poly1305(key, &body)
                .ok()
                .and_then(|plain| String::from_utf8(plain).ok());
        }
        let mut payload = Vec::with_capacity(nonce.len() + body.len());
        payload.extend_from_slice(nonce);
        payload.extend_from_slice(&body);
        let plaintext = match nonce.len() {
            XCHACHA20_POLY1305_NONCE_LEN => decrypt_xchacha20_poly1305(key, &payload).ok(),
            CHACHA20_POLY1305_NONCE_LEN => {
                let encryptor = ChaCha20Poly1305Encryptor::new(key.as_ref()).ok()?;
                encryptor.decrypt(&payload).ok()
            }
            _ => None,
        }?;
        String::from_utf8(plaintext).ok()
    }

    fn prompt_optional_passphrase(existing: Option<String>, prompt: &str) -> Option<String> {
        match existing {
            Some(p) => Some(p),
            None => prompt_password(prompt)
                .ok()
                .map(|input| input.trim().to_string())
                .filter(|s| !s.is_empty()),
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
