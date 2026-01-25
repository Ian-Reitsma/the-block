#![allow(clippy::len_without_is_empty)]
#![forbid(unsafe_code)]

use foundation_lazy::sync::OnceCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use storage_engine::inhouse_engine::InhouseEngine;
use storage_engine::{KeyValue, KeyValueIterator, StorageError};
use sys::{error::SysError, tempfile::TempDir};
use thiserror::Error;

#[cfg(feature = "legacy-format")]
mod legacy;

pub type Result<T> = std::result::Result<T, Error>;
pub type IVec = Vec<u8>;

const DEFAULT_CF: &str = "default";
const INHOUSE_SUBDIR: &str = "inhouse";

#[derive(Debug, Error, Clone)]
pub enum Error {
    #[error("{0}")]
    Unsupported(Box<str>),
    #[error("{0}")]
    ReportableBug(Box<str>),
    #[error("io error: {0}")]
    Io(String),
    #[error("storage error: {0}")]
    Storage(String),
}

impl From<StorageError> for Error {
    fn from(err: StorageError) -> Self {
        Error::Storage(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<SysError> for Error {
    fn from(err: SysError) -> Self {
        match err {
            SysError::Io(io) => Error::Io(io.to_string()),
            SysError::Unsupported(feature) => Error::Unsupported(feature.into()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    path: Option<PathBuf>,
    temporary: bool,
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn temporary(mut self, value: bool) -> Self {
        self.temporary = value;
        self
    }

    pub fn open(self) -> Result<Db> {
        let base = if self.temporary {
            None
        } else {
            Some(
                self.path
                    .as_ref()
                    .ok_or_else(|| Error::Unsupported("missing sled path".into()))?
                    .to_path_buf(),
            )
        };
        Db::open_with_base(base, self.temporary)
    }
}

pub fn open<P: AsRef<Path>>(path: P) -> Result<Db> {
    Config::default().path(path).open()
}

#[derive(Clone)]
pub struct Db {
    engine: InhouseEngine,
    base: Option<PathBuf>,
    temp_dir: Option<Arc<TempDir>>,
}

impl Db {
    fn keep_temp_dir_alive(&self) {
        let _ = self.temp_dir.as_ref();
        let _ = self.base.as_ref();
    }

    fn open_with_base(base: Option<PathBuf>, temporary: bool) -> Result<Self> {
        if temporary {
            let temp_dir = TempDir::new()?;
            let data_path = temp_dir.path().join(INHOUSE_SUBDIR);
            fs::create_dir_all(&data_path)?;
            let engine = InhouseEngine::open(path_to_str(&data_path)?)?;
            engine.ensure_cf(DEFAULT_CF)?;
            Ok(Self {
                engine,
                base: None,
                temp_dir: Some(Arc::new(temp_dir)),
            })
        } else {
            let base = base.expect("non-temporary db requires path");
            fs::create_dir_all(&base)?;
            let data_path = base.join(INHOUSE_SUBDIR);
            fs::create_dir_all(&data_path)?;
            #[cfg(feature = "legacy-format")]
            migrate_legacy_if_needed(&base, &data_path)?;
            #[cfg(not(feature = "legacy-format"))]
            ensure_no_legacy(&base, &data_path)?;
            let engine = InhouseEngine::open(path_to_str(&data_path)?)?;
            engine.ensure_cf(DEFAULT_CF)?;
            Ok(Self {
                engine,
                base: Some(base),
                temp_dir: None,
            })
        }
    }

    pub fn open_tree<S: AsRef<str>>(&self, name: S) -> Result<Tree> {
        self.keep_temp_dir_alive();
        let name = name.as_ref().to_string();
        self.engine.ensure_cf(&name)?;
        Ok(Tree {
            engine: self.engine.clone(),
            name,
        })
    }

    pub fn insert<K, V>(&self, key: K, value: V) -> Result<Option<IVec>>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.open_tree(DEFAULT_CF)?.insert(key, value)
    }

    pub fn remove<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<IVec>> {
        self.open_tree(DEFAULT_CF)?.remove(key)
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<IVec>> {
        self.open_tree(DEFAULT_CF)?.get(key)
    }

    pub fn iter(&self) -> Iter {
        self.open_tree(DEFAULT_CF).expect("default cf").iter()
    }

    pub fn flush(&self) -> Result<()> {
        self.engine.flush()?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.open_tree(DEFAULT_CF)
            .ok()
            .map(|tree| tree.len())
            .unwrap_or(0)
    }

    pub fn tree_names(&self) -> Result<Vec<IVec>> {
        let names = self.engine.list_cfs()?;
        Ok(names.into_iter().map(|name| name.into_bytes()).collect())
    }
}

#[derive(Clone)]
pub struct Tree {
    engine: InhouseEngine,
    name: String,
}

impl Tree {
    pub fn insert<K, V>(&self, key: K, value: V) -> Result<Option<IVec>>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let previous = self.engine.put(&self.name, key.as_ref(), value.as_ref())?;
        Ok(previous)
    }

    pub fn remove<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<IVec>> {
        let previous = self.engine.delete(&self.name, key.as_ref())?;
        Ok(previous)
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<IVec>> {
        Ok(self.engine.get(&self.name, key.as_ref())?)
    }

    pub fn iter(&self) -> Iter {
        self.scan_prefix(Vec::<u8>::new())
    }

    pub fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Iter {
        let mut entries = Vec::new();
        match self.engine.prefix_iterator(&self.name, prefix.as_ref()) {
            Ok(mut inner) => loop {
                match inner.next() {
                    Ok(Some((key, value))) => entries.push(Ok((key, value))),
                    Ok(None) => break,
                    Err(err) => {
                        entries.push(Err(Error::from(err)));
                        break;
                    }
                }
            },
            Err(err) => entries.push(Err(Error::from(err))),
        }
        Iter {
            data: entries,
            index: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn flush(&self) -> Result<()> {
        self.engine.flush()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let keys: Vec<Vec<u8>> = self
            .iter()
            .filter_map(|res| res.ok().map(|(k, _)| k))
            .collect();
        for key in keys {
            let _ = self.engine.delete(&self.name, &key)?;
        }
        Ok(())
    }

    pub fn compare_and_swap<K: AsRef<[u8]>>(
        &self,
        key: K,
        expected: Option<IVec>,
        replacement: Option<IVec>,
    ) -> Result<std::result::Result<Option<IVec>, Option<IVec>>> {
        let current = self.engine.get(&self.name, key.as_ref())?;
        if current == expected {
            match replacement {
                Some(ref value) => {
                    self.engine.put_bytes(&self.name, key.as_ref(), value)?;
                }
                None => {
                    let _ = self.engine.delete(&self.name, key.as_ref())?;
                }
            }
            Ok(Ok(current))
        } else {
            Ok(Err(current))
        }
    }
}

pub struct Iter {
    data: Vec<Result<(IVec, IVec)>>,
    index: usize,
}

impl Iterator for Iter {
    type Item = Result<(IVec, IVec)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.data.len() {
            None
        } else {
            let item = self.data[self.index].clone();
            self.index += 1;
            Some(item)
        }
    }
}

fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Unsupported("invalid database path".into()))
}

#[cfg(feature = "legacy-format")]
fn migrate_legacy_if_needed(base: &Path, data_path: &Path) -> Result<()> {
    if fs::read_dir(data_path)?.next().is_some() {
        return Ok(());
    }
    let legacy_entries = fs::read_dir(base)?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.file_name() != INHOUSE_SUBDIR)
        .count();
    if legacy_entries == 0 {
        return Ok(());
    }
    let legacy_db = legacy::Config::new().path(base).open()?;
    let engine = InhouseEngine::open(path_to_str(data_path)?)?;
    for name in legacy_db.tree_names() {
        let name_bytes = name.to_vec();
        let name_str = String::from_utf8(name_bytes.clone())
            .unwrap_or_else(|_| crypto_suite::hex::encode(&name_bytes));
        engine.ensure_cf(&name_str)?;
        let tree = legacy_db.open_tree(&name_bytes)?;
        let mut iter = tree.iter();
        while let Some(entry) = iter.next() {
            let (key, value) = entry?;
            engine.put_bytes(&name_str, key.as_ref(), value.as_ref())?;
        }
    }
    legacy_db.flush()?;
    Ok(())
}

#[cfg(not(feature = "legacy-format"))]
fn ensure_no_legacy(base: &Path, data_path: &Path) -> Result<()> {
    if fs::read_dir(data_path)?.next().is_some() {
        return Ok(());
    }
    let legacy_entries = fs::read_dir(base)?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.file_name() != INHOUSE_SUBDIR)
        .count();
    if legacy_entries > 0 {
        return Err(Error::Unsupported(
            "legacy sled data detected; rebuild with --features legacy-format to migrate".into(),
        ));
    }
    Ok(())
}

static ENGINE_CACHE: OnceCell<RwLock<HashMap<PathBuf, InhouseEngine>>> = OnceCell::new();

fn cached_engine(path: &Path) -> Result<InhouseEngine> {
    let cache = ENGINE_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(engine) = cache.read().unwrap().get(path) {
        return Ok(engine.clone());
    }
    let engine = InhouseEngine::open(path_to_str(path)?)?;
    cache
        .write()
        .unwrap()
        .insert(path.to_path_buf(), engine.clone());
    Ok(engine)
}

impl Tree {
    pub fn with_path(path: &Path, name: &str) -> Result<Self> {
        let engine = cached_engine(path)?;
        engine.ensure_cf(name)?;
        Ok(Self {
            engine,
            name: name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    use sys::tempfile::tempdir;

    fn open_persistent() -> (Db, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.keep();
        let db = Config::new().path(&path).open().unwrap();
        (db, path)
    }

    #[test]
    fn insert_get_roundtrip() {
        let (db, path) = open_persistent();
        assert_eq!(db.get(b"missing").unwrap(), None);
        assert_eq!(db.insert(b"key", b"value").unwrap(), None);
        assert_eq!(db.get(b"key").unwrap(), Some(b"value".to_vec()));
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn tree_iteration_collects_items() {
        let (db, path) = open_persistent();
        let tree = db.open_tree("iter").unwrap();
        tree.insert(b"a", b"1").unwrap();
        tree.insert(b"b", b"2").unwrap();
        tree.insert(b"c", b"3").unwrap();

        let mut values: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
        for entry in tree.iter() {
            let (k, v) = entry.unwrap();
            values.insert(k, v);
        }

        assert_eq!(values.get(&b"a".to_vec()), Some(&b"1".to_vec()));
        assert_eq!(values.get(&b"b".to_vec()), Some(&b"2".to_vec()));
        assert_eq!(values.get(&b"c".to_vec()), Some(&b"3".to_vec()));
        assert_eq!(values.len(), 3);
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn scan_prefix_filters_entries() {
        let (db, path) = open_persistent();
        let tree = db.open_tree("prefix").unwrap();
        tree.insert(b"user:1", b"alice").unwrap();
        tree.insert(b"user:2", b"bob").unwrap();
        tree.insert(b"order:1", b"paid").unwrap();

        let mut users = Vec::new();
        for entry in tree.scan_prefix(b"user:") {
            let (key, value) = entry.unwrap();
            users.push((key, value));
        }
        users.sort();

        assert_eq!(users.len(), 2);
        assert_eq!(users[0], (b"user:1".to_vec(), b"alice".to_vec()));
        assert_eq!(users[1], (b"user:2".to_vec(), b"bob".to_vec()));
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn compare_and_swap_matches_sled_semantics() {
        let (db, path) = open_persistent();
        let tree = db.open_tree("cas").unwrap();
        tree.insert(b"key", b"value").unwrap();

        let ok = tree
            .compare_and_swap(b"key", Some(b"value".to_vec()), Some(b"next".to_vec()))
            .unwrap();
        assert!(ok.is_ok());
        assert_eq!(ok.unwrap(), Some(b"value".to_vec()));
        assert_eq!(tree.get(b"key").unwrap(), Some(b"next".to_vec()));

        let err = tree
            .compare_and_swap(b"key", Some(b"value".to_vec()), None)
            .unwrap();
        assert_eq!(err, Err(Some(b"next".to_vec())));
        assert_eq!(tree.get(b"key").unwrap(), Some(b"next".to_vec()));
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }

    #[cfg(feature = "legacy-format")]
    #[test]
    fn migrates_existing_legacy_database() {
        let dir = tempdir().unwrap();
        let path = dir.keep();
        let legacy = legacy::Config::new().path(&path).open().unwrap();
        let legacy_tree = legacy.open_tree("old").unwrap();
        legacy_tree.insert(b"hello", b"world").unwrap();
        legacy_tree.insert(b"prefix:1", b"one").unwrap();
        drop(legacy_tree);
        legacy.flush().unwrap();
        drop(legacy);

        let db = Config::new().path(&path).open().unwrap();
        let tree = db.open_tree("old").unwrap();
        assert_eq!(tree.get(b"hello").unwrap(), Some(b"world".to_vec()));
        let entries: Vec<_> = tree
            .scan_prefix(b"prefix:")
            .map(|res| res.unwrap())
            .collect();
        assert_eq!(entries, vec![(b"prefix:1".to_vec(), b"one".to_vec())]);
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }
}
