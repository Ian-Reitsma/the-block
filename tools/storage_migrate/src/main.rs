use blake3::Hasher;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;
use std::process::exit;

use storage_engine::inhouse_engine::InhouseEngine;
use storage_engine::memory_engine::MemoryEngine;
use storage_engine::rocksdb_engine::RocksDbEngine;
use storage_engine::sled_engine::SledEngine;
use storage_engine::{KeyValue, KeyValueIterator, StorageError, StorageResult};

fn usage() {
    eprintln!("usage: storage_migrate <source_dir> <dest_dir> <dest_engine>");
    eprintln!("       dest_engine: memory | inhouse | rocksdb | sled");
}

enum EngineWrapper {
    Memory(MemoryEngine),
    Inhouse(InhouseEngine),
    RocksDb(RocksDbEngine),
    Sled(SledEngine),
}

impl EngineWrapper {
    fn open_source(path: &str) -> Result<Self, String> {
        if let Ok(engine) = InhouseEngine::open(path) {
            return Ok(EngineWrapper::Inhouse(engine));
        }
        if let Ok(engine) = RocksDbEngine::open(path) {
            return Ok(EngineWrapper::RocksDb(engine));
        }
        if let Ok(engine) = SledEngine::open(path) {
            return Ok(EngineWrapper::Sled(engine));
        }
        if let Ok(engine) = MemoryEngine::open(path) {
            return Ok(EngineWrapper::Memory(engine));
        }
        Err(format!("failed to open source database at {path}"))
    }

    fn open_dest(kind: &str, path: &str) -> Result<Self, String> {
        match kind {
            "memory" => MemoryEngine::open(path)
                .map(EngineWrapper::Memory)
                .map_err(|e| e.to_string()),
            "inhouse" => InhouseEngine::open(path)
                .map(EngineWrapper::Inhouse)
                .map_err(|e| e.to_string()),
            "rocksdb" => RocksDbEngine::open(path)
                .map(EngineWrapper::RocksDb)
                .map_err(|e| e.to_string()),
            "sled" => SledEngine::open(path)
                .map(EngineWrapper::Sled)
                .map_err(|e| e.to_string()),
            other => Err(format!("unsupported destination engine `{other}`")),
        }
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        match self {
            EngineWrapper::Memory(engine) => engine.ensure_cf(cf),
            EngineWrapper::Inhouse(engine) => engine.ensure_cf(cf),
            EngineWrapper::RocksDb(engine) => engine.ensure_cf(cf),
            EngineWrapper::Sled(engine) => engine.ensure_cf(cf),
        }
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        match self {
            EngineWrapper::Memory(engine) => engine.list_cfs(),
            EngineWrapper::Inhouse(engine) => engine.list_cfs(),
            EngineWrapper::RocksDb(engine) => engine.list_cfs(),
            EngineWrapper::Sled(engine) => engine.list_cfs(),
        }
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        match self {
            EngineWrapper::Memory(engine) => engine.put_bytes(cf, key, value),
            EngineWrapper::Inhouse(engine) => engine.put_bytes(cf, key, value),
            EngineWrapper::RocksDb(engine) => engine.put_bytes(cf, key, value),
            EngineWrapper::Sled(engine) => engine.put_bytes(cf, key, value),
        }
    }

    fn flush(&self) -> StorageResult<()> {
        match self {
            EngineWrapper::Memory(engine) => engine.flush(),
            EngineWrapper::Inhouse(engine) => engine.flush(),
            EngineWrapper::RocksDb(engine) => engine.flush(),
            EngineWrapper::Sled(engine) => engine.flush(),
        }
    }

    fn for_each<F>(&self, cf: &str, mut f: F) -> StorageResult<()>
    where
        F: FnMut(Vec<u8>, Vec<u8>),
    {
        match self {
            EngineWrapper::Memory(engine) => {
                let mut iter = engine.prefix_iterator(cf, b"")?;
                while let Some((key, value)) = iter.next()? {
                    f(key, value);
                }
                Ok(())
            }
            EngineWrapper::Inhouse(engine) => {
                let mut iter = engine.prefix_iterator(cf, b"")?;
                while let Some((key, value)) = iter.next()? {
                    f(key, value);
                }
                Ok(())
            }
            EngineWrapper::RocksDb(engine) => {
                let mut iter = engine.prefix_iterator(cf, b"")?;
                while let Some((key, value)) = iter.next()? {
                    f(key, value);
                }
                Ok(())
            }
            EngineWrapper::Sled(engine) => {
                let mut iter = engine.prefix_iterator(cf, b"")?;
                while let Some((key, value)) = iter.next()? {
                    f(key, value);
                }
                Ok(())
            }
        }
    }
}

fn collect_entries(
    engine: &EngineWrapper,
) -> Result<Vec<(String, Vec<u8>, Vec<u8>)>, StorageError> {
    let mut entries = Vec::new();
    let mut cfs = engine.list_cfs()?;
    if !cfs.iter().any(|cf| cf == "default") {
        engine.ensure_cf("default")?;
        cfs.push("default".to_string());
    }
    cfs.sort();
    for cf in cfs {
        engine.ensure_cf(&cf)?;
        engine.for_each(&cf, |key, value| {
            entries.push((cf.clone(), key, value));
        })?;
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    Ok(entries)
}

fn compute_checksum(entries: &[(String, Vec<u8>, Vec<u8>)]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    for (cf, key, value) in entries {
        hasher.update(cf.as_bytes());
        hasher.update(&[0]);
        hasher.update(key);
        hasher.update(&[0]);
        hasher.update(value);
    }
    hasher.finalize().into()
}

fn write_entries(
    dest: &EngineWrapper,
    entries: &[(String, Vec<u8>, Vec<u8>)],
) -> Result<(), StorageError> {
    let mut ensured = HashSet::new();
    for (cf, key, value) in entries {
        if ensured.insert(cf.clone()) {
            dest.ensure_cf(cf)?;
        }
        dest.put_bytes(cf, key, value)?;
    }
    dest.flush()?;
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        usage();
        exit(1);
    }
    let source = &args[1];
    let dest = &args[2];
    let engine = &args[3];

    if !Path::new(source).exists() {
        eprintln!("source directory `{source}` does not exist");
        exit(1);
    }

    if let Err(err) = fs::create_dir_all(dest) {
        eprintln!("failed to create destination directory `{dest}`: {err}");
        exit(1);
    }

    let source_engine = match EngineWrapper::open_source(source) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("{err}");
            exit(1);
        }
    };

    let dest_engine = match EngineWrapper::open_dest(engine, dest) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("{err}");
            exit(1);
        }
    };

    let entries = match collect_entries(&source_engine) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("failed to read source entries: {err}");
            exit(1);
        }
    };
    let expected = compute_checksum(&entries);

    if let Err(err) = write_entries(&dest_engine, &entries) {
        eprintln!("failed to write destination entries: {err}");
        exit(1);
    }

    let dest_entries = match collect_entries(&dest_engine) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("failed to verify destination entries: {err}");
            exit(1);
        }
    };
    let actual = compute_checksum(&dest_entries);

    if expected != actual {
        eprintln!("checksum mismatch after migration");
        exit(1);
    }

    println!(
        "migrated {} entries across {} column families",
        entries.len(),
        dest_entries
            .iter()
            .map(|(cf, _, _)| cf)
            .collect::<HashSet<_>>()
            .len()
    );
}
