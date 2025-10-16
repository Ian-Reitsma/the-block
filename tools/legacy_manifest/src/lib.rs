#![forbid(unsafe_code)]

use crypto_suite::hex;
use foundation_serialization::json::{self, Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use storage_engine::inhouse_engine::InhouseEngine;
use storage_engine::memory_engine::MemoryEngine;
use storage_engine::rocksdb_engine::RocksDbEngine;
use storage_engine::{KeyValue, KeyValueIterator, StorageError, StorageResult};

#[derive(Debug)]
pub enum ExportError {
    Io(std::io::Error),
    Storage(StorageError),
    Serialize(foundation_serialization::Error),
    Usage(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Io(err) => write!(f, "io error: {err}"),
            ExportError::Storage(err) => write!(f, "storage error: {err}"),
            ExportError::Serialize(err) => write!(f, "serialization error: {err}"),
            ExportError::Usage(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<std::io::Error> for ExportError {
    fn from(err: std::io::Error) -> Self {
        ExportError::Io(err)
    }
}

impl From<StorageError> for ExportError {
    fn from(err: StorageError) -> Self {
        ExportError::Storage(err)
    }
}

impl From<foundation_serialization::Error> for ExportError {
    fn from(err: foundation_serialization::Error) -> Self {
        ExportError::Serialize(err)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Auto,
    Inhouse,
    RocksDb,
    Memory,
}

impl EngineKind {
    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "inhouse" => Some(EngineKind::Inhouse),
            "rocksdb" | "rocksdb-compat" => Some(EngineKind::RocksDb),
            "memory" => Some(EngineKind::Memory),
            "auto" => Some(EngineKind::Auto),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    source: PathBuf,
    output: Option<PathBuf>,
    engine: EngineKind,
}

impl Config {
    pub fn new(source: PathBuf) -> Self {
        Self {
            source,
            output: None,
            engine: EngineKind::Auto,
        }
    }

    pub fn with_output(mut self, output: PathBuf) -> Self {
        self.output = Some(output);
        self
    }

    pub fn with_engine(mut self, engine: EngineKind) -> Self {
        self.engine = engine;
        self
    }
}

enum Engine {
    Inhouse(InhouseEngine),
    RocksDb(RocksDbEngine),
    Memory(MemoryEngine),
}

impl Engine {
    fn open(path: &Path, kind: EngineKind) -> Result<Self, ExportError> {
        let path_str = path
            .to_str()
            .ok_or_else(|| ExportError::Usage("database path must be valid UTF-8".into()))?;
        match kind {
            EngineKind::Inhouse => InhouseEngine::open(path_str)
                .map(Engine::Inhouse)
                .map_err(ExportError::from),
            EngineKind::RocksDb => RocksDbEngine::open(path_str)
                .map(Engine::RocksDb)
                .map_err(ExportError::from),
            EngineKind::Memory => MemoryEngine::open(path_str)
                .map(Engine::Memory)
                .map_err(ExportError::from),
            EngineKind::Auto => {
                if let Ok(engine) = InhouseEngine::open(path_str) {
                    return Ok(Engine::Inhouse(engine));
                }
                if let Ok(engine) = RocksDbEngine::open(path_str) {
                    return Ok(Engine::RocksDb(engine));
                }
                if let Ok(engine) = MemoryEngine::open(path_str) {
                    return Ok(Engine::Memory(engine));
                }
                Err(ExportError::Usage(format!(
                    "failed to open database at {} with any supported engine",
                    path.display()
                )))
            }
        }
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        match self {
            Engine::Inhouse(engine) => engine.ensure_cf(cf),
            Engine::RocksDb(engine) => engine.ensure_cf(cf),
            Engine::Memory(engine) => engine.ensure_cf(cf),
        }
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        match self {
            Engine::Inhouse(engine) => engine.list_cfs(),
            Engine::RocksDb(engine) => engine.list_cfs(),
            Engine::Memory(engine) => engine.list_cfs(),
        }
    }

    fn prefix_iterator(&self, cf: &str) -> StorageResult<Box<dyn KeyValueIterator + '_>> {
        match self {
            Engine::Inhouse(engine) => Ok(Box::new(engine.prefix_iterator(cf, b"")?)),
            Engine::RocksDb(engine) => Ok(Box::new(engine.prefix_iterator(cf, b"")?)),
            Engine::Memory(engine) => Ok(Box::new(engine.prefix_iterator(cf, b"")?)),
        }
    }
}

pub fn run(config: Config) -> Result<PathBuf, ExportError> {
    if !config.source.exists() {
        return Err(ExportError::Usage(format!(
            "source directory `{}` does not exist",
            config.source.display()
        )));
    }
    let engine = Engine::open(&config.source, config.engine.clone())?;
    let trees = collect_entries(&engine)?;
    let output = config
        .output
        .unwrap_or_else(|| config.source.join("legacy_manifest.json"));
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    write_manifest(&output, &trees)?;
    Ok(output)
}

fn collect_entries(
    engine: &Engine,
) -> Result<BTreeMap<Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>>, ExportError> {
    let mut names = engine.list_cfs()?;
    if !names.iter().any(|cf| cf == "default") {
        engine.ensure_cf("default")?;
        names.push("default".to_string());
    }
    names.sort();
    let mut trees = BTreeMap::new();
    for name in names {
        engine.ensure_cf(&name)?;
        let mut iter = engine.prefix_iterator(&name)?;
        let mut entries = Vec::new();
        while let Some(item) = iter.next()? {
            entries.push(item);
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        trees.insert(name.into_bytes(), entries);
    }
    Ok(trees)
}

fn write_manifest(
    path: &Path,
    trees: &BTreeMap<Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>>,
) -> Result<(), ExportError> {
    let mut trees_obj = Map::new();
    for (name, entries) in trees {
        let mut array = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let mut entry = Map::new();
            entry.insert("key".into(), Value::String(hex::encode(key)));
            entry.insert("value".into(), Value::String(hex::encode(value)));
            array.push(Value::Object(entry));
        }
        trees_obj.insert(hex::encode(name), Value::Array(array));
    }
    let mut root = Map::new();
    root.insert("trees".into(), Value::Object(trees_obj));
    let bytes = json::to_vec(&Value::Object(root))?;
    fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_manifest_for_inhouse_engine() {
        let dir = storage_engine::tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let engine = InhouseEngine::open(path.to_str().unwrap()).expect("open");
        engine.ensure_cf("metrics").expect("cf");
        engine.put_bytes("metrics", b"alpha", b"one").expect("put");
        engine.put_bytes("metrics", b"beta", b"two").expect("put");
        engine.flush().expect("flush");

        let output = path.join("manifest.json");
        let config = Config::new(path.clone())
            .with_output(output.clone())
            .with_engine(EngineKind::Inhouse);
        let written = run(config).expect("export");
        assert_eq!(written, output);

        let data = fs::read(&written).expect("read manifest");
        let value: Value = json::from_slice(&data).expect("parse manifest");
        let trees = value
            .as_object()
            .and_then(|root| root.get("trees"))
            .and_then(Value::as_object)
            .expect("trees map");
        let metrics = trees
            .get(&hex::encode("metrics"))
            .and_then(Value::as_array)
            .expect("metrics array");
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn manifest_sorts_column_families_and_entries() {
        let dir = storage_engine::tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let engine = InhouseEngine::open(path.to_str().unwrap()).expect("open");
        engine.ensure_cf("zzz").expect("cf");
        engine.put_bytes("zzz", b"b", b"2").expect("put");
        engine.put_bytes("zzz", b"a", b"1").expect("put");
        engine.ensure_cf("aaa").expect("cf");
        engine.put_bytes("aaa", b"x", b"9").expect("put");
        engine.flush().expect("flush");

        let output = path.join("ordered.json");
        run(Config::new(path.clone()).with_output(output.clone())).expect("export");
        let data = fs::read(&output).expect("read manifest");
        let value: Value = json::from_slice(&data).expect("parse manifest");
        let trees = value
            .as_object()
            .and_then(|root| root.get("trees"))
            .and_then(Value::as_object)
            .expect("trees map");
        let keys: Vec<_> = trees.keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                hex::encode("aaa"),
                hex::encode("default"),
                hex::encode("zzz")
            ]
        );
        let zzz = trees
            .get(&hex::encode("zzz"))
            .and_then(Value::as_array)
            .expect("zzz array");
        let order: Vec<_> = zzz
            .iter()
            .map(|entry| {
                entry
                    .as_object()
                    .and_then(|obj| obj.get("key"))
                    .and_then(Value::as_str)
                    .expect("key")
                    .to_string()
            })
            .collect();
        assert_eq!(order, vec![hex::encode("a"), hex::encode("b")]);
    }

    #[test]
    fn manifest_includes_default_cf_even_when_missing() {
        let dir = storage_engine::tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let engine = InhouseEngine::open(path.to_str().unwrap()).expect("open");
        engine.ensure_cf("metrics").expect("cf");
        engine.put_bytes("metrics", b"alpha", b"one").expect("put");
        engine.flush().expect("flush");

        let output = path.join("default.json");
        run(Config::new(path.clone()).with_output(output.clone())).expect("export");
        let data = fs::read(&output).expect("read manifest");
        let value: Value = json::from_slice(&data).expect("parse manifest");
        let trees = value
            .as_object()
            .and_then(|root| root.get("trees"))
            .and_then(Value::as_object)
            .expect("trees map");
        assert!(trees.contains_key(&hex::encode("default")));
        let default_entries = trees
            .get(&hex::encode("default"))
            .and_then(Value::as_array)
            .expect("default array");
        assert!(default_entries.is_empty());
    }
}
