# WASM Contracts
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Smart contracts can now be authored in WebAssembly and executed through a
Wasmtime interpreter configured for determinism. The engine enables fuel-based
gas metering and rejects any nondeterministic features such as host time or
randomness.

## Determinism

- Execution uses Wasmtime with `consume_fuel` and NaN canonicalization enabled.
- Gas is tracked via Wasmtime fuel and mirrored to chain gas units.
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