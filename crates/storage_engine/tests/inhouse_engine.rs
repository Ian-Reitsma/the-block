use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use storage_engine::tempfile;
use storage_engine::{inhouse_engine::InhouseEngine, KeyValue, KeyValueIterator};

fn temp_dir(name: &str) -> PathBuf {
    let mut base = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    base.push(format!(
        "the-block-inhouse-{name}-{}-{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    base
}

fn cleanup(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

#[test]
fn loads_legacy_manifest_and_wal_fixture() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path();
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy_cf");

    fs::copy(
        fixture_root.join("manifest.json"),
        root.join("manifest.json"),
    )
    .expect("copy manifest");

    let cf_dir = root.join("default");
    fs::create_dir_all(&cf_dir).expect("create cf dir");
    fs::copy(fixture_root.join("wal.log"), cf_dir.join("wal.log")).expect("copy wal");

    let engine = InhouseEngine::open(root.to_string_lossy().as_ref()).expect("open");
    engine.ensure_cf("default").expect("load cf");

    assert_eq!(engine.get("default", b"foo").unwrap(), None);
    assert_eq!(
        engine.get("default", b"baz").unwrap(),
        Some(b"baz".to_vec())
    );
}

#[test]
fn wal_replayed_on_reopen() {
    let path = temp_dir("wal-replay");
    {
        let engine = InhouseEngine::open(path.to_string_lossy().as_ref()).expect("open");
        engine.ensure_cf("default").expect("cf");
        engine.put_bytes("default", b"alpha", b"one").expect("put");
        engine.put_bytes("default", b"beta", b"two").expect("put");
    }

    let engine = InhouseEngine::open(path.to_string_lossy().as_ref()).expect("reopen");
    let alpha = engine.get("default", b"alpha").expect("get");
    assert_eq!(alpha, Some(b"one".to_vec()));
    let beta = engine.get("default", b"beta").expect("get");
    assert_eq!(beta, Some(b"two".to_vec()));
    cleanup(&path);
}

#[test]
fn delete_tombstone_persists_through_flush() {
    let path = temp_dir("delete");
    let engine = InhouseEngine::open(path.to_string_lossy().as_ref()).expect("open");
    engine.ensure_cf("default").expect("cf");
    engine.put_bytes("default", b"key", b"value").expect("put");
    engine.flush().expect("flush");
    let value = engine.get("default", b"key").expect("get");
    assert_eq!(value, Some(b"value".to_vec()));
    let deleted = engine.delete("default", b"key").expect("delete");
    assert_eq!(deleted, Some(b"value".to_vec()));
    engine.flush().expect("flush");
    assert_eq!(engine.get("default", b"key").unwrap(), None);
    drop(engine);
    let engine = InhouseEngine::open(path.to_string_lossy().as_ref()).expect("reopen");
    assert_eq!(engine.get("default", b"key").unwrap(), None);
    cleanup(&path);
}

#[test]
fn compaction_retains_latest_values() {
    let path = temp_dir("compact");
    let engine = InhouseEngine::open(path.to_string_lossy().as_ref()).expect("open");
    engine.ensure_cf("default").expect("cf");
    engine.set_byte_limit(Some(128)).expect("limit");
    for idx in 0..32 {
        let key = format!("key-{idx:03}");
        engine
            .put_bytes("default", key.as_bytes(), key.as_bytes())
            .expect("put initial");
    }
    engine.flush().expect("flush");
    for idx in 0..32 {
        let key = format!("key-{idx:03}");
        let newer = format!("v2-{idx:03}");
        engine
            .put_bytes("default", key.as_bytes(), newer.as_bytes())
            .expect("put newer");
    }
    engine.compact().expect("compact");
    for idx in 0..32 {
        let key = format!("key-{idx:03}");
        let expected = format!("v2-{idx:03}");
        assert_eq!(
            engine.get("default", key.as_bytes()).expect("get"),
            Some(expected.into_bytes())
        );
    }
    cleanup(&path);
}

#[test]
fn prefix_iterator_respects_prefix() {
    let path = temp_dir("iterator");
    let engine = InhouseEngine::open(path.to_string_lossy().as_ref()).expect("open");
    engine.ensure_cf("metrics").expect("cf");
    engine
        .put_bytes("metrics", b"peer:001", b"one")
        .expect("put 1");
    engine
        .put_bytes("metrics", b"peer:002", b"two")
        .expect("put 2");
    engine
        .put_bytes("metrics", b"other:003", b"three")
        .expect("put 3");
    let mut iter = engine
        .prefix_iterator("metrics", b"peer:")
        .expect("iterator");
    let mut keys = Vec::new();
    while let Some((key, value)) = iter.next().expect("iter next") {
        keys.push((
            String::from_utf8(key).unwrap(),
            String::from_utf8(value).unwrap(),
        ));
    }
    keys.sort();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].0, "peer:001");
    assert_eq!(keys[0].1, "one");
    assert_eq!(keys[1].0, "peer:002");
    assert_eq!(keys[1].1, "two");
    cleanup(&path);
}
