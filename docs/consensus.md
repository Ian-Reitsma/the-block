# CONSENSUS — Fee Routing State Transitions

This document codifies the fee routing logic that is baked into consensus after **FORK-FEE-01**.  The algebra below is the single source of truth for how a transaction's `fee` field is decomposed and applied.

## PoS Finality and Propagation

A stake-weighted validator set finalizes proof-of-work blocks. Validators bond
tokens via `consensus.pos.*` RPCs, and their stake determines membership in the
UNL for BFT-style votes.

Block production is ordered by a Proof-of-History tick generator, and blocks
propagate through a Turbine-style tree gossip, reducing orphan rates and
latency relative to flood gossip.

### Leader Scheduling

Validators take turns producing blocks according to a stake-weighted rotation.
The `LeaderSchedule` in `consensus/leader.rs` expands each validator's stake
into proportional slots, yielding a deterministic leader for every round.

### Fork Choice Tie-Break Rules

When competing tips share the same height, fork choice applies a deterministic
ordering to ensure all nodes converge. The precedence is:

1. Higher block height wins.
2. If heights match, the chain with greater cumulative weight prevails.
3. If both height and weight are equal, the lexicographically greater tip hash
   is selected as a final tie-break.

This mirrors the `choose_tip` function in
[node/src/consensus/fork_choice.rs](../node/src/consensus/fork_choice.rs).

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

