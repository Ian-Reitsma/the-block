use explorer::Explorer;
use sys::tempfile;

fn sample_module() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&the_block::vm::wasm::MAGIC);
    buf.push(the_block::vm::wasm::VERSION_V1);
    buf.extend_from_slice(&[0x01, 9, 0, 0, 0, 0, 0, 0, 0, 0x10, 1]);
    buf
}

#[test]
fn disassembles_wasm() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ex.db");
    let ex = Explorer::open(&db).unwrap();
    let wasm = sample_module();
    let out = ex.wasm_disasm(&wasm).expect("disassemble");
    assert!(out.contains("push_const 9"));
}
