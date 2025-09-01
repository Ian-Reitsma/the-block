use super::{
    abi, bytecode,
    gas::GasMeter,
    state::{ContractId, State},
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
            state: State::default(),
        }
    }

    /// Deploy a contract returning its identifier.
    pub fn deploy(&mut self, code: Vec<u8>) -> ContractId {
        self.state.deploy(code)
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
        let code = self.state.code(id).cloned().ok_or("unknown contract")?;
        let mut meter = GasMeter::new(gas_limit);
        // execute bytecode; append input as pushes onto stack
        let mut exec_code = code.clone();
        if !input.is_empty() {
            // treat input as single push of u64 if length>=8 else zero padded
            let mut buf = [0u8; 8];
            buf[..input.len().min(8)].copy_from_slice(&input[..input.len().min(8)]);
            exec_code.extend_from_slice(&[bytecode::OpCode::Push as u8]);
            exec_code.extend_from_slice(&buf);
        }
        exec_code.push(bytecode::OpCode::Halt as u8);
        let stack = bytecode::execute(&exec_code, &mut meter)?;
        let used = meter.used();
        let fee = used.checked_mul(gas_price).ok_or("fee overflow")?;
        if *balance < fee {
            return Err("insufficient balance");
        }
        *balance -= fee;
        // store last stack element as state if any
        if let Some(last) = stack.last() {
            self.state.set_storage(id, last.to_le_bytes().to_vec());
        }
        Ok((stack.iter().flat_map(|v| v.to_le_bytes()).collect(), used))
    }

    /// Read back contract state.
    #[must_use]
    pub fn read(&self, id: ContractId) -> Option<Vec<u8>> {
        self.state.code(id).cloned()
    }
}

// prevent dead-code warnings for ABI helpers
#[allow(unused_imports)]
use abi::{decode_u64, encode_u64};
