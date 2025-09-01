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
