#![cfg(feature = "integration-tests")]
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::HashMap;
use sys::temp;
use the_block::SimpleDb;

#[derive(Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
}

fn generate_ops(seed: u64) -> Vec<Op> {
    let mut rng = StdRng::seed_from_u64(seed);
    let count = rng.gen_range(0..32);
    let mut ops = Vec::with_capacity(count);
    for _ in 0..count {
        let key_len = rng.gen_range(0..16);
        let mut key = vec![0u8; key_len];
        rng.fill(&mut key[..]);
        if rng.gen_bool(0.5) {
            let val_len = rng.gen_range(0..32);
            let mut value = vec![0u8; val_len];
            rng.fill(&mut value[..]);
            ops.push(Op::Put(key, value));
        } else {
            ops.push(Op::Del(key));
        }
    }
    ops
}

fn apply_ops(ops: &[Op], trunc: Option<u64>) -> HashMap<String, Vec<u8>> {
    let dir = temp::tempdir().unwrap();
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
    for key in mirror.keys() {
        if let Some(v) = reopened.get(key) {
            result.insert(key.clone(), v);
        }
    }
    result
}

#[test]
fn wal_fuzz() {
    let mut rng = StdRng::seed_from_u64(1);
    for seed in 0..64 {
        let ops = generate_ops(seed);
        let trunc = rng.gen_range(0..512);
        let map = apply_ops(&ops, Some(trunc));
        // reopen after full replay without truncation
        let map2 = apply_ops(&ops, None);
        for (k, v) in map.iter() {
            assert_eq!(map2.get(k), Some(v));
        }
    }
}
