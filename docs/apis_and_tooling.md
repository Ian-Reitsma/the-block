# APIs and Tooling

Reference for every public surface: RPC, CLI, gateway, DNS, explorer, telemetry, and schemas.

## JSON-RPC
- Server lives in `node/src/rpc`. Namespaces: `consensus`, `ledger`, `storage`, `compute_market`, `ad_market`, `governance`, `peer`, `treasury`, `vm`, `logs`, `state_stream`, `analytics`.
- Transport: first-party `httpd` router (HTTP/1.1, HTTP/2, WebSocket upgrades) plus mutual-TLS derived from node keys.
- Fault handling: clients clamp `TB_RPC_FAULT_RATE`, saturate exponential backoff after 31 attempts, and expose regression coverage for bounded retries.
- Streaming: `state_stream` (light-client snapshots), `compute.job_cancel`, and telemetry correlations.

## CLI (`tb-cli`)
- Main entry in `cli/src/main.rs`. Subcommands include: `node`, `gov`, `wallet`, `bridge`, `dex`, `compute`, `storage`, `gateway`, `mesh`, `light`, `telemetry`, `probe`, `diagnostics`, `service-badge`, `remediation`, `ai`, `ann`, `identity`.
- Use `tb-cli --help` or `tb-cli <cmd> --help`. Structured output via `--format json` for automation.
- CLI shares foundation crates (serialization, HTTP client, TLS) with the node, so responses stay type-aligned.

## Gateway HTTP and CDN Surfaces
- `node/src/gateway/http.rs` hosts HTTP + WebSocket endpoints for content, APIs, and read receipts. Everything goes through the first-party TLS stack (`crates/httpd::tls`).
- Operators tag responses with `ReadAck` headers so clients can submit proofs later.
- Range-boost forwarding and mobile cache endpoints hang off the same router; see `docs/architecture.md#gateway-and-client-access` for internals.

## HTTP client and TLS diagnostics
- Outbound clients live in `crates/httpd::{client.rs,blocking.rs}`. `httpd::Client` wraps the runtime `TcpStream`, supports HTTPS via the in-house TLS connector, and exposes:
  - `ClientConfig { connect_timeout, request_timeout, max_response_bytes, tls }`.
  - `Client::with_tls_from_env(&["TB_NODE_TLS","TB_HTTP_TLS"])` to reuse the same certs as RPC/gateway surfaces.
  - `RequestBuilder::json(value)` for canonical JSON encoding and `send()` for async execution. Blocking variants offer the same API for CLI tools.
- TLS rotation:
  - Set `TB_NET_CERT_STORE_PATH` to control where mutual-TLS certs are stored. `tb-cli net rotate-cert` and `tb-cli net rotate-key` (see `cli/src/net.rs`) wrap the RPCs that rotate QUIC certs and peer keys.
  - Diagnostics: `tb-cli net quic failures --url <rpc>` lists handshake failures; `tb-cli net overlay-status` confirms which overlay backend is active and where the peer DB lives.
- HTTP troubleshooting: both the node and CLI honour `TB_RPC_TIMEOUT_MS`, `TB_RPC_TIMEOUT_JITTER_MS`, and `TB_RPC_MAX_RETRIES`. Use `tb-cli net dns verify` to confirm TXT records and `tb-cli net gossip-status` to inspect HTTP routing metadata exposed via gossip debug RPCs.

## DNS and Naming
- Publishing: `node/src/gateway/dns.rs` writes `.block` zone files or external DNS records using schemas under `docs/spec/dns_record.schema.json`.
- CLI: `tb-cli gateway dns publish`, `tb-cli gateway dns audit`.

## Explorer and Log Indexer
- Explorer + indexer live under `explorer/` and share the governance crate with the node. They expose HTTP APIs for proposals, treasury events, storage receipts, compute payouts, and light-client timelines.
- SQLite access routes through `foundation_sqlite` wrappers to keep dependency policy intact.

## Metrics and Telemetry APIs
- Node `/metrics` endpoint exports Prometheus text via `runtime::telemetry::TextEncoder`.
- Metrics aggregator exposes `/metrics`, `/wrappers`, `/governance`, `/treasury`, `/bridge`, `/probe`, `/chaos`, `/audit`, `/remediation/*`, `/telemetry/summary`.
- Use `docs/operations.md#metrics-aggregator-ops` for deployment instructions and `monitoring/` for dashboards.

## Probe CLI
- Binary lives in `crates/probe`. Usage examples:
  - `probe ping-rpc --url http://127.0.0.1:3050 --timeout 5 --prom`
  - `probe gossip-check --addr 127.0.0.1:3030`
  - `probe mine-one --miner my-node`
- Emits Prometheus lines when `--prom` is set, so it doubles as a blackbox exporter.

## Storage and Blob APIs
- CLI: `tb-cli storage put|get|manifest|repair`, `tb-cli blob summarize`, `tb-cli storage providers`.
- RPC: `storage.put_blob`, `storage.get_manifest`, `storage.list_providers`.
- Blob manifests follow the binary schema in `node/src/storage/manifest_binary.rs`; object receipts encode `StoreReceipt` structs consumed by the ledger.

## Compute and Ad Market APIs
- Compute RPC/CLI: reserve capacity, post workloads, submit receipts, inspect fairness metrics, cancel jobs (`compute.job_cancel`). Courier snapshots stream through `compute_market.courier_status`.
- Ad market RPC/CLI: reserve impressions, commit deliveries, record conversions, audit ANN proofs, manage mesh queues.

## Light-Client Streaming
- RPC: `light.subscribe`, `light.get_block_range`, `light.get_device_status`, `state_stream.subscribe`. CLI: `tb-cli light sync`, `tb-cli light snapshot`, `tb-cli light device-status`.
- Mobile heuristics (battery, bandwidth, overrides) persist under `~/.the_block/light_client.toml`.

## Bridge, DEX, and Identity APIs
- Bridge RPC: `bridge.submit_proof`, `bridge.challenge`, `bridge.status`, `bridge.claim_reward`. CLI mirrors the same set.
- DEX RPC/CLI: order placement, swaps, trust-line routing, escrow proofs, HTLC settlement.
- Identity RPC: DID registration, revocation, handle lookup; CLI uses `tb-cli identity`.

## Wallet APIs
- CLI supports multisig, hardware signers, remote signers, and escrow-hash configuration: see `cli/src/wallet.rs` and `node/src/bin/wallet.rs`.
- Commands include wallet creation/import, address derivation, signing, broadcast, and governance voting (where applicable). Use `--format json` for automation.
- Remote signer workflows emit telemetry and enforce multisig signer-set policies documented in `docs/security_and_privacy.md#remote-signers-and-key-management`.

## Schemas and Reference Files
- JSON schemas under `docs/spec/` define fee market inputs (`fee_v2.schema.json`) and DNS records. Keep them in sync with code when adding fields.
- Dependency inventory snapshots live in `docs/dependency_inventory*.json`; regenerate after dependency changes.
- Assets (`docs/assets/`) include RSA samples, scheduler diagrams, and architecture SVGs referenced across the docs.
