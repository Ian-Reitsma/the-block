# Virtual Machine Gas Model

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
| storage read  | 10  |
| storage write | 20  |
| contract code load | 5 |

Immediates for `push` cost an additional unit via `GAS_IMMEDIATE`.

## Example

Executing the bytecode `PUSH 6; PUSH 2; DIV; PUSH 3; MUL` consumes:

* `push` ×3 → 6 gas
* `div` → 3 gas
* `mul` → 2 gas
* contract code load → 5 gas
* storage write of the resulting stack value → 20 gas

Total: **36 gas**.

Transactions supply a `gas_limit` and `gas_price`. Fees are deducted
`gas_used * gas_price` from the caller's balance.

For tooling, `rpc/vm.rs` exposes helper methods to estimate gas usage,
trace execution, and inspect contract storage.
