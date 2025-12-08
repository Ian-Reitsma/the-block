#![cfg(feature = "integration-tests")]
use rand::{rngs::StdRng, seq::SliceRandom};
use sys::tempfile::tempdir;
use the_block::SimpleDb;

#[test]
fn wal_survives_random_deletes() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("db");
    let mut db = SimpleDb::open(path.to_str().unwrap());
    db.insert("k", b"v".to_vec());
    db.flush_wal();

    let mut logs: Vec<_> = std::fs::read_dir(&path)
        .expect("read_dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
        .collect();
    let mut rng = StdRng::seed_from_u64(7);
    logs.shuffle(&mut rng);
    if let Some(log) = logs.pop() {
        let _ = std::fs::remove_file(log.path());
    }

    let reopened = SimpleDb::open(path.to_str().unwrap());
    assert_eq!(reopened.get("k"), Some(b"v".to_vec()));
}
