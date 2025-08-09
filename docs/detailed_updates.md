# Detailed Update Log

This document captures the implementation notes from the recent token-amount and coinbase refactor.

## Overview

The chain now stores explicit coinbase values in each `Block`, wraps all amounts in the `TokenAmount` newtype, and migrates on-disk data to schema v3. Python bindings were updated to `pyo3` 0.24.2.

## Highlights

- **TokenAmount Wrapper** – Consensus-critical amounts are now wrapped in a transparent struct. Arithmetic is via helper methods only to ease a future move to `u128`.
- **Header Coinbase Fields** – `Block` records `coinbase_consumer` and `coinbase_industrial` for light-client validation. These are hashed in little-endian order.
- **Schema v3 Migration** – Opening the database upgrades older layouts and removes the legacy `accounts` and `emission` column families.
- **Legacy Fixture Support** – `Blockchain::open` now recomputes emission and block height when migrating v1/v2 databases and `ChainDisk` fields default via Serde for backward compatibility.
- **Hash Preimage Update** – The PoW preimage now includes the new coinbase fields. Genesis is regenerated accordingly.
- **Validation Order** – Reward checks occur only after proof-of-work is validated to avoid trivial DoS vectors.
- **Tests** – Added `test_coinbase_reward_recorded` and `test_import_reward_mismatch` plus updated schema gate assertions.
- **Python API** – Module definition uses `Bound<PyModule>` in accordance with `pyo3` 0.24.2.
- **TokenAmount Display** – Added `__repr__`, `__str__`, and `Display` trait implementations
  so amounts print as plain integers in both Python and Rust logs.
- **Mempool Atomicity** – Unified `mempool_mutex → sender_mutex` critical
  section; counter updates, heap ops, and pending balances execute inside the
  lock. Regression tests (`cap_race_respects_limit` and
  `flood_mempool_never_over_cap`) prove the size cap under threaded floods.
- **Python API Errors** – `fee_decompose` now raises distinct `ErrFeeOverflow` and `ErrInvalidSelector` exceptions for precise error handling.
- **Telemetry Metrics** – Prometheus counters now track TTL expirations
  (`ttl_drop_total`), startup drops (`startup_ttl_drop_total`), lock poisoning
  events (`lock_poison_total`), orphan sweeps (`orphan_sweep_total`), invalid
  fee selectors (`invalid_selector_reject_total`), balance overflows
  (`balance_overflow_reject_total`), drop failures (`drop_not_found_total`),
  and total rejections labelled by reason (`tx_rejected_total{reason=*}`).
- **Startup Rebuild Benchmark** – Criterion bench `startup_rebuild` compares
  batched vs naive mempool hydration throughput.
- **Metrics HTTP Exporter** – `serve_metrics(addr)` spawns a lightweight server
  that returns `gather_metrics()` output. A sample `curl` scrape is shown below.
- **API Change Log** – `API_CHANGELOG.md` records Python error variants and
  telemetry counters.
- **Panic Tests** – Admission path includes panic-inject steps for rollback and
  eviction uses a separate harness (`eviction_panic_rolls_back`) to verify lock
  recovery and metric increments.
- **Schema Migration Tests** – `test_schema_upgrade_compatibility` exercises v1/v2/v3 disks upgrading to v4 with `timestamp_ticks` hydration; `ttl_expired_purged_on_restart` proves TTL expiry across restarts.
- **Tracing Spans** – `mempool_mutex`, `eviction_sweep`, and `startup_rebuild`
  now capture `sender`, `nonce`, `fee_per_byte`, and the current
  `mempool_size` for fine-grained profiling.
- **Admission Panic Property Test** – `admission_panic_rolls_back_all_steps`
  injects panics before and after reservation and proves pending state and
  mempool remain clean.
- **Fuzz Harness Expansion** – `cross_thread_fuzz` now submits random nonces
  and fees over 10k iterations per thread, checking capacity and pending nonce
  uniqueness across 32 accounts.

Example scrape with Prometheus format:

```bash
curl -s localhost:9000/metrics \
  | grep -E 'startup_ttl_drop_total|ttl_drop_total|orphan_sweep_total|lock_poison_total|tx_rejected_total'
```
- **Documentation** – Project disclaimers moved to README and Agents-Sup now details schema migrations and invariant anchors.
- **Test Harness Isolation** – `Blockchain::new(path)` now provisions a unique temp
  directory per instance and removes it on drop. Fixtures call `unique_path` so
  parallel tests cannot interfere.
