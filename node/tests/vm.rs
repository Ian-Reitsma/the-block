use the_block::vm::{bytecode::OpCode, Vm, VmType};

#[test]
fn deploy_and_execute_contract() {
    let mut vm = Vm::new(VmType::Wasm);
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
    let mut vm = Vm::new(VmType::Wasm);
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
    let mut vm = Vm::new_persistent(VmType::Wasm, path.clone());
    let id = vm.deploy(vec![]);
    let mut bal = 100;
    vm.execute(id, &[42], 50, 1, &mut bal).unwrap();
    drop(vm);

    let vm = Vm::new_persistent(VmType::Wasm, path);
    assert_eq!(vm.read(id), Some(42u64.to_le_bytes().to_vec()));
}
