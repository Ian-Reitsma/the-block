# Operations

This guide replaces the scattered `docs/operators/**`, `docs/monitoring*.md`, `docs/runbook.md`, and similar files. Use it when running production nodes, gateways, metrics stacks, or chaos drills.

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
- `cargo build -p the_block --release` builds the node; `cargo build -p cli --bin tb-cli` compiles the CLI.
- `cargo nextest run --all-features` exercises the multi-crate workspace with the telemetry feature enabled.
- Python demo: `python demo.py` wires the PyO3 module from `node/src/py.rs` for deterministic replay tests.

## Running a Node
- `tb-cli node start --config config/node.toml` (or `just node`) spawns the daemon with gateway, RPC, gossip, and compute/storage workers enabled.
- Gateway hosting toggles: `--gateway-http`, `--range-boost`, `--mobile-cache`. DNS publishing requires the `.block` registry key configured via `TB_GATEWAY_ZONE` env vars.
- Mesh + overlay migration: run `scripts/migrate_overlay_store.rs` when upgrading peer stores; reference `docs/architecture.md#overlay-and-peer-persistence` for background.

## Telemetry Wiring
- Enable telemetry via `--telemetry` or `TB_ENABLE_TELEMETRY=1`. Metrics flow into the in-process registry (`node/src/telemetry.rs`) and are exposed on the `/metrics` HTTP endpoint.
- TLS warning sink: export `TB_HTTP_TLS`/`TB_AGGREGATOR_TLS` or rely on bundled roots; warnings stream to the aggregator and dashboards.
- Wrapper telemetry tracks runtime/transport/storage/coding metadata and enforces governance overrides; CLI: `tb-cli telemetry wrappers`.

## Metrics Aggregator Ops
- `metrics-aggregator` runs as its own binary; configure via env (`TB_AGGREGATOR_*`). It ingests node metrics, replicates them, archives snapshots (optional S3), and exposes admin endpoints for bridge remediation and TLS warning acknowledgements.
- Bridge remediation constants (`BRIDGE_REMEDIATION_*`) now reference `docs/operations.md#bridge-liquidity-remediation`; update dashboards accordingly.
- Set `TB_METRICS_ARCHIVE` to append raw JSON into a log for offline audit.

## Monitoring and Dashboards
- Grafana/Prometheus configs live under `monitoring/`. Install with `npm ci --prefix monitoring && make monitor` to render dashboards.
- Dashboards include compute-market fairness, gossip fanout, gateway pacing, telemetry anomalies, bridge liquidity, SLA enforcement, ANN diagnostics, and badge distributions. JSON is committed (e.g., `monitoring/compute_market_dashboard.json`).
- The dashboard README moved here; use `docs/apis_and_tooling.md#metrics-and-telemetry-apis` for endpoint paths.

