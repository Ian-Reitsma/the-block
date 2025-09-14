use explorer::Explorer;
use tempfile::tempdir;

#[test]
fn load_trace() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("trace")).unwrap();
    std::fs::write(
        dir.path().join("trace/tx1.json"),
        serde_json::to_vec(&vec!["Push", "Halt"]).unwrap(),
    )
    .unwrap();
    let db = dir.path().join("explorer.db");
    let ex = Explorer::open(&db).unwrap();
    let trace = ex.opcode_trace("tx1").unwrap();
    assert_eq!(trace, vec!["Push", "Halt"]);
}
