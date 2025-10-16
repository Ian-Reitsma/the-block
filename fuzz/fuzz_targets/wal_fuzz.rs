#![forbid(unsafe_code)]

use foundation_fuzz::{Arbitrary, Result as FuzzResult, Unstructured};
use std::collections::HashMap;
use sys::tempfile::tempdir;
use the_block::SimpleDb;

const MAX_BYTES: usize = 1 << 20; // 1 MiB
const MAX_OPS: usize = 32;

fn small_vec(u: &mut Unstructured<'_>) -> FuzzResult<Vec<u8>> {
    let len = u.int_in_range(0..=MAX_BYTES as u64)? as usize;
    let mut v = vec![0u8; len];
    u.fill_buffer(&mut v)?;
    Ok(v)
}

fn limited_ops(u: &mut Unstructured<'_>) -> FuzzResult<Vec<Op>> {
    let len = u.int_in_range(0..=MAX_OPS as u64)? as usize;
    let mut ops = Vec::with_capacity(len);
    for _ in 0..len {
        ops.push(Op::arbitrary(u)?);
    }
    Ok(ops)
}

#[derive(Debug)]
struct Input {
    ops: Vec<Op>,
}

impl<'a> Arbitrary<'a> for Input {
    fn arbitrary(u: &mut Unstructured<'a>) -> FuzzResult<Self> {
        Ok(Self {
            ops: limited_ops(u)?,
        })
    }
}

#[derive(Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
}

impl<'a> Arbitrary<'a> for Op {
    fn arbitrary(u: &mut Unstructured<'a>) -> FuzzResult<Self> {
        match u.int_in_range(0..=1)? {
            0 => Ok(Op::Put(small_vec(u)?, small_vec(u)?)),
            _ => Ok(Op::Del(small_vec(u)?)),
        }
    }
}

fn apply_ops(ops: &[Op], trunc: Option<u64>) -> HashMap<String, Vec<u8>> {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().to_str().expect("tempdir path");
    let mut db = SimpleDb::open(path);
    let mut mirror = HashMap::new();
    for op in ops {
        match op {
            Op::Put(k, v) => {
                let key = crypto_suite::hex::encode(k);
                db.insert(&key, v.clone());
                mirror.insert(key, v.clone());
            }
            Op::Del(k) => {
                let key = crypto_suite::hex::encode(k);
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

pub fn run(data: &[u8]) {
    let mut cursor = Unstructured::new(data);
    if let Ok(input) = Input::arbitrary(&mut cursor) {
        fuzz_once(&input);
    }
}

fn fuzz_once(input: &Input) {
    let map = apply_ops(&input.ops, Some(256));
    let map2 = apply_ops(&input.ops, None);
    for (k, v) in map.iter() {
        assert_eq!(map2.get(k), Some(v));
    }
}

fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_once_preserves_mirror_state() {
        let ops = vec![Op::Put(b"key".to_vec(), b"value".to_vec())];
        let input = Input { ops };
        fuzz_once(&input);
    }
}
