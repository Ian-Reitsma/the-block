#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::json::{self, Map, Number, Value};

use crate::{
    KeyValue, KeyValueBatch, KeyValueIterator, StorageError, StorageMetrics, StorageResult,
};

const MANIFEST_FILE: &str = "manifest.json";
const WAL_FILE: &str = "wal.log";
const DEFAULT_MEMTABLE_LIMIT: usize = 8 * 1024 * 1024;
const DEFAULT_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone)]
struct Manifest {
    cfs: HashMap<String, CfManifest>,
}

impl Manifest {
    fn load(path: &Path) -> StorageResult<Self> {
        let manifest_path = path.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            return Ok(Manifest {
                cfs: HashMap::new(),
            });
        }
        let data = fs::read(&manifest_path).map_err(StorageError::from)?;
        let value = json::value_from_slice(&data)
            .map_err(|err| StorageError::backend(format!("failed to parse manifest: {err}")))?;
        Manifest::from_value(value)
    }

    fn store(&self, path: &Path) -> StorageResult<()> {
        let manifest_path = path.join(MANIFEST_FILE);
        let tmp_path = manifest_path.with_extension("tmp");
        let value = self.to_value();
        let data = json::to_vec_value(&value);
        fs::write(&tmp_path, data).map_err(StorageError::from)?;
        fs::rename(&tmp_path, &manifest_path).map_err(StorageError::from)
    }

    fn to_value(&self) -> Value {
        let mut manifest = Map::new();
        let mut cfs_map = Map::new();
        for (name, cf) in &self.cfs {
            cfs_map.insert(name.clone(), cf.to_value());
        }
        manifest.insert("cfs".to_string(), Value::Object(cfs_map));
        Value::Object(manifest)
    }

    fn from_value(value: Value) -> StorageResult<Self> {
        let mut manifest = expect_object(value, "manifest")?;
        let cfs_value = manifest
            .remove("cfs")
            .unwrap_or_else(|| Value::Object(Map::new()));
        let cfs_map = expect_object(cfs_value, "manifest.cfs")?;
        let mut cfs = HashMap::new();
        for (name, cf_value) in cfs_map {
            cfs.insert(name, CfManifest::from_value(cf_value)?);
        }
        Ok(Manifest { cfs })
    }
}

#[derive(Debug, Clone)]
struct CfManifest {
    next_file_id: u64,
    sstables: Vec<SstMeta>,
    sequence: u64,
}

impl CfManifest {
    fn new() -> Self {
        Self {
            next_file_id: 0,
            sstables: Vec::new(),
            sequence: 0,
        }
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "next_file_id".to_string(),
            Value::Number(Number::from(self.next_file_id)),
        );
        map.insert(
            "sequence".to_string(),
            Value::Number(Number::from(self.sequence)),
        );
        let sstables = self.sstables.iter().map(SstMeta::to_value).collect();
        map.insert("sstables".to_string(), Value::Array(sstables));
        Value::Object(map)
    }

    fn from_value(value: Value) -> StorageResult<Self> {
        let mut map = expect_object(value, "cf manifest")?;
        let next_file_id = take_u64(&mut map, "next_file_id", "cf manifest")?;
        let sequence = take_u64(&mut map, "sequence", "cf manifest")?;
        let sstables_value = map
            .remove("sstables")
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let sstable_array = expect_array(sstables_value, "cf manifest sstables")?;
        let mut sstables = Vec::with_capacity(sstable_array.len());
        for entry in sstable_array {
            sstables.push(SstMeta::from_value(entry)?);
        }
        Ok(Self {
            next_file_id,
            sstables,
            sequence,
        })
    }
}

#[derive(Debug, Clone)]
struct SstMeta {
    file: String,
    max_sequence: u64,
    min_key: Vec<u8>,
    max_key: Vec<u8>,
}

