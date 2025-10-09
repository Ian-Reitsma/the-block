#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Once, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use concurrency::Lazy;
use foundation_serialization::{Deserialize, Serialize};
use ledger::address::ShardId;
#[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
use storage_engine::rocksdb_engine::RocksDbEngine;
use storage_engine::{
    inhouse_engine::InhouseEngine, memory_engine::MemoryEngine, KeyValue, KeyValueBatch,
    KeyValueIterator, StorageError, StorageMetrics, StorageResult,
};

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_COMPACTION_TOTAL, STORAGE_DISK_FULL_TOTAL, STORAGE_ENGINE_INFO,
    STORAGE_ENGINE_LEVEL0_FILES, STORAGE_ENGINE_MEMTABLE_BYTES, STORAGE_ENGINE_PENDING_COMPACTIONS,
    STORAGE_ENGINE_RUNNING_COMPACTIONS, STORAGE_ENGINE_SIZE_BYTES, STORAGE_ENGINE_SST_BYTES,
};

/// Record of a mutated key for rollback purposes.
pub type DbDelta = (String, Option<Vec<u8>>);

pub mod names {
    pub const DEFAULT: &str = "default";
    pub const BRIDGE: &str = "bridge";
    pub const COMPUTE_SETTLEMENT: &str = "compute_settlement";
    pub const DEX_STORAGE: &str = "dex_storage";
    pub const GATEWAY_DNS: &str = "gateway_dns";
    pub const GOSSIP_RELAY: &str = "gossip_relay";
    pub const IDENTITY_DID: &str = "identity_did";
    pub const IDENTITY_HANDLES: &str = "identity_handle_registry";
    pub const LIGHT_CLIENT_PROOFS: &str = "light_client_proofs";
    pub const LOCALNET_RECEIPTS: &str = "localnet_receipts";
    pub const NET_PEER_CHUNKS: &str = "net_peer_chunks";
    pub const NET_BANS: &str = "net_bans";
    pub const RPC_BRIDGE: &str = "rpc_bridge";
    pub const STORAGE_FS: &str = "storage_fs";
    pub const STORAGE_PIPELINE: &str = "storage_pipeline";
    pub const STORAGE_REPAIR: &str = "storage_repair";
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Memory,
    Inhouse,
    RocksDb,
}

impl EngineKind {
    pub fn default_for_build() -> Self {
        if cfg!(feature = "lightweight-integration") {
            EngineKind::Memory
        } else {
            EngineKind::Inhouse
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Memory => "memory",
            EngineKind::Inhouse => "inhouse",
            EngineKind::RocksDb => "rocksdb",
        }
    }

    pub fn is_available(self) -> bool {
        match self {
            EngineKind::Memory => true,
            EngineKind::Inhouse => true,
            EngineKind::RocksDb => {
                cfg!(all(
                    not(feature = "lightweight-integration"),
                    feature = "storage-rocksdb"
                ))
            }
        }
    }
}

impl Default for EngineKind {
    fn default() -> Self {
        EngineKind::default_for_build()
    }
}

fn default_engine_kind() -> EngineKind {
    EngineKind::default()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineConfig {
    #[serde(default = "default_engine_kind")]
    pub default_engine: EngineKind,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub overrides: HashMap<String, EngineKind>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            default_engine: EngineKind::default(),
            overrides: HashMap::new(),
        }
    }
}

impl EngineConfig {
    pub fn resolve(&self, name: &str) -> EngineKind {
        let requested = self
            .overrides
            .get(name)
            .copied()
            .unwrap_or(self.default_engine);
        if requested.is_available() {
            requested
        } else if self.default_engine.is_available() {
            self.default_engine
        } else {
            EngineKind::default()
        }
    }
}

static ENGINE_CONFIG: Lazy<RwLock<EngineConfig>> =
    Lazy::new(|| RwLock::new(EngineConfig::default()));

static LEGACY_MODE: AtomicBool = AtomicBool::new(false);
static LEGACY_WARN_ONCE: Once = Once::new();

