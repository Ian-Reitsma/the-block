#![forbid(unsafe_code)]

use diagnostics::{anyhow, Result};

use super::super::gas::GasMeter;

/// Four byte module identifier used by the first-party interpreter.
pub const MAGIC: [u8; 4] = *b"TBW1";

/// Currently supported module format version.
pub const VERSION_V1: u8 = 1;

/// Supported opcodes for the minimal stack-based interpreter.
pub mod opcodes {
    pub const NOP: u8 = 0x00;
    pub const PUSH_I64: u8 = 0x01;
    pub const PUSH_INPUT: u8 = 0x02;
    pub const ADD_I64: u8 = 0x03;
    pub const SUB_I64: u8 = 0x04;
    pub const MUL_I64: u8 = 0x05;
    pub const DIV_I64: u8 = 0x06;
    pub const EQ_I64: u8 = 0x07;
    pub const RETURN: u8 = 0x10;
}

const HEADER_LEN: usize = 5; // magic (4) + version (1)

/// Disassembled instruction used for metadata and tooling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Nop,
    PushConst(i64),
    PushInput(u8),
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Return(usize),
}

/// Minimal metadata about a module required by tooling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleMetadata {
    pub version: u8,
    pub instruction_count: usize,
    pub required_inputs: usize,
    pub return_values: Option<usize>,
}

impl ModuleMetadata {
    /// Encode the metadata into a human readable byte vector stored alongside
    /// the deployed contract for inspection.
    pub fn encode(&self) -> Vec<u8> {
        format!(
            "version={};instructions={};inputs={};return_values={}\n",
            self.version,
            self.instruction_count,
            self.required_inputs,
            self.return_values.unwrap_or(0)
        )
        .into_bytes()
    }
}

/// Execute a first-party WASM module against the provided input, returning the
/// resulting bytes written by the `return` instruction.
pub fn execute(code: &[u8], input: &[u8], meter: &mut GasMeter) -> Result<Vec<u8>> {
    let (version, instructions) = parse_program(code)?;
    if version != VERSION_V1 {
        return Err(anyhow!("unsupported wasm module version {version}"));
    }

    let inputs = decode_inputs(input)?;
    let mut stack: Vec<i64> = Vec::new();

    for instr in &instructions {
        meter.charge(1).map_err(|_| anyhow!("out of gas"))?;
        match *instr {
            Instruction::Nop => {}
            Instruction::PushConst(value) => stack.push(value),
            Instruction::PushInput(index) => {
                let idx = index as usize;
                let value = inputs
                    .get(idx)
                    .copied()
                    .ok_or_else(|| anyhow!("missing input at index {idx}"))?;
                stack.push(value);
            }
            Instruction::Add => {
                let (a, b) = pop_pair(&mut stack)?;
                stack.push(a.saturating_add(b));
            }
            Instruction::Sub => {
                let (a, b) = pop_pair(&mut stack)?;
                stack.push(a.saturating_sub(b));
            }
            Instruction::Mul => {
                let (a, b) = pop_pair(&mut stack)?;
                stack.push(a.saturating_mul(b));
            }
            Instruction::Div => {
                let (a, b) = pop_pair(&mut stack)?;
                if b == 0 {
                    return Err(anyhow!("division by zero"));
                }
                stack.push(a / b);
            }
            Instruction::Eq => {
                let (a, b) = pop_pair(&mut stack)?;
                stack.push(if a == b { 1 } else { 0 });
            }
            Instruction::Return(count) => {
                let requested = if count == 0 { stack.len() } else { count };
                if requested > stack.len() {
                    return Err(anyhow!(
                        "return requested {requested} values but stack has {}",
                        stack.len()
                    ));
                }
                let start = stack.len() - requested;
                let mut out = Vec::with_capacity(requested * 8);
                for value in &stack[start..] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                return Ok(out);
            }
        }
    }

    Err(anyhow!("module missing return instruction"))
}

/// Analyze the module and surface metadata consumed by tooling.
pub fn analyze(code: &[u8]) -> Result<ModuleMetadata> {
    let (version, instructions) = parse_program(code)?;
    let mut max_input = None;
    let mut return_values = None;
    for instr in &instructions {
        match *instr {
            Instruction::PushInput(index) => {
                let current = max_input.unwrap_or(0);
                max_input = Some(current.max(index as usize));
            }
            Instruction::Return(values) => {
                return_values = Some(values);
            }
            _ => {}
        }
    }
    let required_inputs = max_input.map(|idx| idx + 1).unwrap_or(0);
    Ok(ModuleMetadata {
        version,
        instruction_count: instructions.len(),
        required_inputs,
        return_values,
    })
}

/// Produce a disassembled view of the module for debugging utilities.
pub fn disassemble(code: &[u8]) -> Result<Vec<Instruction>> {
    let (_, instructions) = parse_program(code)?;
    Ok(instructions)
}

fn decode_inputs(raw: &[u8]) -> Result<Vec<i64>> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if raw.len() % 8 != 0 {
        return Err(anyhow!("inputs must be provided as 8-byte chunks"));
    }
    Ok(raw
        .chunks_exact(8)
        .map(|chunk| {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(chunk);
            i64::from_le_bytes(buf)
        })
        .collect())
}

