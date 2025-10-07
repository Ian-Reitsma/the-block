use codec::{self, profiles};
use explorer::Explorer;
use sys::temp;

#[test]
fn load_trace() {
    let dir = temp::tempdir().unwrap();
    struct DirGuard(std::path::PathBuf);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    let guard = DirGuard(std::env::current_dir().unwrap());
    std::env::set_current_dir(dir.path()).unwrap();
    std::fs::create_dir_all("trace").unwrap();
    std::fs::write(
        "trace/tx1.json",
        codec::serialize(profiles::json(), &vec!["Push", "Halt"]).unwrap(),
    )
    .unwrap();
    let db = dir.path().join("explorer.db");
    let ex = Explorer::open(&db).unwrap();
    let trace = ex.opcode_trace("tx1").unwrap();
    assert_eq!(trace, vec!["Push", "Halt"]);
    drop(guard);
}
