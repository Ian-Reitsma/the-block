use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose, Engine as _};
use blake3::derive_key;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rand::RngCore;
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
    let mut tx = conn.transaction()?;
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

fn encrypt_message(key: &Key, message: &str) -> Result<(String, Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new(key);
    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, message.as_bytes())
        .map_err(|e| LogIndexerError::Encryption(format!("encrypt: {e}")))?;
    Ok((
        general_purpose::STANDARD.encode(ciphertext),
        nonce_bytes.to_vec(),
    ))
}

fn decrypt_message(key: &Key, data: &str, nonce: &[u8]) -> Option<String> {
    let cipher = XChaCha20Poly1305::new(key);
    let nonce = XNonce::from_slice(nonce);
    let bytes = general_purpose::STANDARD.decode(data).ok()?;
    cipher
        .decrypt(nonce, bytes.as_ref())
        .ok()
        .and_then(|plain| String::from_utf8(plain).ok())
}

fn derive_encryption_key(passphrase: &str) -> Key {
    let key_bytes = derive_key("the-block-log-indexer", passphrase.as_bytes());
    Key::clone_from_slice(&key_bytes)
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

fn row_to_entry(row: &Row<'_>, key: Option<&Key>) -> Result<LogEntry> {
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
            crate::telemetry::LOG_CORRELATION_FAIL_TOTAL.inc();
        }
    }
    Ok(out)
}

#[derive(Parser, Debug)]
#[command(about = "Index and query structured logs", version)]
struct Args {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Index a JSON log file into SQLite
    Index {
        /// Path to the JSON log file
        log: String,
        /// Destination SQLite database file
        db: String,
        /// Optional passphrase for encrypting log messages at rest
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Query previously indexed logs
    Search {
        /// SQLite database file produced by `index`
        db: String,
        #[arg(long)]
        peer: Option<String>,
        #[arg(long)]
        tx: Option<String>,
        #[arg(long)]
        block: Option<u64>,
        #[arg(long)]
        correlation: Option<String>,
        #[arg(long)]
        level: Option<String>,
        #[arg(long)]
        since: Option<u64>,
        #[arg(long)]
        until: Option<u64>,
        #[arg(long = "after-id")]
        after_id: Option<u64>,
        /// Passphrase required to decrypt encrypted log messages
        #[arg(long)]
        passphrase: Option<String>,
        /// Maximum number of rows to return
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[cfg(not(test))]
fn main() {
    let args = Args::parse();
    match args.cmd {
        Command::Index {
            log,
            db,
            passphrase,
        } => {
            let opts = IndexOptions { passphrase };
            if let Err(e) = index_logs_with_options(Path::new(&log), Path::new(&db), opts) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Command::Search {
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
            match search_logs(Path::new(&db), &filter) {
                Ok(results) => {
                    for entry in results {
                        println!(
                            "{} [{}] {} :: {}",
                            entry.timestamp, entry.level, entry.correlation_id, entry.message
                        );
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
