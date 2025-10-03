use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose, Engine as _};
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use coding::{
    decrypt_xchacha20_poly1305, encrypt_xchacha20_poly1305, ChaCha20Poly1305Encryptor, Encryptor,
    CHACHA20_POLY1305_KEY_LEN, CHACHA20_POLY1305_NONCE_LEN, XCHACHA20_POLY1305_NONCE_LEN,
};
use crypto_suite::hashing::blake3::derive_key;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LogEntry {
    #[serde(default)]
    pub id: Option<u64>,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub tx: Option<String>,
    #[serde(default)]
    pub block: Option<u64>,
}

#[derive(Debug, Default, Clone)]
pub struct IndexOptions {
    pub passphrase: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct LogFilter {
    pub peer: Option<String>,
    pub tx: Option<String>,
    pub block: Option<u64>,
    pub correlation: Option<String>,
    pub level: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub after_id: Option<u64>,
    pub limit: Option<usize>,
    pub passphrase: Option<String>,
}

pub type Result<T> = std::result::Result<T, LogIndexerError>;

#[derive(Debug)]
pub enum LogIndexerError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    Encryption(String),
}

impl fmt::Display for LogIndexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogIndexerError::Io(err) => write!(f, "io error: {err}"),
            LogIndexerError::Sqlite(err) => write!(f, "sqlite error: {err}"),
            LogIndexerError::Json(err) => write!(f, "json error: {err}"),
            LogIndexerError::Encryption(msg) => write!(f, "encryption error: {msg}"),
        }
    }
}

impl StdError for LogIndexerError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            LogIndexerError::Io(err) => Some(err),
            LogIndexerError::Sqlite(err) => Some(err),
            LogIndexerError::Json(err) => Some(err),
            LogIndexerError::Encryption(_) => None,
        }
    }
}

impl From<rusqlite::Error> for LogIndexerError {
    fn from(err: rusqlite::Error) -> Self {
        LogIndexerError::Sqlite(err)
    }
}

impl From<io::Error> for LogIndexerError {
    fn from(err: io::Error) -> Self {
        LogIndexerError::Io(err)
    }
}

impl From<serde_json::Error> for LogIndexerError {
    fn from(err: serde_json::Error) -> Self {
        LogIndexerError::Json(err)
    }
}

/// Index JSON log lines into a SQLite database using default options.
pub fn index_logs(log_path: &Path, db_path: &Path) -> Result<()> {
    index_logs_with_options(log_path, db_path, IndexOptions::default())
}

