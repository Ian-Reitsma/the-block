use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64_fp::{decode_standard, encode_standard};
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
use foundation_serialization::{json, Deserialize, Error as SerializationError, Serialize};
use sled::{self, Db, Tree};

#[cfg(feature = "sqlite-migration")]
use rusqlite::{Connection, Row};

const ENTRIES_TREE: &str = "entries";
const META_TREE: &str = "meta";
const NEXT_ID_KEY: &str = "next_id";
const OFFSET_PREFIX: &str = "offset:";

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(crate = "foundation_serialization::serde")]
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

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "foundation_serialization::serde")]
struct StoredEntry {
    id: u64,
    timestamp: u64,
    level: String,
    message: String,
    correlation_id: String,
    peer: Option<String>,
    tx: Option<String>,
    block: Option<u64>,
    encrypted: bool,
    nonce: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct IngestState {
    offset: u64,
    updated_at: u64,
}

struct LogStore {
    db: Db,
    entries: Tree,
    meta: Tree,
}

impl LogStore {
    fn open(path: &Path) -> Result<Self> {
        let (store_path, legacy_sqlite) = prepare_store_path(path)?;
        let db = sled::open(&store_path)?;
        let entries = db.open_tree(ENTRIES_TREE)?;
        let meta = db.open_tree(META_TREE)?;
        let store = Self { db, entries, meta };
        if let Some(legacy) = legacy_sqlite {
            migrate_sqlite(&store, &legacy)?;
        }
        Ok(store)
    }

    fn load_next_id(&self) -> Result<u64> {
        if let Some(value) = self.meta.get(NEXT_ID_KEY)? {
            Ok(bytes_to_u64(&value))
        } else {
            Ok(0)
        }
    }

    fn save_next_id(&self, id: u64) -> Result<()> {
        self.meta
            .insert(NEXT_ID_KEY, id.to_be_bytes().to_vec())?
            .map(|_| ());
        Ok(())
    }

    fn load_offset(&self, source: &str) -> Result<u64> {
        let key = format!("{OFFSET_PREFIX}{source}");
        match self.meta.get(key.as_bytes())? {
            Some(value) => {
                let state: IngestState = json::from_slice(&value)?;
                Ok(state.offset)
            }
            None => Ok(0),
        }
    }

    fn save_offset(&self, source: &str, offset: u64) -> Result<()> {
        let key = format!("{OFFSET_PREFIX}{source}");
        let state = IngestState {
            offset,
            updated_at: current_unix_seconds(),
        };
        self.meta
            .insert(key.as_bytes(), json::to_vec(&state)?)?
            .map(|_| ());
        Ok(())
    }

    #[cfg(feature = "sqlite-migration")]
    fn save_offset_with_timestamp(&self, source: &str, offset: u64, updated_at: u64) -> Result<()> {
        let key = format!("{OFFSET_PREFIX}{source}");
        let state = IngestState { offset, updated_at };
        self.meta
            .insert(key.as_bytes(), json::to_vec(&state)?)?
            .map(|_| ());
        Ok(())
    }

    fn store_entry(&self, entry: &StoredEntry) -> Result<()> {
        let key = entry_key(entry.id);
        self.entries
            .insert(key.as_bytes(), json::to_vec(entry)?)?
            .map(|_| ());
        Ok(())
    }

    fn load_entries(&self) -> Result<Vec<StoredEntry>> {
        let mut items = Vec::new();
        for result in self.entries.iter() {
            let (_, value) = result?;
            let entry: StoredEntry = json::from_slice(&value)?;
            items.push(entry);
        }
        Ok(items)
    }

    fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum LogIndexerError {
    Io(io::Error),
    Storage(sled::Error),
    Json(SerializationError),
    Encryption(String),
    MigrationRequired(PathBuf),
    #[cfg(feature = "sqlite-migration")]
    Sqlite(rusqlite::Error),
}

impl fmt::Display for LogIndexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogIndexerError::Io(err) => write!(f, "io error: {err}"),
            LogIndexerError::Storage(err) => write!(f, "storage error: {err}"),
            LogIndexerError::Json(err) => write!(f, "json error: {err}"),
            LogIndexerError::Encryption(msg) => write!(f, "encryption error: {msg}"),
            LogIndexerError::MigrationRequired(path) => write!(
                f,
                "legacy SQLite database detected at '{}'; rebuild with --features sqlite-migration to migrate",
                path.display()
            ),
            #[cfg(feature = "sqlite-migration")]
            LogIndexerError::Sqlite(err) => write!(f, "sqlite error: {err}"),
        }
    }
}

