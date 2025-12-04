# APIs and Tooling

Reference for every public surface: RPC, CLI, gateway, DNS, explorer, telemetry, and schemas.

## JSON-RPC
- Server lives in `node/src/rpc`. Namespaces: `consensus`, `ledger`, `storage`, `compute_market`, `ad_market`, `governance`, `peer`, `treasury`, `vm`, `logs`, `state_stream`, `analytics`.
- Transport: first-party `httpd` router (HTTP/1.1, HTTP/2, WebSocket upgrades) plus mutual-TLS derived from node keys.
- Fault handling: clients clamp `TB_RPC_FAULT_RATE`, saturate exponential backoff after 31 attempts, and expose regression coverage for bounded retries.
- Streaming: `state_stream` (light-client snapshots), `compute.job_cancel`, and telemetry correlations.

## CLI (`tb-cli`)
- Main entry in `cli/src/main.rs`. Subcommands include: `node`, `gov`, `wallet`, `bridge`, `dex`, `compute`, `storage`, `gateway`, `mesh`, `light`, `telemetry`, `probe`, `diagnostics`, `service-badge`, `remediation`, `ai`, `ann`, `identity`.
- `tb-cli energy` wraps the `energy.*` RPCs (`register`, `market`, `settle`, `submit-reading`). It prints friendly tables by default and supports `--verbose`/`--format json` when you need machine-readable payloads or to export providers/receipts for explorer ingestion. See `docs/testnet/ENERGY_QUICKSTART.md` for scripted walkthroughs and dispute drills.
- Use `tb-cli --help` or `tb-cli <cmd> --help`. Structured output via `--format json` for automation.
- CLI shares foundation crates (serialization, HTTP client, TLS) with the node, so responses stay type-aligned.

### Energy RPC payloads, auth, and error contracts
- Endpoints live under `energy.*` and inherit the RPC server’s mutual-TLS/auth policy (`TB_RPC_AUTH_TOKEN`, allowlists) plus IP-based rate limiting defined in `docs/operations.md#gateway-policy`. Use `tb-cli diagnostics rpc-policy` to inspect the live policy before enabling public oracle submitters.
- Endpoint map:

| Method | Description | Request Body |
| --- | --- | --- |
| `energy.register_provider` | Register capacity/jurisdiction/meter binding plus stake. | `{ "capacity_kwh": u64, "price_per_kwh": u64, "meter_address": "string", "jurisdiction": "US_CA", "stake": u64, "owner": "account-id" }` |
| `energy.market_state` | Fetch snapshot of providers, outstanding meter credits, and receipts; pass `{"provider_id":"energy-0x01"}` to filter. | optional object |
| `energy.submit_reading` | Submit signed meter total to mint a credit. | `MeterReadingPayload` JSON (below) |
| `energy.settle` | Burn credit + capacity to settle kWh and produce `EnergyReceipt`. | `{ "provider_id": "energy-0x01", "buyer": "acct"?, "kwh_consumed": u64, "meter_hash": "0x..." }` |

- `energy.market_state` response structure:

```json
{
  "status": "ok",
  "providers": [ { "provider_id": "energy-0x00", "capacity_kwh": 10_000, "...": "..." } ],
  "credits": [ { "provider": "energy-0x00", "meter_hash": "e3c3…", "amount_kwh": 120, "timestamp": 123456 } ],
  "receipts": [ { "buyer": "acct", "seller": "energy-0x00", "kwh_delivered": 50, "price_paid": 2500, "treasury_fee": 125, "slash_applied": 0, "meter_hash": "e3c3…" } ]
}
```

- `MeterReadingPayload` schema (shared by `oracle-adapter`, RPC, CLI, and explorer tooling):

```jsonc
{
  "provider_id": "energy-0x00",
  "meter_address": "mock_meter_1",
  "kwh_reading": 12000,
  "timestamp": 1710000000,
  "signature": "hex-encoded ed25519/schnorr blob"
}
```

- Error strings bubble up from `energy_market::EnergyMarketError` and always return `{ "error": "<string>" }` so clients can check `.error`. Expect the following failure families:
  - `ProviderExists`, `MeterAddressInUse`, `UnknownProvider` when IDs collide.
  - `InsufficientStake`, `InsufficientCapacity`, `InsufficientCredit` for quota/stake mismatches.
  - `StaleReading`, `InvalidMeterValue`, `CreditExpired` when timestamps regress or expiry exceeded.
  - Signature/format errors: RPC rejects payloads where `meter_hash` is not 32 bytes, numbers are missing, or signatures fail decoding (and, once the oracle verifier lands, cryptographic verification failures).
