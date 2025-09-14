use anyhow::Result;
use wasmtime::{Config, Engine, Linker, Module, Store};

use super::{abi_wasm, gas::GasMeter};

pub mod gas;

/// Execute WASM bytecode with deterministic metering.
pub fn execute(code: &[u8], input: &[u8], meter: &mut GasMeter) -> Result<Vec<u8>> {
    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    cfg.cranelift_nan_canonicalization(true);
    let engine = Engine::new(&cfg)?;
    let module = Module::new(&engine, code)?;
    let mut store = Store::new(&engine, ());
    store.add_fuel(gas::to_fuel(meter.remaining()))?;
    let instance = Linker::new(&engine).instantiate(&mut store, &module)?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| anyhow::anyhow!("missing memory"))?;
    let ptr = abi_wasm::write(&mut store, &memory, input);
    let func = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "entry")?;
    let len = func.call(&mut store, (ptr, input.len() as i32))?;
    let out = abi_wasm::read(&store, &memory, ptr, len).unwrap_or_default();
    let fuel = store.fuel_consumed().unwrap_or(0);
    let gas_used = gas::from_fuel(fuel);
    meter.charge(gas_used)?;
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::WASM_CONTRACT_EXECUTIONS_TOTAL.inc();
        crate::telemetry::WASM_GAS_CONSUMED_TOTAL.inc_by(gas_used);
    }
    Ok(out)
}
