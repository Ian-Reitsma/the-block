use super::{
    abi, exec,
    gas::{self, GasMeter},
    opcodes,
    state::{ContractId, State},
    wasm,
};

/// Supported VM types. Extend when additional runtimes are added.
#[derive(Clone, Copy)]
pub enum VmType {
    Evm,
    Wasm,
}

/// Simple VM wrapper. Real engine should execute bytecode.
pub struct Vm {
    pub vm_type: VmType,
    state: State,
}

impl Vm {
    pub fn new(vm_type: VmType) -> Self {
        Self {
            vm_type,
            state: State::new(),
        }
    }

    /// Create a VM backed by persistent storage at the given path.
    pub fn new_persistent(vm_type: VmType, path: std::path::PathBuf) -> Self {
        Self {
            vm_type,
            state: State::with_path(path),
        }
    }

    /// Deploy a contract returning its identifier.
    pub fn deploy(&mut self, code: Vec<u8>) -> ContractId {
        self.state.deploy(code)
    }

    /// Deploy a WASM contract with ABI descriptor.
    pub fn deploy_wasm(&mut self, wasm: Vec<u8>, abi: Vec<u8>) -> ContractId {
        self.state.deploy_wasm(wasm, abi)
    }

    /// Execute a contract. Returns output bytes and gas used.
    /// Fees are deducted from the provided balance at `gas_price` per unit.
    pub fn execute(
        &mut self,
        id: ContractId,
        input: &[u8],
        gas_limit: u64,
        gas_price: u64,
        balance: &mut u64,
    ) -> Result<(Vec<u8>, u64), &'static str> {
        let mut meter = GasMeter::new(gas_limit);
        match self.vm_type {
            VmType::Evm => {
                let code = self.state.code(id).ok_or("unknown contract")?;
                meter.charge(gas::GAS_CODE_READ)?;
                // execute bytecode; append input as pushes onto stack
                let mut exec_code = code.clone();
                if !input.is_empty() {
                    let mut buf = [0u8; 8];
                    buf[..input.len().min(8)].copy_from_slice(&input[..input.len().min(8)]);
                    exec_code.extend_from_slice(&[opcodes::OpCode::Push as u8]);
                    exec_code.extend_from_slice(&buf);
                }
                exec_code.push(opcodes::OpCode::Halt as u8);
                let mut load = || {
                    self.state
                        .storage(id)
                        .and_then(|b| {
                            if b.len() >= 8 {
                                let mut arr = [0u8; 8];
                                arr.copy_from_slice(&b[..8]);
                                Some(u64::from_le_bytes(arr))
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0)
                };
                let mut store = |v: u64| {
                    self.state.set_storage(id, v.to_le_bytes().to_vec());
                };
                let stack = exec::execute(&exec_code, &mut meter, &mut load, &mut store)?;
                let used = meter.used();
                let fee = used.checked_mul(gas_price).ok_or("fee overflow")?;
                if *balance < fee {
                    return Err("insufficient balance");
                }
                *balance -= fee;
                if let Some(last) = stack.last() {
                    meter.charge(gas::GAS_STORAGE_WRITE)?;
                    self.state.set_storage(id, last.to_le_bytes().to_vec());
                }
                Ok((stack.iter().flat_map(|v| v.to_le_bytes()).collect(), used))
            }
            VmType::Wasm => {
                let code = self.state.wasm(id).ok_or("unknown contract")?;
                let out = wasm::execute(&code, input, &mut meter).map_err(|_| "wasm error")?;
                let used = meter.used();
                let fee = used.checked_mul(gas_price).ok_or("fee overflow")?;
                if *balance < fee {
                    return Err("insufficient balance");
                }
                *balance -= fee;
                if !out.is_empty() {
                    meter.charge(gas::GAS_STORAGE_WRITE)?;
                    self.state.set_storage(id, out.clone());
                }
                Ok((out, used))
            }
        }
    }

    /// Read back contract state.
    #[must_use]
    pub fn read(&self, id: ContractId) -> Option<Vec<u8>> {
        self.state.storage(id)
    }

    /// Overwrite contract state directly.
    pub fn write(&mut self, id: ContractId, data: Vec<u8>) {
        self.state.set_storage(id, data);
    }
}

// prevent dead-code warnings for ABI helpers
#[allow(unused_imports)]
use abi::{decode_u64, encode_u64};
