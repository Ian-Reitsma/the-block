use std::sync::Arc;

use storage_engine::memory_engine::MemoryEngine;
use storage_engine::{KeyValue, KeyValueIterator};
use tempfile::tempdir;

fn concurrency_test<E>(engine: E)
where
    E: KeyValue + Send + Sync + 'static,
{
    let engine = Arc::new(engine);
    engine.ensure_cf("default").expect("ensure cf");
    let mut handles = Vec::new();
    for idx in 0..16 {
        let engine = Arc::clone(&engine);
        handles.push(std::thread::spawn(move || {
            let key = format!("concurrency-{idx}");
            engine
                .put("default", key.as_bytes(), key.as_bytes())
                .expect("put value");
        }));
    }
    for handle in handles {
        handle.join().expect("thread join");
    }
    for idx in 0..16 {
        let key = format!("concurrency-{idx}");
        assert_eq!(
            engine.get("default", key.as_bytes()).expect("get value"),
            Some(key.into_bytes())
        );
    }
}

fn prefix_iteration_test<E>(engine: E)
where
    E: KeyValue,
{
    engine.ensure_cf("default").expect("ensure cf");
    engine
        .put("default", b"prefix-1", b"one")
        .expect("put prefix-1");
    engine
        .put("default", b"prefix-2", b"two")
        .expect("put prefix-2");
    engine
        .put("default", b"other", b"three")
        .expect("put other");

    let mut iter = engine
        .prefix_iterator("default", b"prefix")
        .expect("prefix iterator");
    let mut keys = Vec::new();
    while let Some((key, _value)) = iter.next().expect("iterator next") {
        keys.push(String::from_utf8(key).expect("utf8 key"));
    }
    keys.sort();
    assert_eq!(keys, vec!["prefix-1".to_string(), "prefix-2".to_string()]);
}

fn crash_safety_test<F, E>(mut factory: F)
where
    F: FnMut(&str) -> E,
    E: KeyValue,
{
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().into_owned();
    {
        let engine = factory(&db_path_str);
        engine.ensure_cf("default").expect("ensure cf");
        engine
            .put("default", b"crash", b"persisted")
            .expect("put value");
        engine.flush().expect("flush");
    }

    {
        let reopened = factory(&db_path_str);
        assert_eq!(
            reopened.get("default", b"crash").expect("get value"),
            Some(b"persisted".to_vec())
        );
    }
}

#[test]
fn memory_engine_behaviour() {
    let dir = tempdir().expect("memory tempdir");
    let path = dir.path().join("memory");
    let path_str = path.to_string_lossy().into_owned();

    let engine = MemoryEngine::open(&path_str).expect("open memory");
    concurrency_test(engine);

    let engine = MemoryEngine::open(&path_str).expect("reopen memory");
    prefix_iteration_test(engine);

    crash_safety_test(|p| MemoryEngine::open(p).expect("open memory"));
}

#[test]
fn rocksdb_engine_behaviour() {
    use storage_engine::rocksdb_engine::RocksDbEngine;

    let dir = tempdir().expect("rocks tempdir");
    let path = dir.path().join("rocks");
    let path_str = path.to_string_lossy().into_owned();

    let engine = RocksDbEngine::open(&path_str).expect("open rocks");
    concurrency_test(engine);

    let engine = RocksDbEngine::open(&path_str).expect("reopen rocks");
    prefix_iteration_test(engine);

    crash_safety_test(|p| RocksDbEngine::open(p).expect("open rocks"));
}
