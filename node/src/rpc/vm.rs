use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Mutex;

use crate::vm::{
    exec,
    gas::GasMeter,
    opcodes::OpCode,
    runtime::{Vm, VmType},
    state::ContractId,
};

static VM: Lazy<Mutex<Vm>> = Lazy::new(|| Mutex::new(Vm::new(VmType::Evm)));

/// Estimate gas for executing the provided bytecode.
pub fn estimate_gas(code: Vec<u8>) -> u64 {
    let mut meter = GasMeter::new(u64::MAX);
    let mut load = || 0u64;
    let mut store = |_v: u64| {};
    let _ = exec::execute(&code, &mut meter, &mut load, &mut store);
    meter.used()
}

/// Execute bytecode and return the trace of opcodes executed.
pub fn exec_trace(code: Vec<u8>) -> Vec<String> {
    let mut pc = 0usize;
    let mut trace = Vec::new();
    let mut meter = GasMeter::new(u64::MAX);
    while pc < code.len() {
        if let Some(op) = OpCode::from_byte(code[pc]) {
            trace.push(format!("{:?}", op));
            pc += 1;
            if op == OpCode::Push {
                pc += 8;
            }
            let _ = meter.charge(crate::vm::gas::cost(op));
            if op == OpCode::Push {
                let _ = meter.charge(crate::vm::gas::GAS_IMMEDIATE);
            }
            if op == OpCode::Halt {
                break;
            }
        } else {
            break;
        }
    }
    trace
}

/// Read contract storage bytes for off-chain inspection.
pub fn storage_read(id: ContractId) -> Option<Vec<u8>> {
    VM.lock().unwrap().read(id)
}

/// Overwrite contract storage (off-chain inspection helpers).
pub fn storage_write(id: ContractId, data: Vec<u8>) {
    VM.lock().unwrap().write(id, data);
}
