use super::{runtime::Vm, state::ContractId};
use foundation_serialization::{Deserialize, Serialize};

/// Transactions targeting the contract VM.
#[derive(Clone, Serialize, Deserialize)]
pub enum ContractTx {
    Deploy {
        code: Vec<u8>,
    },
    DeployWasm {
        wasm: Vec<u8>,
        abi: Vec<u8>,
    },
    Call {
        id: ContractId,
        input: Vec<u8>,
        gas_limit: u64,
        gas_price: u64,
    },
}

impl ContractTx {
    /// Apply this transaction to the provided VM.
    pub fn apply(self, vm: &mut Vm, balance: &mut u64) -> Result<Vec<u8>, &'static str> {
        match self {
            ContractTx::Deploy { code } => {
                vm.deploy(code);
                Ok(Vec::new())
            }
            ContractTx::DeployWasm { wasm, abi } => {
                vm.deploy_wasm(wasm, abi);
                Ok(Vec::new())
            }
            ContractTx::Call {
                id,
                input,
                gas_limit,
                gas_price,
            } => {
                let (out, _) = vm.execute(id, &input, gas_limit, gas_price, balance)?;
                Ok(out)
            }
        }
    }
}
