# Operations

This guide replaces the scattered `docs/operators/**`, `docs/monitoring*.md`, `docs/runbook.md`, and similar files. Use it when running production nodes, gateways, metrics stacks, or chaos drills.

## Just Want to Run a Node on Your Laptop?

Quick local setup for experimentation:

```bash
# 1. Bootstrap (installs Rust, Python venv, etc.)
./scripts/bootstrap.sh

# 2. Build
cargo build -p the_block --release
cargo build -p contract-cli --bin contract-cli

# 3. Start a single node with default config
./target/release/contract-cli node start --config config/node.toml

# 4. View basic logs and metrics
tail -f logs/node.log
curl http://localhost:26658/metrics | head -20
```

> **Homebrew updates on macOS:** `scripts/bootstrap.sh` exports `HOMEBREW_NO_AUTO_UPDATE=1` so the script leaves your taps unchanged. Manually run `brew update` before the bootstrap when you want fresher packages, or unset the flag (`HOMEBREW_NO_AUTO_UPDATE=0 ./scripts/bootstrap.sh`) to let the script refresh the taps for you. The bootstrap now downloads the macOS `cargo-make`/`cargo-nextest` artifacts, so it no longer aborts with `unsupported architecture: arm64-Darwin` on Apple Silicon.

**What's a "testnet" vs "mainnet"?**
- **Testnet**: A practice network with fake BLOCK. Safe to experiment, break things, learn.
- **Mainnet**: The real network with real BLOCK. Production readiness required.

For production deployment, read the rest of this guide.

## SimpleDb: How State is Stored

> **Plain English:** SimpleDb is the key-value store that keeps small databases on disk (like energy market state, governance state, or snapshot indexes). It uses a crash-safe write pattern:
>
> 1. Write data to a temporary file
> 2. `fsync` to ensure it's on disk
> 3. Atomically rename temp file to final location
>
> This "write-ahead + atomic rename" pattern means you never get a corrupted half-written file. If the node crashes mid-write, the old data is still intact.

Key locations:
- **Energy market**: `TB_ENERGY_MARKET_DIR` (default: `energy_market/`)
- **Governance**: sled-backed, location configured in node config
- **Peer overlay**: `node/src/net/overlay_store`

For backup/restore, copy these directories while the node is stopped (or use the snapshot commands).

## System Requirements
- Rust 1.86+, `cargo-nextest`, `cargo-fuzz` (nightly), Python 3.12.3 (virtualenv), Node 18+ for dashboards. `scripts/bootstrap.sh`/`.ps1` installs everything plus `patchelf` on Linux.
- Storage engines via SimpleDb: in-house (default), RocksDB (feature-gated), and memory (for lightweight builds). Sled is used for dedicated subsystems (for example, governance) via the `sled/` crate. Provision SSDs and enable `storage_engine::KeyValue::flush_wal` watchers to keep WAL sizes bounded.
- Network: QUIC-ready NICs with low jitter; TLS certificates derive directly from node keys so no external PKI is needed.

## Bootstrap and Configuration
1. Clone the repo, run `scripts/bootstrap.sh`, and copy `.env.example` → `.env` for node defaults.
2. `just lint`, `just fmt`, `just test-fast` before coding; `just test-full` mirrors CI.
3. Node configuration flows through `node/.env`, CLI flags, or env vars prefixed with `TB_*` (see `node/src/config.rs`). `config/` holds policy baselines.
4. Use `scripts/bootstrap_ps1` on Windows/WSL; the runtime works cross-platform and telemetry pipes remain identical.