impl SstMeta {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("file".to_string(), Value::String(self.file.clone()));
        map.insert(
            "max_sequence".to_string(),
            Value::Number(Number::from(self.max_sequence)),
        );
        map.insert("min_key".to_string(), encode_bytes(&self.min_key));
        map.insert("max_key".to_string(), encode_bytes(&self.max_key));
        Value::Object(map)
    }

    fn from_value(value: Value) -> StorageResult<Self> {
        let mut map = expect_object(value, "sstable metadata")?;
        let file = take_string(&mut map, "file", "sstable metadata")?;
        let max_sequence = take_u64(&mut map, "max_sequence", "sstable metadata")?;
        let min_key = match map.remove("min_key") {
            Some(value) => decode_bytes(value, "sstable metadata min_key")?,
            None => Vec::new(),
        };
        let max_key = match map.remove("max_key") {
            Some(value) => decode_bytes(value, "sstable metadata max_key")?,
            None => Vec::new(),
        };
        Ok(Self {
            file,
            max_sequence,
            min_key,
            max_key,
        })
    }
}

fn expect_object(value: Value, context: &str) -> StorageResult<Map> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(StorageError::backend(format!(
            "{context} must be an object"
        ))),
    }
}

fn expect_array(value: Value, context: &str) -> StorageResult<Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items),
        Value::Null => Ok(Vec::new()),
        _ => Err(StorageError::backend(format!("{context} must be an array"))),
    }
}

fn take_u64(map: &mut Map, field: &str, context: &str) -> StorageResult<u64> {
    let value = map
        .remove(field)
        .ok_or_else(|| StorageError::backend(format!("missing field '{field}' in {context}")))?;
    json::from_value::<u64>(value)
        .map_err(|err| StorageError::backend(format!("invalid {field} in {context}: {err}")))
}

fn take_string(map: &mut Map, field: &str, context: &str) -> StorageResult<String> {
    let value = map
        .remove(field)
        .ok_or_else(|| StorageError::backend(format!("missing field '{field}' in {context}")))?;
    json::from_value::<String>(value)
        .map_err(|err| StorageError::backend(format!("invalid {field} in {context}: {err}")))
}

fn decode_bytes(value: Value, context: &str) -> StorageResult<Vec<u8>> {
    json::from_value::<Vec<u8>>(value)
        .map_err(|err| StorageError::backend(format!("invalid byte array for {context}: {err}")))
}

fn encode_bytes(bytes: &[u8]) -> Value {
    Value::Array(
        bytes
            .iter()
            .map(|b| Value::Number(Number::from(*b)))
            .collect(),
    )
}

#[derive(Clone)]
pub struct InhouseEngine {
    root: Arc<PathBuf>,
    inner: Arc<EngineInner>,
}

struct EngineInner {
    manifest: RwLock<Manifest>,
    cfs: RwLock<HashMap<String, Arc<CfHandle>>>,
    memtable_limit: RwLock<Option<usize>>,
    cache: Mutex<SstCache>,
}

struct CfHandle {
    name: String,
    path: PathBuf,
    state: Mutex<CfState>,
}

struct CfState {
    memtable: BTreeMap<Vec<u8>, TableEntry>,
    memtable_bytes: usize,
    manifest: CfManifest,
}

impl EngineInner {
    fn load_table(&self, path: &Path) -> StorageResult<Arc<Vec<TableEntry>>> {
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(entries) = cache.get(path) {
                return Ok(entries);
            }
        }
        let entries = read_table(path)?;
        let arc = Arc::new(entries);
        let mut cache = self.cache.lock().unwrap();
        cache.insert(path.to_path_buf(), arc.clone());
        Ok(arc)
    }

    fn invalidate_table(&self, path: &Path) {
        self.cache.lock().unwrap().remove(path);
    }
}

#[derive(Default)]
struct SstCache {
    entries: HashMap<PathBuf, Arc<Vec<TableEntry>>>,
    order: VecDeque<PathBuf>,
    limit: usize,
}

#[derive(Debug, Clone)]
struct TableEntry {
    key: Vec<u8>,
    sequence: u64,
    value: ValueState,
}