pub fn configure_engines(config: EngineConfig) {
    *ENGINE_CONFIG.write().unwrap() = config;
}

pub fn set_legacy_mode(enabled: bool) {
    LEGACY_MODE.store(enabled, Ordering::Relaxed);
    if enabled {
        LEGACY_WARN_ONCE.call_once(|| {
            #[cfg(feature = "telemetry")]
            diagnostics::tracing::warn!(
                target: "storage_legacy_mode",
                "storage legacy mode enabled; this toggle will be removed in the next release"
            );
            #[cfg(not(feature = "telemetry"))]
            eprintln!(
                "warning: storage legacy mode enabled; this toggle will be removed in the next release"
            );
        });
    }
}

pub fn legacy_mode() -> bool {
    LEGACY_MODE.load(Ordering::Relaxed)
}

enum Engine {
    Memory(MemoryEngine),
    Inhouse(InhouseEngine),
    #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
    RocksDb(RocksDbEngine),
}

enum EngineBatch {
    Memory(<MemoryEngine as KeyValue>::Batch),
    Inhouse(<InhouseEngine as KeyValue>::Batch),
    #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
    RocksDb(<RocksDbEngine as KeyValue>::Batch),
}

impl EngineBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        match self {
            EngineBatch::Memory(inner) => inner.put(cf, key, value),
            EngineBatch::Inhouse(inner) => inner.put(cf, key, value),
            #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
            EngineBatch::RocksDb(inner) => inner.put(cf, key, value),
        }
    }

    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()> {
        match self {
            EngineBatch::Memory(inner) => inner.delete(cf, key),
            EngineBatch::Inhouse(inner) => inner.delete(cf, key),
            #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
            EngineBatch::RocksDb(inner) => inner.delete(cf, key),
        }
    }
}

macro_rules! dispatch {
    ($engine:expr, $inner:ident, $body:expr) => {{
        match $engine {
            Engine::Memory($inner) => $body,
            Engine::Inhouse($inner) => $body,
            #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
            Engine::RocksDb($inner) => $body,
        }
    }};
}

