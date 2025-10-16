use crate::{Error, Result};
use crypto_suite::hex;
use foundation_serialization::json::{self, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

const MANIFEST_FILE: &str = "legacy_manifest.json";

#[cfg(feature = "telemetry")]
mod telemetry {
    use foundation_lazy::sync::OnceCell;
    use runtime::telemetry::{IntCounterVec, Opts, Registry, LABEL_REGISTRATION_ERR};

    static REGISTRY: OnceCell<Registry> = OnceCell::new();
    static COUNTER: OnceCell<IntCounterVec> = OnceCell::new();

    fn registry() -> &'static Registry {
        REGISTRY.get_or_init(Registry::new)
    }

    fn counter() -> &'static IntCounterVec {
        COUNTER.get_or_init(|| {
            let registry = registry();
            let counter = IntCounterVec::new(
                Opts::new(
                    "sled_legacy_manifest_errors_total",
                    "Legacy manifest parse and validation errors",
                ),
                &["kind"],
            )
            .expect("create legacy manifest error counter");
            registry
                .register(Box::new(counter.clone()))
                .expect("register legacy manifest error counter");
            counter
        })
    }

    pub fn record_error(kind: &'static str) {
        counter()
            .ensure_handle_for_label_values(&[kind])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
    }

    #[cfg(test)]
    pub fn reset_for_test() {
        if let Some(counter) = COUNTER.get() {
            counter.reset();
        }
    }

    #[cfg(test)]
    pub fn value_for_test(kind: &'static str) -> Option<u64> {
        let counter = COUNTER.get()?;
        counter
            .get_metric_with_label_values(&[kind])
            .ok()
            .map(|metric| metric.value())
    }
}

#[cfg(not(feature = "telemetry"))]
mod telemetry {
    #[allow(dead_code)]
    pub fn record_error(_kind: &'static str) {}

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn reset_for_test() {}

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn value_for_test(_kind: &'static str) -> Option<u64> {
        None
    }
}

#[derive(Clone)]
pub struct Config {
    path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self { path: None }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn open(self) -> Result<Db> {
        let path = self
            .path
            .ok_or_else(|| Error::Unsupported("missing legacy sled path".into()))?;
        Db::open(path)
    }
}

fn manifest_path(base: &Path) -> PathBuf {
    base.join(MANIFEST_FILE)
}

fn manifest_error(kind: &'static str, message: impl Into<String>) -> Error {
    telemetry::record_error(kind);
    Error::Storage(message.into())
}

fn load_manifest(path: &Path) -> Result<BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: Value = json::from_slice(&bytes).map_err(|err| {
        manifest_error("parse", format!("failed to parse legacy manifest: {err}"))
    })?;
    let root = value
        .as_object()
        .ok_or_else(|| manifest_error("structure", "invalid legacy manifest: expected object"))?;
    let trees_value = root
        .get("trees")
        .and_then(Value::as_object)
        .ok_or_else(|| manifest_error("structure", "invalid legacy manifest: missing trees map"))?;
    let mut trees = BTreeMap::new();
    for (name, entries) in trees_value {
        let decoded = hex::decode(name.as_bytes()).unwrap_or_else(|_| name.as_bytes().to_vec());
        if decoded.is_empty() {
            return Err(manifest_error(
                "empty_tree_name",
                format!("invalid legacy manifest: tree `{name}` may not be empty"),
            ));
        }
        let mut map = BTreeMap::new();
        let array = entries.as_array().ok_or_else(|| {
            manifest_error(
                "structure",
                "invalid legacy manifest: tree entries must be arrays",
            )
        })?;
        let mut seen_keys = HashSet::new();
        for entry in array {
            let object = entry.as_object().ok_or_else(|| {
                manifest_error("structure", "invalid legacy manifest: entry must be object")
            })?;
            let key_hex = object.get("key").and_then(Value::as_str).ok_or_else(|| {
                manifest_error("missing_key", "invalid legacy manifest: entry missing key")
            })?;
            let value_hex = object.get("value").and_then(Value::as_str).ok_or_else(|| {
                manifest_error(
                    "missing_value",
                    "invalid legacy manifest: entry missing value",
                )
            })?;
            let key = hex::decode(key_hex.as_bytes()).map_err(|_| {
                manifest_error("invalid_key_hex", "invalid legacy manifest: key hex")
            })?;
            let value = hex::decode(value_hex.as_bytes()).map_err(|_| {
                manifest_error("invalid_value_hex", "invalid legacy manifest: value hex")
            })?;
            if !seen_keys.insert(key.clone()) {
                let label = hex::encode(&decoded);
                return Err(manifest_error(
                    "duplicate_entry",
                    format!("invalid legacy manifest: duplicate key in tree `{label}`"),
                ));
            }
            map.insert(key, value);
        }
        trees.insert(decoded, map);
    }
    Ok(trees)
}

