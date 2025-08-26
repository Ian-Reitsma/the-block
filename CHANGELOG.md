# CHANGELOG

## Unreleased

### Added
- Atomic file writer consolidates durable write‑rename‑sync persistence ([src/util/atomic_file.rs](src/util/atomic_file.rs)).
- Versioned blob framing encodes magic bytes, version tags, and CRC32 checksums for on‑disk schemas ([src/util/versioned_blob.rs](src/util/versioned_blob.rs)).
- Python: `mine_block(txs)` helper to mine a block from signed transactions for scripts and demos ([src/lib.rs](src/lib.rs)).
- Asynchronous JSON-RPC server built on `tokio` replaces the thread-per-connection model and dispatches requests with async tasks while preserving spec-compliant errors ([src/rpc.rs](src/rpc.rs), [src/bin/node.rs](src/bin/node.rs), [tests/node_rpc.rs](tests/node_rpc.rs)).
- Network partition/rejoin and invalid gossip cases ensure longest-chain convergence ([tests/net_gossip.rs](tests/net_gossip.rs)).
- Demo auto-builds the extension and defaults the purge loop to one second; CI captures logs and clears manual flags ([demo.py](demo.py), [tests/demo.rs](tests/demo.rs)).
- Telemetry logs TTL drops and orphan sweeps with stable `code` fields and sample JSON lines ([src/telemetry.rs](src/telemetry.rs), [tests/logging.rs](tests/logging.rs)).
- Stress tests spawn overlapping purge loops, log start/stop times, and assert metrics after each join ([tests/test_spawn_purge_loop.py](tests/test_spawn_purge_loop.py)).
- Test harness installs `maturin` on demand and builds the Python extension before running tests ([tests/conftest.py](tests/conftest.py)).
- Prototype service-badge tracker mints placeholder badges after high-uptime epochs ([src/service_badge.rs](src/service_badge.rs), [tests/service_badge.rs](tests/service_badge.rs)).
- Grafana dashboard now graphs snapshot duration/failures and service badge metrics (`badge_active`, `badge_last_change_seconds`) for monitoring.
- Network topology diagrams and an RPC walkthrough illustrate partition tests and end-to-end transaction flow ([docs/network_topologies.md](docs/network_topologies.md), [README.md](README.md), [AGENTS.md](AGENTS.md)).

### Changed
- Moving-average difficulty retargeting validates block headers against expected difficulty ([src/lib.rs](src/lib.rs)).
- README and agent handbooks document JSON-RPC sessions, networking demos, and purge-loop defaults ([README.md](README.md), [AGENTS.md](AGENTS.md), [Agents-Sup.md](Agents-Sup.md)).
- Bootstraps pin `cargo-nextest` v0.9.97-b.2 to match the Rust 1.82 toolchain ([bootstrap.sh](bootstrap.sh), [bootstrap.ps1](bootstrap.ps1), [scripts/bootstrap_test.sh](scripts/bootstrap_test.sh)).

### Fixed
- Telemetry exporter always emits keys such as `orphan_sweep_total` even before they increment ([src/telemetry.rs](src/telemetry.rs)).
- Python: `RawTxPayload` now exposes both `from_` and `from` properties, restoring examples that accessed either name after decode ([src/transaction.rs](src/transaction.rs)).

### Breaking
- Renamed `fee_token` to `fee_selector` and bumped the crypto domain tag to `THE_BLOCKv2|` ([src/lib.rs](src/lib.rs)).

- Fix: make `demo.py` build the `the_block` extension with `maturin` when
  missing and default `TB_PURGE_LOOP_SECS` to `1`, preventing module and
  purge-loop errors during quick starts.
- Feat: log `orphan_sweep_total` alongside `ttl_drop_total` in purge loop
  telemetry, extend logging tests for nonce-gap and balance rejections, and
  document sample JSON log output.
- Breaking: Fee routing overhaul, overflow clamp, invariants **INV-FEE-01** and **INV-FEE-02**.
- Breaking: rename `fee_token` to `fee_selector` and bump crypto domain tag to `THE_BLOCKv2|`.
- Breaking: database schema **v4** adds per-account mempool caps and TTL
  indexes; `Blockchain::open` rebuilds the mempool on startup dropping
  expired or orphaned entries.