impl Engine {
    fn unique_temp_dir(prefix: &str) -> StorageResult<PathBuf> {
        let mut base = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| StorageError::backend(err))?
            .as_nanos();
        let suffix = format!("the-block-{prefix}-{}-{}", std::process::id(), nanos);
        base.push(suffix);
        fs::create_dir_all(&base).map_err(StorageError::from)?;
        Ok(base)
    }

    fn open(kind: EngineKind, path: &str) -> StorageResult<Self> {
        match kind {
            EngineKind::Memory => MemoryEngine::open(path).map(Engine::Memory),
            EngineKind::Inhouse => InhouseEngine::open(path).map(Engine::Inhouse),
            EngineKind::RocksDb => {
                #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
                {
                    RocksDbEngine::open(path).map(Engine::RocksDb)
                }
                #[cfg(not(all(
                    not(feature = "lightweight-integration"),
                    feature = "storage-rocksdb"
                )))]
                {
                    Err(StorageError::backend("rocksdb engine not available"))
                }
            }
        }
    }

    fn temporary(kind: EngineKind) -> StorageResult<Self> {
        match kind {
            EngineKind::Memory => Ok(Engine::Memory(MemoryEngine::default())),
            EngineKind::Inhouse => {
                let dir = Self::unique_temp_dir("inhouse")?;
                InhouseEngine::open(&dir.to_string_lossy()).map(Engine::Inhouse)
            }
            EngineKind::RocksDb => {
                #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
                {
                    Ok(Engine::RocksDb(RocksDbEngine::default()))
                }
                #[cfg(not(all(
                    not(feature = "lightweight-integration"),
                    feature = "storage-rocksdb"
                )))]
                {
                    Err(StorageError::backend("rocksdb engine not available"))
                }
            }
        }
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        dispatch!(self, engine, engine.ensure_cf(cf))
    }

    fn make_batch(&self) -> EngineBatch {
        match self {
            Engine::Memory(engine) => EngineBatch::Memory(engine.make_batch()),
            Engine::Inhouse(engine) => EngineBatch::Inhouse(engine.make_batch()),
            #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
            Engine::RocksDb(engine) => EngineBatch::RocksDb(engine.make_batch()),
        }
    }

    fn write_batch(&self, batch: EngineBatch) -> StorageResult<()> {
        match (self, batch) {
            (Engine::Memory(engine), EngineBatch::Memory(batch)) => engine.write_batch(batch),
            (Engine::Inhouse(engine), EngineBatch::Inhouse(batch)) => engine.write_batch(batch),
            #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
            (Engine::RocksDb(engine), EngineBatch::RocksDb(batch)) => engine.write_batch(batch),
            #[allow(unreachable_patterns)]
            _ => Err(StorageError::backend("mismatched engine batch")),
        }
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        dispatch!(self, engine, engine.get(cf, key))
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        dispatch!(self, engine, engine.put(cf, key, value))
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        dispatch!(self, engine, engine.put_bytes(cf, key, value))
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        dispatch!(self, engine, engine.delete(cf, key))
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        dispatch!(self, engine, engine.list_cfs())
    }

    fn flush_wal(&self) -> StorageResult<()> {
        dispatch!(self, engine, engine.flush_wal())
    }

    fn flush(&self) -> StorageResult<()> {
        dispatch!(self, engine, engine.flush())
    }

    fn compact(&self) -> StorageResult<()> {
        dispatch!(self, engine, engine.compact())
    }

    fn set_byte_limit(&self, limit: Option<usize>) -> StorageResult<()> {
        dispatch!(self, engine, engine.set_byte_limit(limit))
    }

    #[cfg(feature = "telemetry")]
    fn metrics(&self) -> StorageResult<StorageMetrics> {
        dispatch!(self, engine, engine.metrics())
    }

    #[cfg(not(feature = "telemetry"))]
    fn metrics(&self) -> StorageResult<StorageMetrics> {
        Err(StorageError::backend(
            "storage metrics are unavailable without the telemetry feature",
        ))
    }

    fn backend_name(&self) -> &'static str {
        match self {
            Engine::Memory(_) => "memory",
            Engine::Inhouse(_) => "inhouse",
            #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
            Engine::RocksDb(_) => "rocksdb",
        }
    }
}

/// Thin wrapper that adapts the storage-engine traits to the historical SimpleDb API.
pub struct SimpleDb {
    #[cfg(feature = "telemetry")]
    name: String,
    engine: Engine,
}

pub struct SimpleDbBatch {
    inner: EngineBatch,
    column_families: HashSet<String>,
}

impl SimpleDbBatch {
    pub fn put(&mut self, key: &str, value: &[u8]) -> io::Result<()> {
        self.put_cf("default", key, value)
    }

    pub fn put_cf(&mut self, cf: &str, key: &str, value: &[u8]) -> io::Result<()> {
        self.column_families.insert(cf.to_string());
        self.inner
            .put(cf, key.as_bytes(), value)
            .map_err(to_io_error)
    }

    pub fn delete(&mut self, key: &str) -> io::Result<()> {
        self.delete_cf("default", key)
    }

    pub fn delete_cf(&mut self, cf: &str, key: &str) -> io::Result<()> {
        self.column_families.insert(cf.to_string());
        self.inner.delete(cf, key.as_bytes()).map_err(to_io_error)
    }

    fn into_inner(self) -> (EngineBatch, HashSet<String>) {
        (self.inner, self.column_families)
    }
}

impl SimpleDb {
    #[cfg(feature = "telemetry")]
    fn from_parts(name: &str, engine: Engine) -> Self {
        Self {
            name: name.to_string(),
            engine,
        }
    }

    #[cfg(not(feature = "telemetry"))]
    fn from_parts(_name: &str, engine: Engine) -> Self {
        let _ = _name;
        Self { engine }
    }

    /// Open (or create) a database at the given path.
    pub fn open(path: &str) -> Self {
        Self::open_named(names::DEFAULT, path)
    }