impl SstCache {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            limit,
        }
    }

    fn get(&mut self, path: &Path) -> Option<Arc<Vec<TableEntry>>> {
        let path = path.to_path_buf();
        if let Some(entry) = self.entries.get(&path).cloned() {
            self.order.retain(|p| p != &path);
            self.order.push_back(path);
            Some(entry)
        } else {
            None
        }
    }

    fn insert(&mut self, path: PathBuf, entries: Arc<Vec<TableEntry>>) {
        if self.limit == 0 {
            return;
        }
        if self.entries.contains_key(&path) {
            self.order.retain(|p| p != &path);
        }
        self.entries.insert(path.clone(), entries);
        self.order.push_back(path.clone());
        while self.order.len() > self.limit {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }

    fn remove(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.entries.remove(&path);
        self.order.retain(|p| p != &path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ValueState {
    Present(Vec<u8>),
    Tombstone,
}

impl TableEntry {
    fn value_bytes(&self) -> Option<&[u8]> {
        match &self.value {
            ValueState::Present(ref value) => Some(value.as_slice()),
            ValueState::Tombstone => None,
        }
    }
}

pub struct InhouseIterator {
    data: Vec<(Vec<u8>, Vec<u8>)>,
    index: usize,
}

impl KeyValueIterator for InhouseIterator {
    fn next(&mut self) -> StorageResult<Option<(Vec<u8>, Vec<u8>)>> {
        if self.index >= self.data.len() {
            Ok(None)
        } else {
            let item = self.data[self.index].clone();
            self.index += 1;
            Ok(Some(item))
        }
    }
}

#[derive(Default)]
pub struct InhouseBatch {
    ops: Vec<BatchOp>,
}

enum BatchOp {
    Put {
        cf: String,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf: String,
        key: Vec<u8>,
    },
}

impl KeyValueBatch for InhouseBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.ops.push(BatchOp::Put {
            cf: cf.to_string(),
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()> {
        self.ops.push(BatchOp::Delete {
            cf: cf.to_string(),
            key: key.to_vec(),
        });
        Ok(())
    }
}

impl InhouseEngine {
    pub fn open(path: &str) -> StorageResult<Self> {
        let root = PathBuf::from(path);
        fs::create_dir_all(&root).map_err(StorageError::from)?;
        let manifest = Manifest::load(&root)?;
        let inner = EngineInner {
            manifest: RwLock::new(manifest),
            cfs: RwLock::new(HashMap::new()),
            memtable_limit: RwLock::new(Some(DEFAULT_MEMTABLE_LIMIT)),
            cache: Mutex::new(SstCache::new(DEFAULT_CACHE_CAPACITY)),
        };
        let engine = InhouseEngine {
            root: Arc::new(root),
            inner: Arc::new(inner),
        };
        engine.reload_cfs()?;
        Ok(engine)
    }

    fn reload_cfs(&self) -> StorageResult<()> {
        let manifest = self.inner.manifest.read().unwrap().clone();
        for (cf, _) in &manifest.cfs {
            let _ = self.cf_handle(cf)?;
        }
        Ok(())
    }

    fn cf_handle(&self, cf: &str) -> StorageResult<Arc<CfHandle>> {
        if let Some(existing) = self.inner.cfs.read().unwrap().get(cf) {
            return Ok(existing.clone());
        }
        let mut write_guard = self.inner.cfs.write().unwrap();
        if let Some(existing) = write_guard.get(cf) {
            return Ok(existing.clone());
        }
        let cf_path = self.root.join(cf);
        fs::create_dir_all(&cf_path).map_err(StorageError::from)?;
        let manifest = {
            let mut manifest_guard = self.inner.manifest.write().unwrap();
            manifest_guard
                .cfs
                .entry(cf.to_string())
                .or_insert_with(CfManifest::new)
                .clone()
        };
        let mut state = CfState {
            memtable: BTreeMap::new(),
            memtable_bytes: 0,
            manifest,
        };
        state.replay_wal(&cf_path)?;
        if state.ensure_key_ranges(&cf_path, &self.inner)? {
            self.persist_manifest(cf, &state.manifest)?;
        }
        let handle = Arc::new(CfHandle {
            name: cf.to_string(),
            path: cf_path,
            state: Mutex::new(state),
        });
        write_guard.insert(cf.to_string(), handle.clone());
        Ok(handle)
    }

    fn with_cf<R, F>(&self, cf: &str, f: F) -> StorageResult<R>
    where
        F: FnOnce(&mut CfState, &Path, &EngineInner) -> StorageResult<R>,
    {
        let handle = self.cf_handle(cf)?;
        let mut guard = handle.state.lock().unwrap();
        let result = f(&mut guard, &handle.path, &self.inner);
        if result.is_ok() {
            self.persist_manifest(&handle.name, &guard.manifest)?;
        }
        result
    }

    fn persist_manifest(&self, cf: &str, manifest: &CfManifest) -> StorageResult<()> {
        let mut manifest_guard = self.inner.manifest.write().unwrap();
        manifest_guard.cfs.insert(cf.to_string(), manifest.clone());
        manifest_guard.store(&self.root)
    }
}

impl CfState {
    fn replay_wal(&mut self, cf_path: &Path) -> StorageResult<()> {
        let wal_path = cf_path.join(WAL_FILE);
        if !wal_path.exists() {
            return Ok(());
        }
        let mut wal_reader = File::open(&wal_path).map_err(StorageError::from)?;
        wal_reader
            .seek(SeekFrom::Start(0))
            .map_err(StorageError::from)?;
        let mut buf = String::new();
        wal_reader
            .read_to_string(&mut buf)
            .map_err(StorageError::from)?;
        for line in buf.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let value = json::value_from_str(line)
                .map_err(|err| StorageError::backend(format!("invalid wal record: {err}")))?;
            let record = WalRecord::from_value(value)?;
            self.apply_record(record);
        }
        Ok(())
    }

    fn apply_record(&mut self, record: WalRecord) {
        self.manifest.sequence = self.manifest.sequence.max(record.sequence);
        match record.kind {
            WalKind::Put { value } => {
                let entry = TableEntry {
                    key: record.key.clone(),
                    sequence: record.sequence,
                    value: ValueState::Present(value),
                };
                self.insert_mem(entry);
            }
            WalKind::Delete => {
                let entry = TableEntry {
                    key: record.key.clone(),
                    sequence: record.sequence,
                    value: ValueState::Tombstone,
                };
                self.insert_mem(entry);
            }
        }
    }

    fn insert_mem(&mut self, entry: TableEntry) {
        self.memtable_bytes = self
            .memtable_bytes
            .saturating_sub(self.memtable.get(&entry.key).map(byte_cost).unwrap_or(0));
        self.memtable_bytes += byte_cost(&entry);
        self.memtable.insert(entry.key.clone(), entry);
    }

    fn allocate_sequence(&mut self) -> u64 {
        self.manifest.sequence += 1;
        self.manifest.sequence
    }

    fn append_wal(&mut self, cf_path: &Path, record: &WalRecord) -> StorageResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(cf_path.join(WAL_FILE))
            .map_err(StorageError::from)?;
        let line = json::to_string_value(&record.to_value());
        file.write_all(line.as_bytes())
            .map_err(StorageError::from)?;
        file.write_all(b"\n").map_err(StorageError::from)?;
        file.sync_all().map_err(StorageError::from)
    }

    fn maybe_flush(
        &mut self,
        cf_path: &Path,
        engine: &EngineInner,
        limit: Option<usize>,
    ) -> StorageResult<()> {
        if let Some(limit) = limit {
            if self.memtable_bytes >= limit {
                self.flush_memtable(cf_path, engine)?;
            }
        }
        Ok(())
    }

    fn flush_memtable(&mut self, cf_path: &Path, engine: &EngineInner) -> StorageResult<()> {
        if self.memtable.is_empty() {
            return Ok(());
        }
        let mut entries: Vec<TableEntry> = self.memtable.values().cloned().collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key).then(a.sequence.cmp(&b.sequence)));
        let filename = format!("sst-{:020}.bin", self.manifest.next_file_id);
        self.manifest.next_file_id += 1;
        let max_sequence = entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let table_path = cf_path.join(&filename);
        let data = encode_table(&entries);
        fs::write(&table_path, data).map_err(StorageError::from)?;
        let min_key = entries.first().map(|e| e.key.clone()).unwrap_or_default();
        let max_key = entries.last().map(|e| e.key.clone()).unwrap_or_default();
        self.manifest.sstables.push(SstMeta {
            file: filename,
            max_sequence,
            min_key,
            max_key,
        });
        self.memtable.clear();
        self.memtable_bytes = 0;
        // reset wal
        fs::write(cf_path.join(WAL_FILE), &[]).map_err(StorageError::from)?;
        engine.invalidate_table(&table_path);
        Ok(())
    }

    fn compact(&mut self, cf_path: &Path, engine: &EngineInner) -> StorageResult<()> {
        if self.manifest.sstables.len() < 2 {
            return Ok(());
        }
        let mut merged = BTreeMap::<Vec<u8>, TableEntry>::new();
        for meta in &self.manifest.sstables {
            let table_path = cf_path.join(&meta.file);
            let entries = engine.load_table(&table_path)?;
            for entry in entries.iter() {
                match merged.get(&entry.key) {
                    Some(existing) if existing.sequence > entry.sequence => {}
                    _ => {
                        merged.insert(entry.key.clone(), entry.clone());
                    }
                }
            }
        }
        let mut entries: Vec<TableEntry> = merged.into_values().collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key).then(a.sequence.cmp(&b.sequence)));
        let filename = format!("sst-{:020}.bin", self.manifest.next_file_id);
        self.manifest.next_file_id += 1;
        let max_sequence = entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let data = encode_table(&entries);
        let table_path = cf_path.join(&filename);
        fs::write(&table_path, data).map_err(StorageError::from)?;
        let min_key = entries.first().map(|e| e.key.clone()).unwrap_or_default();
        let max_key = entries.last().map(|e| e.key.clone()).unwrap_or_default();
        for meta in &self.manifest.sstables {
            let old_path = cf_path.join(&meta.file);
            let _ = fs::remove_file(&old_path);
            engine.invalidate_table(&old_path);
        }
        self.manifest.sstables = vec![SstMeta {
            file: filename,
            max_sequence,
            min_key,
            max_key,
        }];
        Ok(())
    }

    fn ensure_key_ranges(&mut self, cf_path: &Path, engine: &EngineInner) -> StorageResult<bool> {
        let mut updated = false;
        for meta in &mut self.manifest.sstables {
            if meta.min_key.is_empty() || meta.max_key.is_empty() {
                let table_path = cf_path.join(&meta.file);
                let entries = engine.load_table(&table_path)?;
                if let Some(first) = entries.first() {
                    meta.min_key = first.key.clone();
                }
                if let Some(last) = entries.last() {
                    meta.max_key = last.key.clone();
                }
                updated = true;
            }
        }
        Ok(updated)
    }

    fn scan(
        &self,
        cf_path: &Path,
        engine: &EngineInner,
        prefix: &[u8],
    ) -> StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut merged: BTreeMap<Vec<u8>, TableEntry> = BTreeMap::new();
        for entry in self.memtable.values() {
            merged.insert(entry.key.clone(), entry.clone());
        }
        for meta in self.manifest.sstables.iter().rev() {
            let table_path = cf_path.join(&meta.file);
            let entries = engine.load_table(&table_path)?;
            for entry in entries.iter() {
                merged
                    .entry(entry.key.clone())
                    .and_modify(|existing| {
                        if existing.sequence < entry.sequence {
                            *existing = entry.clone();
                        }
                    })
                    .or_insert(entry.clone());
            }
        }
        let mut data = Vec::new();
        for (key, entry) in merged {
            if !key.starts_with(prefix) {
                continue;
            }
            if let Some(value) = entry.value_bytes() {
                data.push((key, value.to_vec()));
            }
        }
        Ok(data)
    }

    fn get(
        &self,
        cf_path: &Path,
        engine: &EngineInner,
        key: &[u8],
    ) -> StorageResult<Option<Vec<u8>>> {
        if let Some(entry) = self.memtable.get(key) {
            return Ok(entry.value_bytes().map(|v| v.to_vec()));
        }
        for meta in self.manifest.sstables.iter().rev() {
            let table_path = cf_path.join(&meta.file);
            let entries = engine.load_table(&table_path)?;
            for entry in entries.iter().rev() {
                if entry.key == key {
                    return Ok(entry.value_bytes().map(|v| v.to_vec()));
                }
            }
        }
        Ok(None)
    }
}