fn persist_manifest(
    base: &Path,
    trees: &BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>,
) -> Result<()> {
    fs::create_dir_all(base)?;
    let mut trees_obj = Map::new();
    for (name, entries) in trees {
        let mut entry_array = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let mut entry_obj = Map::new();
            entry_obj.insert("key".into(), Value::String(hex::encode(key)));
            entry_obj.insert("value".into(), Value::String(hex::encode(value)));
            entry_array.push(Value::Object(entry_obj));
        }
        trees_obj.insert(hex::encode(name), Value::Array(entry_array));
    }
    let mut root = Map::new();
    root.insert("trees".into(), Value::Object(trees_obj));
    let bytes = json::to_vec(&Value::Object(root))
        .map_err(|err| Error::Storage(format!("failed to serialise legacy manifest: {err}")))?;
    fs::write(manifest_path(base), bytes)?;
    Ok(())
}

#[derive(Clone)]
pub struct Db {
    base: PathBuf,
    trees: Arc<RwLock<BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>>>,
}

impl Db {
    fn open(path: PathBuf) -> Result<Self> {
        fs::create_dir_all(&path)?;
        let manifest = load_manifest(&manifest_path(&path))?;
        Ok(Self {
            base: path,
            trees: Arc::new(RwLock::new(manifest)),
        })
    }

    pub fn open_tree<N: AsRef<[u8]>>(&self, name: N) -> Result<Tree> {
        let mut guard = self.trees.write().unwrap();
        let key = name.as_ref().to_vec();
        guard.entry(key.clone()).or_insert_with(BTreeMap::new);
        Ok(Tree {
            base: self.base.clone(),
            name: key,
            trees: Arc::clone(&self.trees),
        })
    }

    pub fn tree_names(&self) -> Vec<Vec<u8>> {
        self.trees.read().unwrap().keys().cloned().collect()
    }

    pub fn flush(&self) -> Result<()> {
        let guard = self.trees.read().unwrap();
        persist_manifest(&self.base, &*guard)
    }
}

#[derive(Clone)]
pub struct Tree {
    #[cfg_attr(not(test), allow(dead_code))]
    base: PathBuf,
    name: Vec<u8>,
    trees: Arc<RwLock<BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>>>,
}

impl Tree {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn insert<K, V>(&self, key: K, value: V) -> Result<Option<Vec<u8>>>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut guard = self.trees.write().unwrap();
        let map = guard
            .get_mut(&self.name)
            .expect("tree exists after open_tree");
        Ok(map.insert(key.as_ref().to_vec(), value.as_ref().to_vec()))
    }

    pub fn iter(&self) -> Iter {
        let guard = self.trees.read().unwrap();
        let entries = guard
            .get(&self.name)
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        Iter { entries, index: 0 }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn flush(&self) -> Result<()> {
        let guard = self.trees.read().unwrap();
        persist_manifest(&self.base, &*guard)
    }
}

pub struct Iter {
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    index: usize,
}

