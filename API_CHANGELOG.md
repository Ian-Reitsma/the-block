# API Change Log

## Unreleased

### Python
- `TxAdmissionError::LockPoisoned` is returned when a mempool mutex guard is poisoned.
- `TxAdmissionError::PendingLimit` indicates the per-account pending cap was reached.
- `Blockchain::panic_in_admission_after(step)` panics mid-admission for test harnesses;
  `Blockchain::heal_admission()` clears the flag.
- `Blockchain::panic_next_evict()` triggers a panic during the next eviction and
  `Blockchain::heal_mempool()` clears the poisoned mutex.

### Telemetry
- `TTL_DROP_TOTAL` counts transactions purged due to TTL expiry.
- `STARTUP_TTL_DROP_TOTAL` reports how many transactions were dropped during
  startup rebuild.
- `ORPHAN_SWEEP_TOTAL` tracks heap rebuilds triggered by orphan ratios.
- `LOCK_POISON_TOTAL` records mutex poisoning events.
- `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`, and
  `DROP_NOT_FOUND_TOTAL` expose detailed rejection counts.
- `TX_REJECTED_TOTAL{reason=*}` aggregates all rejection reasons.
- `serve_metrics(addr)` exposes Prometheus text over a lightweight HTTP listener.
- Spans `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and mempool size
  ([src/lib.rs](src/lib.rs#L1065-L1081),
  [src/lib.rs](src/lib.rs#L1535-L1541),
  [src/lib.rs](src/lib.rs#L1615-L1650)).
- Documented `mempool_mutex → sender_mutex` lock order and added
  `admit_and_mine_never_over_cap` regression to prove the mempool size
  invariant. `Blockchain::open` now invokes `purge_expired`, recording
  `ttl_drop_total` and `expired_drop_total` on restart.
