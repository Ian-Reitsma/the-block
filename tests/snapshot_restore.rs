#[path = "../tools/snapshot.rs"]
mod snapshot_tool;
use snapshot_tool::{create_snapshot, restore_snapshot};
use rocksdb::DB;
use tempfile::tempdir;

#[test]
fn round_trip_snapshot() {
    let dir = tempdir().unwrap();
    let db_dir = dir.path().join("db");
    let db = DB::open_default(&db_dir).unwrap();
    db.put(b"k", b"v").unwrap();
    let snap = dir.path().join("snap.zst");
    create_snapshot(&db_dir, &snap).unwrap();
    db.delete(b"k").unwrap();
    restore_snapshot(&snap, &db_dir).unwrap();
    let db = DB::open_default(&db_dir).unwrap();
    let val = db.get(b"k").unwrap().unwrap();
    assert_eq!(val.as_ref(), b"v");
}
