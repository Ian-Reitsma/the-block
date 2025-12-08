#![forbid(unsafe_code)]

use base64_fp::{decode_standard, encode_standard};
use coding::{
    decrypt_xchacha20_poly1305, encrypt_xchacha20_poly1305, ChaCha20Poly1305Encryptor, Encryptor,
    CHACHA20_POLY1305_KEY_LEN, CHACHA20_POLY1305_NONCE_LEN, XCHACHA20_POLY1305_NONCE_LEN,
};
use crypto_suite::hashing::blake3::derive_key;
use foundation_serialization::{
    json,
    serde::{Deserialize, Serialize},
    Error as SerializationError,
};
use sled::{self, Db, Tree};
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[cfg(feature = "sqlite-migration")]
use foundation_sqlite::{params, Connection, Row};

const ENTRIES_TREE: &str = "entries";
const META_TREE: &str = "meta";
const NEXT_ID_KEY: &str = "next_id";
const OFFSET_PREFIX: &str = "offset:";

pub type Result<T> = std::result::Result<T, LogIndexError>;

#[derive(Debug, Error)]
pub enum LogIndexError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("storage error: {0}")]
    Storage(#[from] sled::Error),
    #[error("json error: {0}")]
    Json(#[from] SerializationError),
    #[error("encryption error: {0}")]
    Encryption(String),
    #[error("log index migration required: {0:?}")]
    MigrationRequired(PathBuf),
    #[cfg(feature = "sqlite-migration")]
    #[error("sqlite error: {0}")]
    Sqlite(#[from] foundation_sqlite::Error),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(crate = "foundation_serialization::serde")]
pub struct LogEntry {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub id: Option<u64>,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub correlation_id: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub peer: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub tx: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
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
pub struct StoredEntry {
    pub id: u64,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    pub correlation_id: String,
    pub peer: Option<String>,
    pub tx: Option<String>,
    pub block: Option<u64>,
    pub encrypted: bool,
    pub nonce: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(not(feature = "sqlite-migration"), allow(dead_code))]
#[serde(crate = "foundation_serialization::serde")]
struct IngestState {
    offset: u64,
    updated_at: u64,
}

#[derive(Clone)]
pub struct LogStore {
    db: Db,
    entries: Tree,
    meta: Tree,
}

impl LogStore {
    pub fn open(path: &Path) -> Result<Self> {
        let (store_path, legacy_sqlite) = prepare_store_path(path)?;
        #[cfg(not(feature = "sqlite-migration"))]
        let _ = legacy_sqlite;
        let db = sled::open(&store_path)?;
        let entries = db.open_tree(ENTRIES_TREE)?;
        let meta = db.open_tree(META_TREE)?;
        let store = Self { db, entries, meta };
        #[cfg(feature = "sqlite-migration")]
        if let Some(legacy) = legacy_sqlite {
            migrate_sqlite(&store, &legacy)?;
        }
        Ok(store)
    }

    pub fn load_next_id(&self) -> Result<u64> {
        if let Some(value) = self.meta.get(NEXT_ID_KEY)? {
            Ok(bytes_to_u64(&value))
        } else {
            Ok(0)
        }
    }

    pub fn save_next_id(&self, id: u64) -> Result<()> {
        self.meta
            .insert(NEXT_ID_KEY, id.to_be_bytes().to_vec())?
            .map(|_| ());
        Ok(())
    }

    pub fn load_offset(&self, source: &str) -> Result<u64> {
        let key = format!("{OFFSET_PREFIX}{source}");
        match self.meta.get(key.as_bytes())? {
            Some(value) => {
                let state: IngestState = json::from_slice(&value)?;
                Ok(state.offset)
            }
            None => Ok(0),
        }
    }

    pub fn save_offset(&self, source: &str, offset: u64) -> Result<()> {
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
    pub fn save_offset_with_timestamp(
        &self,
        source: &str,
        offset: u64,
        updated_at: u64,
    ) -> Result<()> {
        let key = format!("{OFFSET_PREFIX}{source}");
        let state = IngestState { offset, updated_at };
        self.meta
            .insert(key.as_bytes(), json::to_vec(&state)?)?
            .map(|_| ());
        Ok(())
    }

    pub fn store_entry(&self, entry: &StoredEntry) -> Result<()> {
        let key = entry_key(entry.id);
        self.entries
            .insert(key.as_bytes(), json::to_vec(entry)?)?
            .map(|_| ());
        Ok(())
    }

    pub fn store_entries_with_rollback(
        &self,
        updated: &[StoredEntry],
        original: &[StoredEntry],
    ) -> Result<()> {
        for entry in updated {
            if let Err(err) = self.store_entry(entry) {
                for original_entry in original {
                    let _ = self.store_entry(original_entry);
                }
                let _ = self.flush();
                return Err(err);
            }
        }
        Ok(())
    }

    pub fn load_entries(&self) -> Result<Vec<StoredEntry>> {
        let mut items = Vec::new();
        for result in self.entries.iter() {
            let (_, value) = result?;
            let entry: StoredEntry = json::from_slice(&value)?;
            items.push(entry);
        }
        Ok(items)
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
}

pub fn search_logs_in_store(store: &LogStore, filter: &LogFilter) -> Result<Vec<LogEntry>> {
    let entries = store.load_entries()?;
    let key = filter
        .passphrase
        .as_ref()
        .map(|pass| derive_encryption_key(pass));
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
    Ok(results)
}

pub fn search_logs(path: &Path, filter: &LogFilter) -> Result<Vec<LogEntry>> {
    let store = LogStore::open(path)?;
    search_logs_in_store(&store, filter)
}

pub fn derive_encryption_key(passphrase: &str) -> [u8; CHACHA20_POLY1305_KEY_LEN] {
    derive_key("the-block-log-indexer", passphrase.as_bytes())
}

pub fn encrypt_message(
    key: &[u8; CHACHA20_POLY1305_KEY_LEN],
    message: &str,
) -> Result<(String, Vec<u8>)> {
    let payload = encrypt_xchacha20_poly1305(key, message.as_bytes())
        .map_err(|e| LogIndexError::Encryption(format!("encrypt: {e}")))?;
    let (nonce, body) = payload.split_at(XCHACHA20_POLY1305_NONCE_LEN);
    Ok((encode_standard(body), nonce.to_vec()))
}

pub fn decrypt_message(
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

pub fn stored_to_public(
    entry: StoredEntry,
    key: Option<&[u8; CHACHA20_POLY1305_KEY_LEN]>,
) -> LogEntry {
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

pub fn entry_key(id: u64) -> String {
    format!("entry:{id:016x}")
}

pub fn bytes_to_u64(value: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let len = value.len().min(8);
    buf[8 - len..].copy_from_slice(&value[value.len() - len..]);
    u64::from_be_bytes(buf)
}

pub fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn prepare_store_path(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if path.exists() {
        if path.is_dir() {
            return Ok((path.to_path_buf(), None));
        }
        #[cfg(not(feature = "sqlite-migration"))]
        {
            return Err(LogIndexError::MigrationRequired(path.to_path_buf()));
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
    let mut conn = Connection::open(legacy_path)?;
    let tx = conn.transaction()?;
    let mut stmt = tx.prepare(
        "SELECT id, timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce \
         FROM logs ORDER BY id",
    )?;
    let rows = stmt.query_map(params![], |row| row_to_stored_entry(row))?;
    for row in rows {
        let entry = row?;
        store.store_entry(&entry)?;
    }
    let mut offset_stmt = tx.prepare("SELECT source, offset, updated_at FROM ingest_offsets")?;
    let offsets = offset_stmt.query_map(params![], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)? as u64,
            row.get::<_, i64>(2)? as u64,
        ))
    })?;
    for offset in offsets {
        let (source, value, updated_at) = offset?;
        store.save_offset_with_timestamp(&source, value, updated_at)?;
    }
    store.flush()?;
    drop(stmt);
    drop(offset_stmt);
    tx.commit()?;
    Ok(())
}

#[cfg(feature = "sqlite-migration")]
fn row_to_stored_entry(row: &Row) -> foundation_sqlite::Result<StoredEntry> {
    Ok(StoredEntry {
        id: row.get::<_, i64>("id")?.max(0) as u64,
        timestamp: row.get("timestamp")?,
        level: row.get("level")?,
        message: row.get("message")?,
        correlation_id: row.get("correlation_id")?,
        peer: row.get("peer")?,
        tx: row.get("tx")?,
        block: row.get::<_, Option<i64>>("block")?.map(|v| v as u64),
        encrypted: row.get::<_, i64>("encrypted")? != 0,
        nonce: row.get("nonce")?,
    })
}

#[cfg(feature = "sqlite-migration")]
pub fn migrate_sqlite_file(path: &Path, store: &LogStore) -> Result<()> {
    migrate_sqlite(store, path)
}

fn ingest_reader_internal<R: BufRead, F>(
    reader: R,
    source: &str,
    options: &IndexOptions,
    store: &LogStore,
    mut observer: F,
) -> Result<()>
where
    F: FnMut(&StoredEntry),
{
    let mut next_id = store.load_next_id()?;
    let mut offset = store.load_offset(source)?;
    let key = options
        .passphrase
        .as_ref()
        .map(|pass| derive_encryption_key(pass));
    for line in reader.lines() {
        let line = line?;
        offset = offset.saturating_add(line.len() as u64 + 1);
        if line.trim().is_empty() {
            continue;
        }
        let entry: LogEntry = json::from_str(&line)?;
        let id = next_id;
        next_id += 1;
        let (message, encrypted, nonce) = if let Some(key) = key.as_ref() {
            let (cipher, nonce) = encrypt_message(key, &entry.message)?;
            (cipher, true, Some(nonce))
        } else {
            (entry.message.clone(), false, None)
        };
        let stored = StoredEntry {
            id,
            timestamp: entry.timestamp,
            level: entry.level.clone(),
            message,
            correlation_id: entry.correlation_id.clone(),
            peer: entry.peer.clone(),
            tx: entry.tx.clone(),
            block: entry.block,
            encrypted,
            nonce,
        };
        store.store_entry(&stored)?;
        observer(&stored);
    }
    store.save_next_id(next_id)?;
    store.save_offset(source, offset)?;
    store.flush()?;
    Ok(())
}

pub fn ingest_reader<R: BufRead>(
    reader: R,
    source: &str,
    options: &IndexOptions,
    store: &LogStore,
) -> Result<()> {
    ingest_reader_internal(reader, source, options, store, |_| {})
}

pub fn ingest_reader_with_observer<R: BufRead, F>(
    reader: R,
    source: &str,
    options: &IndexOptions,
    store: &LogStore,
    observer: F,
) -> Result<()>
where
    F: FnMut(&StoredEntry),
{
    ingest_reader_internal(reader, source, options, store, observer)
}

pub fn ingest_file(path: &Path, options: &IndexOptions, store: &LogStore) -> Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    ingest_reader(reader, path.to_string_lossy().as_ref(), options, store)
}

pub fn ingest_file_with_observer<F>(
    path: &Path,
    options: &IndexOptions,
    store: &LogStore,
    observer: F,
) -> Result<()>
where
    F: FnMut(&StoredEntry),
{
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    ingest_reader_internal(
        reader,
        path.to_string_lossy().as_ref(),
        options,
        store,
        observer,
    )
}

pub fn ingest_with_seek(
    file: &mut File,
    source: &str,
    options: &IndexOptions,
    store: &LogStore,
) -> Result<()> {
    ingest_with_seek_and_observer(file, source, options, store, |_| {})
}

pub fn ingest_with_seek_and_observer<F>(
    file: &mut File,
    source: &str,
    options: &IndexOptions,
    store: &LogStore,
    observer: F,
) -> Result<()>
where
    F: FnMut(&StoredEntry),
{
    let offset = store.load_offset(source)?;
    file.seek(SeekFrom::Start(offset))?;
    let reader = BufReader::new(file);
    ingest_reader_internal(reader, source, options, store, observer)
}

pub fn rotate_key(store: &LogStore, current: Option<&str>, new_passphrase: &str) -> Result<()> {
    let old_key = current.map(derive_encryption_key);
    let new_key = derive_encryption_key(new_passphrase);
    let entries = store.load_entries()?;
    let originals = entries.clone();

    let mut staged: Vec<(StoredEntry, String)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let plaintext = if entry.encrypted {
            let key = old_key
                .as_ref()
                .ok_or_else(|| LogIndexError::Encryption("missing current passphrase".into()))?;
            let nonce = entry
                .nonce
                .as_deref()
                .ok_or_else(|| LogIndexError::Encryption("missing nonce".into()))?;
            decrypt_message(key, &entry.message, nonce)
                .ok_or_else(|| LogIndexError::Encryption("decrypt failed".into()))?
        } else {
            entry.message.clone()
        };
        staged.push((entry, plaintext));
    }

    let mut updated = Vec::with_capacity(staged.len());
    for (mut entry, plaintext) in staged {
        let (cipher, nonce) = encrypt_message(&new_key, &plaintext)?;
        entry.message = cipher;
        entry.nonce = Some(nonce);
        entry.encrypted = true;
        updated.push(entry);
    }

    store.store_entries_with_rollback(&updated, &originals)?;
    store.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn escape_str(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len());
        for ch in value.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                other => escaped.push(other),
            }
        }
        escaped
    }

    fn encode_entry(entry: &LogEntry) -> String {
        fn field(name: &str, value: String) -> String {
            format!("\"{name}\":{value}")
        }

        let mut parts = Vec::new();
        if let Some(id) = entry.id {
            parts.push(field("id", id.to_string()));
        }
        parts.push(field("timestamp", entry.timestamp.to_string()));
        parts.push(field("level", format!("\"{}\"", escape_str(&entry.level))));
        parts.push(field(
            "message",
            format!("\"{}\"", escape_str(&entry.message)),
        ));
        parts.push(field(
            "correlation_id",
            format!("\"{}\"", escape_str(&entry.correlation_id)),
        ));
        match &entry.peer {
            Some(peer) => parts.push(field("peer", format!("\"{}\"", escape_str(peer)))),
            None => parts.push(field("peer", "null".into())),
        }
        match &entry.tx {
            Some(tx) => parts.push(field("tx", format!("\"{}\"", escape_str(tx)))),
            None => parts.push(field("tx", "null".into())),
        }
        match entry.block {
            Some(block) => parts.push(field("block", block.to_string())),
            None => parts.push(field("block", "null".into())),
        }
        format!("{{{}}}", parts.join(","))
    }

    fn json_backend_available() -> bool {
        if foundation_serialization::json::from_str::<u64>("0").is_err() {
            return false;
        }
        let probe = LogEntry {
            id: None,
            timestamp: 0,
            level: String::new(),
            message: String::new(),
            correlation_id: String::new(),
            peer: None,
            tx: None,
            block: None,
        };
        match foundation_serialization::json::to_string(&probe) {
            Ok(serialized) => {
                foundation_serialization::json::from_str::<LogEntry>(&serialized).is_ok()
            }
            Err(_) => false,
        }
    }

    fn temp_store_path(suffix: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "the_block_log_index_tests_{}_{}_{}",
            std::process::id(),
            id,
            suffix
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp store directory");
        path
    }

    #[test]
    fn ingest_and_search_plaintext() {
        if !json_backend_available() {
            eprintln!("skipping ingest_and_search_plaintext: foundation_serde stub backend active");
            return;
        }
        let path = temp_store_path("plaintext");
        let store = LogStore::open(&path).expect("open store");
        let log_lines = [
            LogEntry {
                id: None,
                timestamp: 100,
                level: "INFO".into(),
                message: "first".into(),
                correlation_id: "corr-a".into(),
                peer: Some("peer-a".into()),
                tx: Some("tx-a".into()),
                block: Some(42),
            },
            LogEntry {
                id: None,
                timestamp: 200,
                level: "ERROR".into(),
                message: "second".into(),
                correlation_id: "corr-b".into(),
                peer: Some("peer-b".into()),
                tx: None,
                block: None,
            },
        ];
        let payload = log_lines
            .iter()
            .map(encode_entry)
            .collect::<Vec<_>>()
            .join("\n");
        ingest_reader(
            Cursor::new(payload.as_bytes()),
            "unit-test",
            &IndexOptions::default(),
            &store,
        )
        .expect("ingest logs");

        let mut filter = LogFilter {
            limit: Some(10),
            ..Default::default()
        };
        let mut results = search_logs_in_store(&store, &filter).expect("search logs");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].message, "second");
        assert_eq!(results[1].message, "first");

        filter.correlation = Some("corr-a".into());
        results = search_logs_in_store(&store, &filter).expect("search filtered");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].correlation_id, "corr-a");

        fs::remove_dir_all(path).expect("cleanup temp store");
    }

    #[test]
    fn ingest_and_search_encrypted() {
        if !json_backend_available() {
            eprintln!("skipping ingest_and_search_encrypted: foundation_serde stub backend active");
            return;
        }
        let path = temp_store_path("encrypted");
        let store = LogStore::open(&path).expect("open store");
        let entry = LogEntry {
            id: None,
            timestamp: 300,
            level: "WARN".into(),
            message: "secret".into(),
            correlation_id: "corr-secret".into(),
            peer: None,
            tx: None,
            block: None,
        };
        let payload = encode_entry(&entry);
        let mut options = IndexOptions::default();
        options.passphrase = Some("pass".into());
        ingest_reader(
            Cursor::new(format!("{payload}\n").into_bytes()),
            "encrypted",
            &options,
            &store,
        )
        .expect("ingest encrypted");

        let mut filter = LogFilter {
            correlation: Some("corr-secret".into()),
            ..Default::default()
        };
        let mut results = search_logs_in_store(&store, &filter).expect("search logs");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "<encrypted>");

        filter.passphrase = Some("pass".into());
        results = search_logs_in_store(&store, &filter).expect("search decrypted");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "secret");

        fs::remove_dir_all(path).expect("cleanup temp store");
    }

    #[test]
    fn rotate_key_reencrypts_entries() {
        if !json_backend_available() {
            eprintln!(
                "skipping rotate_key_reencrypts_entries: foundation_serde stub backend active"
            );
            return;
        }
        let path = temp_store_path("rotate");
        let store = LogStore::open(&path).expect("open store");
        let entry = LogEntry {
            id: None,
            timestamp: 400,
            level: "INFO".into(),
            message: "rotate".into(),
            correlation_id: "corr-rotate".into(),
            peer: None,
            tx: None,
            block: None,
        };
        let payload = encode_entry(&entry);
        let mut options = IndexOptions::default();
        options.passphrase = Some("old".into());
        ingest_reader(
            Cursor::new(format!("{payload}\n").into_bytes()),
            "rotate",
            &options,
            &store,
        )
        .expect("ingest encrypted");

        rotate_key(&store, Some("old"), "new").expect("rotate key");

        let mut filter = LogFilter {
            correlation: Some("corr-rotate".into()),
            passphrase: Some("new".into()),
            ..Default::default()
        };
        let results = search_logs_in_store(&store, &filter).expect("search with new key");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "rotate");

        filter.passphrase = Some("old".into());
        let results = search_logs_in_store(&store, &filter).expect("search with old key");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "<decrypt-failed>");

        fs::remove_dir_all(path).expect("cleanup temp store");
    }

    #[test]
    fn rotate_key_is_atomic_on_failure() {
        if !json_backend_available() {
            eprintln!(
                "skipping rotate_key_is_atomic_on_failure: foundation_serde stub backend active"
            );
            return;
        }
        let path = temp_store_path("rotate-failure");
        let store = LogStore::open(&path).expect("open store");
        let entry = LogEntry {
            id: None,
            timestamp: 500,
            level: "INFO".into(),
            message: "sealed".into(),
            correlation_id: "corr-failure".into(),
            peer: None,
            tx: None,
            block: None,
        };
        let payload = encode_entry(&entry);
        let mut options = IndexOptions::default();
        options.passphrase = Some("correct".into());
        ingest_reader(
            Cursor::new(format!("{payload}\n").into_bytes()),
            "rotate-failure",
            &options,
            &store,
        )
        .expect("ingest encrypted");

        let err = rotate_key(&store, Some("wrong"), "new").expect_err("rotation should fail");
        match err {
            LogIndexError::Encryption(message) => {
                assert!(message.contains("decrypt") || message.contains("passphrase"))
            }
            other => panic!("unexpected error: {other:?}", other = other),
        }

        let mut filter = LogFilter {
            correlation: Some("corr-failure".into()),
            passphrase: Some("correct".into()),
            ..Default::default()
        };
        let results = search_logs_in_store(&store, &filter).expect("search with original key");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "sealed");

        filter.passphrase = Some("new".into());
        let results = search_logs_in_store(&store, &filter).expect("search with new key");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "<decrypt-failed>");

        fs::remove_dir_all(path).expect("cleanup temp store");
    }
}
