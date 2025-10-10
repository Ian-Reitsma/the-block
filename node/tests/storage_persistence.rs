#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::SimpleDb;

#[test]
fn restart_recovers_state() {
    let dir = tempdir().unwrap();
    {
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.insert("foo", b"bar".to_vec());
    }
    {
        let db = SimpleDb::open(dir.path().to_str().unwrap());
        assert_eq!(db.get("foo"), Some(b"bar".to_vec()));
    }
}
