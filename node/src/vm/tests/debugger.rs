use super::super::{Debugger, OpCode};

#[test]
fn trace_deterministic() {
    let code = vec![
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
        OpCode::Halt as u8,
    ];
    let mut d1 = Debugger::new(code.clone());
    let mut d2 = Debugger::new(code);
    let t1 = d1.run().to_vec();
    let t2 = d2.run().to_vec();
    assert_eq!(t1, t2);
}