fn pop_pair(stack: &mut Vec<i64>) -> Result<(i64, i64)> {
    let b = stack.pop().ok_or_else(|| anyhow!("stack underflow"))?;
    let a = stack.pop().ok_or_else(|| anyhow!("stack underflow"))?;
    Ok((a, b))
}

fn parse_program(code: &[u8]) -> Result<(u8, Vec<Instruction>)> {
    if code.len() < HEADER_LEN {
        return Err(anyhow!("wasm module missing header"));
    }
    if code[..4] != MAGIC {
        return Err(anyhow!("invalid wasm magic"));
    }
    let version = code[4];
    let mut pc = HEADER_LEN;
    let mut instructions = Vec::new();
    while pc < code.len() {
        let opcode = code[pc];
        pc += 1;
        let instr = match opcode {
            opcodes::NOP => Instruction::Nop,
            opcodes::PUSH_I64 => {
                let imm = read_i64(code, &mut pc)?;
                Instruction::PushConst(imm)
            }
            opcodes::PUSH_INPUT => {
                let index = read_byte(code, &mut pc)?;
                Instruction::PushInput(index)
            }
            opcodes::ADD_I64 => Instruction::Add,
            opcodes::SUB_I64 => Instruction::Sub,
            opcodes::MUL_I64 => Instruction::Mul,
            opcodes::DIV_I64 => Instruction::Div,
            opcodes::EQ_I64 => Instruction::Eq,
            opcodes::RETURN => {
                let count = read_byte(code, &mut pc)? as usize;
                Instruction::Return(count)
            }
            other => {
                return Err(anyhow!("unknown opcode 0x{other:02x}"));
            }
        };
        instructions.push(instr);
    }
    Ok((version, instructions))
}

fn read_i64(code: &[u8], pc: &mut usize) -> Result<i64> {
    let end = *pc + 8;
    if end > code.len() {
        return Err(anyhow!("unexpected eof while reading immediate"));
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&code[*pc..end]);
    *pc = end;
    Ok(i64::from_le_bytes(buf))
}

fn read_byte(code: &[u8], pc: &mut usize) -> Result<u8> {
    if *pc >= code.len() {
        return Err(anyhow!("unexpected eof while reading byte"));
    }
    let byte = code[*pc];
    *pc += 1;
    Ok(byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_with_ops(ops: &[u8]) -> Vec<u8> {
        let mut module = Vec::with_capacity(HEADER_LEN + ops.len());
        module.extend_from_slice(&MAGIC);
        module.push(VERSION_V1);
        module.extend_from_slice(ops);
        module
    }

    fn push_i64(value: i64) -> Vec<u8> {
        let mut buf = vec![opcodes::PUSH_I64];
        buf.extend_from_slice(&value.to_le_bytes());
        buf
    }

    #[test]
    fn parses_and_executes_simple_program() {
        let mut ops = Vec::new();
        ops.extend_from_slice(&push_i64(2));
        ops.extend_from_slice(&push_i64(3));
        ops.push(opcodes::ADD_I64);
        ops.extend_from_slice(&[opcodes::RETURN, 1]);
        let module = module_with_ops(&ops);
        let mut meter = GasMeter::new(10);
        let output = execute(&module, &[], &mut meter).expect("executes");
        assert_eq!(output, 5i64.to_le_bytes());
        assert!(meter.used() >= 4);
    }

    #[test]
    fn uses_inputs() {
        let ops = [
            opcodes::PUSH_INPUT,
            1,
            opcodes::PUSH_INPUT,
            0,
            opcodes::SUB_I64,
            opcodes::RETURN,
            1,
        ];
        let module = module_with_ops(&ops);
        let mut meter = GasMeter::new(10);
        let mut input = Vec::new();
        input.extend_from_slice(&4i64.to_le_bytes());
        input.extend_from_slice(&10i64.to_le_bytes());
        let out = execute(&module, &input, &mut meter).expect("executes");
        assert_eq!(out, 6i64.to_le_bytes());
    }

    #[test]
    fn metadata_reports_inputs_and_returns() {
        let ops = [
            opcodes::PUSH_INPUT,
            0,
            opcodes::PUSH_INPUT,
            1,
            opcodes::ADD_I64,
            opcodes::RETURN,
            1,
        ];
        let module = module_with_ops(&ops);
        let meta = analyze(&module).expect("analyze");
        assert_eq!(meta.version, VERSION_V1);
        assert_eq!(meta.instruction_count, 4);
        assert_eq!(meta.required_inputs, 2);
        assert_eq!(meta.return_values, Some(1));
    }

    #[test]
    fn disassemble_roundtrip() {
        let ops = [
            opcodes::PUSH_I64,
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            opcodes::EQ_I64,
            opcodes::RETURN,
            1,
        ];
        let module = module_with_ops(&ops);
        let dis = disassemble(&module).expect("disassemble");
        assert!(matches!(dis[0], Instruction::PushConst(1)));
        assert!(matches!(dis[1], Instruction::Eq));
    }
}
