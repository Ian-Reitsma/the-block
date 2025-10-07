# Virtual Machine Gas Model
> **Review (2025-09-25):** Synced Virtual Machine Gas Model guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

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

The WASM executor translates remaining gas into engine fuel using
`FUEL_PER_GAS`. Modules target a lightweight first-party interpreter with the
`TBW1` magic header and a compact stack machine instruction set. Each opcode
(`push_i64`, `push_input`, arithmetic, equality, and `return`) consumes one unit
of gas when executed. A return count of zero yields the entire stack as the
result payload; a positive count returns that many values from the top of the
stack. Modules that request more inputs than supplied or attempt division by
zero raise an execution error.

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
