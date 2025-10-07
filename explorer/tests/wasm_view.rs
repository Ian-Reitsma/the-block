use explorer::Explorer;
use tempfile::tempdir;

fn sample_module() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&the_block::vm::wasm::MAGIC);
    buf.push(the_block::vm::wasm::VERSION_V1);
    buf.extend_from_slice(&[
        the_block::vm::wasm::opcodes::PUSH_I64,
        9,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        the_block::vm::wasm::opcodes::RETURN,
        1,
    ]);
    buf
}

#[test]
fn disassembles_wasm() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("ex.db");
    let ex = Explorer::open(&db).unwrap();
    let wasm = sample_module();
    let out = ex.wasm_disasm(&wasm).expect("disassemble");
    assert!(out.contains("push_const 9"));
}
