use crate::vm::{runtime::Vm, runtime::VmType};

fn sample_module() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&crate::vm::wasm::MAGIC);
    buf.push(crate::vm::wasm::VERSION_V1);
    buf.extend_from_slice(&[
        crate::vm::wasm::opcodes::PUSH_I64,
        3,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        crate::vm::wasm::opcodes::PUSH_INPUT,
        0,
        crate::vm::wasm::opcodes::MUL_I64,
        crate::vm::wasm::opcodes::RETURN,
        1,
    ]);
    buf
}

#[test]
fn wasm_runs_deterministically() {
    let mut vm = Vm::new(VmType::Wasm);
    let module = sample_module();
    let id = vm.deploy_wasm(module, vec![]);
    let mut balance = 1_000_000;
    let mut input = 5i64.to_le_bytes().to_vec();
    let first = vm
        .execute(id, &input, 1_000_000, 1, &mut balance)
        .expect("executes");
    // reset state and rerun
    let mut vm_again = Vm::new(VmType::Wasm);
    let id_again = vm_again.deploy_wasm(sample_module(), vec![]);
    let mut balance_again = 1_000_000;
    let second = vm_again
        .execute(id_again, &input, 1_000_000, 1, &mut balance_again)
        .expect("executes");
    assert_eq!(first.0, second.0);
}