fn read_table(path: &Path) -> StorageResult<Vec<TableEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read(path).map_err(StorageError::from)?;
    decode_table(&data)
}

fn encode_table(entries: &[TableEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + entries.len() * 32);
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for entry in entries {
        write_bytes(&mut out, &entry.key);
        out.extend_from_slice(&entry.sequence.to_le_bytes());
        match &entry.value {
            ValueState::Present(value) => {
                out.extend_from_slice(&0u32.to_le_bytes());
                write_bytes(&mut out, value);
            }
            ValueState::Tombstone => {
                out.extend_from_slice(&1u32.to_le_bytes());
            }
        }
    }
    out
}

fn write_bytes(buffer: &mut Vec<u8>, bytes: &[u8]) {
    buffer.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buffer.extend_from_slice(bytes);
}

fn decode_table(data: &[u8]) -> StorageResult<Vec<TableEntry>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut decoder = TableDecoder::new(data);
    let count = decoder.read_u64()? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = decoder.read_bytes()?;
        let sequence = decoder.read_u64()?;
        let variant = decoder.read_u32()?;
        let value = match variant {
            0 => ValueState::Present(decoder.read_bytes()?),
            1 => ValueState::Tombstone,
            other => {
                return Err(StorageError::backend(format!(
                    "unknown value variant {other} in table"
                )))
            }
        };
        entries.push(TableEntry {
            key,
            sequence,
            value,
        });
    }
    if decoder.remaining() != 0 {
        return Err(StorageError::backend("corrupt table: trailing bytes"));
    }
    Ok(entries)
}

