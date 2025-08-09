# CHANGELOG

## Unreleased

- Breaking: Fee routing overhaul, overflow clamp, invariants **INV-FEE-01** and **INV-FEE-02**.
- Breaking: rename `fee_token` to `fee_selector` and bump crypto domain tag to `THE_BLOCKv2|`.
- Breaking: database schema **v4** adds per-account mempool caps and TTL
  indexes; `Blockchain::open` rebuilds the mempool on startup dropping
  expired or orphaned entries.
- Breaking: mempool entries persist admission timestamps (`timestamp_millis`
  and monotonic `timestamp_ticks`); schema v4 serializes pending transactions
  and enforces TTL on restart.
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
- Feat: rejection counters `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, and `drop_not_found_total` accompany
  labelled `tx_rejected_total{reason=*}` metrics.
- Feat: batched startup mempool rebuild reports `startup_ttl_drop_total` and
  benchmark `startup_rebuild` compares throughput.
- Feat: minimal `serve_metrics` HTTP exporter returns `gather_metrics()` output for Prometheus scrapes.
- Feat: rejection counter `tx_rejected_total{reason=*}` and spans
  `mempool_mutex`, `admission_lock`, `eviction_sweep`, `startup_rebuild`
  capture sender, nonce, fee-per-byte, and mempool size for traceability
  ([src/lib.rs](src/lib.rs#L1065-L1081),
  [src/lib.rs](src/lib.rs#L1535-L1541),
  [src/lib.rs](src/lib.rs#L1615-L1650)).
- Test: add panic-inject harness covering drop path lock poisoning and recovery.
- Test: add panic-inject harness for admission eviction proving full rollback
  and advancing `lock_poison_total` and rejection counters.
- Test: add admission panic hook verifying reservation rollback across steps.
- Test: expand 32-thread fuzz harness with randomized nonces and fees over
  10k iterations to stress capacity and uniqueness invariants.
- Test: add `flood_mempool_never_over_cap` regression verifying mempool cap
  under threaded submission floods.
- Test: add `admit_and_mine_never_over_cap` ensuring concurrent admission and
  mining never exceed the mempool cap.
- Test: regression tests decrement the orphan counter on explicit drops and
  TTL purges.
- Test: `rejection_reasons` asserts telemetry for invalid selector, balance
  overflow, and drop-not-found paths.
- Feat: `Blockchain::open` invokes `purge_expired`, logging `expired_drop_total`
  and advancing `ttl_drop_total` on restart.
- Doc: introduce `API_CHANGELOG.md` for Python error codes and telemetry endpoints.
- Test: add unit test verifying mempool comparator priority order and regression for TTL expiry telemetry.
- Test: `test_schema_upgrade_compatibility` migrates v1/v2/v3 disks to v4 with `timestamp_ticks` hydration and `ttl_expired_purged_on_restart` covers TTL purges across restarts.
- Doc: refresh `AGENTS.md`, `Agents-Sup.md`, `Agent-Next-Instructions.md`, and `AUDIT_NOTES.md` with authoritative next-step directives.

### CLI Flags

- `--mempool-max` / `TB_MEMPOOL_MAX`
- `--mempool-account-cap` / `TB_MEMPOOL_ACCOUNT_CAP`
- `--mempool-ttl` / `TB_MEMPOOL_TTL_SECS`
- `--min-fee-per-byte` / `TB_MIN_FEE_PER_BYTE`