impl Iterator for Iter {
    type Item = Result<(Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.entries.len() {
            return None;
        }
        let item = self.entries[self.index].clone();
        self.index += 1;
        Some(Ok(item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage_engine::tempfile;

    #[test]
    fn load_manifest_rejects_duplicate_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest_path = manifest_path(dir.path());
        let json = r#"{
            "trees": {
                "64656661756c74": [
                    {"key": "00", "value": "01"},
                    {"key": "00", "value": "02"}
                ]
            }
        }"#;
        fs::write(&manifest_path, json).expect("write manifest");

        let err = load_manifest(&manifest_path).expect_err("duplicate keys should error");
        assert!(
            matches!(err, Error::Storage(ref message) if message.contains("duplicate key")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn load_manifest_parses_valid_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest_path = manifest_path(dir.path());
        let json = r#"{
            "trees": {
                "64656661756c74": [
                    {"key": "00", "value": "01"},
                    {"key": "01", "value": "02"}
                ]
            }
        }"#;
        fs::write(&manifest_path, json).expect("write manifest");

        let trees = load_manifest(&manifest_path).expect("manifest should parse");
        let default = trees.get(b"default".as_ref()).expect("tree present");
        assert_eq!(default.len(), 2);
        assert_eq!(default.get(&[0u8][..]).unwrap(), &[1u8]);
        assert_eq!(default.get(&[1u8][..]).unwrap(), &[2u8]);
    }

    #[cfg(feature = "telemetry")]
    #[test]
    fn telemetry_counts_parse_errors() {
        telemetry::reset_for_test();
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest_path = manifest_path(dir.path());
        fs::write(&manifest_path, b"not json").expect("write manifest");

        assert!(load_manifest(&manifest_path).is_err());
        let count = telemetry::value_for_test("parse").unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn config_round_trips_tree_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Config::new()
            .path(dir.path())
            .open()
            .expect("open legacy db");
        let tree = db.open_tree(b"default").expect("open tree");
        tree.insert(b"alpha", b"one").expect("insert alpha");
        tree.insert(b"beta", b"two").expect("insert beta");
        tree.flush().expect("flush tree");
        drop(tree);
        db.flush().expect("flush db");
        drop(db);

        let reopened = Config::new()
            .path(dir.path())
            .open()
            .expect("reopen legacy db");
        let mut names = reopened.tree_names();
        names.sort();
        assert_eq!(names, vec![b"default".to_vec()]);
        let tree = reopened.open_tree(b"default").expect("open default tree");
        let entries: Vec<_> = tree.iter().map(|res| res.expect("iter entry")).collect();
        assert_eq!(
            entries,
            vec![
                (b"alpha".to_vec(), b"one".to_vec()),
                (b"beta".to_vec(), b"two".to_vec())
            ]
        );
    }

    #[test]
    fn open_tree_records_multiple_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Config::new()
            .path(dir.path())
            .open()
            .expect("open legacy db");
        let alpha = db.open_tree(b"alpha").expect("open alpha");
        let beta = db.open_tree(b"beta").expect("open beta");
        alpha.insert(b"k1", b"v1").expect("insert alpha entry");
        beta.insert(b"k2", b"v2").expect("insert beta entry");
        db.flush().expect("flush db");
        drop(alpha);
        drop(beta);
        drop(db);

        let reopened = Config::new()
            .path(dir.path())
            .open()
            .expect("reopen legacy db");
        let mut names = reopened.tree_names();
        names.sort();
        assert_eq!(names, vec![b"alpha".to_vec(), b"beta".to_vec()]);
        let alpha = reopened.open_tree(b"alpha").expect("open alpha tree");
        let beta = reopened.open_tree(b"beta").expect("open beta tree");
        let alpha_entries: Vec<_> = alpha.iter().map(|res| res.expect("alpha entry")).collect();
        let beta_entries: Vec<_> = beta.iter().map(|res| res.expect("beta entry")).collect();
        assert_eq!(alpha_entries, vec![(b"k1".to_vec(), b"v1".to_vec())]);
        assert_eq!(beta_entries, vec![(b"k2".to_vec(), b"v2".to_vec())]);
    }
}
