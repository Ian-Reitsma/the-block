use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use coding::{ChaCha20Poly1305Encryptor, Encryptor, CHACHA20_POLY1305_NONCE_LEN};
use concurrency::{mutex, Lazy, MutexGuard, MutexT};
use foundation_serialization::{binary, json, Deserialize, Serialize};
use rand::rngs::OsRng;
use rand::RngCore;

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    MOBILE_CACHE_ENTRY_BYTES, MOBILE_CACHE_ENTRY_TOTAL, MOBILE_CACHE_EVICT_TOTAL,
    MOBILE_CACHE_HIT_TOTAL, MOBILE_CACHE_MISS_TOTAL, MOBILE_CACHE_REJECT_TOTAL,
    MOBILE_CACHE_STALE_TOTAL, MOBILE_TX_QUEUE_DEPTH,
};

#[cfg(feature = "telemetry")]
use crate::telemetry::{MOBILE_CACHE_QUEUE_BYTES, MOBILE_CACHE_QUEUE_TOTAL};

#[cfg(feature = "telemetry")]
use crate::telemetry::{MOBILE_CACHE_SWEEP_TOTAL, MOBILE_CACHE_SWEEP_WINDOW_SECONDS};

#[derive(Debug)]
pub enum MobileCacheError {
    Persistence(sled::Error),
    Serialization(String),
    Encryption,
    Decryption,
    EntryTooLarge { size: usize, limit: usize },
    Capacity { limit: usize },
    QueueCapacity { limit: usize },
    MissingKey,
    InvalidKeyLength,
    LockPoisoned,
}

impl fmt::Display for MobileCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MobileCacheError::Persistence(err) => write!(f, "persistence error: {err}"),
            MobileCacheError::Serialization(err) => write!(f, "serialization error: {err}"),
            MobileCacheError::Encryption => write!(f, "encryption failure"),
            MobileCacheError::Decryption => write!(f, "decryption failure"),
            MobileCacheError::EntryTooLarge { size, limit } => {
                write!(f, "entry exceeds max payload ({size} > {limit} bytes)")
            }
            MobileCacheError::Capacity { limit } => {
                write!(f, "cache capacity reached ({limit} entries)")
            }
            MobileCacheError::QueueCapacity { limit } => {
                write!(f, "queue capacity reached ({limit} entries)")
            }
            MobileCacheError::MissingKey => write!(
                f,
                "missing encryption key (set TB_MOBILE_CACHE_KEY_HEX or TB_NODE_KEY_HEX)"
            ),
            MobileCacheError::InvalidKeyLength => write!(f, "invalid encryption key length"),
            MobileCacheError::LockPoisoned => write!(f, "cache lock poisoned"),
        }
    }
}

impl std::error::Error for MobileCacheError {}

impl From<sled::Error> for MobileCacheError {
    fn from(value: sled::Error) -> Self {
        MobileCacheError::Persistence(value)
    }
}

#[derive(Clone)]
pub struct MobileCacheConfig {
    pub ttl: Duration,
    pub sweep_interval: Duration,
    pub max_entries: usize,
    pub max_payload_bytes: usize,
    pub max_queue: usize,
    pub db_path: PathBuf,
    pub encryption_key: [u8; 32],
    pub temporary: bool,
}

impl MobileCacheConfig {
    pub fn from_env() -> Result<Self, MobileCacheError> {
        let ttl = env_duration("TB_MOBILE_CACHE_TTL_SECS", 300);
        let sweep_interval = env_duration("TB_MOBILE_CACHE_SWEEP_SECS", 30);
        let max_entries = env_usize("TB_MOBILE_CACHE_MAX_ENTRIES", 512);
        let max_payload_bytes = env_usize("TB_MOBILE_CACHE_MAX_BYTES", 64 * 1024);
        let max_queue = env_usize("TB_MOBILE_CACHE_MAX_QUEUE", 256);
        let db_path = std::env::var("TB_MOBILE_CACHE_DB")
            .unwrap_or_else(|_| "mobile_cache.db".into())
            .into();
        let key_hex = std::env::var("TB_MOBILE_CACHE_KEY_HEX")
            .or_else(|_| std::env::var("TB_NODE_KEY_HEX"))
            .map_err(|_| MobileCacheError::MissingKey)?;
        let key_bytes =
            hex::decode(key_hex.trim()).map_err(|_| MobileCacheError::InvalidKeyLength)?;
        let encryption_key: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| MobileCacheError::InvalidKeyLength)?;
        Ok(Self {
            ttl,
            sweep_interval,
            max_entries,
            max_payload_bytes,
            max_queue,
            db_path,
            encryption_key,
            temporary: false,
        })
    }

    pub fn ephemeral(path: &Path, ttl: Duration, key: [u8; 32]) -> Self {
        Self {
            ttl,
            sweep_interval: Duration::from_millis(10),
            max_entries: 64,
            max_payload_bytes: 8 * 1024,
            max_queue: 32,
            db_path: path.to_path_buf(),
            encryption_key: key,
            temporary: true,
        }
    }
}

