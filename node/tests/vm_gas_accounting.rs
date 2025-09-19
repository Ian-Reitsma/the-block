#![cfg(feature = "integration-tests")]
use the_block::vm::{bytecode, bytecode::OpCode, gas::GasMeter};

#[test]
fn gas_determinism() {
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
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Add as u8,
        OpCode::Push as u8,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        OpCode::Mul as u8,
        OpCode::Halt as u8,
    ];
    let mut meter = GasMeter::new(100);
    let stack1 = bytecode::execute(&code, &mut meter).unwrap();
    let used1 = meter.used();

    // run again with fresh meter to ensure deterministic cost
    let mut meter2 = GasMeter::new(100);
    let stack2 = bytecode::execute(&code, &mut meter2).unwrap();
    assert_eq!(stack1, stack2);
    assert_eq!(used1, meter2.used());
    // expected gas: pushes 3*2=6, add=1, mul=2 -> 9
    assert_eq!(used1, 9);
}
