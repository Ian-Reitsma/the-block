use crate::vm::gas::{self, GasMeter};
pub use crate::vm::opcodes::OpCode;

/// Execute bytecode returning the final stack as `Vec<u64>` and total gas used.
/// Each opcode costs 1 gas plus any immediates cost of 1.
pub fn execute(code: &[u8], meter: &mut GasMeter) -> Result<Vec<u64>, &'static str> {
    let mut pc = 0usize;
    let mut stack: Vec<u64> = Vec::new();
    while pc < code.len() {
        let op = OpCode::from_byte(code[pc]).ok_or("bad opcode")?;
        pc += 1;
        meter.charge(gas::cost(op))?;
        match op {
            OpCode::Halt => break,
            OpCode::Push => {
                if pc + 8 > code.len() {
                    return Err("truncated push");
                }
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&code[pc..pc + 8]);
                pc += 8;
                meter.charge(gas::GAS_IMMEDIATE)?;
                stack.push(u64::from_le_bytes(buf));
            }
            OpCode::Add => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a.wrapping_add(b));
            }
            OpCode::Sub => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a.wrapping_sub(b));
            }
            OpCode::Mul => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a.wrapping_mul(b));
            }
            OpCode::Div => {
                let b = stack.pop().ok_or("stack underflow")?;
                if b == 0 {
                    return Err("div by zero");
                }
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a / b);
            }
        }
    }
    Ok(stack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_add() {
        let code: Vec<u8> = vec![
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
            OpCode::Halt as u8,
        ];
        let mut meter = GasMeter::new(10);
        let stack = execute(&code, &mut meter).unwrap();
        assert_eq!(stack, vec![5]);
        assert_eq!(meter.used(), 5); // push(1+1)*2 + add
    }
}