See [`ECONOMICS.md`](economics.md#inv-fee-02) for the algebraic proof.

## Genesis Hash

The `GENESIS_HASH` constant is asserted at compile time against the hash derived from the canonical block encoding. Any change to this value or to the genesis block layout constitutes a hard fork and must be recorded in `GENESIS_HISTORY.md`.

## Difficulty Retargeting

Each block header carries a `difficulty` field representing the proof‑of‑work
target. After every block the next difficulty is computed from a sliding window
of the most recent 120 blocks:

- **Target spacing:** 1 000 ms per block
- **Adjustment factor:** `(expected_spacing / actual_spacing)` over the window
- **Clamp:** the factor is restricted to the range [¼, ×4] relative to the
  previous difficulty

The 120-block window (~2 minutes) dampens timestamp jitter yet reacts to real
hash‑rate swings. Clamping the factor to one-quarter and four-times of the
previous difficulty thwarts miners from skewing timestamps to force extreme
jumps. `Blockchain::mine_block` encodes the computed difficulty in the new
block, and `validate_block`/`is_valid_chain` recompute
`expected_difficulty` to reject blocks that advertise an easier target.

## Mempool Semantics

`Blockchain` maintains lane-specific mempools, `mempool_consumer` and
`mempool_industrial`, each backed by a `DashMap<(sender, nonce), MempoolEntry>`.
Mutations across both lanes are guarded by a global `mempool_mutex`. A tracing
span captures each admission at this lock boundary
([node/src/lib.rs](../node/src/lib.rs#L1539-L1553)).
Each lane owns a binary heap ordered by `(fee_per_byte DESC, expires_at ASC,
tx_hash ASC)` for `O(log n)` eviction. Example ordering:

| fee_per_byte | expires_at | tx_hash | rank |
|-------------:|-----------:|--------:|-----:|
|        2000  |          9 | 0x01…   | 1    |
|        1000  |          8 | 0x02…   | 2    |
|        1000  |          9 | 0x01…   | 3    |

An atomic counter per lane enforces a maximum size of 1024 entries. Each
transaction must pay at least the `min_fee_per_byte` (default `1`); lower fees
yield `FeeTooLow`. When a lane is full, the lowest-priority entry is evicted and
its reserved balances unwound atomically. All mutations acquire
`mempool_mutex` before the per-sender lock to preserve atomicity. Counter
updates, heap pushes/pops, and pending balance/nonces occur within this order,
guaranteeing `mempool_size{lane} ≤ max_mempool_size`. Each sender is limited to
16 pending transactions per lane. Entries expire after `tx_ttl` seconds (default
1800) based on the persisted admission timestamp and are purged on new
submissions and at startup via `purge_expired()`, logging `expired_drop_total`
and advancing `ttl_drop_total`. In schema v4 each record serializes
`[sender, nonce, tx, timestamp_millis, timestamp_ticks]` where `timestamp_ticks`
is a monotonic counter used for deterministic tie breaking. `Blockchain::open`
rebuilds each heap from this list, skips entries whose sender account is
missing, invokes `purge_expired` to drop any whose TTL has elapsed, and restores
`mempool_size{lane}` from the survivors
([node/src/lib.rs](../node/src/lib.rs#L855-L916)). Transactions whose sender
account has been removed are counted in an `orphan_counter`. TTL purges and
explicit drops decrement this counter. When `orphan_counter > mempool_size / 2`
(orphans exceed half of the pool) a sweep rebuilds both heaps, drops all
orphans, emits `ORPHAN_SWEEP_TOTAL`, and resets the counter
([node/src/lib.rs](../node/src/lib.rs#L1638-L1663)).
Nodes may optionally run a background purge loop to enforce TTL even when
no new transactions arrive. Calling `maybe_spawn_purge_loop` after opening the
chain reads `TB_PURGE_LOOP_SECS` (or the `--mempool-purge-interval` CLI flag)
and, if the value is positive, spawns a thread that invokes `purge_expired`
on that interval, advancing `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` as
entries age out.

### Startup Rebuild & TTL Purge

On restart `Blockchain::open` rehydrates each lane's mempool from disk,
incrementing `mempool_size{lane}` for every inserted record and counting
missing-account entries. After hydration it calls
[`purge_expired`](../node/src/lib.rs#L1597-L1666) to drop TTL-expired entries,
update [`orphan_counter`](../node/src/lib.rs#L1638-L1663), and return the number
removed. The sum of these drops is reported as `expired_drop_total`;
`TTL_DROP_TOTAL` and `STARTUP_TTL_DROP_TOTAL` advance for visibility as entries
load in 256-entry batches
([node/src/lib.rs](../node/src/lib.rs#L918-L935)).

Transactions from unknown senders are rejected. Nodes must provision accounts via
`add_account` before submitting any transaction.

Telemetry counters exported: `mempool_size{lane}`, `evictions_total`,
`fee_floor_reject_total`, `dup_tx_reject_total`, `ttl_drop_total`,
`startup_ttl_drop_total` (expired entries dropped during startup),
`industrial_rejected_total{reason}`, `admission_mode{mode}`, `lock_poison_total`,
`orphan_sweep_total`, `invalid_selector_reject_total`,
`balance_overflow_reject_total`, `drop_not_found_total`,
`tx_rejected_total{reason=*}`. `serve_metrics(addr)` exposes these metrics over
HTTP; e.g. `curl -s localhost:9000/metrics | grep invalid_selector_reject_total`.
See `API_CHANGELOG.md` for Python error and telemetry endpoint history.

### Transaction Admission Error Codes

| Code | Constant                  | Reason                  |
|----:|---------------------------|-------------------------|
| 0   | `ERR_OK`                  | accepted                |
| 1   | `ERR_UNKNOWN_SENDER`      | sender account missing  |
| 2   | `ERR_INSUFFICIENT_BALANCE`| balance below required  |
| 3   | `ERR_NONCE_GAP`           | nonce does not follow   |
| 4   | `ERR_INVALID_SELECTOR`    | fee selector unsupported|
| 5   | `ERR_BAD_SIGNATURE`       | signature invalid       |
| 6   | `ERR_DUPLICATE`           | tx already pending      |
| 7   | `ERR_NOT_FOUND`           | tx absent on drop       |
| 8   | `ERR_BALANCE_OVERFLOW`    | balance arithmetic overflow |
| 9   | `ERR_FEE_OVERFLOW`        | fee arithmetic overflow |
| 10  | `ERR_FEE_TOO_LOW`         | below fee-per-byte floor|
| 11  | `ERR_MEMPOOL_FULL`        | global mempool capacity reached |
| 12  | `ERR_LOCK_POISONED`       | mutex poisoned          |
| 13  | `ERR_PENDING_LIMIT`       | per-account cap reached |
| 14  | `ERR_FEE_TOO_LARGE`       | fee exceeds 2^63-1      |

### Capacity & Flags

| Limit               | Default | CLI Flag                | Env Var                    |
|---------------------|---------|------------------------|----------------------------|
| Global entries      | 1024    | `--mempool-max`        | `TB_MEMPOOL_MAX`           |
| Per-account entries | 16      | `--mempool-account-cap`| `TB_MEMPOOL_ACCOUNT_CAP`   |
| TTL (seconds)       | 1800    | `--mempool-ttl`        | `TB_MEMPOOL_TTL_SECS`      |
| Purge interval (s)  | 0       | `--mempool-purge-interval` | `TB_PURGE_LOOP_SECS` |
| Fee floor (fpb)     | 1       | `--min-fee-per-byte`   | `TB_MIN_FEE_PER_BYTE`      |