struct TableDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TableDecoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_exact(&mut self, len: usize) -> StorageResult<&'a [u8]> {
        if self.pos + len > self.data.len() {
            return Err(StorageError::backend(
                "corrupt table: unexpected end of data",
            ));
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn read_u64(&mut self) -> StorageResult<u64> {
        let bytes = self.read_exact(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(array))
    }

    fn read_u32(&mut self) -> StorageResult<u32> {
        let bytes = self.read_exact(4)?;
        let mut array = [0u8; 4];
        array.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(array))
    }

    fn read_bytes(&mut self) -> StorageResult<Vec<u8>> {
        let len = self.read_u64()? as usize;
        let bytes = self.read_exact(len)?;
        Ok(bytes.to_vec())
    }
}

fn byte_cost(entry: &TableEntry) -> usize {
    let value_len = entry.value_bytes().map(|v| v.len()).unwrap_or(0);
    entry.key.len() + value_len + std::mem::size_of::<u64>() + 1
}

#[derive(Debug)]
struct WalRecord {
    key: Vec<u8>,
    sequence: u64,
    kind: WalKind,
}

#[derive(Debug)]
enum WalKind {
    Put { value: Vec<u8> },
    Delete,
}

impl WalRecord {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("key".to_string(), encode_bytes(&self.key));
        map.insert(
            "sequence".to_string(),
            Value::Number(Number::from(self.sequence)),
        );
        map.insert("kind".to_string(), self.kind.to_value());
        Value::Object(map)
    }

    fn from_value(value: Value) -> StorageResult<Self> {
        let mut map = expect_object(value, "wal record")?;
        let key = map
            .remove("key")
            .map(|value| decode_bytes(value, "wal record key"))
            .transpose()?
            .ok_or_else(|| StorageError::backend("missing field 'key' in wal record"))?;
        let sequence = take_u64(&mut map, "sequence", "wal record")?;
        let kind_value = map
            .remove("kind")
            .ok_or_else(|| StorageError::backend("missing field 'kind' in wal record"))?;
        let kind = WalKind::from_value(kind_value)?;
        Ok(Self {
            key,
            sequence,
            kind,
        })
    }
}