impl StdError for LogIndexerError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            LogIndexerError::Io(err) => Some(err),
            LogIndexerError::Storage(err) => Some(err),
            LogIndexerError::Json(err) => Some(err),
            LogIndexerError::Encryption(_) => None,
            LogIndexerError::MigrationRequired(_) => None,
            #[cfg(feature = "sqlite-migration")]
            LogIndexerError::Sqlite(err) => Some(err),
        }
    }
}

impl From<sled::Error> for LogIndexerError {
    fn from(err: sled::Error) -> Self {
        LogIndexerError::Storage(err)
    }
}

impl From<io::Error> for LogIndexerError {
    fn from(err: io::Error) -> Self {
        LogIndexerError::Io(err)
    }
}

impl From<SerializationError> for LogIndexerError {
    fn from(err: SerializationError) -> Self {
        LogIndexerError::Json(err)
    }
}

#[cfg(feature = "sqlite-migration")]
impl From<rusqlite::Error> for LogIndexerError {
    fn from(err: rusqlite::Error) -> Self {
        LogIndexerError::Sqlite(err)
    }
}

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
    let mut offset = store.load_offset(&source)?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))?;
    }
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let key = opts.passphrase.as_ref().map(|p| derive_encryption_key(p));
    let mut next_id = store.load_next_id()?;
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
        let entry: LogEntry = json::from_str(line.trim_end())?;
        next_id = next_id.saturating_add(1);
        let (message, encrypted, nonce) = if let Some(key) = key.as_ref() {
            let (cipher, nonce) = encrypt_message(key, &entry.message)?;
            (cipher, true, Some(nonce))
        } else {
            (entry.message.clone(), false, None)
        };
        let stored = StoredEntry {
            id: next_id,
            timestamp: entry.timestamp,
            level: entry.level,
            message,
            correlation_id: entry.correlation_id,
            peer: entry.peer,
            tx: entry.tx,
            block: entry.block,
            encrypted,
            nonce,
        };
        store.store_entry(&stored)?;
        increment_indexed_metric(&stored.correlation_id);
    }
    store.save_offset(&source, offset)?;
    store.save_next_id(next_id)?;
    store.flush()?;
    Ok(())
}

