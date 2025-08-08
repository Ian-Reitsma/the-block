# CHANGELOG

## Unreleased

- Breaking: Fee routing overhaul, overflow clamp, invariants **INV-FEE-01** and **INV-FEE-02**.
- Breaking: rename `fee_token` to `fee_selector` and bump crypto domain tag to `THE_BLOCKv2|`.
- Breaking: database schema **v4** adds per-account mempool caps and TTL
  indexes; `Blockchain::open` rebuilds the mempool on startup dropping
  expired or orphaned entries.
- Breaking: mempool entries persist `timestamp_millis`; schema v4 serializes
  pending transactions and enforces TTL on restart.
- Fix: isolate temporary chain directories for tests and enable replay attack
  prevention to reject duplicate `(sender, nonce)` pairs.
- Fix: enforce mempool capacity via atomic counter and `O(log n)` priority
  heap ordered by `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)`;
  timestamps stored as monotonic ticks.
- Fix: guard mining mempool mutations with global mutex to enforce
  capacity under concurrency.
- Feat: introduce minimum fee-per-byte floor with `FeeTooLow` rejection.
- Feat: expose mempool limits (`max_mempool_size`, `min_fee_per_byte`,
  `tx_ttl`, `max_pending_per_account`) via `TB_*` env vars and sweep expired
  entries on startup.
- Feat: add Prometheus metrics for TTL drops (`ttl_drop_total`) and
  lock poisoning (`lock_poison_total`).
- Feat: orphan sweeps rebuild heap when `orphan_counter > mempool_size / 2` and
  reset the counter; panic-inject test covers global mempool mutex.
- Test: add panic-inject harness covering drop path lock poisoning and recovery.
- Test: add panic-inject harness for admission eviction to ensure no deadlock.
- Feat: startup TTL purge logs `expired_drop_total`.
- Doc: introduce `API_CHANGELOG.md` for Python error codes and telemetry endpoints.

### CLI Flags

- `--mempool-max` / `TB_MEMPOOL_MAX`
- `--mempool-account-cap` / `TB_MEMPOOL_ACCOUNT_CAP`
- `--mempool-ttl` / `TB_MEMPOOL_TTL_SECS`
- `--min-fee-per-byte` / `TB_MIN_FEE_PER_BYTE`