impl WalKind {
    fn to_value(&self) -> Value {
        match self {
            WalKind::Put { value } => {
                let mut inner = Map::new();
                inner.insert("value".to_string(), encode_bytes(value));
                let mut outer = Map::new();
                outer.insert("Put".to_string(), Value::Object(inner));
                Value::Object(outer)
            }
            WalKind::Delete => Value::String("Delete".to_string()),
        }
    }

    fn from_value(value: Value) -> StorageResult<Self> {
        match value {
            Value::String(s) if s == "Delete" => Ok(WalKind::Delete),
            Value::Object(mut outer) => {
                if let Some(inner) = outer.remove("Put") {
                    let mut inner = expect_object(inner, "wal kind Put")?;
                    let value = inner
                        .remove("value")
                        .map(|value| decode_bytes(value, "wal kind Put value"))
                        .transpose()?
                        .ok_or_else(|| {
                            StorageError::backend("missing field 'value' for wal kind Put")
                        })?;
                    Ok(WalKind::Put { value })
                } else if let Some(inner) = outer.remove("Delete") {
                    // Older serde_json encodings for unit variants sometimes rendered
                    // {"Delete":{}}; accept that as well.
                    let _ = expect_object(inner, "wal kind Delete")?;
                    Ok(WalKind::Delete)
                } else {
                    Err(StorageError::backend("unknown wal kind variant"))
                }
            }
            _ => Err(StorageError::backend("invalid wal kind encoding")),
        }
    }
}

