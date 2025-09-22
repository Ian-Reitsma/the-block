use anyhow::{anyhow, ensure, Context, Result};
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
    let remaining = meter.remaining();
    if remaining == 0 {
        return Err(anyhow!("out of gas"));
    }
    let fuel = gas::to_fuel(remaining);
    ensure!(fuel > 0, "wasm execution requires positive fuel");
    store.set_fuel(fuel).with_context(|| {
        "wasmtime engine compiled without fuel support; enable Config::consume_fuel"
    })?;
    let instance = Linker::new(&engine).instantiate(&mut store, &module)?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| anyhow!("missing memory"))?;
    let ptr = abi_wasm::write(&mut store, &memory, input);
    let func = instance.get_typed_func::<(i32, i32), i32>(&mut store, "entry")?;
    let len = func.call(&mut store, (ptr, input.len() as i32))?;
    let out = abi_wasm::read(&store, &memory, ptr, len).unwrap_or_default();
    let remaining_fuel = store.get_fuel().with_context(|| {
        "wasmtime engine compiled without fuel support; enable Config::consume_fuel"
    })?;
    let fuel_used = fuel.saturating_sub(remaining_fuel);
    let mut gas_used = gas::from_fuel(fuel_used);
    if gas_used == 0 {
        gas_used = remaining.min(1);
        if gas_used == 0 {
            return Err(anyhow!("out of gas"));
        }
    } else if gas_used > remaining {
        gas_used = remaining;
    }
    meter.charge(gas_used).map_err(anyhow::Error::msg)?;
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::WASM_CONTRACT_EXECUTIONS_TOTAL.inc();
        crate::telemetry::WASM_GAS_CONSUMED_TOTAL.inc_by(gas_used);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{execute, GasMeter};

    fn sample_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "entry") (param i32 i32) (result i32)
                    local.get 1)
            )"#,
        )
        .expect("valid wat")
    }

    #[test]
    fn execution_respects_remaining_gas() {
        let code = sample_wasm();
        let mut empty_meter = GasMeter::new(0);
        let err = execute(&code, b"ping", &mut empty_meter).expect_err("zero gas fails");
        assert!(err.to_string().contains("out of gas"));

        let mut probe_meter = GasMeter::new(10_000);
        execute(&code, b"ping", &mut probe_meter).expect("probe execution succeeds");
        let consumed = probe_meter.used();
        assert!(consumed > 0, "execution must consume gas");

        let mut limited_meter = GasMeter::new(consumed);
        execute(&code, b"pong", &mut limited_meter).expect("exact budget succeeds");
        assert_eq!(limited_meter.remaining(), 0);
        let second = execute(&code, b"pong", &mut limited_meter).expect_err("exhausted");
        assert!(second.to_string().contains("out of gas"));
    }
}
