#![allow(clippy::unwrap_used)]

use the_block::SimpleDb;

mod util;
use util::temp::temp_dir;

#[test]
fn wal_recovers_unflushed_ops() {
    let dir = temp_dir("wal_db");
    {
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.insert("k", b"v".to_vec());
        // Intentionally omit flush to simulate crash
    }
    let db2 = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db2.get("k"), Some(b"v".to_vec()));
}
