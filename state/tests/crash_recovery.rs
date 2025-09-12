use rocksdb::{Options, DB};

#[test]
fn rocksdb_recovers_after_crash() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, dir.path()).expect("open db");
        db.put(b"k", b"v").unwrap();
        // drop without explicit flush to simulate crash
        std::mem::drop(db);
    }
    let mut opts = Options::default();
    opts.create_if_missing(false);
    let db = DB::open(&opts, dir.path()).expect("reopen db");
    let val = db.get(b"k").unwrap().unwrap();
    assert_eq!(val.as_slice(), b"v");
}
