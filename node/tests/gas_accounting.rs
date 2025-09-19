#![cfg(feature = "integration-tests")]
use the_block::vm::{exec, gas::GasMeter, opcodes::OpCode};

#[test]
fn deterministic_gas_usage() {
    let code: Vec<u8> = vec![
        OpCode::Push as u8,
        1,
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
        OpCode::Add as u8,
        OpCode::Hash as u8,
        OpCode::Halt as u8,
    ];
    let mut load = || 0u64;
    let mut store = |_v: u64| {};
    let mut m1 = GasMeter::new(1_000);
    exec::execute(&code, &mut m1, &mut load, &mut store).unwrap();
    let mut m2 = GasMeter::new(1_000);
    let mut load2 = || 0u64;
    let mut store2 = |_v: u64| {};
    exec::execute(&code, &mut m2, &mut load2, &mut store2).unwrap();
    assert_eq!(m1.used(), m2.used());
}
