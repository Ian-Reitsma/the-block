use node::vm::{
    bytecode::OpCode,
    runtime::{Vm, VmType},
};

#[test]
fn gas_accounting_deterministic() {
    let mut vm = Vm::new(VmType::Evm);
    let code = vec![
        OpCode::Push as u8,
        6,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Push as u8,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Div as u8,
        OpCode::Push as u8,
        3,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Mul as u8,
    ];
    let id = vm.deploy(code);
    let mut balance = 1000u64;
    let (out, gas) = vm.execute(id, &[], 1000, 1, &mut balance).unwrap();
    assert_eq!(&out[..8], &9u64.to_le_bytes());
    assert_eq!(gas, 36);
    assert_eq!(balance, 1000 - gas);
}
