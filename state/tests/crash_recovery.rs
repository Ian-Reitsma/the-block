use storage_engine::{inhouse_engine::InhouseEngine, KeyValue};
use sys::tempfile;

#[test]
fn inhouse_recovers_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = InhouseEngine::open(dir.path().to_string_lossy().as_ref()).expect("open db");
        db.ensure_cf("default").expect("ensure cf");
        db.put("default", b"k", b"v").expect("write value");
        db.flush().expect("flush");
    }
    let db = InhouseEngine::open(dir.path().to_string_lossy().as_ref()).expect("reopen db");
    db.ensure_cf("default").expect("ensure cf");
    let val = db
        .get("default", b"k")
        .expect("read value")
        .expect("value exists");
    assert_eq!(val.as_slice(), b"v");
}
