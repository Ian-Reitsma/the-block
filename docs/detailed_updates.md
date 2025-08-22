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
- **RPC Rate Limit** – Stabilized rate-limit enforcement with a deterministic
  `tokio::time::pause`-driven test and typed errors for rate-limited and banned
  clients.
- **Compute Market** – Added an execution path for slice outputs, expanded unit tests,
  and published a sample Grafana dashboard for backlog monitoring.
- **Formal Harness** – Introduced `formal/compute_market.fst` and wired it into the
  `formal/Makefile` so CI can type-check compute-market invariants, auto-downloading
  a pinned F★ toolchain if missing.
- **Security & Abuse Controls** – Introduced SBOM generation and license
  gating via `deny.toml`, a `check_cla.sh` helper for contributor license
  enforcement, and minimal law-enforcement request and warrant-canary logs
  that hash sensitive metadata.
- **License Audit & Formal Checks** – Expanded `deny.toml` to allow Unicode,
  BSD-2-Clause, MPL-2.0, and LLVM-exception licenses and added F★ stubs
  so `make -C formal` type-checks both `Fee_v2` and `Compute_market`.
- **NonceGap Error & Purge Helpers** – Exposed `ErrNonceGap`,
  `decode_payload`, and purge-loop controls (`ShutdownFlag`, `PurgeLoopHandle`,
  `maybe_spawn_purge_loop`) honoring `TB_PURGE_LOOP_SECS`.
- **Mempool Atomicity** – Unified `mempool_mutex → sender_mutex` critical
  section; counter updates, heap ops, and pending balances execute inside the
  lock. Regression tests (`cap_race_respects_limit` and
  `flood_mempool_never_over_cap`) prove the size cap under threaded floods.
- **Dynamic Difficulty Retargeting** – `expected_difficulty` computes a moving
  average over recent block timestamps, bounding adjustments to ×4/¼ and
  validating block header `difficulty` fields.
- **In-block Nonce Continuity** – `validate_block` tracks per-sender nonces and
  rejects blocks with gaps or repeats.
- **Serialization Equivalence** – Rust generates canonical payload CSV vectors
  and `scripts/serialization_equiv.py` reencodes them in Python to assert byte
  equality.
- **Demo Purge Automation** – `demo.py` narrates fee selectors, nonce reuse, and
  manages a TTL purge loop via Python `ShutdownFlag`/`PurgeLoopHandle` helpers.
- **Python API Errors** – `fee_decompose` now raises distinct `ErrFeeOverflow` and `ErrInvalidSelector` exceptions for precise error handling.
- **Telemetry Metrics** – Prometheus counters track total submissions
  (`tx_submitted_total`), blocks mined (`block_mined_total`), TTL expirations
  (`ttl_drop_total`), startup drops (`startup_ttl_drop_total` (expired mempool
  entries dropped during startup)), lock poisoning events (`lock_poison_total`),
  orphan sweeps (`orphan_sweep_total`), invalid fee selectors
  (`invalid_selector_reject_total`), balance overflows
  (`balance_overflow_reject_total`), drop failures (`drop_not_found_total`), and
  total rejections labelled by reason (`tx_rejected_total{reason=*}`).