    pub fn open_named(name: &str, path: &str) -> Self {
        let kind = if legacy_mode() {
            EngineKind::default()
        } else if cfg!(feature = "lightweight-integration") {
            EngineKind::Memory
        } else {
            ENGINE_CONFIG.read().unwrap().resolve(name)
        };
        let engine = Engine::open(kind, path)
            .or_else(|_| Engine::open(EngineKind::default(), path))
            .unwrap_or_else(|e| panic!("open simple db {name}: {e}"));
        let db = Self::from_parts(name, engine);
        db.record_metrics_if_enabled();
        db
    }

    pub fn batch(&self) -> SimpleDbBatch {
        SimpleDbBatch {
            inner: self.engine.make_batch(),
            column_families: HashSet::new(),
        }
    }

    pub fn write_batch(&self, batch: SimpleDbBatch) -> io::Result<()> {
        let (inner, cfs) = batch.into_inner();
        for cf in cfs {
            self.engine.ensure_cf(&cf).map_err(to_io_error)?;
        }
        self.engine.write_batch(inner).map_err(to_io_error)?;
        self.record_metrics_if_enabled();
        Ok(())
    }

    /// Flush outstanding WAL entries to SST files.
    pub fn flush_wal(&self) {
        let _ = self.engine.flush_wal();
        self.record_metrics_if_enabled();
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.engine.get("default", key.as_bytes()).ok().flatten()
    }

    pub fn try_insert(&mut self, key: &str, value: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
        self.engine.ensure_cf("default").map_err(to_io_error)?;
        let res = self
            .engine
            .put("default", key.as_bytes(), &value)
            .map_err(to_io_error);
        if res.is_ok() {
            self.record_metrics_if_enabled();
        }
        res
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        self.try_insert(key, value).ok().flatten()
    }

    fn put_cf_raw(&self, cf: &str, key: &[u8], value: &[u8]) -> io::Result<()> {
        let res = self
            .engine
            .ensure_cf(cf)
            .and_then(|_| self.engine.put_bytes(cf, key, value))
            .map_err(to_io_error);
        if res.is_ok() {
            self.record_metrics_if_enabled();
        }
        res
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> io::Result<()> {
        self.put_cf_raw("default", key, value)
    }

    fn insert_cf_with_delta(
        &mut self,
        cf: &str,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        self.engine.ensure_cf(cf).map_err(to_io_error)?;
        let prev = self
            .engine
            .put(cf, key.as_bytes(), &value)
            .map_err(to_io_error)?;
        deltas.push((format!("{cf}|{key}"), prev));
        self.record_metrics_if_enabled();
        Ok(())
    }

    pub fn insert_with_delta(
        &mut self,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        self.insert_cf_with_delta("default", key, value, deltas)
    }

    pub fn insert_shard_with_delta(
        &mut self,
        shard: ShardId,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        let cf = format!("shard:{shard}");
        self.insert_cf_with_delta(cf.as_str(), key, value, deltas)
    }

    pub fn try_remove(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let res = self
            .engine
            .delete("default", key.as_bytes())
            .map_err(to_io_error);
        if res.is_ok() {
            self.record_metrics_if_enabled();
        }
        res
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.try_remove(key).ok().flatten()
    }

    pub fn rollback(&mut self, deltas: Vec<DbDelta>) {
        for (full, prev) in deltas.into_iter().rev() {
            let (cf_name, key) = full
                .split_once('|')
                .map(|(c, k)| (c.to_string(), k.to_string()))
                .unwrap_or_else(|| ("default".to_string(), full.clone()));
            match prev {
                Some(v) => {
                    let _ = self.engine.ensure_cf(&cf_name);
                    let _ = self.engine.put_bytes(&cf_name, key.as_bytes(), &v);
                }
                None => {
                    let _ = self.engine.delete(&cf_name, key.as_bytes());
                }
            }
        }
        self.record_metrics_if_enabled();
    }

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        fn collect<E: KeyValue>(engine: &E, prefix: &[u8]) -> Vec<String> {
            let mut iter = match engine.prefix_iterator("default", prefix) {
                Ok(iter) => iter,
                Err(_) => return Vec::new(),
            };
            let mut keys = Vec::new();
            while let Ok(Some((key, _))) = iter.next() {
                if let Ok(s) = String::from_utf8(key) {
                    keys.push(s);
                }
            }
            keys
        }

        dispatch!(&self.engine, engine, collect(engine, prefix.as_bytes()))
    }

