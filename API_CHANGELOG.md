# API Change Log

## Unreleased

### Python
- `TxAdmissionError::LockPoisoned` is returned when a mempool mutex guard is poisoned.
- `TxAdmissionError::PendingLimit` indicates the per-account pending cap was reached.
- `Blockchain::panic_in_admission_after(step)` panics mid-admission for test harnesses;
  `Blockchain::heal_admission()` clears the flag.

### Telemetry
- `TTL_DROP_TOTAL` counts transactions purged due to TTL expiry.
- `ORPHAN_SWEEP_TOTAL` tracks heap rebuilds triggered by orphan ratios.
- `LOCK_POISON_TOTAL` records mutex poisoning events.
- `serve_metrics(addr)` exposes Prometheus text over a lightweight HTTP listener.