- **B‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` batches mempool
  rebuilds and invokes [`purge_expired`](../src.lib.rs#L1597-L1666) on startup
  ([../src/lib.rs](../src.lib.rs#L918-L935)), updating
  [`orphan_counter`](../src.lib.rs#L1638-L1663) and logging `expired_drop_total`
  while `ttl_drop_total` and `startup_ttl_drop_total` advance.
- **Startup Rebuild Benchmark** – Criterion bench `startup_rebuild` compares
  batched vs naive mempool hydration throughput.
- **Metrics HTTP Exporter** – `serve_metrics(addr)` spawns a lightweight server
  that returns `gather_metrics()` output. A sample `curl` scrape is shown below.
- **Async RPC Server** – JSON-RPC listener now uses `tokio` with async tasks for connection handling, eliminating per-connection threads and improving scalability.
- **Feature-Bit Handshake** – Peers negotiate protocol versions and feature bits; connections missing required bits (`0x0004`) are dropped.
- **Peer Rate Limits & Ban List** – Nodes track per-peer message rates, banning noisy peers and exporting `peer_error_total{code}` counters.
- **RPC Token-Bucket Limiter** – Clients consume from a token bucket refilled at `TB_RPC_TOKENS_PER_SEC`. Metrics `rpc_tokens_available{client}` and `rpc_bans_total` track current tokens and total bans while typed errors (`-32001` rate limit, `-32002` banned) surface over RPC.
- **RPC Nonce Guard** – Mutating RPC methods require a unique `nonce` parameter and reject replays.
- **Crash-safe WAL** – `SimpleDb` appends all writes to a BLAKE3‑checked write‑ahead log and replays it on restart before truncating.
- **Snapshot Rotation & Diffs** – The node emits full snapshots every `TB_SNAPSHOT_INTERVAL` blocks and incremental diffs in between; CI now restores from the latest snapshot + diffs via `scripts/snapshot_ci.sh`.
- **State Root Proofs** – Each block commits a state Merkle root and `account_proof` exposes inclusion proofs for light clients.
- **API Change Log** – `API_CHANGELOG.md` records Python error variants and
  telemetry counters.
- **Panic Tests** – Admission path includes panic-inject steps for rollback and
  eviction uses a separate harness (`eviction_panic_rolls_back`) to verify lock
  recovery and metric increments.
- **Schema Migration Tests** – `test_schema_upgrade_compatibility` exercises v1/v2/v3 disks upgrading to v4 with `timestamp_ticks` hydration; `ttl_expired_purged_on_restart` proves TTL expiry across restarts.
- **Tracing Spans** – `mempool_mutex`, `admission_lock`, `eviction_sweep`, and `startup_rebuild`
  ([../src/lib.rs](../src/lib.rs#L1067-L1082),
  [../src/lib.rs](../src/lib.rs#L1536-L1542),
  [../src/lib.rs](../src/lib.rs#L1622-L1657),
  [../src/lib.rs](../src.lib.rs#L879-L889))
  capture `sender`, `nonce`, `fee_per_byte`, and the current
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
- **Test Harness Isolation** – `Blockchain::new(path)` now provisions a unique
  temp directory per instance and removes it on drop. Fixtures call
  `tests::util::temp::temp_dir` so parallel tests cannot interfere and cleanup
  happens automatically.
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
- **Background Purge Loop** – `maybe_spawn_purge_loop` reads
  `TB_PURGE_LOOP_SECS` / `--mempool-purge-interval` and periodically calls
  `purge_expired`, advancing TTL and orphan-sweep metrics even when the mempool
  is idle.
- **Mempool Entry Cache** – Each mempool entry now caches its serialized size,
  allowing `purge_expired` to compute fee-per-byte without reserializing
  transactions.
- **Anchor Validation** – Added `scripts/check_anchors.py` and CI step to ensure
  Markdown links to `src/lib.rs` remain valid.
- **Schema v4 Note** – Migration serializes mempool contents with timestamps;
  `Blockchain::open` rebuilds the mempool on startup, encoding both
  `timestamp_millis` and `timestamp_ticks` per entry, skips missing-account
  entries, and invokes [`purge_expired`](../src.lib.rs#L1597-L1666) to drop
  TTL-expired transactions and update [`orphan_counter`](../src.lib.rs#L1638-L1663).
  Startup rebuild loads entries in batches of 256, logs the combined
  `expired_drop_total`, and `ttl_drop_total` and `startup_ttl_drop_total`
  advance for visibility
  ([../src/lib.rs](../src.lib.rs#L918-L935)).
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
| `ErrNonceGap`          | nonce gap                                 |
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

## Roadmap

### Immediate

- Finish atomic `(sender, nonce)` admission with rollback-safe reservations.
- Add property tests for pending-balance invariants and nonce continuity.
- Land initial compute-market primitives: stake-backed offers, per-slice
  verification, price bands, and carry-to-earn receipts.
- Expand compute-market module with job/offer matching, slice payouts,
  price board tracking, and receipt validation helpers.
- Add cancellation paths, backlog-based price adjustment, and a sliding
  price board to refine compute-market economics.
- Track heartbeat proofs for service-badge minting and revoke badges on
  sustained lapses to keep governance eligibility current.
- Scaffold a bicameral voting module with Operator and Builder houses,
  enforcing quorum and a configurable timelock before execution.
- Publish SRE runbooks covering mempool spikes, state corruption, and
  block-halt recovery procedures.
- Introduce a reproducible-build flow with a pinned toolchain and
  Dockerfile that emits deterministic hashes.

### Medium Term

- Abstract `SimpleDb` behind a trait to enable sled/RocksDB backends.
- Introduce P2P gossip and a CLI/RPC layer for node control and metrics.

### Long Term

- Research proof-of-service extensions, pluggable post-quantum signatures,
  and on-chain governance mechanics.

---

See [README.md#disclaimer](../README.md#disclaimer) for licensing and risk notices.
