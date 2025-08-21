#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use std::collections::HashMap;
use tempfile::tempdir;
use the_block::SimpleDb;

#[derive(Arbitrary, Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
}

fn apply_ops(ops: &[Op], trunc: Option<u64>) -> HashMap<String, Vec<u8>> {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let mut db = SimpleDb::open(path);
    let mut mirror = HashMap::new();
    for op in ops {
        match op {
            Op::Put(k, v) => {
                let key = hex::encode(k);
                db.insert(&key, v.clone());
                mirror.insert(key, v.clone());
            }
            Op::Del(k) => {
                let key = hex::encode(k);
                db.remove(&key);
                mirror.remove(&key);
            }
        }
    }
    if let Some(t) = trunc {
        let wal = dir.path().join("wal");
        if let Ok(meta) = std::fs::metadata(&wal) {
            let len = meta.len();
            let cut = std::cmp::min(len, t);
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(&wal)
                .and_then(|f| f.set_len(cut));
        }
    }
    drop(db);
    let reopened = SimpleDb::open(path);
    let mut result = HashMap::new();
    for key in mirror.keys().cloned() {
        if let Some(v) = reopened.get(&key) {
            result.insert(key, v);
        }
    }
    result
}

fuzz_target!(|ops: Vec<Op>| {
    let map = apply_ops(&ops, Some(256));
    let map2 = apply_ops(&ops, None);
    for (k, v) in map.iter() {
        assert_eq!(map2.get(k), Some(v));
    }
});
