use crypto_suite::hashing::blake3;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use concurrency::Lazy;
use foundation_serialization::json::{self, json};
use serde::Serialize;

use super::{
    gas::{self, GasMeter},
    opcodes::OpCode,
    state::{ContractId, State},
};

static VM_DEBUG_ENABLED: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

/// Enable or disable global VM debugging features at runtime.
pub fn set_vm_debug_enabled(v: bool) {
    VM_DEBUG_ENABLED.store(v, Ordering::Relaxed);
}

/// Check if VM debugging is enabled.
pub fn vm_debug_enabled() -> bool {
    VM_DEBUG_ENABLED.load(Ordering::Relaxed)
}

#[derive(Clone, Serialize, Debug, PartialEq)]
pub struct TraceStep {
    pub pc: usize,
    pub op: String,
    pub stack: Vec<u64>,
    pub storage: Vec<u8>,
}

/// Simple single-step debugger for the toy VM.
pub struct Debugger {
    code: Vec<u8>,
    pc: usize,
    stack: Vec<u64>,
    state: State,
    contract_id: ContractId,
    breakpoints: HashSet<usize>,
    meter: GasMeter,
    trace: Vec<TraceStep>,
}

impl Debugger {
    pub fn new(code: Vec<u8>) -> Self {
        let mut state = State::new();
        let contract_id = state.deploy(code.clone());
        Self {
            code,
            pc: 0,
            stack: Vec::new(),
            state,
            contract_id,
            breakpoints: HashSet::new(),
            meter: GasMeter::new(u64::MAX),
            trace: Vec::new(),
        }
    }

    pub fn add_breakpoint(&mut self, pc: usize) {
        self.breakpoints.insert(pc);
    }

    pub fn step(&mut self) -> Option<TraceStep> {
        if self.pc >= self.code.len() {
            return None;
        }
        let op = OpCode::from_byte(self.code[self.pc])?;
        self.pc += 1;
        let res = match op {
            OpCode::Halt => {
                // record halt step then finish
                self.meter.charge(gas::cost(op)).ok()?;
                let storage = self.state.storage(self.contract_id).unwrap_or_default();
                let step = TraceStep {
                    pc: self.pc - 1,
                    op: format!("{:?}", op),
                    stack: self.stack.clone(),
                    storage,
                };
                self.trace.push(step.clone());
                return None;
            }
            OpCode::Push => {
                if self.pc + 8 > self.code.len() {
                    return None;
                }
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&self.code[self.pc..self.pc + 8]);
                self.pc += 8;
                self.meter.charge(gas::cost(op)).ok()?;
                self.meter.charge(gas::GAS_IMMEDIATE).ok()?;
                self.stack.push(u64::from_le_bytes(buf));
                Some(())
            }
            OpCode::Add => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                let a = self.stack.pop()?;
                self.stack.push(a.wrapping_add(b));
                Some(())
            }
            OpCode::Sub => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                let a = self.stack.pop()?;
                self.stack.push(a.wrapping_sub(b));
                Some(())
            }
            OpCode::Mul => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                let a = self.stack.pop()?;
                self.stack.push(a.wrapping_mul(b));
                Some(())
            }
            OpCode::Div => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                if b == 0 {
                    return None;
                }
                let a = self.stack.pop()?;
                self.stack.push(a / b);
                Some(())
            }
            OpCode::Mod => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                if b == 0 {
                    return None;
                }
                let a = self.stack.pop()?;
                self.stack.push(a % b);
                Some(())
            }
            OpCode::And => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                let a = self.stack.pop()?;
                self.stack.push(a & b);
                Some(())
            }
            OpCode::Or => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                let a = self.stack.pop()?;
                self.stack.push(a | b);
                Some(())
            }
            OpCode::Xor => {
                self.meter.charge(gas::cost(op)).ok()?;
                let b = self.stack.pop()?;
                let a = self.stack.pop()?;
                self.stack.push(a ^ b);
                Some(())
            }
            OpCode::Load => {
                self.meter.charge(gas::cost(op)).ok()?;
                self.meter.charge(gas::GAS_STORAGE_READ).ok()?;
                let val = self
                    .state
                    .storage(self.contract_id)
                    .and_then(|b| {
                        if b.len() >= 8 {
                            let mut arr = [0u8; 8];
                            arr.copy_from_slice(&b[..8]);
                            Some(u64::from_le_bytes(arr))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                self.stack.push(val);
                Some(())
            }
            OpCode::Store => {
                self.meter.charge(gas::cost(op)).ok()?;
                let v = self.stack.pop()?;
                self.meter.charge(gas::GAS_STORAGE_WRITE).ok()?;
                self.state
                    .set_storage(self.contract_id, v.to_le_bytes().to_vec());
                Some(())
            }
            OpCode::Hash => {
                self.meter.charge(gas::cost(op)).ok()?;
                let v = self.stack.pop()?;
                self.meter.charge(gas::GAS_HASH).ok()?;
                let hash = blake3::hash(&v.to_le_bytes());
                let mut out = [0u8; 8];
                out.copy_from_slice(&hash.as_bytes()[..8]);
                self.stack.push(u64::from_le_bytes(out));
                Some(())
            }
        };
        if res.is_none() {
            return None;
        }
        self.state.snapshot(self.contract_id);
        let storage = self.state.storage(self.contract_id).unwrap_or_default();
        let step = TraceStep {
            pc: self.pc - 1,
            op: format!("{:?}", op),
            stack: self.stack.clone(),
            storage,
        };
        self.trace.push(step.clone());
        Some(step)
    }

    pub fn run(&mut self) -> &[TraceStep] {
        while self.pc < self.code.len() {
            if self.breakpoints.contains(&self.pc) {
                break;
            }
            if self.step().is_none() {
                break;
            }
        }
        &self.trace
    }

    pub fn dump_json<P: AsRef<Path>>(&self, path: P) {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = json::to_vec_pretty(&self.trace) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn dump_chrome<P: AsRef<Path>>(&self, path: P) {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut events = Vec::new();
        for (i, step) in self.trace.iter().enumerate() {
            events.push(json!({
                "name": step.op,
                "ph": "X",
                "ts": i as u64,
                "dur": 1,
            }));
        }
        let out = json!({"traceEvents": events});
        let _ = std::fs::write(path, json::to_vec_pretty(&out).unwrap());
    }

    pub fn trace(&self) -> &[TraceStep] {
        &self.trace
    }
}