## Energy Market Operations
- **Scope** — Everything is first-party: `crates/energy-market` (providers/credits/receipts + metrics), `node/src/energy.rs` (sled store + treasury hooks), `node/src/rpc/energy.rs` (JSON-RPC), `cli/src/energy.rs` (operator commands), `crates/oracle-adapter` (ingest client), and `services/mock-energy-oracle` (World OS drill). No third-party RPC stacks or DBs.
- **State & backups** — The market uses `SimpleDb::open_named(names::ENERGY_MARKET, path)` and serializes the entire `EnergyMarket` struct (providers, credits, receipts) after every mutation. `path` defaults to `energy_market/` but can be overridden with `TB_ENERGY_MARKET_DIR`. Snapshot the directory with the same fsync+rename guarantees as other `SimpleDb` stores; keep it in your backup/DR rotation alongside consensus/governance sleds.
- **Bootstrap script** — `scripts/deploy-worldos-testnet.sh` builds the node with `--features worldos-testnet`, starts it with `--chain worldos-energy --validator`, launches the mock oracle (`cargo run --release` inside `services/mock-energy-oracle`), and (if `docker/telemetry-stack.yml` exists) spins up Grafana/Prometheus. This is the canonical energy drill; pair it with `docs/testnet/ENERGY_QUICKSTART.md` for CLI/RPC steps.
- **RPC/CLI usage** — Operators interact through `tb-cli energy register|market|settle|submit-reading`, which sends the same JSON the RPC expects. The endpoints (`energy.register_provider`, `energy.market_state`, `energy.submit_reading`, `energy.settle`) inherit mutual-TLS, `TB_RPC_AUTH_TOKEN`, and rate-limit policy from the RPC server. Always log snapshots via `tb-cli energy market --verbose > energy_snapshot.json` before/after maintenance.
- **Governance params** — `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps` live in the shared governance store (`governance/src/params.rs`). Proposals update them, runtime hooks call `node::energy::set_governance_params`, and the sled DB is re-snapshotted. Track activations/rollbacks with `tb-cli gov param history` or explorer timelines. Upcoming work (batch vs real-time settlement payloads, dependency validation, rollback audits) is tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks`.
- **Telemetry & dashboards** — Metrics include `energy_providers_count`, `energy_avg_price`, `energy_kwh_traded_total`, `energy_settlements_total{provider}`, `energy_provider_fulfillment_ms`, and `oracle_reading_latency_seconds`. Health checks emit logs when pending credits exceed safe thresholds or settlements stall. Update Grafana dashboards to show provider counts, pending credits, slash totals, oracle latency, and settlement rate; alert when latency > SLO, slash spikes, or settlement throughput drops.
- **Oracle hygiene** — Production adapters must enforce real signature verification (forthcoming in `crates/oracle-adapter`), source keys from secure env/keystores, redact secrets from logs, and honour RPC rate limits. The mock oracle service (`services/mock-energy-oracle`, HTTP endpoints `/meter/:id/reading` and `/meter/:id/submit`) is for dev/testnet only.
- **Dispute workflow** — Until dedicated `energy.dispute`/`energy.receipts.list` endpoints ship, disputes run through governance: capture the suspect `meter_hash` + provider ID via `energy.market_state`, submit a `gov param update` (tighten slashing rate or pause settlement), and document rollback steps. Keep explorers/CLI in sync so operators can see activation/rollback history.
- **Snapshot/restore drills** — Practice quiescing the node, copying `TB_ENERGY_MARKET_DIR`, and restoring it on staging nodes. Mirror the SimpleDb snapshot/restore drills described earlier so operators can rehearse schema migrations or recovery from corruption. Integration tests for backward-compatibility live under `node/tests/gov_param_wiring.rs`; extend them when modifying the schema.

## Chaos and Fault Drills
- Gossip chaos: `tests/net_gossip.rs` exercises packet loss/jitter; ensure convergence through tie-break rules and inspect `partition_watch` metrics.
- QUIC chaos: `node/tests/net_quic.rs` captures retransmit counters and handshake distributions; aggregator `/chaos` endpoints record incidents.
- Disk-full and repair: `node/tests/storage_repair.rs` simulates storage failures; use `tb-cli storage repair` and monitor `STORAGE_*` metrics.
- Range-boost drills: toggle `TB_PARTITION_TAG`, adjust mesh peers, and verify recovery with `tb-cli mesh status`.

## Probe CLI and Diagnostics
- `crates/probe` provides synthetic health checks: `probe ping-rpc`, `probe gossip-check`, `probe mine-one`, `probe tip`. Flags: `--timeout`, `--expect`, `--prom` for Prometheus output.
- Diagnostics harness: `tb-cli diagnostics range-boost`, `tb-cli diagnostics gossip`, `tb-cli diagnostics tls` expose cached stats for on-call triage.
- AI diagnostics toggles live in governance params; metrics and CLI output share the same flag.

## Deployment and Release
- Build provenance lives in `node/src/provenance.rs` and `docs/security_and_privacy.md#release-provenance`. Release gating requires deterministic hashes and signatures listed in `config/release_signers.txt` or env overrides.
- `cargo vendor` snapshots and `provenance.json`/`checksums.txt` block tagging unless the dependency-registry audit passes (`just dependency-audit`).
- Upgrades: follow `tb-cli gov release approve`, ensure metrics dashboards show `release_attestation_*`, and leverage the built-in rollback windows.

## Incident Response
- Runbook coverage: bridge liquidity remediation, DHT recovery, gateway flush, snapshot repair. Each subsection lives below for quick linking.
- **Bridge Liquidity Remediation** – aggregator dispatch endpoints `/remediation/bridge/*` plus dashboards (**Bridge Remediation** row) keep quorum on pending actions. Operators must acknowledge via CLI + aggregator ack endpoints.
- **DHT / Gossip Recovery** – purge peer DBs (`simple_db::names::OVERLAY`), reseed via bootstrap peers, run `provision_overlay_store` helper, monitor `partition_watch` metrics.
- **Gateway Flush** – use `tb-cli gateway mobile-cache flush` and `tb-cli read-acks export` before restarts.

## Storage, Snapshots, and WAL Management
- Snapshots: `tb-cli snapshots create --path <dir>` writes fsync’d temp files before atomic rename. Legacy dumps stay until the new snapshot lands.
- WAL hygiene: `SimpleDb::flush_wal` runs before snapshots; set `TB_SIMPLE_DB_LIMIT_BYTES` to guard disk usage.
- Repair: `tb-cli storage repair --manifest <file>` reissues pulls, rebuilds Lagrange-coded shards, and flags under-replicated providers.

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
| Light-client proofs | `proof_tracker` | `TB_PROOF_TRACKER_PATH` (implied) | Path is displayed by `tb-cli light-client rebate-status`; back it up with explorer data. |
| LE portal | `./le_portal` | CLI `--base` | `le_requests.log`, `le_actions.log`, `le_evidence.log`, `warrant_canary.log`, plus `evidence/` blob files. |

Backups should snapshot these directories before upgrades. Restores require stopping the node, restoring the directory, and restarting with the same `TB_*` overrides to avoid partial migrations.

## Network Recovery and Chaos
- Chaos harness: `docs/architecture.md#telemetry-and-instrumentation` + `monitoring/grafana/...` capture WAN-scale drills. Use `tests/net_gossip.rs` fixtures with injected loss/latency before rolling changes.
- Partition drills: toggle `TB_PARTITION_TAG`, observe `partition_watch` alerts, ensure quorum recovers, document remediation in telemetry dashboards.
- QUIC chaos: `node/tests/net_quic.rs` and aggregator `/chaos` endpoints record retransmit counters and handshake distributions.

## Range Boost and LocalNet Operations
- Enable with `--range-boost` or `TB_RANGE_BOOST=1`. Peers are configured via `TB_MESH_STATIC_PEERS` (comma-separated `host:port` list). Diagnostics: `tb-cli mesh status`, `tb-cli diagnostics range-boost`, metrics `RANGE_BOOST_*`.
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