    pub fn shard_ids(&self) -> Vec<ShardId> {
        self.engine
            .list_cfs()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| name.strip_prefix("shard:")?.parse::<ShardId>().ok())
            .collect()
    }

    pub fn get_shard(&self, shard: ShardId, key: &str) -> Option<Vec<u8>> {
        let cf = format!("shard:{shard}");
        self.engine.get(cf.as_str(), key.as_bytes()).ok().flatten()
    }

    pub fn try_flush(&self) -> io::Result<()> {
        match self.engine.flush() {
            Ok(()) => {
                self.record_metrics_if_enabled();
                Ok(())
            }
            Err(err) => {
                #[cfg(feature = "telemetry")]
                {
                    if is_disk_full(&err) {
                        STORAGE_DISK_FULL_TOTAL.inc();
                    }
                }
                Err(to_io_error(err))
            }
        }
    }

    pub fn flush(&self) {
        let _ = self.try_flush();
    }

    pub fn set_byte_limit(&mut self, limit: usize) {
        let _ = self.engine.set_byte_limit(Some(limit));
        self.record_metrics_if_enabled();
    }

    pub fn compact(&self) {
        let _ = self.engine.compact();
        #[cfg(feature = "telemetry")]
        {
            STORAGE_COMPACTION_TOTAL.inc();
        }
        self.record_metrics_if_enabled();
    }

    pub fn backend_name(&self) -> &'static str {
        self.engine.backend_name()
    }
}

impl Default for SimpleDb {
    fn default() -> Self {
        let kind = if legacy_mode() {
            EngineKind::default()
        } else if cfg!(feature = "lightweight-integration") {
            EngineKind::Memory
        } else {
            ENGINE_CONFIG.read().unwrap().resolve(names::DEFAULT)
        };
        let engine = Engine::temporary(kind)
            .or_else(|_| Engine::temporary(EngineKind::default()))
            .unwrap_or_else(|e| panic!("open temp simple db: {e}"));
        let db = Self::from_parts(names::DEFAULT, engine);
        db.record_metrics_if_enabled();
        db
    }
}

fn to_io_error(err: StorageError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}

#[cfg(feature = "telemetry")]
fn is_disk_full(err: &StorageError) -> bool {
    err.to_string().contains("No space")
}

impl SimpleDb {
    #[cfg(feature = "telemetry")]
    fn record_metrics(&self) {
        if let Ok(metrics) = self.engine.metrics() {
            let labels = &[self.name.as_str(), self.engine.backend_name()];
            STORAGE_ENGINE_PENDING_COMPACTIONS
                .with_label_values(labels)
                .set(to_gauge(metrics.pending_compactions));
            STORAGE_ENGINE_RUNNING_COMPACTIONS
                .with_label_values(labels)
                .set(to_gauge(metrics.running_compactions));
            STORAGE_ENGINE_LEVEL0_FILES
                .with_label_values(labels)
                .set(to_gauge(metrics.level0_files));
            STORAGE_ENGINE_SST_BYTES
                .with_label_values(labels)
                .set(to_gauge(metrics.total_sst_bytes));
            STORAGE_ENGINE_MEMTABLE_BYTES
                .with_label_values(labels)
                .set(to_gauge(metrics.memtable_bytes));
            STORAGE_ENGINE_SIZE_BYTES
                .with_label_values(labels)
                .set(to_gauge(metrics.size_on_disk_bytes));
            for engine in ["memory", "rocksdb", "rocksdb-compat", "inhouse"] {
                let value = if engine == self.engine.backend_name() {
                    1
                } else {
                    0
                };
                STORAGE_ENGINE_INFO
                    .with_label_values(&[self.name.as_str(), engine])
                    .set(value);
            }
        }
    }