- **Replay Guard Test** – Reactivated `test_replay_attack_prevention` to prove duplicates
  with the same `(sender, nonce)` are rejected.
- **Mempool Hardening** – Admission now uses an atomic size counter and binary
  heap to evict the lowest-priority transaction ordered by
  `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)`. Entry timestamps persist as
  UNIX milliseconds.
- **Comparator Proof** – Unit test `comparator_orders_by_fee_expiry_hash`
  verifies the priority comparator (fee-per-byte, expiry, tx hash).
- **TTL Drop Metrics** – Test `ttl_expiry_purges_and_counts` asserts
  `ttl_drop_total` increments when expired transactions are purged.
- **Lock Poison Metrics** – Tests `lock_poisoned_error_and_recovery` and
  `drop_lock_poisoned_error_and_recovery` assert `lock_poison_total` and
  `tx_rejected_total` increment on poisoned lock paths.
- **Orphan Sweep Metrics** – `orphan_sweep_removes_missing_sender` confirms
  `orphan_sweep_total` rises when missing-account entries are swept.
- **Rejection Reason Metrics** – `rejection_reasons` regression suite asserts
  `invalid_selector_reject_total`, `balance_overflow_reject_total`, and
  `drop_not_found_total` alongside the labelled `tx_rejected_total` entries.
- **Schema v4 Note** – Migration serializes mempool contents with timestamps;
  `Blockchain::open` rebuilds the mempool on startup, encoding both
  `timestamp_millis` and `timestamp_ticks` per entry, skips missing-account
  entries, and invokes [`purge_expired`](../src/lib.rs#L1590-L1659) to drop
  TTL-expired transactions and update [`orphan_counter`](../src/lib.rs#L1631-L1656).
  Startup rebuild loads entries in batches of 256, logs the combined
  `expired_drop_total`, and `ttl_drop_total` and `startup_ttl_drop_total`
  advance for visibility
  ([../src/lib.rs](../src/lib.rs#L868-L900)).
- **Configurable Limits** – `max_mempool_size`, `min_fee_per_byte`, `tx_ttl`
  and per-account pending limits are configurable via `TB_*` environment
  variables. Expired transactions are purged on startup and new submissions.

### Mempool State Chart

```
[submitted]
    |
    v
[admitted] --> [mined]
    |             ^
    |             |
    +--> [evicted]
    |
    +--> [expired]
```

### Admission Error Codes

| Code                   | Meaning                                   |
|------------------------|-------------------------------------------|
| `ErrUnknownSender`     | sender not provisioned                    |
| `ErrInsufficientBalance` | insufficient funds                     |
| `ErrBadNonce`          | nonce mismatch                            |
| `ErrInvalidSelector`   | fee selector out of range                 |
| `ErrBadSignature`      | Ed25519 signature invalid                 |
| `ErrDuplicateTx`       | `(sender, nonce)` already present         |
| `ErrTxNotFound`        | transaction missing                       |
| `ErrBalanceOverflow`   | balance addition overflow                 |
| `ErrFeeOverflow`       | fee ≥ 2^63                                |
| `ErrFeeTooLow`         | below `min_fee_per_byte`                  |
| `ErrMempoolFull`       | capacity exceeded                         |
| `ErrPendingLimit`      | per-account pending limit hit             |
| `ErrLockPoisoned`      | mutex guard poisoned                      |

Drop path lock poisoning is covered by `drop_lock_poisoned_error_and_recovery`.

### CLI & Environment Flags

| Flag                    | Env Var                  | Effect                       |
|-------------------------|--------------------------|------------------------------|
| `--mempool-max`         | `TB_MEMPOOL_MAX`         | global mempool size cap      |
| `--mempool-account-cap` | `TB_MEMPOOL_ACCOUNT_CAP` | per-account pending limit    |
| `--mempool-ttl`         | `TB_MEMPOOL_TTL_SECS`    | entry time-to-live in seconds|
| `--min-fee-per-byte`    | `TB_MIN_FEE_PER_BYTE`    | minimum fee per byte         |

For the full rationale see `analysis.txt` and the commit history.

---

See [README.md#disclaimer](../README.md#disclaimer) for licensing and risk notices.