fn env_duration(key: &str, default_secs: u64) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(default_secs))
}

fn env_usize(key: &str, default_val: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default_val)
}

#[derive(Serialize, Deserialize)]
struct PersistedResponse {
    stored_at: u64,
    expires_at: u64,
    value: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct PersistedQueueItem {
    id: u64,
    enqueued_at: u64,
    value: Vec<u8>,
}

struct CacheEntry {
    value: String,
    stored_at_system: SystemTime,
    expires_at_instant: Instant,
    expires_at_system: SystemTime,
    token: u64,
}

struct QueueItem {
    id: u64,
    value: String,
    enqueued_at: SystemTime,
}

#[derive(Clone)]
struct Expiry {
    when: Instant,
    token: u64,
    key: String,
}

impl PartialEq for Expiry {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token && self.key == other.key
    }
}

impl Eq for Expiry {}

impl PartialOrd for Expiry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Expiry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.when.cmp(&other.when)
    }
}

#[derive(Serialize)]
pub struct CacheStatus {
    pub totals: CacheTotals,
    pub config: CacheStatusConfig,
    pub entries: Vec<CacheStatusEntry>,
    pub queue: CacheQueueStatus,
}

#[derive(Serialize)]
pub struct CacheTotals {
    pub hits: u64,
    pub misses: u64,
    pub stale_evictions: u64,
    pub rejections: u64,
    pub entry_count: usize,
    pub entry_bytes: usize,
}

#[derive(Serialize)]
pub struct CacheStatusConfig {
    pub ttl_secs: u64,
    pub sweep_interval_secs: u64,
    pub max_entries: usize,
    pub max_payload_bytes: usize,
    pub max_queue: usize,
    pub db_path: String,
}

#[derive(Serialize)]
pub struct CacheStatusEntry {
    pub key: String,
    pub age_secs: u64,
    pub expires_in_secs: u64,
    pub size_bytes: usize,
}

#[derive(Serialize)]
pub struct CacheQueueStatus {
    pub depth: usize,
    pub max: usize,
    pub bytes: usize,
    pub oldest_age_secs: Option<u64>,
}

pub struct MobileCache {
    config: MobileCacheConfig,
    cipher: ChaCha20Poly1305Encryptor,
    db: sled::Db,
    responses: sled::Tree,
    queue_tree: sled::Tree,
    store: HashMap<String, CacheEntry>,
    expirations: BinaryHeap<Reverse<Expiry>>,
    queue: VecDeque<QueueItem>,
    next_id: u64,
    next_queue_id: u64,
    total_payload_bytes: usize,
    queue_payload_bytes: usize,
    hits: u64,
    misses: u64,
    stale_evictions: u64,
    rejections: u64,
    next_sweep: Instant,
}