    #[cfg(feature = "telemetry")]
    fn record_metrics_if_enabled(&self) {
        self.record_metrics();
    }

    #[cfg(not(feature = "telemetry"))]
    fn record_metrics_if_enabled(&self) {
        let _ = self.engine.metrics();
    }
}

#[cfg(feature = "telemetry")]
fn to_gauge(value: Option<u64>) -> i64 {
    value.and_then(|v| i64::try_from(v).ok()).unwrap_or(0)
}

#[cfg(test)]
impl SimpleDb {
    fn backend_name_for_test(&self) -> &'static str {
        self.backend_name()
    }
}

#[cfg(test)]
mod tests {
    use super::{configure_engines, names, Engine, EngineConfig, EngineKind, SimpleDb};
    use concurrency::Lazy;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use sys::tempfile::tempdir;

    static TEST_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn read_current_config() -> EngineConfig {
        super::ENGINE_CONFIG.read().unwrap().clone()
    }

    fn engine_kind_label(kind: EngineKind) -> &'static str {
        match kind {
            EngineKind::Memory => "memory",
            EngineKind::Inhouse => "inhouse",
            EngineKind::RocksDb => "rocksdb",
        }
    }

    fn pick_supported_alternate() -> Option<EngineKind> {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("probe");
        let path = path.to_string_lossy().into_owned();
        for kind in [EngineKind::RocksDb] {
            if let Ok(engine) = Engine::open(kind, &path) {
                drop(engine);
                return Some(kind);
            }
        }
        None
    }

    #[test]
    fn resolve_prefers_override_when_supported() {
        let mut overrides = HashMap::new();
        overrides.insert("custom".to_string(), EngineKind::Memory);
        let config = EngineConfig {
            default_engine: EngineKind::Inhouse,
            overrides,
        };
        assert_eq!(config.resolve("custom"), EngineKind::Memory);
    }

    #[test]
    fn resolve_falls_back_when_override_unsupported() {
        let mut overrides = HashMap::new();
        overrides.insert("custom".to_string(), EngineKind::RocksDb);
        let config = EngineConfig {
            default_engine: EngineKind::Memory,
            overrides,
        };
        let resolved = config.resolve("custom");
        #[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
        assert_eq!(resolved, EngineKind::RocksDb);
        #[cfg(not(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb")))]
        assert_eq!(resolved, EngineKind::Memory);
    }

    #[test]
    fn configure_engines_applies_runtime_overrides() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let original = read_current_config();

        let mut overrides = HashMap::new();
        overrides.insert(names::GOSSIP_RELAY.to_string(), EngineKind::Memory);
        let config = EngineConfig {
            default_engine: EngineKind::Inhouse,
            overrides,
        };
        configure_engines(config);

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("db");
        let db = SimpleDb::open_named(names::GOSSIP_RELAY, &path.to_string_lossy());
        assert_eq!(db.backend_name_for_test(), "memory");

        configure_engines(original);
    }

    #[test]
    fn configure_engines_reload_switches_backend_when_supported() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let original = read_current_config();

        let mut config = EngineConfig {
            default_engine: EngineKind::Memory,
            overrides: HashMap::new(),
        };
        configure_engines(config.clone());

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("first");
        let db = SimpleDb::open_named("reload-test", &path.to_string_lossy());
        assert_eq!(db.backend_name_for_test(), "memory");
        drop(db);

        if let Some(alternate) = pick_supported_alternate() {
            config
                .overrides
                .insert("reload-test".to_string(), alternate);
            configure_engines(config.clone());

            let dir = tempdir().expect("tempdir");
            let path = dir.path().join("second");
            let db = SimpleDb::open_named("reload-test", &path.to_string_lossy());
            assert_eq!(db.backend_name_for_test(), engine_kind_label(alternate));
        }

        configure_engines(original);
    }
}