## Building and Testing
- `cargo build -p the_block --release` builds the node; `cargo build -p contract-cli --bin contract-cli` compiles the CLI.
- `cargo nextest run --all-features` exercises the multi-crate workspace with the telemetry feature enabled.
- Python demo: `python demo.py` wires the PyO3 module from `node/src/py.rs` for deterministic replay tests.
- **Workstream checklist** — Ship every change with the standard gate run (`just lint`, `just fmt`, `just test-fast`, the required tier of `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, and `scripts/fuzz_coverage.sh`). Attach the log (or CI link) plus fuzz `.profraw` artifacts to the PR so reviewers can verify compliance with `AGENTS.md §0.6`.

## Running a Node
- `contract-cli node start --config config/node.toml` (or `just node`) spawns the daemon with gateway, RPC, gossip, and compute/storage workers enabled.
- Gateway hosting toggles: `--gateway-http`, `--range-boost`, `--mobile-cache`. DNS publishing requires the `.block` registry key configured via `TB_GATEWAY_ZONE` env vars.
- Mesh + overlay migration: run `scripts/migrate_overlay_store.rs` when upgrading peer stores; reference `docs/architecture.md#overlay-and-peer-persistence` for background.

## Telemetry Wiring
- Enable telemetry via `--telemetry` or `TB_ENABLE_TELEMETRY=1`. Metrics flow into the in-process registry (`node/src/telemetry.rs`) and are exposed on the `/metrics` HTTP endpoint.
- TLS warning sink: export `TB_HTTP_TLS`/`TB_AGGREGATOR_TLS` or rely on bundled roots; warnings stream to the aggregator and dashboards.
- Wrapper telemetry tracks runtime/transport/storage/coding metadata and enforces governance overrides; CLI: `contract-cli telemetry wrappers`.
- When adding a metric/CLI surface/RPC method: update `node/src/telemetry.rs`, run `npm ci --prefix monitoring && make monitor`, refresh `monitoring/*.json` dashboards, and document the `/wrappers` + Grafana exposure here so operators see the new signal the day it lands.
- **Ad market observability contract** — Ad readiness must be provable from Grafana. Whenever selectors, privacy budgets, presence proofs, or readiness reports change:
  - Export the segment counters `ad_segment_ready_total{domain_tier,presence_bucket,interest_tag}`, competitiveness stats `ad_auction_top_bid_usd_micros{selector}`, `ad_auction_win_rate{selector}`, shading gauges `ad_bid_shading_factor_bps{selector}`, conversion totals `ad_conversion_value_ct_total{selector}`, and privacy health `ad_privacy_budget_utilization_ratio{selector}`, `ad_privacy_denial_total{reason}` through `metrics-aggregator/src/lib.rs` and the `/wrappers` endpoint.
  - Regenerate the `monitoring/ad_market_dashboard.json` (and any derivative dashboards) via `npm ci --prefix monitoring && make monitor`, capture screenshots of the refreshed “Ad Market Readiness,” “Presence Freshness,” and “Selector Competitiveness” panels, and attach them to the PR or incident log so operators can diff before/after.
  - Record the `/wrappers` hash emitted by `contract-cli telemetry wrappers` for the ad-market namespace and include it in the PR description along with the metrics command transcripts.
  - Any omission requires written approval from `@ad-market` and `@telemetry-ops`; mirror that approval in `AGENTS.md §15.K` until the dashboards land.

## Metrics Aggregator Ops
- `metrics-aggregator` runs as its own binary; configure via env (`TB_AGGREGATOR_*`). It ingests node metrics, replicates them, archives snapshots (optional S3), and exposes admin endpoints for bridge remediation and TLS warning acknowledgements.
- Bridge remediation constants (`BRIDGE_REMEDIATION_*`) now reference `docs/operations.md#bridge-liquidity-remediation`; update dashboards accordingly.
- Set `TB_METRICS_ARCHIVE` to append raw JSON into a log for offline audit.

## Launch Governor Operations

The launch governor automates network readiness transitions by monitoring chain and DNS metrics. See `docs/architecture.md#launch-governor` for the full technical design.

### Enabling the Governor

```bash
# Required
export TB_GOVERNOR_ENABLED=1

# Optional overrides
export TB_GOVERNOR_DB=/path/to/governor_db      # Default: governor_db/ in node dir
export TB_GOVERNOR_WINDOW_SECS=120              # Default: 2 × epoch duration
export TB_GOVERNOR_SIGN=1                       # Enable decision signing
export TB_NODE_KEY_HEX=<32-byte-hex>            # Required if signing enabled
```

### Monitoring Gate Status

```bash
# Check current gate states via RPC
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"governor.status","params":[],"id":1}'

# View recent decisions
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"governor.decisions","params":[10],"id":1}'

# Retrieve signed snapshot for specific epoch
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"governor.snapshot","params":[42],"id":1}'
```

### Understanding Gate Transitions

| Gate | Transition | Meaning |
|------|------------|---------|
| operational | `Inactive` → `Active` | Core network stable, normal operations enabled |
| operational | `Active` → `Inactive` | Metrics degraded, entering safe mode |
| naming | `Inactive` → `Rehearsal` | DNS metrics healthy, test auctions enabled |
| naming | `Rehearsal` → `Trade` | Stake coverage met, live auctions enabled |
| naming | `Trade` → `Rehearsal` | Disputes/failures spiked, reverting to test mode |

### Backup and Recovery

Decision snapshots are stored at `{base_path}/governor/decisions/epoch-{N}.json` with optional `.sig` sidecar files. Include this directory in your backup rotation:

```bash
# Backup governor state
cp -r /path/to/node/governor_db /backup/governor_db_$(date +%Y%m%d)
cp -r /path/to/node/governor/decisions /backup/decisions_$(date +%Y%m%d)
```

### Alerts to Configure

- **gate_transition** — Alert when any gate changes state (for awareness)
- **governor_disabled** — Alert if `governor.status` returns `"enabled": false` unexpectedly
- **intent_backlog** — Alert if pending intents exceed reasonable count (suggests apply failures)

## Monitoring and Dashboards
- Grafana/Prometheus configs live under `monitoring/`. Install with `npm ci --prefix monitoring && make monitor` to render dashboards.
- Dashboards include compute-market fairness, gossip fanout, gateway pacing, telemetry anomalies, bridge liquidity, SLA enforcement, ANN diagnostics, and badge distributions. JSON is committed (e.g., `monitoring/compute_market_dashboard.json`).
- The dashboard README moved here; use `docs/apis_and_tooling.md#metrics-and-telemetry-apis` for endpoint paths.
- Compute-market panels now include SLA slashing + remediation widgets fed by `compute_sla_slash_total{lane}` and `match_loop_latency_seconds{lane}`. Pair them with `contract-cli compute lanes --format json` for direct remediation workflows per `AGENTS.md §15.B`.
- **Ad market operations** — Treat selector/presence changes like release events:
  1. **Snapshot readiness** — Run `contract-cli ad-market readiness --with-privacy --format json` before and after changes. Archive the `/wrappers` hash, JSON payload, and Grafana “Ad Market Readiness”/“Presence Freshness”/“Selector Competitiveness” panels in the PR or incident log.
  2. **Inventory sanity** — Use `contract-cli ad-market inventory --selector domain_tier=premium --selector presence_bucket=<bucket>` and `contract-cli ad-market list-campaigns --selector interest_tag=travel` to confirm new selectors appear in cohort prices. Record CLI output in the worklog.
  3. **Presence management** — Operators mint LocalNet/Range Boost receipts via their respective CLIs, then monitor `ad_market.list_presence_cohorts` for stale buckets. `contract-cli ad-market presence list --min-confidence 9500` enumerates buckets; `contract-cli ad-market presence reserve --campaign <id> --bucket <bucket-id> --slots N` proves the reservation flow still honors governance TTL/radius knobs.
  4. **Reservation cancellations** — When `node/src/bin/node.rs` cancels a presence reservation (crowd counts fail policy), you must capture the emitted `AD_READINESS_SKIPPED{presence}` counter and note remediation steps in the corresponding runbook entry.
  5. **Gate checklist** — Anytime `crates/ad_market`, `node/src/rpc/ad_market.rs`, `node/src/localnet`, `node/src/range_boost`, `node/src/gateway/dns.rs`, `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs`, `metrics-aggregator/**`, or `monitoring/**` changes, run the full suite (`just lint`, `just fmt`, `just test-fast`, `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, `scripts/fuzz_coverage.sh`) and attach transcripts + fuzz `.profraw` summaries. This mirrors the checklist in [`docs/overview.md#ad--targeting-readiness-checklist`](overview.md#ad--targeting-readiness-checklist) and `AGENTS.md §0.6`.

## Treasury Disbursement Operations
- **Runbook scope** — Treasury actions now flow exclusively through disbursement proposals (draft → voting → queued → timelocked → executed → finalized/rolled-back). Operators no longer queue ad-hoc sled entries; instead they drive everything through `contract-cli gov disburse …` plus the executor lease machinery in `governance::store`. Keep `examples/governance/disbursement_example.json` handy as the canonical template.
- **Snapshots & audits** — Every execution persists a `foundation_serialization` snapshot to `provenance.json` (Option A) alongside the sled record and ledger journal entry. When performing audits or preparing a rollback, export the snapshot (`contract-cli gov disburse show --id … --format json`) and archive it with the signed receipts; never manually edit sled trees.
- **Metrics/telemetry** — The node exposes `treasury_balance_ct`, `treasury_balance_it`, `treasury_disbursement_backlog`, and `governance_disbursements_total{status}`. The metrics aggregator fans them out via the new `/treasury/summary` and `/governance/disbursements` endpoints; dashboards reference the same expressions (`treasury_disbursement_count`, `treasury_disbursement_scheduled_oldest_age_seconds`, etc.). Guard rails: alert if backlog age exceeds 2 hours or if any proposal stalls longer than `ROLLBACK_WINDOW_EPOCHS`.
- **Explorer/CLI parity** — Explorer timelines (proposal metadata, quorum/votes, timelock height, execution tx hash, receipts, rollback annotations) must match `contract-cli gov disburse show`. CI enforces this by running `contract-cli gov disburse preview --json examples/governance/disbursement_example.json --check` plus ledger replay tests. When the executor misbehaves, run `contract-cli gov treasury executor-status` to inspect lease holders and intent queues before issuing a rollback.
- **Rollbacks** — Within the block-height-bounded window you can call `contract-cli gov disburse rollback --id … --reason …`. This records a compensating ledger event, marks the disbursement `RolledBack`, and pushes the rationale into `provenance.json`. Include screenshots/log excerpts when filing incident reports under `docs/operations.md#troubleshooting-playbook`.
- **Dashboards** — After edits to `metrics-aggregator/**` or `monitoring/**`, re-run `npm ci --prefix monitoring && make monitor`, refresh the treasury panels (count by status, BLOCK amount by status—exposed via `treasury_disbursement_amount_ct`), timelock age, and capture new screenshots for the ops wiki.

## Energy Market Operations
- **Scope** — Everything is first-party: `crates/energy-market` (providers/credits/receipts + metrics), `node/src/energy.rs` (sled store + treasury hooks), `node/src/rpc/energy.rs` (JSON-RPC), `cli/src/energy.rs` (operator commands), `crates/oracle-adapter` (ingest client), and `services/mock-energy-oracle` (World OS drill). No third-party RPC stacks or DBs.
- **State & backups** — The market uses `SimpleDb::open_named(names::ENERGY_MARKET, path)` and serializes the entire `EnergyMarket` struct (providers, credits, receipts) after every mutation. `path` defaults to `energy_market/` but can be overridden with `TB_ENERGY_MARKET_DIR`. Snapshot the directory with the same fsync+rename guarantees as other `SimpleDb` stores; keep it in your backup/DR rotation alongside consensus/governance sleds.
- **Bootstrap script** — `scripts/deploy-worldos-testnet.sh` builds the node with `--features worldos-testnet`, starts it with `--chain worldos-energy --validator`, launches the mock oracle (`cargo run --release` inside `services/mock-energy-oracle`), and (if `docker/telemetry-stack.yml` exists) spins up Grafana/Prometheus. This is the canonical energy drill; pair it with `docs/testnet/ENERGY_QUICKSTART.md` for CLI/RPC steps.
- **RPC/CLI usage** — Operators interact through `contract-cli energy register|market|settle|submit-reading`, which sends the same JSON the RPC expects. The endpoints (`energy.register_provider`, `energy.market_state`, `energy.submit_reading`, `energy.settle`) inherit mutual-TLS, `TB_RPC_AUTH_TOKEN`, and rate-limit policy from the RPC server. Always log snapshots via `contract-cli energy market --verbose > energy_snapshot.json` before/after maintenance.
- **Governance params** — `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps` live in the shared governance store (`governance/src/params.rs`). Proposals update them, runtime hooks call `node::energy::set_governance_params`, and the sled DB is re-snapshotted. Track activations/rollbacks with `contract-cli gov param history` or explorer timelines. Upcoming work (batch vs real-time settlement payloads, dependency validation, rollback audits) is tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks`.
- **Provider trust roots** — Define oracle keys in `config/default.toml` via the `energy.provider_keys` array (`{ provider_id, public_key_hex }` entries). Reloads hot-swap the verifier registry through `node::energy::configure_provider_keys`, so rotating/revoking keys never requires a restart. Audit this file under the same approvals as other security knobs.
- **Telemetry & dashboards** — Metrics include `energy_provider_total`, `energy_pending_credits_total`, `energy_receipt_total`, `energy_active_disputes_total`, `energy_provider_register_total`, `energy_meter_reading_total{provider}`, `energy_settlement_total{provider}`, `energy_treasury_fee_ct_total`, `energy_dispute_{open,resolve}_total`, plus the existing price/latency series (`energy_avg_price`, `energy_kwh_traded_total`, `energy_signature_failure_total{provider,reason}`, `energy_provider_fulfillment_ms`, `oracle_reading_latency_seconds`). Health checks emit logs when pending credits exceed safe thresholds or settlements stall. Update Grafana dashboards to show provider growth, pending credits, dispute volume, settlement throughput, and fee/slash totals; alert when latency > SLO, signature failures climb, slash spikes, dispute backlog grows, or settlement throughput drops.
- **Structured RPC flows** — Enforce `TB_RPC_AUTH_TOKEN`, rate limits, and the new structured errors for `energy.submit_reading` (signature/timestamp/meter validation). Keep CLI/explorer schema snapshots under version control and add round-trip tests so automation cannot diverge from the node schema described in `docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts`.
- **Snapshot drills + telemetry** — Practice quiescing the node, copying `TB_ENERGY_MARKET_DIR`, restoring on staging, and recording telemetry (`energy_snapshot_duration_seconds`, `energy_snapshot_fail_total`). Publish the drill outcome via `/wrappers` so remote operators can verify migration readiness.
- **Oracle hygiene** — Production adapters must enforce the in-tree Ed25519 verification (`crates/oracle-adapter`), source keys from secure env/keystores, redact secrets from logs, and honour RPC rate limits. The mock oracle service (`services/mock-energy-oracle`, HTTP endpoints `/meter/:id/reading` and `/meter/:id/submit`) is for dev/testnet only.
- **Dispute workflow** — Until dedicated `energy.dispute`/`energy.receipts.list` endpoints ship, disputes run through governance: capture the suspect `meter_hash` + provider ID via `energy.market_state`, submit a `gov param update` (tighten slashing rate or pause settlement), and document rollback steps. Keep explorers/CLI in sync so operators can see activation/rollback history.
- **Snapshot/restore drills** — Practice quiescing the node, copying `TB_ENERGY_MARKET_DIR`, and restoring it on staging nodes. Mirror the SimpleDb snapshot/restore drills described earlier so operators can rehearse schema migrations or recovery from corruption. Integration tests for backward-compatibility live under `node/tests/gov_param_wiring.rs`; extend them when modifying the schema.

## Chaos and Fault Drills
- Gossip chaos: `tests/net_gossip.rs` exercises packet loss/jitter; ensure convergence through tie-break rules and inspect `partition_watch` metrics.
- QUIC chaos: `node/tests/net_quic.rs` captures retransmit counters and handshake distributions; aggregator `/chaos` endpoints record incidents.
- Disk-full and repair: `node/tests/storage_repair.rs` simulates storage failures; use `contract-cli storage repair` and monitor `STORAGE_*` metrics.
- Range-boost drills: toggle `TB_PARTITION_TAG`, adjust mesh peers, and verify recovery with `contract-cli mesh status`.
- WAN-scale QUIC chaos (per `AGENTS.md §15.C`): run `scripts/chaos_quic.sh` or the equivalent automation to fault multiple transport providers, rotate mutual-TLS fingerprints, and validate failover telemetry (`quic_failover_total`, `range_boost_ttl_violation_total`, `transport_capability_mismatch_total`). Capture Grafana screenshots and log links in this doc every time you execute the drill.

## Probe CLI and Diagnostics
- `crates/probe` provides synthetic health checks: `probe ping-rpc`, `probe gossip-check`, `probe mine-one`, `probe tip`. Flags: `--timeout`, `--expect`, `--prom` for Prometheus output.
- Diagnostics harness: `contract-cli diagnostics range-boost`, `contract-cli diagnostics gossip`, `contract-cli diagnostics tls` expose cached stats for on-call triage.
- AI diagnostics toggles live in governance params; metrics and CLI output share the same flag.

## Deployment and Release
- Build provenance lives in `node/src/provenance.rs` and `docs/security_and_privacy.md#release-provenance`. Release gating requires deterministic hashes and signatures listed in `config/release_signers.txt` or env overrides.
- `cargo vendor` snapshots and `provenance.json`/`checksums.txt` block tagging unless the dependency-registry audit passes (`just dependency-audit`).
- Upgrades: follow `contract-cli gov release approve`, ensure metrics dashboards show `release_attestation_*`, and leverage the built-in rollback windows.

## Incident Response
- Runbook coverage: bridge liquidity remediation, DHT recovery, gateway flush, snapshot repair. Each subsection lives below for quick linking.
- **Bridge Liquidity Remediation** – aggregator dispatch endpoints `/remediation/bridge/*` plus dashboards (**Bridge Remediation** row) keep quorum on pending actions. Operators must acknowledge via CLI + aggregator ack endpoints.
- **DHT / Gossip Recovery** – purge peer DBs (`simple_db::names::OVERLAY`), reseed via bootstrap peers, run `provision_overlay_store` helper, monitor `partition_watch` metrics.
- **Gateway Flush** – use `contract-cli gateway mobile-cache flush` and `contract-cli read-acks export` before restarts.

## Storage, Snapshots, and WAL Management
- Snapshots: `contract-cli snapshots create --path <dir>` writes fsync’d temp files before atomic rename. Legacy dumps stay until the new snapshot lands.
- WAL hygiene: `SimpleDb::flush_wal` runs before snapshots; set `TB_SIMPLE_DB_LIMIT_BYTES` to guard disk usage.
- Repair: `contract-cli storage repair --manifest <file>` reissues pulls, rebuilds Lagrange-coded shards, and flags under-replicated providers.

## Backup and Restore Path Reference

The directories below map directly onto the SimpleDb column families listed in `docs/system_reference.md#appendix-c--simpledb-column-family-and-prefix-map`, so operators can tie on-disk artifacts back to the logical subsystems referenced throughout the system reference.

| Subsystem | Default path | Env/flag | Notes |
| --- | --- | --- | --- |
| Overlay peer store | `~/.the_block/overlay/overlay_peers.json` | `TB_OVERLAY_DB_PATH` | JSON list of peers + last-seen timestamps (`p2p_overlay`). |
| Gossip/QUIC peer caches | `~/.the_block/peer_db` / `peer_db_quic` | `TB_PEER_DB_PATH`, `TB_QUIC_PEER_DB_PATH`, `TB_PEER_KEY_HISTORY_PATH`, `TB_CHUNK_DB_PATH` | Keys, reputation history, and chunk dedup stores from `node/src/net/peer.rs`. |
| DNS auctions | `dns_db` | `TB_DNS_DB_PATH` | SimpleDb backing auctions, stakes, and ownership. |
| Gateway read receipts | `gateway_receipts` | `TB_GATEWAY_RECEIPTS` | Hourly CBOR batches + Merkle roots; archive before purging. |
| Mobile cache | `mobile_cache.db` | `TB_MOBILE_CACHE_DB`, `TB_MOBILE_CACHE_KEY_HEX` | ChaCha20-Poly1305 encrypted sled. |
| Storage pipeline | `blobstore/` | `TB_STORAGE_PIPELINE_DIR` | Holds manifests, rent-escrow records, and provider overrides. |
| Storage market contracts | `storage_market/` | `TB_STORAGE_MARKET_DIR` | Sled tree (`market/contracts`) plus importer checkpoints. |
| Compute scheduler | `~/.the_block/compute/{pending,cancel,reputation}` | `TB_PENDING_PATH`, `TB_CANCEL_PATH`, `TB_REPUTATION_DB_PATH` | Pending job queue, cancellation log, and reputation DB. |
| Bridge sled | `bridge_db/` | `TB_BRIDGE_DB_PATH`, `TB_BRIDGE_SLED_PATH` | Persisted headers, withdrawals, and duty logs. |
| Light-client proofs | `proof_tracker` | `TB_PROOF_TRACKER_PATH` (implied) | Path is displayed by `contract-cli light-client rebate-status`; back it up with explorer data. |
| LE portal | `./le_portal` | CLI `--base` | `le_requests.log`, `le_actions.log`, `le_evidence.log`, `warrant_canary.log`, plus `evidence/` blob files. |

Backups should snapshot these directories before upgrades. Restores require stopping the node, restoring the directory, and restarting with the same `TB_*` overrides to avoid partial migrations.

## Network Recovery and Chaos
- Chaos harness: `docs/architecture.md#telemetry-and-instrumentation` + `monitoring/grafana/...` capture WAN-scale drills. Use `tests/net_gossip.rs` fixtures with injected loss/latency before rolling changes.
- Partition drills: toggle `TB_PARTITION_TAG`, observe `partition_watch` alerts, ensure quorum recovers, document remediation in telemetry dashboards.
- QUIC chaos: `node/tests/net_quic.rs` and aggregator `/chaos` endpoints record retransmit counters and handshake distributions.

## Range Boost and LocalNet Operations
- Enable with `--range-boost` or `TB_RANGE_BOOST=1`. Peers are configured via `TB_MESH_STATIC_PEERS` (comma-separated `host:port` list). Diagnostics: `contract-cli mesh status`, `contract-cli diagnostics range-boost`, metrics `RANGE_BOOST_*`.
- Forwarder control: set `TB_RANGE_BOOST=0` (or remove `--range-boost`) then restart to pause deliveries while keeping the queue on disk. `node/src/range_boost/mod.rs::set_enabled(false)` drains the forwarder gracefully; re-enable to resume.
- Queue handling:
  1. Before reseeding peers, disable range boost and wait for `range_boost_queue_depth` to hit zero.
  2. Adjust `TB_MESH_STATIC_PEERS` / mesh discovery, then re-enable and confirm `range_boost_forwarder_fail_total` stays flat.
  3. For forced drains, delete the persisted queue directory under `~/.the_block/range_boost` (only after confirming backups) and restart.
- Chaos drills can simulate failures with `FaultMode::{ForceDisabled,ForceNoPeers,ForceEncode,ForceIo}` (see `node/src/range_boost/mod.rs`). Build-time toggles wire these modes into diagnostic RPCs; use them to validate monitoring before production changes.

## Simulation and Replay
- Use `sim/`, `examples/`, and `tests/` harnesses to rehearse dependency swaps, storage migrations, and governance policy changes. Replay harnesses guarantee byte-identical results across CPU architectures.
- Chaos + replay logs feed both the aggregator and `docs/developer_handbook.md#simulation-and-chaos` for developer workflows.

## Operator Checklist
- Keep `scripts/pre-commit.sample` installed to enforce fmt/lint.
- Regenerate dependency inventories whenever `Cargo.lock` changes.
- Run settlement audits and badge/SLA telemetry before and after upgrades.
- Document incidents in the aggregator’s `/audit` endpoints and link to the relevant sections above for forensics.