- **B‑5 Startup TTL Purge — COMPLETED**: `Blockchain::open` batches mempool
  rebuilds, invokes [`purge_expired`](src/lib.rs#L1597-L1666) during startup
  ([src/lib.rs](src/lib.rs#L918-L935)), logs `expired_drop_total`, and
  increments `ttl_drop_total` and `startup_ttl_drop_total`.
- Breaking: mempool entries persist admission timestamps (`timestamp_millis`
  and monotonic `timestamp_ticks`); schema v4 serializes pending transactions
  and enforces TTL on restart.
- Fix: isolate temporary chain directories for tests and enable replay attack
  prevention to reject duplicate `(sender, nonce)` pairs.
- Fix: enforce mempool capacity via atomic counter and `O(log n)` priority
  heap ordered by `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)`;
- Change: `maybe_spawn_purge_loop` errors when `TB_PURGE_LOOP_SECS` is unset,
  non-numeric, or ≤0 and Python raises ``ValueError``.
- Fix: guard mining mempool mutations with global mutex to enforce
  capacity under concurrency.
- Fix: `PurgeLoopHandle.join` surfaces purge thread panics as `RuntimeError`,
  appending a Rust backtrace when `RUST_BACKTRACE=1`.
- Fix: dropping `PurgeLoopHandle` triggers its shutdown flag to halt the
  purge thread when `ShutdownFlag.trigger()` is omitted.
- Feat: expose `spawn_purge_loop(bc, interval_secs, shutdown)` to Python for
  manual TTL purge scheduling.
- Docs: document `TB_PURGE_LOOP_SECS` in `README` and `.env.example`.
- Docs: add `decode_payload` usage example in `README` and `demo.py`.
- Feat: assign numeric error codes (`ERR_*`) to transaction admission
  failures; Python exceptions expose `error.code` and JSON logs include the
  `code` field.
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
- Breaking: rename `BadNonce` to `NonceGap` and expose `decode_payload` to Python for
  canonical payload round-trips.
- Fix: schema v4 migration recomputes coinbase amounts and fee checksums to
  preserve total supply.
- Feat: dynamic difficulty retargeting adjusts PoW targets using a moving
  average over recent block timestamps with step clamped to ×4/¼; validators
  reject blocks whose header difficulty mismatches `expected_difficulty`.
- Feat: block validation enforces per-sender nonce continuity, rejecting gaps
  or repeats inside a mined block.
- Feat: Python purge-loop controls (`ShutdownFlag`, `PurgeLoopHandle`,
  `maybe_spawn_purge_loop`) allow TTL cleanup threads from Python and demo.
- Test: cross-language serialization determinism ensured via
  `serialization_equiv.rs` and `scripts/serialization_equiv.py`.
- Feat: batched startup mempool rebuild reports `startup_ttl_drop_total`
  (expired mempool entries dropped during startup) and
  benchmark `startup_rebuild` compares throughput.
- Feat: minimal `serve_metrics` HTTP exporter returns `gather_metrics()` output for Prometheus scrapes.
- Feat: optional purge loop `maybe_spawn_purge_loop` reads
  `TB_PURGE_LOOP_SECS` / `--mempool-purge-interval` and calls
  `purge_expired` on a fixed interval, advancing `ttl_drop_total` and
  `orphan_sweep_total`.
- Feat: added `node` binary with clap-based CLI and JSON-RPC endpoints for
  balances, transaction submission, mining control, and metrics; flags
  `--mempool-purge-interval` and `--metrics-addr` configure purge loop and
  Prometheus exporter.
- Test: `tests/node_rpc.rs` smoke-tests JSON-RPC metrics, balance queries, and
  mining control.
- Test: env-driven purge loop inserts a TTL-expired transaction and an orphan
  (removing the sender) and asserts `ttl_drop_total` and
  `orphan_sweep_total` each increase by one while `mempool_size` returns to
  zero.
- Fix: `Blockchain::open`, `mine_block`, and `import_chain` refresh the
  public `difficulty` field via `expected_difficulty`.
- Test: table-driven `test_tx_error_codes.py` covers every admission error,
  including lock-poison, asserting each `exc.code` matches `ERR_*`.
- Test: `tests/logging.rs` (with `--features telemetry-json`) captures
  admitted and duplicate transactions and verifies the telemetry `code`
  field matches `ERR_OK` and `ERR_DUPLICATE`.
- Perf: cache serialized transaction size in each mempool entry so
  `purge_expired` can compute fee-per-byte without reserializing.
- Dev: CI validates Markdown anchors via `scripts/check_anchors.py`.
- Feat: rejection counter `tx_rejected_total{reason=*}` and spans
  `mempool_mutex`, `admission_lock`, `eviction_sweep`, `startup_rebuild`
  capture sender, nonce, fee-per-byte, and mempool size for traceability
    ([src/lib.rs](src/lib.rs#L1067-L1082),
    [src/lib.rs](src/lib.rs#L1536-L1542),
    [src/lib.rs](src/lib.rs#L1622-L1657),
    [src/lib.rs](src/lib.rs#L879-L889)).
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
- Feat: minimal TCP gossip layer (`net`) broadcasts transactions and blocks and
  applies the longest-chain rule; `tests/net_gossip.rs` verifies convergence.
  - Dev: `scripts/run_all_tests.sh` warns and skips feature detection when `jq`
    is missing instead of aborting. [0df8a72]
  - Doc: README documents opting into the manual purge-loop demo via
    `TB_DEMO_MANUAL_PURGE` and notes concurrent purge-loop coverage.
  - Fix: stabilize `demo_runs_clean` by shortening purge-loop waits, clearing
    manual purge flags, and capturing logs. [c7c8a84]
  - Docs: expand networking and difficulty demos and record purge-loop env
    defaults across contributor guides. [a0da8b3]
  - Test: `tests/test_spawn_purge_loop.py` runs two manual purge loops with
    different intervals and cross-order joins, ensuring clean shutdown and
    idempotent handle joins; `tests/demo.rs` sets `TB_PURGE_LOOP_SECS=1`,
  clears manual purge via `TB_DEMO_MANUAL_PURGE=""`, forces
  `PYTHONUNBUFFERED=1`, enforces a 10-second timeout, and prints logs on
  failure so the demo exits reliably in CI.
  - Fix: RPC server returns JSON-RPC compliant errors for malformed JSON and
    unknown methods.
  - Test: `rpc_concurrent_controls` exercises concurrent `start_mining`,
    `stop_mining`, and `submit_tx` requests to ensure thread safety.
  - Fix: RPC server parses `Content-Length`, applies read timeouts, accepts
    connections concurrently, and handles fragmented HTTP bodies without
    hanging.
  - Docs: expand JSON-RPC section with a full request/response session and a
    minimal Python client example.

### CLI Flags

- `--mempool-max` / `TB_MEMPOOL_MAX`
- `--mempool-account-cap` / `TB_MEMPOOL_ACCOUNT_CAP`
- `--mempool-ttl` / `TB_MEMPOOL_TTL_SECS`
- `--min-fee-per-byte` / `TB_MIN_FEE_PER_BYTE`