impl MobileCache {
    pub fn open(config: MobileCacheConfig) -> Result<Self, MobileCacheError> {
        let cipher = ChaCha20Poly1305Encryptor::new(&config.encryption_key)
            .map_err(|_| MobileCacheError::InvalidKeyLength)?;
        let mut sled_cfg = sled::Config::default().path(&config.db_path);
        if config.temporary {
            sled_cfg = sled_cfg.temporary(true);
        }
        let db = sled_cfg.open()?;
        let responses = db.open_tree("responses")?;
        let queue_tree = db.open_tree("queue")?;

        let mut cache = Self {
            cipher,
            db,
            responses,
            queue_tree,
            store: HashMap::new(),
            expirations: BinaryHeap::new(),
            queue: VecDeque::new(),
            next_id: 0,
            next_queue_id: 0,
            total_payload_bytes: 0,
            queue_payload_bytes: 0,
            hits: 0,
            misses: 0,
            stale_evictions: 0,
            rejections: 0,
            next_sweep: Instant::now() + config.sweep_interval,
            config,
        };
        cache.load_responses()?;
        cache.load_queue()?;
        cache.update_entry_gauges();
        cache.update_queue_gauges();
        Ok(cache)
    }

    fn load_responses(&mut self) -> Result<(), MobileCacheError> {
        let now_sys = SystemTime::now();
        let now_inst = Instant::now();
        let mut evicted = 0usize;
        for item in self.responses.iter() {
            let (key_bytes, val_bytes) = item?;
            let key = String::from_utf8(key_bytes.to_vec())
                .map_err(|e| MobileCacheError::Serialization(e.to_string()))?;
            match self.deserialize_response(&val_bytes) {
                Ok(record) => {
                    let stored_at = UNIX_EPOCH + Duration::from_secs(record.stored_at);
                    let expires_at = UNIX_EPOCH + Duration::from_secs(record.expires_at);
                    if expires_at <= now_sys {
                        evicted += 1;
                        let _ = self.responses.remove(key_bytes);
                        continue;
                    }
                    let remaining = expires_at
                        .duration_since(now_sys)
                        .unwrap_or_else(|_| Duration::from_secs(0));
                    let expires_at_instant = now_inst + remaining;
                    let entry = CacheEntry {
                        value: String::from_utf8(record.value)
                            .map_err(|e| MobileCacheError::Serialization(e.to_string()))?,
                        stored_at_system: stored_at,
                        expires_at_instant,
                        expires_at_system: expires_at,
                        token: self.next_id,
                    };
                    self.total_payload_bytes += entry.value.len();
                    self.expirations.push(Reverse(Expiry {
                        when: expires_at_instant,
                        token: entry.token,
                        key: key.clone(),
                    }));
                    self.store.insert(key, entry);
                    self.next_id += 1;
                }
                Err(_) => {
                    evicted += 1;
                    let _ = self.responses.remove(key_bytes);
                }
            }
        }
        if evicted > 0 {
            #[cfg(feature = "telemetry")]
            {
                MOBILE_CACHE_STALE_TOTAL.inc_by(evicted as u64);
                MOBILE_CACHE_EVICT_TOTAL.inc_by(evicted as u64);
            }
        }
        Ok(())
    }

    fn load_queue(&mut self) -> Result<(), MobileCacheError> {
        let mut max_id = 0u64;
        for item in self.queue_tree.iter() {
            let (key_bytes, val_bytes) = item?;
            let mut id_bytes = [0u8; 8];
            id_bytes.copy_from_slice(key_bytes.as_ref());
            let id = u64::from_be_bytes(id_bytes);
            match self.deserialize_queue_item(&val_bytes) {
                Ok(record) => {
                    let enqueued = UNIX_EPOCH + Duration::from_secs(record.enqueued_at);
                    let payload = String::from_utf8(record.value)
                        .map_err(|e| MobileCacheError::Serialization(e.to_string()))?;
                    max_id = max_id.max(id);
                    self.queue_payload_bytes += payload.len();
                    self.queue.push_back(QueueItem {
                        id,
                        value: payload,
                        enqueued_at: enqueued,
                    });
                }
                Err(_) => {
                    let _ = self.queue_tree.remove(key_bytes);
                }
            }
        }
        self.queue.make_contiguous().sort_by_key(|item| item.id);
        self.next_queue_id = max_id.saturating_add(1);
        Ok(())
    }

    fn update_entry_gauges(&self) {
        #[cfg(feature = "telemetry")]
        {
            MOBILE_CACHE_ENTRY_TOTAL.set(self.store.len() as i64);
            MOBILE_CACHE_ENTRY_BYTES.set(self.total_payload_bytes as i64);
        }
    }

