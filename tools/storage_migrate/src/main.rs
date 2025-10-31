use crypto_suite::{hashing::blake3::Hasher, hex};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;
use std::process::exit;

use storage_engine::inhouse_engine::InhouseEngine;
use storage_engine::memory_engine::MemoryEngine;
use storage_engine::rocksdb_engine::RocksDbEngine;
use storage_engine::{KeyValue, KeyValueIterator, StorageError, StorageResult};

fn usage() {
    eprintln!("usage:");
    eprintln!("  storage_migrate migrate <source_dir> <dest_dir> <dest_engine>");
    eprintln!("  storage_migrate checksum <dir> [--scope=contracts|all] [--json]");
    eprintln!("       dest_engine: memory | inhouse | rocksdb-compat");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChecksumScopeArg {
    Contracts,
    All,
}

impl ChecksumScopeArg {
    fn label(self) -> &'static str {
        match self {
            ChecksumScopeArg::Contracts => "contracts",
            ChecksumScopeArg::All => "all",
        }
    }
}

enum EngineWrapper {
    Memory(MemoryEngine),
    Inhouse(InhouseEngine),
    RocksDb(RocksDbEngine),
}

impl EngineWrapper {
    fn open_source(path: &str) -> Result<Self, String> {
        if let Ok(engine) = InhouseEngine::open(path) {
            return Ok(EngineWrapper::Inhouse(engine));
        }
        if let Ok(engine) = RocksDbEngine::open(path) {
            return Ok(EngineWrapper::RocksDb(engine));
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
            "rocksdb-compat" | "rocksdb" => RocksDbEngine::open(path)
                .map(EngineWrapper::RocksDb)
                .map_err(|e| e.to_string()),
            other => Err(format!("unsupported destination engine `{other}`")),
        }
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        match self {
            EngineWrapper::Memory(engine) => engine.ensure_cf(cf),
            EngineWrapper::Inhouse(engine) => engine.ensure_cf(cf),
            EngineWrapper::RocksDb(engine) => engine.ensure_cf(cf),
        }
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        match self {
            EngineWrapper::Memory(engine) => engine.list_cfs(),
            EngineWrapper::Inhouse(engine) => engine.list_cfs(),
            EngineWrapper::RocksDb(engine) => engine.list_cfs(),
        }
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        match self {
            EngineWrapper::Memory(engine) => engine.put_bytes(cf, key, value),
            EngineWrapper::Inhouse(engine) => engine.put_bytes(cf, key, value),
            EngineWrapper::RocksDb(engine) => engine.put_bytes(cf, key, value),
        }
    }

    fn flush(&self) -> StorageResult<()> {
        match self {
            EngineWrapper::Memory(engine) => engine.flush(),
            EngineWrapper::Inhouse(engine) => engine.flush(),
            EngineWrapper::RocksDb(engine) => engine.flush(),
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
    if args.len() < 2 {
        usage();
        exit(1);
    }

    let result = match args[1].as_str() {
        "migrate" => run_migrate(&args[2..]),
        "checksum" | "export" => run_checksum(&args[2..]),
        _ => {
            if args.len() == 4 {
                run_migrate(&args[1..])
            } else {
                usage();
                Err(String::new())
            }
        }
    };

    if let Err(err) = result {
        if !err.is_empty() {
            eprintln!("{err}");
        }
        exit(1);
    }
}

fn run_migrate(args: &[String]) -> Result<(), String> {
    if args.len() != 3 {
        return Err("expected <source_dir> <dest_dir> <dest_engine>".into());
    }
    let source = &args[0];
    let dest = &args[1];
    let engine = &args[2];

    if !Path::new(source).exists() {
        return Err(format!("source directory `{source}` does not exist"));
    }

    fs::create_dir_all(dest)
        .map_err(|err| format!("failed to create destination directory `{dest}`: {err}"))?;

    let source_engine = EngineWrapper::open_source(source)?;
    let dest_engine = EngineWrapper::open_dest(engine, dest)?;

    let entries = collect_entries(&source_engine)
        .map_err(|err| format!("failed to read source entries: {err}"))?;
    let expected = compute_checksum(&entries);

    write_entries(&dest_engine, &entries)
        .map_err(|err| format!("failed to write destination entries: {err}"))?;

    let dest_entries = collect_entries(&dest_engine)
        .map_err(|err| format!("failed to verify destination entries: {err}"))?;
    let actual = compute_checksum(&dest_entries);

    if expected != actual {
        return Err("checksum mismatch after migration".into());
    }

    let cf_count = dest_entries
        .iter()
        .map(|(cf, _, _)| cf)
        .collect::<HashSet<_>>()
        .len();
    println!(
        "migrated {} entries across {} column families",
        entries.len(),
        cf_count
    );
    Ok(())
}

fn run_checksum(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("expected <dir> for checksum".into());
    }

    let mut dir: Option<String> = None;
    let mut scope = ChecksumScopeArg::Contracts;
    let mut json = false;

    for arg in args {
        if arg == "--json" {
            json = true;
        } else if let Some(value) = arg.strip_prefix("--scope=") {
            scope = match value.to_ascii_lowercase().as_str() {
                "contracts" | "contract" | "market" | "market/contracts" => {
                    ChecksumScopeArg::Contracts
                }
                "all" | "all-cfs" | "full" => ChecksumScopeArg::All,
                other => return Err(format!("unsupported scope `{other}`")),
            };
        } else if arg.starts_with("--") {
            return Err(format!("unknown flag `{arg}`"));
        } else if dir.is_none() {
            dir = Some(arg.clone());
        } else {
            return Err(format!("unexpected argument `{arg}`"));
        }
    }

    let dir = dir.ok_or_else(|| "missing database directory".to_string())?;
    if !Path::new(&dir).exists() {
        return Err(format!("directory `{dir}` does not exist"));
    }

    let engine = EngineWrapper::open_source(&dir)?;
    let entries =
        collect_entries(&engine).map_err(|err| format!("failed to read entries: {err}"))?;
    let filtered: Vec<(String, Vec<u8>, Vec<u8>)> = match scope {
        ChecksumScopeArg::Contracts => entries
            .into_iter()
            .filter(|(cf, _, _)| cf == "market/contracts")
            .collect(),
        ChecksumScopeArg::All => entries,
    };
    let checksum = compute_checksum(&filtered);
    let hash = hex::encode(checksum);

    if json {
        println!(
            "{{\"directory\":\"{}\",\"scope\":\"{}\",\"entries\":{},\"hash\":\"{}\"}}",
            dir,
            scope.label(),
            filtered.len(),
            hash
        );
    } else {
        println!(
            "directory {} scope {} checksum: {} (entries: {})",
            dir,
            scope.label(),
            hash,
            filtered.len()
        );
    }

    Ok(())
}
