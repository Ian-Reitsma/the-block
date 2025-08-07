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

## Mempool Semantics

`Blockchain::mempool` is backed by a lock-free `DashMap` keyed by `(sender, nonce)`.
A binary heap ordered by `(fee_per_byte DESC, timestamp_ticks ASC, tx_hash ASC)`
provides `O(log n)` eviction. An atomic counter enforces a maximum size of 1024
entries. Each transaction must pay at least the `min_fee_per_byte` (default `1`);
lower fees yield `FeeTooLow`. When full, the lowest-priority entry is evicted
and its reserved balances unwound atomically. `submit_transaction`,
`drop_transaction`, and `mine_block` may run concurrently without leaking
reservations. Each sender is limited to 16 pending transactions. Entries expire
after `tx_ttl` seconds (default 1800) based on the admission timestamp and are
purged on new submissions and at startup.

Transactions from unknown senders are rejected. Nodes must provision accounts via
`add_account` before submitting any transaction.

### Capacity & Flags

| Limit               | Default | CLI Flag                | Env Var                    |
|---------------------|---------|------------------------|----------------------------|
| Global entries      | 1024    | `--mempool-max`        | `TB_MEMPOOL_MAX`           |
| Per-account entries | 16      | `--mempool-account-cap`| `TB_MEMPOOL_ACCOUNT_CAP`   |
| TTL (seconds)       | 1800    | `--mempool-ttl`        | `TB_MEMPOOL_TTL_SECS`      |
| Fee floor (fpb)     | 1       | `--min-fee-per-byte`   | `TB_MIN_FEE_PER_BYTE`      |

