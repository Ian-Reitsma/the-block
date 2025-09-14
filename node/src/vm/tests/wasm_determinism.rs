use super::*;
use crate::vm::{runtime::Vm, runtime::VmType};

#[test]
fn wasm_runs_deterministically() {
    let wat = r#"(module
        (memory (export "memory") 1)
        (func (export "entry") (param i32 i32) (result i32)
            local.get 1)
    )"#;
    let wasm = wat::parse_str(wat).unwrap();
    let mut vm = Vm::new(VmType::Wasm);
    let id = vm.deploy_wasm(wasm, vec![]);
    let input = b"hello";
    let mut balance = 1_000_000;
    let (out1, gas1) = vm.execute(id, input, 1_000_000, 1, &mut balance).unwrap();
    let (out2, gas2) = vm.execute(id, input, 1_000_000, 1, &mut balance).unwrap();
    assert_eq!(out1, out2);
    assert_eq!(gas1, gas2);
}
