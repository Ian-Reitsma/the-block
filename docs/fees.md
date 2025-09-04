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

Transactions must bid a fee ≥ `base_fee`. The mempool sorts by effective fee
and evicts the lowest bidders when full. After a block is mined, the miner
collects the `base_fee` and any tips above it.

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
