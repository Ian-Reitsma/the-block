use super::{gas, opcodes::OpCode};

/// Execute raw bytecode with optional storage hooks.
/// `load` returns the current storage value and `store` persists a value.
#[allow(clippy::too_many_lines)]
pub fn execute<FLoad, FStore>(
    code: &[u8],
    meter: &mut gas::GasMeter,
    mut load: FLoad,
    mut store: FStore,
) -> Result<Vec<u64>, &'static str>
where
    FLoad: FnMut() -> u64,
    FStore: FnMut(u64),
{
    let mut stack: Vec<u64> = Vec::new();
    let mut pc = 0usize;
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
            OpCode::Mod => {
                let b = stack.pop().ok_or("stack underflow")?;
                if b == 0 {
                    return Err("mod by zero");
                }
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a % b);
            }
            OpCode::And => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a & b);
            }
            OpCode::Or => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a | b);
            }
            OpCode::Xor => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                stack.push(a ^ b);
            }
            OpCode::Load => {
                meter.charge(gas::GAS_STORAGE_READ)?;
                stack.push(load());
            }
            OpCode::Store => {
                let v = stack.pop().ok_or("stack underflow")?;
                meter.charge(gas::GAS_STORAGE_WRITE)?;
                store(v);
            }
            OpCode::Hash => {
                let v = stack.pop().ok_or("stack underflow")?;
                meter.charge(gas::GAS_HASH)?;
                let hash = blake3::hash(&v.to_le_bytes());
                let mut out = [0u8; 8];
                out.copy_from_slice(&hash.as_bytes()[..8]);
                stack.push(u64::from_le_bytes(out));
            }
        }
    }
    Ok(stack)
}