/// Index JSON log lines with explicit options such as encryption.
pub fn index_logs_with_options(log_path: &Path, db_path: &Path, opts: IndexOptions) -> Result<()> {
    let mut conn = Connection::open(db_path)?;
    ensure_schema(&conn)?;
    let mut file = File::open(log_path)?;
    let source = canonical_source_key(log_path);
    let mut offset = last_ingested_offset(&conn, &source)?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))?;
    }
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let key = opts.passphrase.as_ref().map(|p| derive_encryption_key(p));
    let tx = conn.transaction()?;
    let mut insert = tx.prepare(
        "INSERT INTO logs (
            timestamp,
            level,
            message,
            correlation_id,
            peer,
            tx,
            block,
            encrypted,
            nonce
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        offset += bytes as u64;
        if line.trim().is_empty() {
            continue;
        }
        let entry: LogEntry = serde_json::from_str(line.trim_end())?;
        let (message, encrypted, nonce) = if let Some(key) = key.as_ref() {
            let (cipher, nonce) = encrypt_message(key, &entry.message)?;
            (cipher, 1i64, Some(nonce))
        } else {
            (entry.message.clone(), 0i64, None)
        };
        insert.execute(params![
            entry.timestamp,
            entry.level,
            message,
            entry.correlation_id,
            entry.peer,
            entry.tx,
            entry.block.map(|b| b as i64),
            encrypted,
            nonce,
        ])?;
        increment_indexed_metric(&entry.correlation_id);
    }
    drop(insert);
    update_ingest_offset(&tx, &source, offset)?;
    tx.commit()?;
    Ok(())
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER,
            level TEXT,
            message TEXT,
            correlation_id TEXT,
            peer TEXT,
            tx TEXT,
            block INTEGER,
            encrypted INTEGER NOT NULL DEFAULT 0,
            nonce BLOB
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ingest_state (
            source TEXT PRIMARY KEY,
            offset INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

fn canonical_source_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn last_ingested_offset(conn: &Connection, source: &str) -> Result<u64> {
    let value = conn
        .query_row(
            "SELECT offset FROM ingest_state WHERE source = ?1",
            params![source],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(value.unwrap_or(0).max(0) as u64)
}

fn update_ingest_offset(tx: &rusqlite::Transaction<'_>, source: &str, offset: u64) -> Result<()> {
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    tx.execute(
        "INSERT INTO ingest_state (source, offset, updated_at) VALUES (?1, ?2, ?3)
        ON CONFLICT(source) DO UPDATE SET offset = excluded.offset, updated_at = excluded.updated_at",
        params![source, offset as i64, updated_at],
    )?;
    Ok(())
}

fn encrypt_message(
    key: &[u8; CHACHA20_POLY1305_KEY_LEN],
    message: &str,
) -> Result<(String, Vec<u8>)> {
    let payload = encrypt_xchacha20_poly1305(key, message.as_bytes())
        .map_err(|e| LogIndexerError::Encryption(format!("encrypt: {e}")))?;
    let (nonce, body) = payload.split_at(XCHACHA20_POLY1305_NONCE_LEN);
    Ok((general_purpose::STANDARD.encode(body), nonce.to_vec()))
}

fn decrypt_message(
    key: &[u8; CHACHA20_POLY1305_KEY_LEN],
    data: &str,
    nonce: &[u8],
) -> Option<String> {
    let body = general_purpose::STANDARD.decode(data).ok()?;
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

fn derive_encryption_key(passphrase: &str) -> [u8; CHACHA20_POLY1305_KEY_LEN] {
    derive_key("the-block-log-indexer", passphrase.as_bytes())
}

#[cfg(feature = "telemetry")]
fn increment_indexed_metric(correlation_id: &str) {
    crate::telemetry::LOG_ENTRIES_INDEXED_TOTAL.inc();
    use std::borrow::Cow;
    let label: Cow<'_, str> = if correlation_id.is_empty() {
        Cow::Borrowed("unknown")
    } else {
        let shortened: String = correlation_id.chars().take(64).collect();
        let trimmed = shortened.trim();
        if trimmed.is_empty() {
            Cow::Borrowed("unknown")
        } else if trimmed.len() == shortened.len() {
            Cow::Owned(shortened)
        } else {
            Cow::Owned(trimmed.to_string())
        }
    };
    crate::telemetry::LOG_CORRELATION_INDEX_TOTAL
        .with_label_values(&[label.as_ref()])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
fn increment_indexed_metric(_correlation_id: &str) {}

fn row_to_entry(row: &Row<'_>, key: Option<&[u8; CHACHA20_POLY1305_KEY_LEN]>) -> Result<LogEntry> {
    let encrypted: i64 = row.get("encrypted")?;
    let nonce: Option<Vec<u8>> = row.get("nonce")?;
    let stored_msg: String = row.get("message")?;
    let message = if encrypted == 1 {
        if let (Some(key), Some(nonce)) = (key, nonce.as_ref()) {
            decrypt_message(key, &stored_msg, nonce).unwrap_or_else(|| "<decrypt-failed>".into())
        } else {
            "<encrypted>".into()
        }
    } else {
        stored_msg
    };
    Ok(LogEntry {
        id: row.get::<_, Option<i64>>("id")?.map(|v| v.max(0) as u64),
        timestamp: row.get("timestamp")?,
        level: row.get("level")?,
        message,
        correlation_id: row.get("correlation_id")?,
        peer: row.get("peer")?,
        tx: row.get("tx")?,
        block: row.get::<_, Option<i64>>("block")?.map(|v| v as u64),
    })
}

/// Search indexed logs with optional filters.
pub fn search_logs(db_path: &Path, filter: &LogFilter) -> Result<Vec<LogEntry>> {
    let conn = Connection::open(db_path)?;
    ensure_schema(&conn)?;
    let mut clauses = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(after) = filter.after_id {
        clauses.push("id > ?".to_string());
        values.push((after as i64).into());
    }
    if let Some(peer) = &filter.peer {
        clauses.push("peer = ?".to_string());
        values.push(peer.clone().into());
    }
    if let Some(tx) = &filter.tx {
        clauses.push("tx = ?".to_string());
        values.push(tx.clone().into());
    }
    if let Some(block) = filter.block {
        clauses.push("block = ?".to_string());
        values.push((block as i64).into());
    }
    if let Some(corr) = &filter.correlation {
        clauses.push("correlation_id = ?".to_string());
        values.push(corr.clone().into());
    }
    if let Some(level) = &filter.level {
        clauses.push("level = ?".to_string());
        values.push(level.clone().into());
    }
    if let Some(since) = filter.since {
        clauses.push("timestamp >= ?".to_string());
        values.push((since as i64).into());
    }
    if let Some(until) = filter.until {
        clauses.push("timestamp <= ?".to_string());
        values.push((until as i64).into());
    }
    let mut sql = String::from(
        "SELECT id, timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce FROM logs",
    );
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY timestamp DESC");
    if let Some(limit) = filter.limit {
        sql.push_str(" LIMIT ?");
        values.push((limit as i64).into());
    }
    let mut stmt = conn.prepare(&sql)?;
    let key = filter.passphrase.as_ref().map(|p| derive_encryption_key(p));
    let key_ref = key.as_ref();
    let mut rows = stmt.query(params_from_iter(values.iter()))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_entry(row, key_ref)?);
    }
    #[cfg(feature = "telemetry")]
    {
        if filter
            .correlation
            .as_ref()
            .map(|c| !c.is_empty())
            .unwrap_or(false)
            && out.is_empty()
        {
            crate::telemetry::record_log_correlation_fail();
        }
    }
    Ok(out)
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
    let bin = argv.next().unwrap_or_else(|| "log-indexer".to_string());
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
            "Index a JSON log file into SQLite",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "log",
            "Path to the JSON log file",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "db",
            "Destination SQLite database file",
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
            "SQLite database file produced by 'index'",
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
