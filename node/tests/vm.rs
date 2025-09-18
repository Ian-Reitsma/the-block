use the_block::vm::{bytecode::OpCode, Vm, VmType};

#[test]
fn deploy_and_execute_contract() {
    let mut vm = Vm::new(VmType::Evm);
    // Program: PUSH0; PUSH1; ADD; HALT
    let code = vec![
        OpCode::Push as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Push as u8,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Add as u8,
    ];
    let id = vm.deploy(code);
    let mut bal = 100;
    let (out, gas) = vm.execute(id, &[], 100, 1, &mut bal).expect("exec");
    assert_eq!(out, vec![1, 0, 0, 0, 0, 0, 0, 0]);
    assert!(gas > 0);
}

#[test]
fn state_isolation() {
    let mut vm = Vm::new(VmType::Evm);
    let a = vm.deploy(vec![]);
    let b = vm.deploy(vec![]);
    let mut bal_a = 100;
    let mut bal_b = 100;
    vm.execute(a, &[1], 50, 1, &mut bal_a).unwrap();
    vm.execute(b, &[2], 50, 1, &mut bal_b).unwrap();
    assert_ne!(vm.read(a), vm.read(b));
}

#[test]
fn state_persists_across_restarts() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let path = dir.path().join("contracts.bin");
    let mut vm = Vm::new_persistent(VmType::Evm, path.clone());
    let id = vm.deploy(vec![]);
    let mut bal = 100;
    vm.execute(id, &[42], 50, 1, &mut bal).unwrap();
    drop(vm);

    let vm = Vm::new_persistent(VmType::Evm, path);
    assert_eq!(vm.read(id), Some(42u64.to_le_bytes().to_vec()));
}

#[test]
fn evm_store_and_load_roundtrip() {
    fn push(word: u64) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 8);
        buf.push(OpCode::Push as u8);
        buf.extend_from_slice(&word.to_le_bytes());
        buf
    }

    let mut vm = Vm::new(VmType::Evm);
    let mut code = Vec::new();
    code.extend_from_slice(&push(5));
    code.push(OpCode::Store as u8);
    code.push(OpCode::Load as u8);
    let id = vm.deploy(code);

    let mut balance = 1_000;
    let (out, gas_used) = vm
        .execute(id, &[], 1_000, 1, &mut balance)
        .expect("evm execution");
    assert_eq!(out, 5u64.to_le_bytes());
    assert!(gas_used > 0, "gas must be consumed");
    assert_eq!(vm.read(id), Some(5u64.to_le_bytes().to_vec()));
}

#[test]
fn wasm_execution_reports_gas_and_storage() {
    let module = wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "entry") (param i32 i32) (result i32)
                local.get 1)
        )"#,
    )
    .expect("valid wasm module");

    let mut vm = Vm::new(VmType::Wasm);
    let id = vm.deploy_wasm(module, vec![]);
    let mut balance = 10_000;
    let (out, used) = vm
        .execute(id, b"ping", 50_000, 2, &mut balance)
        .expect("wasm execution succeeds");
    assert_eq!(out, b"ping");
    assert!(used > 0, "fuel conversion must report usage");
    assert_eq!(vm.read(id), Some(out.clone()));

    // Running with a reduced budget should trap before charging a fee.
    let mut retry_balance = 10_000;
    let limited_limit = used.saturating_sub(1).max(1);
    let err = vm.execute(id, b"pong", limited_limit, 2, &mut retry_balance);
    assert_eq!(err, Err("wasm error"));
    assert_eq!(retry_balance, 10_000);
}
