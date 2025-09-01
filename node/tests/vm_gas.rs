use the_block::vm::{bytecode::OpCode, Vm, VmType};

#[test]
fn deterministic_gas_and_fee() {
    let mut vm = Vm::new(VmType::Evm);
    // Program: PUSH 2; PUSH 3; ADD; HALT
    let code = vec![
        OpCode::Push as u8,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Push as u8,
        3,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Add as u8,
    ];
    let id = vm.deploy(code.clone());
    let mut balance = 1000u64;
    let (out1, gas1) = vm.execute(id, &[], 100, 2, &mut balance).unwrap();
    assert_eq!(out1, vec![5, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(balance, 1000 - gas1 * 2);
    // Redeploy to ensure deterministic output independent of prior state
    let id2 = vm.deploy(code);
    let mut balance2 = 1000u64;
    let (out2, gas2) = vm.execute(id2, &[], 100, 2, &mut balance2).unwrap();
    assert_eq!(out1, out2);
    assert_eq!(gas1, gas2);
    assert_eq!(balance2, 1000 - gas2 * 2);
}
