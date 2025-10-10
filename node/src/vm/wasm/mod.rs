#![forbid(unsafe_code)]

pub use super::gas::GasMeter;

mod interpreter;
pub use interpreter::{
    analyze, disassemble, execute, opcodes, Instruction, ModuleMetadata, MAGIC, VERSION_V1,
};

pub mod gas;

#[cfg(test)]
mod tests {
    use super::{execute, opcodes, GasMeter, MAGIC, VERSION_V1};

    fn sample_module() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC);
        buf.push(VERSION_V1);
        buf.extend_from_slice(&[
            opcodes::PUSH_I64,
            5,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            opcodes::PUSH_I64,
            7,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            opcodes::ADD_I64,
            opcodes::RETURN,
            1,
        ]);
        buf
    }

    #[test]
    fn execution_produces_expected_bytes() {
        let module = sample_module();
        let mut meter = GasMeter::new(32);
        let out = execute(&module, &[], &mut meter).expect("executes");
        assert_eq!(out, 12i64.to_le_bytes());
    }
}
