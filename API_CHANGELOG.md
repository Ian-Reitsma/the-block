# API Change Log

## Unreleased

### Python
- `TxAdmissionError::LockPoisoned` is returned when a mempool mutex guard is poisoned.
- `TxAdmissionError::PendingLimit` indicates the per-account pending cap was reached.
- `TxAdmissionError::NonceGap` surfaces as `ErrNonceGap` when a nonce skips the expected sequence.
- `decode_payload(bytes)` decodes canonical payload bytes back into `RawTxPayload`.
- `ShutdownFlag` and `PurgeLoopHandle` manage purge threads when used with
  `maybe_spawn_purge_loop`.
- `maybe_spawn_purge_loop(bc, shutdown)` reads `TB_PURGE_LOOP_SECS` and returns
  a `PurgeLoopHandle` that joins the background TTL cleanup thread.
- `Blockchain::panic_in_admission_after(step)` panics mid-admission for test harnesses;
  `Blockchain::heal_admission()` clears the flag.
- `Blockchain::panic_next_evict()` triggers a panic during the next eviction and
  `Blockchain::heal_mempool()` clears the poisoned mutex.

### Telemetry
- `TTL_DROP_TOTAL` counts transactions purged due to TTL expiry.
- `STARTUP_TTL_DROP_TOTAL` reports expired mempool entries dropped during
  startup rebuild.
- `ORPHAN_SWEEP_TOTAL` tracks heap rebuilds triggered by orphan ratios.
- `LOCK_POISON_TOTAL` records mutex poisoning events.
- `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`, and
  `DROP_NOT_FOUND_TOTAL` expose detailed rejection counts.
- `TX_REJECTED_TOTAL{reason=*}` aggregates all rejection reasons.
- `serve_metrics(addr)` exposes Prometheus text over a lightweight HTTP listener.
- `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS` and spawns a background
  thread that periodically calls `purge_expired`, advancing
  `ttl_drop_total` and `orphan_sweep_total`.
- Spans `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and mempool size
  ([src/lib.rs](src/lib.rs#L1067-L1082),
  [src/lib.rs](src/lib.rs#L1536-L1542),
  [src/lib.rs](src/lib.rs#L1622-L1657),
  [src/lib.rs](src/lib.rs#L879-L889)).
- Documented `mempool_mutex → sender_mutex` lock order and added
  `admit_and_mine_never_over_cap` regression to prove the mempool size
  invariant.
- **B ‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` now invokes [`purge_expired`](src/lib.rs#L1597-L1666)
  ([src/lib.rs](src/lib.rs#L918-L935)), recording
  `ttl_drop_total`, `startup_ttl_drop_total`, and `expired_drop_total` on restart.
- Cached serialized transaction sizes in `MempoolEntry` so `purge_expired`
  avoids reserializing transactions (internal optimization).