fn canonical_source_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn encrypt_message(
    key: &[u8; CHACHA20_POLY1305_KEY_LEN],
    message: &str,
) -> Result<(String, Vec<u8>)> {
    let payload = encrypt_xchacha20_poly1305(key, message.as_bytes())
        .map_err(|e| LogIndexerError::Encryption(format!("encrypt: {e}")))?;
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

fn bytes_to_u64(value: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let len = value.len().min(8);
    buf[8 - len..].copy_from_slice(&value[value.len() - len..]);
    u64::from_be_bytes(buf)
}

fn entry_key(id: u64) -> String {
    format!("entry:{id:016x}")
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Search indexed logs with optional filters.
pub fn search_logs(db_path: &Path, filter: &LogFilter) -> Result<Vec<LogEntry>> {
    let store = LogStore::open(db_path)?;
    let entries = store.load_entries()?;
    let key = filter.passphrase.as_ref().map(|p| derive_encryption_key(p));
    let key_ref = key.as_ref();
    let mut results: Vec<LogEntry> = entries
        .into_iter()
        .filter(|entry| match filter.after_id {
            Some(after) => entry.id > after,
            None => true,
        })
        .filter(|entry| match &filter.peer {
            Some(peer) => entry.peer.as_deref() == Some(peer.as_str()),
            None => true,
        })
        .filter(|entry| match &filter.tx {
            Some(tx) => entry.tx.as_deref() == Some(tx.as_str()),
            None => true,
        })
        .filter(|entry| match filter.block {
            Some(block) => entry.block == Some(block),
            None => true,
        })
        .filter(|entry| match &filter.correlation {
            Some(corr) => entry.correlation_id == *corr,
            None => true,
        })
        .filter(|entry| match &filter.level {
            Some(level) => entry.level == *level,
            None => true,
        })
        .filter(|entry| match filter.since {
            Some(since) => entry.timestamp >= since,
            None => true,
        })
        .filter(|entry| match filter.until {
            Some(until) => entry.timestamp <= until,
            None => true,
        })
        .map(|entry| stored_to_public(entry, key_ref))
        .collect();

    results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
    if let Some(limit) = filter.limit {
        if results.len() > limit {
            results.truncate(limit);
        }
    }

    #[cfg(feature = "telemetry")]
    {
        if filter
            .correlation
            .as_ref()
            .map(|c| !c.is_empty())
            .unwrap_or(false)
            && results.is_empty()
        {
            crate::telemetry::record_log_correlation_fail();
        }
    }

    Ok(results)
}

fn stored_to_public(entry: StoredEntry, key: Option<&[u8; CHACHA20_POLY1305_KEY_LEN]>) -> LogEntry {
    let message = if entry.encrypted {
        if let (Some(key), Some(nonce)) = (key, entry.nonce.as_ref()) {
            decrypt_message(key, &entry.message, nonce).unwrap_or_else(|| "<decrypt-failed>".into())
        } else {
            "<encrypted>".into()
        }
    } else {
        entry.message
    };

    LogEntry {
        id: Some(entry.id),
        timestamp: entry.timestamp,
        level: entry.level,
        message,
        correlation_id: entry.correlation_id,
        peer: entry.peer,
        tx: entry.tx,
        block: entry.block,
    }
}

fn prepare_store_path(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if path.exists() {
        if path.is_dir() {
            return Ok((path.to_path_buf(), None));
        }
        #[cfg(not(feature = "sqlite-migration"))]
        {
            return Err(LogIndexerError::MigrationRequired(path.to_path_buf()));
        }
        #[cfg(feature = "sqlite-migration")]
        {
            let legacy = rename_legacy_file(path)?;
            fs::create_dir_all(path)?;
            return Ok((path.to_path_buf(), Some(legacy)));
        }
    }
    fs::create_dir_all(path)?;
    Ok((path.to_path_buf(), None))
}

#[cfg(feature = "sqlite-migration")]
fn rename_legacy_file(path: &Path) -> Result<PathBuf> {
    let mut candidate = path.with_extension("sqlite");
    let mut counter = 0u32;
    while candidate.exists() {
        counter += 1;
        candidate = path.with_extension(format!("sqlite.{counter}"));
    }
    fs::rename(path, &candidate)?;
    Ok(candidate)
}

#[cfg(feature = "sqlite-migration")]
fn migrate_sqlite(store: &LogStore, legacy_path: &Path) -> Result<()> {
    let conn = Connection::open(legacy_path)?;
    ensure_schema(&conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce FROM logs ORDER BY id ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut max_id = store.load_next_id()?;
    while let Some(row) = rows.next()? {
        let entry = row_to_stored_entry(row)?;
        max_id = max_id.max(entry.id);
        store.store_entry(&entry)?;
    }
    let mut offset_stmt = conn.prepare("SELECT source, offset, updated_at FROM ingest_state")?;
    let mut offset_rows = offset_stmt.query([])?;
    while let Some(row) = offset_rows.next()? {
        let source: String = row.get(0)?;
        let offset: i64 = row.get(1)?;
        let updated_at: i64 = row.get(2)?;
        store.save_offset_with_timestamp(
            &source,
            offset.max(0) as u64,
            updated_at.max(0) as u64,
        )?;
    }
    store.save_next_id(max_id)?;
    store.flush()?;
    Ok(())
}

#[cfg(not(feature = "sqlite-migration"))]
fn migrate_sqlite(_store: &LogStore, _legacy_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(feature = "sqlite-migration")]
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

#[cfg(feature = "sqlite-migration")]
fn row_to_stored_entry(row: &Row<'_>) -> Result<StoredEntry> {
    let encrypted: i64 = row.get("encrypted")?;
    let nonce: Option<Vec<u8>> = row.get("nonce")?;
    let message: String = row.get("message")?;
    Ok(StoredEntry {
        id: row
            .get::<_, Option<i64>>("id")?
            .map(|v| v.max(0) as u64)
            .unwrap_or_default(),
        timestamp: row.get("timestamp")?,
        level: row.get("level")?,
        message,
        correlation_id: row.get("correlation_id")?,
        peer: row.get("peer")?,
        tx: row.get("tx")?,
        block: row.get::<_, Option<i64>>("block")?.map(|v| v as u64),
        encrypted: encrypted == 1,
        nonce,
    })
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
