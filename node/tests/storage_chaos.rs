use node::simple_db::SimpleDb;
use rand::{seq::SliceRandom, thread_rng};
use tempfile::tempdir;

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
    logs.shuffle(&mut thread_rng());
    if let Some(log) = logs.pop() {
        let _ = std::fs::remove_file(log.path());
    }

    let reopened = SimpleDb::open(path.to_str().unwrap());
    assert_eq!(reopened.get("k"), Some(b"v".to_vec()));
}
