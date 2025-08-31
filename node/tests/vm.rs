use the_block::vm::{abi::encode_u64, Vm, VmType};

#[test]
fn deploy_and_execute_contract() {
    let mut vm = Vm::new(VmType::Wasm);
    let id = vm.deploy(vec![0xAA]);
    let input = encode_u64(7);
    let (out, gas) = vm.execute(id, &input, 100).expect("exec");
    assert!(gas > 0);
    assert!(out.ends_with(&input));
    assert!(vm.read(id).unwrap().ends_with(&input));
}

#[test]
fn state_isolation() {
    let mut vm = Vm::new(VmType::Wasm);
    let a = vm.deploy(vec![0x01]);
    let b = vm.deploy(vec![0x02]);
    vm.execute(a, b"first", 100).unwrap();
    vm.execute(b, b"second", 100).unwrap();
    assert!(vm.read(a).unwrap().ends_with(b"first"));
    assert!(vm.read(b).unwrap().ends_with(b"second"));
}
