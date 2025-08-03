# CONSENSUS — Fee Routing State Transitions

This document codifies the fee routing logic that is baked into consensus after **FORK-FEE-01**.  The algebra below is the single source of truth for how a transaction's `fee` field is decomposed and applied.

## Fee Selector and Base

For every transaction, let:

- `f` be the raw `u64` fee base supplied by the sender.  Admission clamps `f < 2^63`.
- `ν` (`nu`) be the 2-bit fee selector embedded in the payload:
  - `0` → consumer-token only (CT)
  - `1` → industrial-token only (IT)
  - `2` → split CT∶IT
  - `3` → reserved; transactions using this value are invalid.

## Decomposition Function

Define `fee::decompose(ν, f) -> (fee_ct, fee_it)` as:

- `ν = 0`: `fee_ct = f`, `fee_it = 0`
- `ν = 1`: `fee_ct = 0`, `fee_it = f`
- `ν = 2` (split):
  - `fee_ct = ceil(f / 2)`
  - `fee_it = floor(f / 2)`

Both outputs are `u64` and satisfy `fee_ct + fee_it = f`.

## State Transition

For each valid transaction `tx` with amounts `(amount_ct, amount_it)` and fee components `(fee_ct, fee_it)` the state transition at commit time is:

```
Δsender_CT = -(amount_ct + fee_ct)
Δsender_IT = -(amount_it + fee_it)
Δminer_CT  =  fee_ct
Δminer_IT  =  fee_it
Δrecipient_CT = amount_ct
Δrecipient_IT = amount_it
```

All debits apply to the sender's unreserved balances and credit the recipient and miner atomically.

## INV-FEE-01 — Supply Neutrality

For every block `B` and token `T ∈ {CT, IT}`:

```
Σ balances_T(before B) - Σ balances_T(after B)
  = Σ fees_T(deducted_by_senders in B) - Σ fees_T(credited_to_miners in B) = 0.
```

When `ν = 2` and `f` is odd, the `ceil`/`floor` split above ensures `fee_ct + fee_it = f`, preserving the equality.

## Overflow Guard

The transition is only valid if `f < 2^63` and:

```
amount_T + fee_component_T ≤ MAX_SUPPLY_T
fee_component_T ≤ 2^63 − 1
```

See [`ECONOMICS.md`](ECONOMICS.md#inv-fee-02) for the algebraic proof.

## Genesis Hash

The `GENESIS_HASH` constant is asserted at compile time against the hash derived from the canonical block encoding. Any change to this value or to the genesis block layout constitutes a hard fork and must be recorded in `GENESIS_HISTORY.md`.