- Negative tests live next to the RPC module; mimic them for client libraries so bad signatures, stale timestamps, and meter mismatches produce structured failures instead of panics.
- Observer tooling: `tb-cli energy market --verbose` dumps the whole response; `tb-cli diagnostics rpc-log --method energy.submit_reading` tails submissions with auth metadata so you can trace rate-limit hits.
- Per the architecture roadmap, the next RPC/CLI work items are: adding authenticated disputes + receipt listings, wiring explorer/CLI visualizations for param history, and enforcing per-endpoint rate limiting once the QUIC chaos drills complete (tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks`).

### Treasury disbursement CLI, RPC, and schema
- Disbursement proposals live inside the governance namespace. CLI entrypoints sit under `tb-cli gov disburse`:
  - `create` scaffolds a JSON template (see `examples/governance/disbursement_example.json`) and fills in proposer defaults (badge identity, default timelock/rollback windows).
  - `preview --json <file>` validates the payload against the schema and prints the derived timeline: quorum requirements, vote window, activation epoch, timelock height, and resulting treasury deltas.
  - `submit --json <file>` posts the signed proposal to the node via `gov.treasury.submit_disbursement`; dry-run with `--check` to ensure hashes match before sending live traffic.
  - `show --id <proposal-id>` renders the explorer-style timeline (metadata, quorum/vote tallies, timelock window, execution tx hash, receipts, rollback annotations).
  - `queue`, `execute`, and `rollback` mirror the on-chain transitions for operators who hold the treasury executor lease. `queue` acknowledges that the proposal passed and seeds the executor queue, `execute` pushes the signed transaction (recording `tx_hash`, nonce, receipts), and `rollback` reverts executions within the bounded window.
- `DisbursementPayload` JSON schema (shared by CLI, explorer, RPC, and tests):

```jsonc
{
  "proposal": {
    "title": "2024-Q2 Core Grants",
    "summary": "Fund three core contributors + auditor retainer.",
    "deps": [1203, 1207],
    "attachments": [
      { "name": "proposal-pdf", "uri": "ipfs://bafy..." }
    ],
    "quorum": { "operators": 0.67, "builders": 0.67 },
    "vote_window_epochs": 6,
    "timelock_epochs": 2,
    "rollback_window_epochs": 1
  },
  "disbursement": {
    "destination": "ct1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqe4tqx9",
    "amount_ct": 125_000_000,
    "amount_it": 0,
    "memo": "Core grants Q2",
    "scheduled_epoch": 180_500,
    "expected_receipts": [
      { "account": "foundation", "amount_ct": 100_000_000 },
      { "account": "audit-retainer", "amount_ct": 25_000_000 }
    ]
  }
}
```

- RPC exposure:
  - `gov.treasury.submit_disbursement { payload, signature }` – create proposal from JSON.
  - `gov.treasury.disbursement { id }` – fetch canonical status/timeline for a single record.
  - `gov.treasury.queue_disbursement { id }`, `gov.treasury.execute_disbursement { id, tx_hash, receipts }`, `gov.treasury.rollback_disbursement { id, reason }` – maintenance hooks for executor operators (all auth gated).
  - `gov.treasury.list_disbursements { cursor?, status?, limit? }` – explorer/CLI listings.
- CLI exposes `--schema` and `--check` flags to dump the JSON schema and to validate payloads offline. CI keeps the examples under `examples/governance/` in sync by running `tb-cli gov disburse preview --json … --check` during docs tests.
- Explorer’s REST API mirrors the RPC fields so UI timelines and CLI scripts stay aligned; see `explorer/src/treasury.rs`.

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

## Compute, Energy, and Ad Market APIs
- Compute RPC/CLI: reserve capacity, post workloads, submit receipts, inspect fairness metrics, cancel jobs (`compute.job_cancel`). Courier snapshots stream through `compute_market.courier_status`, proof bundles (with fingerprints + circuit artifacts) are downloadable via `compute_market.sla_history(limit)`, `tb-cli compute proofs --limit N` pretty-prints recent SLA/proof entries, `tb-cli explorer sync-proofs --db explorer.db` ingests them into the explorer SQLite tables (`compute_sla_history` + `compute_sla_proofs`), and the explorer HTTP server exposes `/compute/sla/history?limit=N` so dashboards can render proof fingerprints without RPC access. The `snark` CLI (`cli/src/snark.rs`) still outputs attested circuit artifacts for out-of-band prover rollout.
- Energy RPC/CLI: `energy.register_provider`, `energy.market_state`, `energy.settle`, and `energy.submit_reading` expose the `crates/energy-market` state plus oracle submissions. Governance feeds `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps` into this module, so operators can tune the market via proposals rather than recompiling.
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