    fn update_queue_gauges(&self) {
        #[cfg(feature = "telemetry")]
        {
            MOBILE_TX_QUEUE_DEPTH.set(self.queue.len() as i64);
            MOBILE_CACHE_QUEUE_TOTAL.set(self.queue.len() as i64);
            MOBILE_CACHE_QUEUE_BYTES.set(self.queue_payload_bytes as i64);
        }
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, MobileCacheError> {
        self.cipher
            .encrypt(plaintext)
            .map_err(|_| MobileCacheError::Encryption)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, MobileCacheError> {
        if ciphertext.len() <= CHACHA20_POLY1305_NONCE_LEN {
            return Err(MobileCacheError::Decryption);
        }
        self.cipher
            .decrypt(ciphertext)
            .map_err(|_| MobileCacheError::Decryption)
    }

    fn serialize_response(&self, entry: &CacheEntry) -> Result<Vec<u8>, MobileCacheError> {
        let stored_at = entry
            .stored_at_system
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_at = entry
            .expires_at_system
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let payload = PersistedResponse {
            stored_at,
            expires_at,
            value: entry.value.as_bytes().to_vec(),
        };
        let plain =
            binary::encode(&payload).map_err(|e| MobileCacheError::Serialization(e.to_string()))?;
        self.encrypt(&plain)
    }

    fn deserialize_response(&self, data: &[u8]) -> Result<PersistedResponse, MobileCacheError> {
        let plain = self.decrypt(data)?;
        binary::decode(&plain).map_err(|e| MobileCacheError::Serialization(e.to_string()))
    }

    fn serialize_queue_item(&self, item: &QueueItem) -> Result<Vec<u8>, MobileCacheError> {
        let enqueued_at = item
            .enqueued_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let payload = PersistedQueueItem {
            id: item.id,
            enqueued_at,
            value: item.value.as_bytes().to_vec(),
        };
        let plain =
            binary::encode(&payload).map_err(|e| MobileCacheError::Serialization(e.to_string()))?;
        self.encrypt(&plain)
    }

    fn deserialize_queue_item(&self, data: &[u8]) -> Result<PersistedQueueItem, MobileCacheError> {
        let plain = self.decrypt(data)?;
        binary::decode(&plain).map_err(|e| MobileCacheError::Serialization(e.to_string()))
    }

    fn persist_entry(&self, key: &str, entry: &CacheEntry) -> Result<(), MobileCacheError> {
        let encoded = self.serialize_response(entry)?;
        self.responses.insert(key.as_bytes(), encoded)?;
        self.db.flush()?;
        Ok(())
    }

    fn persist_queue_item(&self, item: &QueueItem) -> Result<(), MobileCacheError> {
        let encoded = self.serialize_queue_item(item)?;
        self.queue_tree.insert(item.id.to_be_bytes(), encoded)?;
        self.db.flush()?;
        Ok(())
    }

    fn remove_queue_entry(&self, id: u64) -> Result<(), MobileCacheError> {
        self.queue_tree.remove(id.to_be_bytes())?;
        self.db.flush()?;
        Ok(())
    }

    fn sweep_if_needed(&mut self) -> Result<(), MobileCacheError> {
        let now = Instant::now();
        if now < self.next_sweep {
            return Ok(());
        }
        self.next_sweep = now + self.config.sweep_interval;
        #[cfg(feature = "telemetry")]
        {
            MOBILE_CACHE_SWEEP_TOTAL.inc();
            MOBILE_CACHE_SWEEP_WINDOW_SECONDS.set(self.config.sweep_interval.as_secs() as i64);
        }
        let mut removed = 0u64;
        let mut dirty = false;
        while let Some(Reverse(expiry)) = self.expirations.peek() {
            if expiry.when > now {
                break;
            }
            let Reverse(expiry) = self.expirations.pop().unwrap();
            let should_remove = match self.store.get(&expiry.key) {
                Some(entry) if entry.token == expiry.token => entry.expires_at_instant <= now,
                _ => false,
            };
            if should_remove {
                if let Some(entry) = self.store.remove(&expiry.key) {
                    self.total_payload_bytes =
                        self.total_payload_bytes.saturating_sub(entry.value.len());
                    self.responses.remove(expiry.key.as_bytes())?;
                    removed += 1;
                    dirty = true;
                }
            }
        }
        if dirty {
            self.db.flush()?;
        }
        if removed > 0 {
            self.stale_evictions += removed;
            #[cfg(feature = "telemetry")]
            {
                MOBILE_CACHE_STALE_TOTAL.inc_by(removed);
                MOBILE_CACHE_EVICT_TOTAL.inc_by(removed);
            }
        }
        self.update_entry_gauges();
        Ok(())
    }

    pub fn get(&mut self, key: &str) -> Result<Option<String>, MobileCacheError> {
        self.sweep_if_needed()?;
        match self.store.get(key) {
            Some(entry) if entry.expires_at_instant > Instant::now() => {
                self.hits += 1;
                #[cfg(feature = "telemetry")]
                {
                    MOBILE_CACHE_HIT_TOTAL.inc();
                }
                Ok(Some(entry.value.clone()))
            }
            Some(_) => {
                if let Some(entry) = self.store.remove(key) {
                    self.total_payload_bytes =
                        self.total_payload_bytes.saturating_sub(entry.value.len());
                }
                self.responses.remove(key.as_bytes())?;
                self.db.flush()?;
                self.update_entry_gauges();
                self.stale_evictions += 1;
                #[cfg(feature = "telemetry")]
                {
                    MOBILE_CACHE_STALE_TOTAL.inc();
                    MOBILE_CACHE_EVICT_TOTAL.inc();
                }
                Ok(None)
            }
            None => {
                self.misses += 1;
                #[cfg(feature = "telemetry")]
                {
                    MOBILE_CACHE_MISS_TOTAL.inc();
                }
                Ok(None)
            }
        }
    }

    pub fn insert(&mut self, key: String, value: String) -> Result<(), MobileCacheError> {
        self.sweep_if_needed()?;
        let size = value.len();
        if size > self.config.max_payload_bytes {
            self.rejections += 1;
            #[cfg(feature = "telemetry")]
            {
                MOBILE_CACHE_REJECT_TOTAL.inc();
            }
            return Err(MobileCacheError::EntryTooLarge {
                size,
                limit: self.config.max_payload_bytes,
            });
        }
        if !self.store.contains_key(&key) && self.store.len() >= self.config.max_entries {
            self.rejections += 1;
            #[cfg(feature = "telemetry")]
            {
                MOBILE_CACHE_REJECT_TOTAL.inc();
            }
            return Err(MobileCacheError::Capacity {
                limit: self.config.max_entries,
            });
        }
        let now_inst = Instant::now();
        let now_sys = SystemTime::now();
        let expires_inst = now_inst + self.config.ttl;
        let expires_sys = now_sys + self.config.ttl;
        let entry = CacheEntry {
            value,
            stored_at_system: now_sys,
            expires_at_instant: expires_inst,
            expires_at_system: expires_sys,
            token: self.next_id,
        };
        self.persist_entry(&key, &entry)?;
        if let Some(old) = self.store.insert(key.clone(), entry) {
            self.total_payload_bytes = self.total_payload_bytes.saturating_sub(old.value.len());
        }
        self.total_payload_bytes += size;
        self.expirations.push(Reverse(Expiry {
            when: expires_inst,
            token: self.next_id,
            key,
        }));
        self.next_id = self.next_id.wrapping_add(1);
        self.update_entry_gauges();
        Ok(())
    }

    pub fn invalidate(&mut self, key: &str) -> Result<bool, MobileCacheError> {
        let existed = if let Some(entry) = self.store.remove(key) {
            self.total_payload_bytes = self.total_payload_bytes.saturating_sub(entry.value.len());
            self.responses.remove(key.as_bytes())?;
            true
        } else {
            false
        };
        if existed {
            self.db.flush()?;
            self.update_entry_gauges();
        }
        Ok(existed)
    }

    pub fn invalidate_prefix(&mut self, prefix: &str) -> Result<usize, MobileCacheError> {
        let keys: Vec<String> = self
            .store
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        let mut removed = 0usize;
        for key in keys {
            if self.invalidate(&key)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn queue_tx(&mut self, tx: String) -> Result<(), MobileCacheError> {
        self.sweep_if_needed()?;
        if self.queue.len() >= self.config.max_queue {
            self.rejections += 1;
            #[cfg(feature = "telemetry")]
            {
                MOBILE_CACHE_REJECT_TOTAL.inc();
            }
            return Err(MobileCacheError::QueueCapacity {
                limit: self.config.max_queue,
            });
        }
        if tx.len() > self.config.max_payload_bytes {
            self.rejections += 1;
            #[cfg(feature = "telemetry")]
            MOBILE_CACHE_REJECT_TOTAL.inc();
            return Err(MobileCacheError::EntryTooLarge {
                size: tx.len(),
                limit: self.config.max_payload_bytes,
            });
        }
        let item = QueueItem {
            id: self.next_queue_id,
            value: tx,
            enqueued_at: SystemTime::now(),
        };
        self.persist_queue_item(&item)?;
        self.queue_payload_bytes += item.value.len();
        self.queue.push_back(item);
        self.next_queue_id = self.next_queue_id.wrapping_add(1);
        self.update_queue_gauges();
        Ok(())
    }

    pub fn drain_queue<F>(&mut self, mut send: F) -> Result<usize, MobileCacheError>
    where
        F: FnMut(&str),
    {
        self.sweep_if_needed()?;
        let mut sent = 0usize;
        while let Some(item) = self.queue.pop_front() {
            send(&item.value);
            self.queue_payload_bytes = self.queue_payload_bytes.saturating_sub(item.value.len());
            self.remove_queue_entry(item.id)?;
            sent += 1;
        }
        self.update_queue_gauges();
        Ok(sent)
    }

    pub fn flush(&mut self) -> Result<(), MobileCacheError> {
        self.store.clear();
        self.expirations.clear();
        self.queue.clear();
        self.total_payload_bytes = 0;
        self.queue_payload_bytes = 0;
        self.responses.clear()?;
        self.queue_tree.clear()?;
        self.db.flush()?;
        self.update_entry_gauges();
        self.update_queue_gauges();
        Ok(())
    }

    pub fn status(&self) -> CacheStatus {
        let now = SystemTime::now();
        let mut entries: Vec<CacheStatusEntry> = self
            .store
            .iter()
            .map(|(key, entry)| {
                let age_secs = now
                    .duration_since(entry.stored_at_system)
                    .unwrap_or_else(|_| Duration::from_secs(0))
                    .as_secs();
                let expires_in_secs = entry
                    .expires_at_system
                    .duration_since(now)
                    .unwrap_or_else(|_| Duration::from_secs(0))
                    .as_secs();
                CacheStatusEntry {
                    key: key.clone(),
                    age_secs,
                    expires_in_secs,
                    size_bytes: entry.value.len(),
                }
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.age_secs));
        const MAX_STATUS_ENTRIES: usize = 64;
        if entries.len() > MAX_STATUS_ENTRIES {
            entries.truncate(MAX_STATUS_ENTRIES);
        }
        let oldest_age = self
            .queue
            .front()
            .and_then(|item| now.duration_since(item.enqueued_at).ok())
            .map(|d| d.as_secs());
        CacheStatus {
            totals: CacheTotals {
                hits: self.hits,
                misses: self.misses,
                stale_evictions: self.stale_evictions,
                rejections: self.rejections,
                entry_count: self.store.len(),
                entry_bytes: self.total_payload_bytes,
            },
            config: CacheStatusConfig {
                ttl_secs: self.config.ttl.as_secs(),
                sweep_interval_secs: self.config.sweep_interval.as_secs(),
                max_entries: self.config.max_entries,
                max_payload_bytes: self.config.max_payload_bytes,
                max_queue: self.config.max_queue,
                db_path: self.config.db_path.to_string_lossy().into_owned(),
            },
            entries,
            queue: CacheQueueStatus {
                depth: self.queue.len(),
                max: self.config.max_queue,
                bytes: self.queue_payload_bytes,
                oldest_age_secs: oldest_age,
            },
        }
    }
}

static GLOBAL_CACHE: Lazy<MutexT<MobileCache>> = Lazy::new(|| {
    let cfg = MobileCacheConfig::from_env().unwrap_or_else(|err| {
        #[cfg(not(feature = "telemetry"))]
        let _ = &err;
        #[cfg(feature = "telemetry")]
        diagnostics::log::warn!(
            "mobile cache config from env failed: {err}; using ephemeral fallback"
        );
        let tmp = std::env::temp_dir().join("mobile_cache_ephemeral");
        let mut key = [0u8; 32];
        OsRng::default().fill_bytes(&mut key);
        MobileCacheConfig {
            ttl: Duration::from_secs(300),
            sweep_interval: Duration::from_secs(30),
            max_entries: 128,
            max_payload_bytes: 64 * 1024,
            max_queue: 64,
            db_path: tmp,
            encryption_key: key,
            temporary: true,
        }
    });
    let cache = MobileCache::open(cfg).unwrap_or_else(|err| {
        panic!("failed to initialise mobile cache: {err}");
    });
    mutex(cache)
});

fn lock_cache() -> Option<MutexGuard<'static, MobileCache>> {
    match GLOBAL_CACHE.lock() {
        Ok(guard) => Some(guard),
        Err(poisoned) => {
            #[cfg(feature = "telemetry")]
            diagnostics::log::error!("mobile cache poisoned: {}", poisoned);
            Some(poisoned.into_inner())
        }
    }
}

fn policy_key(domain: &str) -> String {
    format!("gateway.policy:{domain}")
}

pub fn cache_get(key: &str) -> Option<String> {
    lock_cache()
        .and_then(|mut cache| cache.get(key).ok())
        .flatten()
}

pub fn cache_insert(key: String, value: String) -> Result<(), MobileCacheError> {
    lock_cache()
        .ok_or(MobileCacheError::LockPoisoned)?
        .insert(key, value)
}

pub fn queue_offline_tx(tx: String) -> Result<(), MobileCacheError> {
    lock_cache()
        .ok_or(MobileCacheError::LockPoisoned)?
        .queue_tx(tx)
}

pub fn drain_offline_queue<F>(send: F) -> Result<usize, MobileCacheError>
where
    F: FnMut(&str),
{
    lock_cache()
        .ok_or(MobileCacheError::LockPoisoned)?
        .drain_queue(send)
}

pub fn purge_policy(domain: &str) {
    if let Some(mut cache) = lock_cache() {
        if let Err(err) = cache.invalidate(&policy_key(domain)) {
            #[cfg(not(feature = "telemetry"))]
            let _ = &err;
            #[cfg(feature = "telemetry")]
            diagnostics::log::warn!("failed to invalidate policy cache for {domain}: {err}");
        }
    }
}

pub fn status_snapshot() -> Value {
    match lock_cache() {
        Some(cache) => json!({
            "status": "ok",
            "cache": cache.status(),
        }),
        None => json!({
            "status": "error",
            "error": "lock",
        }),
    }
}

pub fn flush_cache() -> Value {
    match lock_cache() {
        Some(mut cache) => match cache.flush() {
            Ok(()) => json!({
                "status": "ok",
            }),
            Err(err) => json!({
                "status": "error",
                "error": err.to_string(),
            }),
        },
        None => json!({
            "status": "error",
            "error": "lock",
        }),
    }
}

pub fn cache_policy(domain: &str, value: &Value) {
    if let Ok(json) = json::to_string(value) {
        if let Err(err) = cache_insert(policy_key(domain), json) {
            #[cfg(not(feature = "telemetry"))]
            let _ = &err;
            #[cfg(feature = "telemetry")]
            diagnostics::log::debug!("policy cache insert failed: {err}");
        }
    }
}

pub fn cached_policy(domain: &str) -> Option<Value> {
    cache_get(&policy_key(domain)).and_then(|val| json::from_str(&val).ok())
}

pub fn invalidate_prefix(prefix: &str) {
    if let Some(mut cache) = lock_cache() {
        if let Err(err) = cache.invalidate_prefix(prefix) {
            #[cfg(not(feature = "telemetry"))]
            let _ = &err;
            #[cfg(feature = "telemetry")]
            diagnostics::log::warn!("failed to invalidate prefix {prefix}: {err}");
        }
    }
}
