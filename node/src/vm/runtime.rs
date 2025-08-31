use super::{
    abi,
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
    pub fn execute(
        &mut self,
        id: ContractId,
        input: &[u8],
        gas_limit: u64,
    ) -> Result<(Vec<u8>, u64), &'static str> {
        let mut meter = GasMeter::new(gas_limit);
        meter.charge(1)?; // base cost
        let mut output = self.state.code(id).cloned().ok_or("unknown contract")?;
        meter.charge(output.len() as u64)?;
        output.extend_from_slice(input);
        self.state.set_storage(id, output.clone());
        Ok((output, meter.used()))
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
