# Contract Development Guide

This guide covers the basic contract workflow for The‑Block's prototype VM.
For commands that inspect WASM exports or log indexes, build the CLI with the
`full` feature flag (`cargo run -p contract-cli --features full -- …`) so the
optional Wasmtime and SQLite helpers are available. Lean builds without the flag
skip those integrations to keep test harnesses lightweight.

## Opcode ABI

Generate the opcode ABI JSON for tooling:

```bash
cargo run -p contract-cli -- abi opcodes.json
```

The resulting `opcodes.json` maps opcode names to their numeric values.

## Deploying Contracts

Compile or hand‑write bytecode and deploy it via the CLI. Bytecode must be
provided as a hex string. The CLI writes both the raw code and an empty key/value
store to `~/.the_block/state/contracts/<hash>/`.

```bash
cargo run -p contract-cli -- deploy <HEX_CODE> \
  --from alice --gas-limit 1_000_000
```

The command prints the assigned contract hash. Subsequent calls reference this
hash and modify on‑disk state atomically.

## Calling Contracts

Invoke a deployed contract by hash and calldata:

```bash
cargo run -p contract-cli -- call <HASH> <HEX_INPUT> \
  --from alice --gas-limit 50_000 --gas-price 1
```

Results include return data and gas used. Contract execution persists state across restarts.
Inspect stored state by calling again after restarting the node or CLI.

## Persistence Details

Each contract gets a dedicated directory keyed by its BLAKE3 hash. Values are
encoded with bincode and prefixed with a checksum. Writes use a journal file so
an unexpected crash rolls back the entire transaction.

### Tests

- `node/tests/vm.rs::state_persists_across_restarts` ensures state survives a
  VM restart.
- `node/tests/vm.rs::contract_cli_flow` deploys a contract, performs a call,
  restarts, and verifies the state root is unchanged.

