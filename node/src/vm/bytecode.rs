use std::cell::Cell;

#[cfg(test)]
use crate::vm::gas;
use crate::vm::gas::GasMeter;
pub use crate::vm::opcodes::OpCode;

use super::exec;

/// Execute bytecode returning the final stack as `Vec<u64>` and total gas used.
/// Each opcode costs 1 gas plus any immediates cost of 1.
pub fn execute(code: &[u8], meter: &mut GasMeter) -> Result<Vec<u64>, &'static str> {
    let storage_cell = Cell::new(0u64);
    exec::execute(code, meter, || storage_cell.get(), |v| storage_cell.set(v))
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

    #[test]
    fn bitwise_ops() {
        let code: Vec<u8> = vec![
            OpCode::Push as u8,
            0b1100,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Push as u8,
            0b1010,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::And as u8,
            OpCode::Push as u8,
            0b0011,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Or as u8,
            OpCode::Halt as u8,
        ];
        let mut meter = GasMeter::new(20);
        let stack = execute(&code, &mut meter).unwrap();
        assert_eq!(stack, vec![0b1111]);
        // pushes:3 *2 gas =6, and=1, or=1 -> total 8 gas
        assert_eq!(meter.used(), 8);
    }

    #[test]
    fn modulo_and_xor() {
        let code: Vec<u8> = vec![
            OpCode::Push as u8,
            10,
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
            OpCode::Mod as u8,
            OpCode::Push as u8,
            0b1010,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Xor as u8,
            OpCode::Halt as u8,
        ];
        let mut meter = GasMeter::new(100);
        let stack = execute(&code, &mut meter).unwrap();
        assert_eq!(stack, vec![1 ^ 0b1010]);
        // gas: pushes 3*2=6, mod cost=3, xor cost=1 => 10
        assert_eq!(meter.used(), 10);
    }

    #[test]
    fn storage_roundtrip() {
        let code: Vec<u8> = vec![
            OpCode::Push as u8,
            42,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Store as u8,
            OpCode::Load as u8,
            OpCode::Halt as u8,
        ];
        let mut meter = GasMeter::new(200);
        let stack = execute(&code, &mut meter).unwrap();
        assert_eq!(stack, vec![42]);
        // push:2, store: cost(20)+extra(20)=40, load:10+10=20 => total 62
        assert_eq!(meter.used(), 62);
    }

    #[test]
    fn load_before_store_reads_zero() {
        let code: Vec<u8> = vec![OpCode::Load as u8, OpCode::Halt as u8];
        let mut meter = GasMeter::new(50);
        let stack = execute(&code, &mut meter).unwrap();
        assert_eq!(stack, vec![0]);
        assert_eq!(meter.used(), gas::GAS_STORAGE_READ * 2); // base cost + explicit charge
    }

    #[test]
    fn hash_opcode() {
        let code: Vec<u8> = vec![
            OpCode::Push as u8,
            7,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Hash as u8,
            OpCode::Halt as u8,
        ];
        let mut meter = GasMeter::new(200);
        let stack = execute(&code, &mut meter).unwrap();
        let expected = {
            let hash = blake3::hash(&7u64.to_le_bytes());
            let mut out = [0u8; 8];
            out.copy_from_slice(&hash.as_bytes()[..8]);
            u64::from_le_bytes(out)
        };
        assert_eq!(stack, vec![expected]);
        // push:2, hash:50(cost)+50(extra)=100 => 102 total
        assert_eq!(meter.used(), 102);
    }

    #[test]
    fn mod_by_zero_errors() {
        let code: Vec<u8> = vec![
            OpCode::Push as u8,
            5,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Push as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            OpCode::Mod as u8,
            OpCode::Halt as u8,
        ];
        let mut meter = GasMeter::new(50);
        let err = execute(&code, &mut meter).unwrap_err();
        assert_eq!(err, "mod by zero");
    }
}
