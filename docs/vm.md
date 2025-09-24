# Virtual Machine Gas Model
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The Block VM uses a minimal stack-based executor with explicit gas
accounting. Every operation consumes a deterministic amount of gas. The
current schedule is:

| Operation | Gas |
|-----------|-----|
| `halt`    | 0   |
| `push`    | 1 (+1 per immediate byte)
| `add`     | 1   |
| `sub`     | 1   |
| `mul`     | 2   |
| `div`     | 3   |
| `mod`     | 3   |
| `and`     | 1   |
| `or`      | 1   |
| `xor`     | 1   |
| `load`    | 10  |
| `store`   | 20  |
| `hash`    | 50  |
| contract code load | 5 |

Immediates for `push` cost an additional unit via `GAS_IMMEDIATE`.

`mod` and `xor` operate on the top two stack elements in the same manner as
`div` and `and`, returning the remainder and bitwise exclusive-or respectively
while preserving wrapping semantics. `load` and `store` are storage opcodes:
`store` pops a value, charges the storage write cost, and persists it for later
`load` calls; `load` pushes the last stored value back onto the stack and pays
the storage read cost. `hash` pops a value, charges the hash gas, and pushes the
first eight bytes of its BLAKE3 digest as a `u64`.

## Fuel conversion

The WASM executor translates remaining gas into Wasmtime fuel using
`FUEL_PER_GAS`. Each call to `vm::wasm::execute` seeds the store via
`Store::set_fuel` with the full `GasMeter::remaining()` budget and refuses to
start if no gas is left, ensuring contracts observe deterministic out-of-gas
failures before any code runs. After execution completes the remaining fuel is
queried with `Store::get_fuel` so the meter can be charged exactly. If the host
Wasmtime build was compiled without fuel support the executor now surfaces a
clear error instructing operators to enable `Config::consume_fuel` rather than
silently mis-accounting gas.

## Example

Executing the bytecode `PUSH 6; PUSH 2; DIV; PUSH 3; MUL` consumes:

* `push` ×3 → 6 gas
* `div` → 3 gas
* `mul` → 2 gas
* contract code load → 5 gas
* storage write of the resulting stack value → 20 gas

Total: **36 gas**.

Extending the sequence with `STORE; LOAD; HASH` would incur an additional
`20 + 20 + 100 = 140` gas (write, read, hash) and leave the stack containing the
stored value followed by the truncated hash of that value.

Transactions supply a `gas_limit` and `gas_price`. Fees are deducted
`gas_used * gas_price` from the caller's balance.

For tooling, `rpc/vm.rs` exposes helper methods to estimate gas usage,
trace execution, and inspect contract storage. Storage can also be
manipulated directly via the `vm.storage_read` and `vm.storage_write`
RPC calls for off-chain inspection.