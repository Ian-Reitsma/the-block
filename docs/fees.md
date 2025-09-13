# Fee Market Reference

The Block uses an EIP‑1559 style fee mechanism to balance demand across
consumer and industrial lanes. Each block carries a `base_fee` that adjusts
according to gas usage, ensuring congestion results in higher fees while low
usage reduces costs.

## Base Fee Adjustment

The adjustment algorithm lives in `node/src/fees.rs`:

\[
\text{next} = \text{prev} + \frac{\text{prev} \times (\text{used} - \text{target})}{\text{target} \times 8}
\]

- `prev` – base fee from the previous block.
- `used` – gas consumed in the previous block.
- `target` – `TARGET_GAS_PER_BLOCK` (1 000 000) representing 50 % fullness.
- The factor of 1/8 caps per-block adjustment at 12.5 %.
- The result is clamped to a minimum of 1 to avoid a zero floor.

## Dual Fee Lanes

Transactions specify a lane tag: `consumer` or `industrial`. Separate mempools
ensure consumer traffic remains protected. A comfort guard monitors the p90 fee
in the consumer lane and temporarily defers industrial submissions when it
exceeds a configured threshold.

## Transaction Admission

Transactions must bid a fee ≥ `base_fee`. The mempool sorts by priority tips
and evicts the lowest bidders when full. When a block is mined, the protocol
burns the `base_fee` portion of each transaction and credits only the tip
excess to the miner.

## Querying the Base Fee

- **RPC** – `fees.current` returns the base fee expected for the next block.
- **CLI** – `tb-cli fees status` prints current fee, target, and recent usage.
- **Metrics** – `base_fee` gauge exposes the value for monitoring and alerting.

## Examples

Submitting a transaction with a tip:

```bash
tb-cli tx send --to addr --amount 1 --max-fee 500 --tip 50
```

The node rejects the transaction if `max-fee < base_fee`.

## Economic Rationale

Keeping block fullness near 50 % provides headroom for bursts and simplifies
capacity planning. Validators can adjust `TARGET_GAS_PER_BLOCK` through
`governance.params` proposals if demand patterns change.

## CT/IT Fee Splits

The `pct_ct` selector routes an arbitrary percentage of each transaction's fee
to consumer tokens (`CT`), with the remainder paid in industrial tokens (`IT`).
Examples:

| `pct_ct` | CT share | IT share |
|----------|---------|---------|
| `0`      | `0%`    | `100%`  |
| `37`     | `37%`   | `63%`   |
| `100`    | `100%`  | `0%`    |

During admission, `reserve_pending` debits the caller's balances according to
this split; `reserve_pending` in `node/src/transaction.rs` subtracts the CT and
IT portions separately. When the block is mined, the coinbase accumulates the
same proportions, so fee accounting matches the original split. See
[docs/transaction_lifecycle.md](transaction_lifecycle.md) for a full payload
example using `pct_ct`.
