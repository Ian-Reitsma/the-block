use explorer::Explorer;
use tempfile::tempdir;

#[test]
fn disassembles_wasm() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("ex.db");
    let ex = Explorer::open(&db).unwrap();
    let wat = "(module)";
    let wasm = wat::parse_str(wat).unwrap();
    let out = ex.wasm_disasm(&wasm).unwrap();
    assert!(out.contains("(module"));
}
