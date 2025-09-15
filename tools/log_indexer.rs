use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use base64::{engine::general_purpose, Engine as _};
use blake3::derive_key;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rand::RngCore;
use rusqlite::{params, params_from_iter, Connection, Result, Row};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct LogEntry {
    pub timestamp: u64,
    pub level: String,
    pub message: String,
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
    pub limit: Option<usize>,
    pub passphrase: Option<String>,
}

/// Index JSON log lines into a SQLite database using default options.
pub fn index_logs(log_path: &Path, db_path: &Path) -> Result<()> {
    index_logs_with_options(log_path, db_path, IndexOptions::default())
}

/// Index JSON log lines with explicit options such as encryption.
pub fn index_logs_with_options(log_path: &Path, db_path: &Path, opts: IndexOptions) -> Result<()> {
    let conn = Connection::open(db_path)?;
    ensure_schema(&conn)?;
    let file = File::open(log_path)?;
    let reader = BufReader::new(file);
    let key = opts.passphrase.as_ref().map(|p| derive_encryption_key(p));
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: LogEntry = serde_json::from_str(&line)?;
        let (message, encrypted, nonce) = if let Some(key) = key.as_ref() {
            let (cipher, nonce) = encrypt_message(key, &entry.message)?;
            (cipher, 1i64, Some(nonce))
        } else {
            (entry.message.clone(), 0i64, None)
        };
        conn.execute(
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
            params![
                entry.timestamp,
                entry.level,
                message,
                entry.correlation_id,
                entry.peer,
                entry.tx,
                entry.block.map(|b| b as i64),
                encrypted,
                nonce,
            ],
        )?;
        increment_indexed_metric();
    }
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
    Ok(())
}

fn encrypt_message(key: &Key<XChaCha20Poly1305>, message: &str) -> Result<(String, Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new(key);
    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, message.as_bytes())
        .map_err(|e| rusqlite::Error::ExecuteReturnedError(format!("encrypt: {e}").into()))?;
    Ok((
        general_purpose::STANDARD.encode(ciphertext),
        nonce_bytes.to_vec(),
    ))
}

fn decrypt_message(key: &Key<XChaCha20Poly1305>, data: &str, nonce: &[u8]) -> Option<String> {
    let cipher = XChaCha20Poly1305::new(key);
    let nonce = XNonce::from_slice(nonce);
    let bytes = general_purpose::STANDARD.decode(data).ok()?;
    cipher
        .decrypt(nonce, bytes.as_ref())
        .ok()
        .and_then(|plain| String::from_utf8(plain).ok())
}

fn derive_encryption_key(passphrase: &str) -> Key<XChaCha20Poly1305> {
    let mut key_bytes = [0u8; 32];
    derive_key(
        "the-block-log-indexer",
        passphrase.as_bytes(),
        &mut key_bytes,
    );
    Key::from_slice(&key_bytes).to_owned()
}

#[cfg(feature = "telemetry")]
fn increment_indexed_metric() {
    crate::telemetry::LOG_ENTRIES_INDEXED_TOTAL.inc();
}

#[cfg(not(feature = "telemetry"))]
fn increment_indexed_metric() {}

fn row_to_entry(row: &Row<'_>, key: Option<&Key<XChaCha20Poly1305>>) -> Result<LogEntry> {
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
    let mut sql = String::from(
        "SELECT timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce FROM logs",
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
            passphrase,
            limit,
        } => {
            let filter = LogFilter {
                peer,
                tx,
                block,
                correlation,
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