impl KeyValue for InhouseEngine {
    type Batch = InhouseBatch;
    type Iter = InhouseIterator;

    fn open(path: &str) -> StorageResult<Self> {
        InhouseEngine::open(path)
    }

    fn flush_wal(&self) -> StorageResult<()> {
        let handles = self.inner.cfs.read().unwrap().clone();
        for handle in handles.values() {
            let wal_path = handle.path.join(WAL_FILE);
            if wal_path.exists() {
                let file = OpenOptions::new()
                    .append(true)
                    .open(&wal_path)
                    .map_err(StorageError::from)?;
                file.sync_all().map_err(StorageError::from)?;
            }
        }
        Ok(())
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        let _ = self.cf_handle(cf)?;
        Ok(())
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.with_cf(cf, |state, path, engine| state.get(path, engine, key))
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let previous = self.get(cf, key)?;
        self.put_bytes(cf, key, value)?;
        Ok(previous)
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        let limit = *self.inner.memtable_limit.read().unwrap();
        self.with_cf(cf, |state, path, engine| {
            let sequence = state.allocate_sequence();
            let record = WalRecord {
                key: key.to_vec(),
                sequence,
                kind: WalKind::Put {
                    value: value.to_vec(),
                },
            };
            state.append_wal(path, &record)?;
            state.apply_record(record);
            state.maybe_flush(path, engine, limit)
        })
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let previous = self.get(cf, key)?;
        let limit = *self.inner.memtable_limit.read().unwrap();
        self.with_cf(cf, |state, path, engine| {
            let sequence = state.allocate_sequence();
            let record = WalRecord {
                key: key.to_vec(),
                sequence,
                kind: WalKind::Delete,
            };
            state.append_wal(path, &record)?;
            state.apply_record(record);
            state.maybe_flush(path, engine, limit)
        })?;
        Ok(previous)
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter> {
        let data = self.with_cf(cf, |state, path, engine| state.scan(path, engine, prefix))?;
        Ok(InhouseIterator { data, index: 0 })
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        let manifest = self.inner.manifest.read().unwrap();
        Ok(manifest.cfs.keys().cloned().collect())
    }

    fn make_batch(&self) -> Self::Batch {
        InhouseBatch::default()
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        for op in batch.ops {
            match op {
                BatchOp::Put { cf, key, value } => {
                    self.put_bytes(&cf, &key, &value)?;
                }
                BatchOp::Delete { cf, key } => {
                    let _ = self.delete(&cf, &key)?;
                }
            }
        }
        Ok(())
    }

    fn flush(&self) -> StorageResult<()> {
        let limit = *self.inner.memtable_limit.read().unwrap();
        let handles = self.inner.cfs.read().unwrap().clone();
        for handle in handles.values() {
            let mut state = handle.state.lock().unwrap();
            state.maybe_flush(&handle.path, &self.inner, limit)?;
        }
        Ok(())
    }

    fn compact(&self) -> StorageResult<()> {
        let handles = self.inner.cfs.read().unwrap().clone();
        for handle in handles.values() {
            let mut state = handle.state.lock().unwrap();
            state.flush_memtable(&handle.path, &self.inner)?;
            state.compact(&handle.path, &self.inner)?;
        }
        Ok(())
    }

    fn set_byte_limit(&self, limit: Option<usize>) -> StorageResult<()> {
        *self.inner.memtable_limit.write().unwrap() = limit;
        Ok(())
    }

    fn metrics(&self) -> StorageResult<StorageMetrics> {
        let handles = self.inner.cfs.read().unwrap().clone();
        let mut mem_bytes = 0usize;
        let mut sst_bytes = 0u64;
        for handle in handles.values() {
            let state = handle.state.lock().unwrap();
            mem_bytes += state.memtable_bytes;
            for meta in &state.manifest.sstables {
                let path = handle.path.join(&meta.file);
                if let Ok(metadata) = fs::metadata(path) {
                    sst_bytes += metadata.len();
                }
            }
        }
        Ok(StorageMetrics {
            backend: "inhouse",
            memtable_bytes: Some(mem_bytes as u64),
            total_sst_bytes: Some(sst_bytes),
            pending_compactions: None,
            running_compactions: None,
            level0_files: None,
            size_on_disk_bytes: Some((mem_bytes as u64) + sst_bytes),
        })
    }
}
