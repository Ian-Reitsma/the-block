# WASM Contracts
> **Review (2025-09-25):** Synced WASM Contracts guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Smart contracts can now be authored in WebAssembly and executed through the
first-party interpreter. Modules begin with the `TBW1` magic header followed by
bytecode instructions (`push_i64`, `push_input`, arithmetic, `eq`, `return`).
Each instruction consumes one unit of gas, so execution remains deterministic
and easy to profile without the Wasmtime/Cranelift stack.

## Determinism

- Execution relies entirely on the in-house interpreter—no Wasmtime/wasmi
  dependency is linked. Contracts produce deterministic output and may return
  multiple values via `return N`.
- Gas accounting remains deterministic through `GasMeter`, so higher layers can
  simulate success/failure paths without depending on external crates.
- Contracts interact with the host only through the exposed memory and
  exported `entry` function.

## CLI

Deploy contracts by pointing the CLI at a compiled module:

```bash
contract deploy --wasm path/to/contract.wasm
```

ABI descriptors are extracted automatically and stored alongside the bytecode.

## Telemetry

Runtime usage surfaces via `wasm_contract_executions_total` and
`wasm_gas_consumed_total` metrics.

## Security

Always audit uploaded bytecode and ABI descriptors. Contracts run in a sandbox
but can still exhaust resources via unbounded loops if gas limits are too high.
Keep ABI descriptors minimal and validate any memory offsets provided by the
contract.
