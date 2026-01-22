# APIs and Tooling

Reference for every public surface: RPC, CLI, gateway, DNS, explorer, telemetry, and schemas.

## How to Think About APIs Here

> **For newcomers:** There are three main ways to interact with The Block:
>
> | Interface | What It Is | When to Use |
> |-----------|------------|-------------|
> | **JSON-RPC** | Machine-to-machine API (JSON over HTTP) | Building apps, automation, integrations |
> | **CLI (`contract-cli`)** | Command-line tool — a "friendly face" over RPC | Manual operations, debugging, scripting |
> | **Explorer & Dashboards** | Web UI for viewing chain state | Human operators watching the network |
>
> All three talk to the same underlying node. The CLI is essentially a wrapper around RPC calls with nice formatting.

## RPC Namespace Overview

| Namespace | What It Does | Example Method | Code |
|-----------|--------------|----------------|------|
| `consensus` | Block production, finality, validator info | `consensus.block_height` | `node/src/rpc/consensus.rs` |
| `consensus.pos` | Proof-of-Stake validator operations | `consensus.pos.register`, `consensus.pos.bond` | `node/src/rpc/pos.rs` |
| `ledger` | Account balances, transaction history | `ledger.balance` | `node/src/rpc/ledger.rs` |
| `storage` | File storage operations | `storage.put`, `storage.get` | `node/src/rpc/storage.rs` |
| `compute_market` | Submit jobs, query receipts, SLA history | `compute_market.submit_job` | `node/src/rpc/compute_market.rs` |
| `ad_market` | Ad bidding, cohort queries, policy snapshots | `ad_market.submit_bid`, `ad_market.policy_snapshot` | `node/src/rpc/ad_market.rs` |
| `governance` | Proposals, voting, parameters | `governance.proposals` | `node/src/rpc/governance.rs` |
| `governor` | Launch readiness gates, staged rollout | `governor.status`, `governor.decisions` | `node/src/rpc/governor.rs` |

`governor.status` now returns the deterministic `economics_prev_market_metrics` array (ppm values per market) alongside the gate states and intents, so dashboards and CLI tooling can correlate the Prometheus gauges `economics_prev_market_metrics_{utilization,provider_margin}_ppm` with the persisted sample the governor records.
| `treasury` | Disbursements, balances | `treasury.balance`, `treasury.submit_disbursement` | `node/src/rpc/treasury.rs` |
| `energy` | Energy market operations | `energy.register_provider`, `energy.settle` | `node/src/rpc/energy.rs` |
| `peer` | Network peer info | `peer.list`, `peer.stats` | `node/src/rpc/peer.rs` |
| `vm` | Smart contract execution | `vm.call`, `vm.trace` | `node/src/rpc/vm.rs` |
| `state_stream` | Light-client streaming | `state_stream.subscribe` | `node/src/rpc/state_stream.rs` |
| `scheduler` | Scheduler queue statistics | `scheduler.stats` | `node/src/rpc/scheduler.rs` |
| `jurisdiction` | Jurisdiction policy management | `jurisdiction.status`, `jurisdiction.set` | `node/src/rpc/jurisdiction.rs` |
| `node` | Node configuration and privacy settings | `node.get_ack_privacy`, `node.set_ack_privacy` | `node/src/rpc/mod.rs` |
| `config` | Runtime configuration reload | `config.reload` | `node/src/rpc/mod.rs` |
| `mesh` | LocalNet peer discovery | `mesh.peers` | `node/src/rpc/mod.rs` |
| `rent` | Storage rent escrow status | `rent.escrow.balance` | `node/src/rpc/mod.rs` |
| `gateway` | Gateway, DNS, venue, mobile cache operations | `gateway.dns_lookup`, `gateway.venue_status` | `node/src/rpc/mod.rs` |
| `settlement` | Compute market settlement auditing | `settlement.audit` | `node/src/rpc/mod.rs` |
| `anomaly` | Telemetry anomaly labeling | `anomaly.label` | `node/src/rpc/mod.rs` |
| `analytics` | Aggregated analytics stats (telemetry feature) | `analytics` | `node/src/rpc/analytics.rs` |

## JSON-RPC
- Server lives in `node/src/rpc`. Namespaces: `consensus`, `ledger`, `storage`, `compute_market`, `ad_market`, `governance`, `peer`, `treasury`, `vm`, `logs`, `state_stream`, `analytics`.
- Transport: first-party `httpd` router (HTTP/1.1, HTTP/2, WebSocket upgrades) plus mutual-TLS derived from node keys.
- Fault handling: clients clamp `TB_RPC_FAULT_RATE`, saturate exponential backoff after 31 attempts, and expose regression coverage for bounded retries.
- Streaming: `state_stream` (light-client snapshots), `compute.job_cancel`, and telemetry correlations.

## CLI (`contract-cli`)
- Main entry in `cli/src/main.rs`. Subcommands include: `node`, `gov`, `wallet`, `bridge`, `dex`, `compute`, `storage`, `gateway`, `mesh`, `light`, `telemetry`, `probe`, `diagnostics`, `service-badge`, `remediation`, `ai`, `ann`, `identity`.
- `contract-cli energy` wraps the `energy.*` RPCs (`register`, `market`, `receipts`, `credits`, `settle`, `submit-reading`, `disputes`, `flag-dispute`, `resolve-dispute`, `slashes`). It prints friendly tables by default and supports `--verbose`/`--format json` when you need machine-readable payloads or to export providers/receipts/disputes/slashes for explorer ingestion. See `docs/testnet/ENERGY_QUICKSTART.md` for scripted walkthroughs and dispute drills.
- Provider trust roots live in `config/default.toml` under `energy.provider_keys`. Reloads hot-swap the verifier registry, so keep this file in sync with the public keys your adapters sign with; unlisted providers remain in shadow mode and their readings will be rejected once keys are registered.
- The `/wrappers` energy section now exposes `energy_quorum_shortfall_total`, `energy_reading_reject_total{reason}`, and `energy_dispute_total{state}` so CLI diagnostics and alerts can point operators to the exact condition that triggered a rejection or a dispute lifecycle update.
- Use `contract-cli --help` or `contract-cli <cmd> --help`. Structured output via `--format json` for automation.
- CLI shares foundation crates (serialization, HTTP client, TLS) with the node, so responses stay type-aligned.

### CLI Command Reference

| Command | Purpose | Code |
|---------|---------|------|
| `contract-cli node` | Node lifecycle (start, stop, status) | `cli/src/node.rs` |
| `contract-cli gov` | Governance proposals, voting, parameters | `cli/src/gov.rs` |
| `contract-cli gov disburse` | Treasury disbursement workflow | `cli/src/gov.rs` |
| `contract-cli wallet` | Wallet creation, signing, balances | `cli/src/wallet.rs` |
| `contract-cli bridge` | Cross-chain bridge operations | `cli/src/bridge.rs` |
| `contract-cli dex` | DEX trading, order books, trust lines | `cli/src/dex.rs` |
| `contract-cli compute` | Compute market jobs, receipts, proofs | `cli/src/compute.rs` |
| `contract-cli storage` | File storage operations | `cli/src/storage.rs` |
| `contract-cli energy` | Energy market (register, settle, readings) | `cli/src/energy.rs` |
| `contract-cli gateway` | Gateway and DNS management | `cli/src/gateway.rs` |
| `contract-cli mesh` | LocalNet/mesh peer operations | `cli/src/mesh.rs` |
| `contract-cli light` | Light client sync and proofs | `cli/src/light_sync.rs` |
| `contract-cli telemetry` | Telemetry metrics and wrappers | `cli/src/telemetry.rs` |
| `contract-cli probe` | Network probing and diagnostics | `cli/src/probe.rs` |
| `contract-cli diagnostics` | Debug dumps (mempool, scheduler, gossip, RPC) | `cli/src/debug_cli.rs` |
| `contract-cli service-badge` | Badge issuance, revocation, verification | `cli/src/service_badge.rs` |
| `contract-cli remediation` | Network partition recovery tools | `cli/src/remediation.rs` |
| `contract-cli ai` | AI diagnostics and analysis | `cli/src/ai.rs` |
| `contract-cli ann` | Ad network/ANN mesh operations | `cli/src/ann.rs` |
| `contract-cli identity` | DID and handle management | `cli/src/identity.rs` |
| `contract-cli config` | Configuration management | `cli/src/config.rs` |
| `contract-cli logs` | Log searching and filtering | `cli/src/logs.rs` |
| `contract-cli tls` | TLS certificate management | `cli/src/tls.rs` |
| `contract-cli scheduler` | Scheduler queue statistics | `cli/src/scheduler.rs` |
| `contract-cli explorer` | Explorer sync and queries | `cli/src/explorer.rs` |
| `contract-cli wasm` | WASM build and deployment | `cli/src/wasm.rs` |
| `contract-cli contract` | Smart contract deploy/call | `cli/src/contract_dev.rs` |
| `contract-cli vm` | VM tracing and debugging | `cli/src/vm.rs` |

**Energy CLI** (`contract-cli energy`) wraps the `energy.*` RPCs (`register`, `market`, `settle`, `submit-reading`). It prints friendly tables by default and supports `--verbose`/`--format json` when you need machine-readable payloads. See `docs/testnet/ENERGY_QUICKSTART.md` for walkthroughs.

**Energy settlement governance** uses `gov.energy_settlement` to submit mode changes and `gov.energy_settlement_history` to enumerate the applied/rollback timeline. `contract-cli gov energy-settlement` supports `--dry-run` to print the request payload and `--timeline` to fetch the persisted history before executing. Both RPCs/CLI commands surface the `energy_settlement_mode` gauge and `energy_settlement_rollback_total` counter described in `docs/operations.md#telemetry-wiring`.

### Energy RPC payloads, auth, and error contracts
- Endpoints live under `energy.*` and inherit the RPC server’s mutual-TLS/auth policy (`TB_RPC_AUTH_TOKEN`, allowlists) plus IP-based rate limiting defined in `docs/operations.md#gateway-policy`. Use `contract-cli diagnostics rpc-policy` to inspect the live policy before enabling public oracle submitters.
- Endpoint map:

| Method | Description | Request Body |
| --- | --- | --- |
| `energy.register_provider` | Register capacity/jurisdiction/meter binding plus stake. | `{ "capacity_kwh": u64, "price_per_kwh": u64, "meter_address": "string", "jurisdiction": "US_CA", "stake": u64, "owner": "account-id" }` |
| `energy.market_state` | Fetch snapshot of providers, outstanding meter credits, receipts, and open disputes; pass `{"provider_id":"energy-0x01"}` to filter. | optional object |
| `energy.submit_reading` | Submit signed meter total to mint a credit. | `MeterReadingPayload` JSON (below) |
| `energy.settle` | Burn credit + capacity to settle kWh and produce `EnergyReceipt`. | `{ "provider_id": "energy-0x01", "buyer": "acct"?, "kwh_consumed": u64, "meter_hash": "0x..." }` |
| `energy.receipts` | Paginated settlement history (optionally filtered by provider). | `{ "provider_id"?: "energy-0x00", "page"?: u64, "page_size"?: u64 }` |
| `energy.credits` | Paginated meter-credit listing (optionally filtered by provider). | `{ "provider_id"?: "energy-0x00", "page"?: u64, "page_size"?: u64 }` |
| `energy.disputes` | Paginated dispute log with optional filters (provider, status, meter hash). | `{ "provider_id"?: "energy-0x00", "status"?: "open", "meter_hash"?: "hex", "page"?: u64, "page_size"?: u64 }` |
| `energy.slashes` | Paginated energy slash receipts (quorum/expiry/conflict) with provider filter. | `{ "provider_id"?: "energy-0x00", "page"?: u64, "page_size"?: u64 }` |
| `energy.flag_dispute` | Open a dispute tied to a `meter_hash`. | `{ "meter_hash": "hex", "reason": "string", "reporter"?: "account" }` |
| `energy.resolve_dispute` | Resolve an existing dispute, recording a resolver/note. | `{ "dispute_id": u64, "resolver"?: "account", "resolution_note"?: "string" }` |
| `energy.slashes` | Retrieve the ledger of slap receipts (quorum, expiry, conflict). | `{ "provider_id"?: "energy-0x00", "page"?: u64, "page_size"?: u64 }` |

- `energy.market_state` response structure:

```json
{
  "status": "ok",
  "providers": [ { "provider_id": "energy-0x00", "capacity_kwh": 10_000, "...": "..." } ],
  "credits": [ { "provider": "energy-0x00", "meter_hash": "e3c3…", "amount_kwh": 120, "timestamp": 123456 } ],
  "receipts": [ { "buyer": "acct", "seller": "energy-0x00", "kwh_delivered": 50, "price_paid": 2500, "treasury_fee": 125, "slash_applied": 0, "meter_hash": "e3c3…" } ],
  "disputes": [ { "id": 1, "meter_hash": "e3c3…", "provider_id": "energy-0x00", "status": "open", "reason": "bad reading", "opened_at": 1234567890 } ],
  "slashes": [ { "provider_id": "energy-0x00", "meter_hash": "e3c3…", "reason": "quorum", "amount": 10, "block_height": 1234 } ]
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

- Errors map to JSON-RPC codes instead of opaque strings. The server returns RPC errors with `code`/`message` populated; clients should branch on `code`:
  - `-33006` Provider inactive/conflict (`ProviderExists`, `MeterAddressInUse`, `UnknownProvider`, `InsufficientStake`, `InsufficientCapacity`, `InsufficientCredit`)
  - `-33005` Settlement conflict (`CreditExpired`, `SettlementNotDue`, `UnknownReading`, dispute errors: `UnknownMeterReading`, `AlreadyOpen`, `UnknownDispute`, `AlreadyResolved`)
  - `-33004` Quorum failed (`SettlementBelowQuorum`)
  - `-33003` Meter mismatch (`StaleReading`, `InvalidMeterValue`)
  - `-33001` Signature invalid (`SignatureVerificationFailed`)
  - `-32602` Invalid params (missing/ill-typed JSON fields)
- Signature/format errors: RPC rejects payloads where `meter_hash` is not 32 bytes, numbers are missing, or signatures fail decoding. Bad signatures map to `-33001`.
- Negative tests live next to the RPC module; mimic them for client libraries so bad signatures, stale timestamps, meter mismatches, and quorum/settlement gating produce structured failures instead of panics. IP-based rate limiting for `energy.*` endpoints uses `TB_RPC_ENERGY_TOKENS_PER_SEC` (default 20 tokens/sec) and surfaces `-32001` rate-limit errors.
- Observer tooling: `contract-cli energy market --verbose` dumps the whole response, `contract-cli energy receipts|credits --json` streams paginated settlements/credits, and `contract-cli diagnostics rpc-log --method energy.submit_reading` tails submissions with auth metadata so you can trace rate-limit hits. `contract-cli energy disputes|flag-dispute|resolve-dispute` mirrors the RPC contracts for dispute management.
- Remaining RPC/CLI work items (tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks`) focus on explorer timelines, richer governance payloads, and tightening per-endpoint rate limiting once the QUIC chaos drills complete.

### Treasury disbursement CLI, RPC, and schema
- Disbursement proposals live inside the governance namespace. CLI entrypoints sit under `contract-cli gov disburse`:
  - `create` scaffolds a JSON template (see `examples/governance/disbursement_example.json`) and fills in proposer defaults (badge identity, default timelock/rollback windows).
  - `preview --json <file>` validates the payload against the schema and prints the derived timeline: quorum requirements, vote window, activation epoch, timelock height, and resulting treasury deltas.
  - `submit --json <file>` posts the signed proposal to the node via `gov.treasury.submit_disbursement`; dry-run with `--check` to ensure hashes match before sending live traffic.
  - `show --id <proposal-id>` renders the explorer-style timeline (metadata, quorum/vote tallies, timelock window, execution tx hash, receipts, rollback annotations).
  - `queue`, `execute`, and `rollback` mirror the on-chain transitions for operators who hold the treasury executor lease. `queue` acknowledges that the proposal passed and seeds the executor queue (pass `--epoch`; the CLI clamps missing/zero values to epoch 1), `execute` pushes the signed transaction (recording `tx_hash`, nonce, receipts), and `rollback` reverts executions within the bounded window.
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
    "quorum": { "operators_ppm": 670000, "builders_ppm": 670000 },
    "vote_window_epochs": 6,
    "timelock_epochs": 2,
    "rollback_window_epochs": 1
  },
  "disbursement": {
    "destination": "tb1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqe4tqx9",
    "amount": 125000000,
    "memo": "Core grants Q2",
    "scheduled_epoch": 180500,
    "expected_receipts": [
      { "account": "foundation", "amount": 100000000 },
      { "account": "audit-retainer", "amount": 25000000 }
    ]
  }
}
```

- Validation: destinations must start with `tb1`, memos are capped at 8KiB, dependency lists are limited to 100 entries (proposal `deps` take precedence over memo hints), and `expected_receipts` must sum to `amount`.
- Explorer/CLI surfaces clamp `deps` to 100 entries and ignore memo-derived dependency hints once the memo crosses the 8KiB cap so RPC payloads, explorer timelines, and schema snapshots stay in lockstep (see `explorer/tests/treasury_api.rs` for the enforced shape).
- RPC exposure:
  - `gov.treasury.submit_disbursement { payload, signature }` – create proposal from JSON.
  - `gov.treasury.disbursement { id }` – fetch canonical status/timeline for a single record.
  - `gov.treasury.queue_disbursement { id, current_epoch }`, `gov.treasury.execute_disbursement { id, tx_hash, receipts }`, `gov.treasury.rollback_disbursement { id, reason }` – maintenance hooks for executor operators (all auth gated). Provide the current epoch explicitly so timelock math matches the chain (CLI defaults to epoch 1 if omitted).
  - `gov.treasury.list_disbursements { cursor?, status?, limit? }` – explorer/CLI listings; responses flatten the governance struct and expose `expected_receipts` plus a canonical `deps` vector (proposal.deps if present, else memo-derived and capped at 100).
- CLI exposes `--schema` and `--check` flags to dump the JSON schema and to validate payloads offline. CI keeps the examples under `examples/governance/` in sync by running `contract-cli gov disburse preview --json … --check` during docs tests.
- Explorer’s REST API mirrors the RPC fields so UI timelines and CLI scripts stay aligned; see `explorer/src/treasury.rs`.
- Timeline response shape is pinned via a Blake3 hash (`c48f401c3792195c9010024b8ba0269b0efd56c227be9cb5dd1ddba793b2cbd1`) enforced in explorer/CLI tests; bump the fixtures and the documented hash intentionally when adding or removing fields.
- `/wrappers` governance summaries are likewise hash-checked in CI (`e6982a8b84b28b043f1470eafbb8ae77d12e79a9059e21eec518beeb03566595`) so dashboards and downstream consumers detect schema drift; refresh the wrappers snapshot and Grafana panels together when the telemetry surface changes.


## Gateway HTTP and CDN Surfaces
- `node/src/gateway/http.rs` hosts HTTP + WebSocket endpoints for content, APIs, and read receipts. Everything goes through the first-party TLS stack (`crates/httpd::tls`).
- Operators tag responses with `ReadAck` headers so clients can submit proofs later.
- Range-boost forwarding and mobile cache endpoints hang off the same router; see `docs/architecture.md#gateway-and-client-access` for internals.

## HTTP client and TLS diagnostics
- Outbound clients live in `crates/httpd::{client.rs,blocking.rs}`. `httpd::Client` wraps the runtime `TcpStream`, supports HTTPS via the in-house TLS connector, and exposes:
  - `ClientConfig { connect_timeout, request_timeout, read_timeout?, tls_handshake_timeout, max_response_bytes, tls }`.
  - `Client::with_tls_from_env(&["TB_NODE_TLS","TB_HTTP_TLS"])` to reuse the same certs as RPC/gateway surfaces.
  - `RequestBuilder::json(value)` for canonical JSON encoding and `send()` for async execution. Blocking variants offer the same API for CLI tools.
- TLS rotation:
  - Set `TB_NET_CERT_STORE_PATH` to control where mutual-TLS certs are stored. `contract-cli net rotate-cert` and `contract-cli net rotate-key` (see `cli/src/net.rs`) wrap the RPCs that rotate QUIC certs and peer keys.
  - Diagnostics: `contract-cli net quic failures --url <rpc>` lists handshake failures; `contract-cli net overlay-status` confirms which overlay backend is active and where the peer DB lives.
- HTTP troubleshooting: both the node and CLI honour `TB_RPC_TIMEOUT_MS`, `TB_RPC_TIMEOUT_JITTER_MS`, and `TB_RPC_MAX_RETRIES`. Use `contract-cli net dns verify` to confirm TXT records and `contract-cli net gossip-status` to inspect HTTP routing metadata exposed via gossip debug RPCs.

## DNS and Naming
- Publishing: `node/src/gateway/dns.rs` writes `.block` zone files or external DNS records using schemas under `docs/spec/dns_record.schema.json`.
- CLI: `contract-cli gateway dns publish`, `contract-cli gateway dns audit`.

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
- CLI: `contract-cli storage put|get|manifest|repair`, `contract-cli blob summarize`, `contract-cli storage providers`.
- RPC: `storage.put_blob`, `storage.get_manifest`, `storage.list_providers`.
- Blob manifests follow the binary schema in `node/src/storage/manifest_binary.rs`; object receipts encode `StoreReceipt` structs consumed by the ledger.

## Compute, Energy, and Ad Market APIs
- Compute RPC/CLI: reserve capacity, post workloads, submit receipts, inspect fairness metrics, cancel jobs (`compute.job_cancel`). Courier snapshots stream through `compute_market.courier_status`, proof bundles (with fingerprints + circuit artifacts) are downloadable via `compute_market.sla_history(limit)`, `contract-cli compute proofs --limit N` pretty-prints recent SLA/proof entries, `contract-cli explorer sync-proofs --db explorer.db` ingests them into the explorer SQLite tables (`compute_sla_history` + `compute_sla_proofs`), and the explorer HTTP server exposes `/compute/sla/history?limit=N` so dashboards can render proof fingerprints without RPC access. The `snark` CLI (`cli/src/snark.rs`) still outputs attested circuit artifacts for out-of-band prover rollout.
- Energy RPC/CLI: `energy.register_provider`, `energy.market_state`, `energy.settle`, and `energy.submit_reading` expose the `crates/energy-market` state plus oracle submissions. Governance feeds `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps` into this module, so operators can tune the market via proposals rather than recompiling.
- Ad market RPC/CLI: campaigns now target multi-signal cohorts (domain tiers, badge mixes, interest tags, and proof-of-presence buckets). Key endpoints:
  - `ad_market.inventory`, `ad_market.list_campaigns`, `ad_market.distribution`, `ad_market.budget`, `ad_market.broker_state`, `ad_market.readiness` return selector-aware snapshots. Every `CohortPriceSnapshot`/`ReadinessSnapshot` carries `selectors_version`, `domain_tier`, `interest_tags`, optional `presence_bucket`, and privacy-budget gauges so downstream tooling can render per-segment pricing/utilization/freshness.
  - `ad_market.register_campaign` accepts selector maps (per-selector bid shading, pacing caps, presence/domain filters, conversion-value rules). CLI: `contract-cli ad-market register --file campaign.json` enforces the same schema and exposes `--selector` helpers for quick edits.
  - Presence APIs: `ad_market.list_presence_cohorts` enumerates privacy-safe presence buckets operators can bid on (with supply/freshness and guardrail reasons), and `ad_market.reserve_presence` lets campaigns reserve slots for high-value cohorts while consuming privacy budget. CLI: `contract-cli ad-market presence list|reserve`.
  - Conversion/value APIs: `ad_market.record_conversion` now supports `value`, `currency_code`, `selector_weights[]`, and `attribution_window_secs` so advertisers can compute ROAS per selector. CLI supports `contract-cli ad-market record-conversion --file conversion.json`.
  - Claims + attribution: `ad_market.register_claim_route` sets payout routing per domain/role (publisher/host/hardware/verifier/liquidity/viewer) and `ad_market.claim_routes` returns the resolved map for a cohort snapshot; settlement/readiness surfaces the routes so explorers can render attribution. CLI: `contract-cli ad-market claim-routes --domain example.test` and `... register-claim-route` cover both directions.

### Presence Cohort JSON Schemas

#### `ad_market.list_presence_cohorts` — Request Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ListPresenceCohortsRequest",
  "type": "object",
  "properties": {
    "region": {
      "type": "string",
      "description": "ISO 3166-1 alpha-2 region filter (e.g., 'US', 'EU')",
      "pattern": "^[A-Z]{2}$"
    },
    "domain_tier": {
      "type": "string",
      "enum": ["premium", "reserved", "community", "unverified"],
      "description": "Filter by domain tier from .block auctions"
    },
    "min_confidence_bps": {
      "type": "integer",
      "minimum": 0,
      "maximum": 10000,
      "description": "Minimum presence confidence in basis points (0-10000)"
    },
    "interest_tag": {
      "type": "string",
      "description": "Filter by interest tag ID from governance registry"
    },
    "beacon_id": {
      "type": "string",
      "description": "Filter by specific beacon/venue identifier"
    },
    "kind": {
      "type": "string",
      "enum": ["localnet", "range_boost"],
      "description": "Filter by presence proof source"
    },
    "include_expired": {
      "type": "boolean",
      "default": false,
      "description": "Include buckets past TB_PRESENCE_TTL_SECS (for debugging)"
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1000,
      "default": 100,
      "description": "Maximum cohorts to return"
    },
    "cursor": {
      "type": "string",
      "description": "Pagination cursor from previous response"
    }
  },
  "additionalProperties": false
}
```

#### `ad_market.list_presence_cohorts` — Response Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ListPresenceCohortsResponse",
  "type": "object",
  "required": ["status", "cohorts", "privacy_budget"],
  "properties": {
    "status": { "type": "string", "const": "ok" },
    "cohorts": {
      "type": "array",
      "items": {
        "$ref": "#/definitions/PresenceCohortSummary"
      }
    },
    "privacy_budget": {
      "type": "object",
      "required": ["remaining_ppm"],
      "properties": {
        "remaining_ppm": {
          "type": "integer",
          "minimum": 0,
          "maximum": 1000000,
          "description": "Remaining privacy budget in parts per million (min across relevant families)"
        },
        "denied_ppm": {
          "type": "integer",
          "minimum": 0,
          "maximum": 1000000,
          "description": "Largest observed denial ratio in ppm across relevant families"
        },
        "cooldown_remaining": {
          "type": "integer",
          "description": "Max cooldown windows remaining across relevant families"
        },
        "denied_count": {
          "type": "integer",
          "description": "Number of cohorts redacted due to k-anonymity guardrails"
        }
      }
    },
    "next_cursor": {
      "type": "string",
      "description": "Cursor for next page (absent when no more results)"
    }
  },
  "definitions": {
    "PresenceCohortSummary": {
      "type": "object",
      "required": ["bucket", "ready_slots", "privacy_guardrail", "selector_prices"],
      "properties": {
        "bucket": { "$ref": "#/definitions/PresenceBucket" },
        "ready_slots": {
          "type": "integer",
          "minimum": 0,
          "description": "Available impression slots meeting readiness thresholds"
        },
        "privacy_guardrail": {
          "type": "string",
          "enum": ["ok", "k_anonymity_redacted", "budget_exhausted", "cooldown"],
          "description": "Reason code if privacy guardrails limit readiness data"
        },
        "selector_prices": {
          "type": "array",
          "items": { "$ref": "#/definitions/SelectorBidSpec" }
        },
        "freshness_histogram": {
          "type": "object",
          "description": "Distribution of proof ages (buckets: <1h, 1-6h, 6-24h, >24h)",
          "properties": {
            "under_1h_ppm": { "type": "integer" },
            "1h_to_6h_ppm": { "type": "integer" },
            "6h_to_24h_ppm": { "type": "integer" },
            "over_24h_ppm": { "type": "integer" }
          }
        },
        "domain_tier_supply": {
          "type": "object",
          "description": "Supply breakdown by domain tier within this presence bucket",
          "additionalProperties": { "type": "integer" }
        }
      }
    },
    "PresenceBucket": {
      "type": "object",
      "required": ["bucket_id", "kind", "radius_meters", "confidence_bps"],
      "properties": {
        "bucket_id": { "type": "string", "description": "Deterministic hash of bucket parameters" },
        "kind": { "type": "string", "enum": ["localnet", "range_boost"] },
        "region": { "type": "string", "description": "Optional region hint for the bucket" },
        "radius_meters": { "type": "integer", "minimum": 0, "maximum": 65535 },
        "confidence_bps": { "type": "integer", "minimum": 0, "maximum": 10000 },
        "minted_at_micros": { "type": "integer", "description": "Unix timestamp in microseconds" },
        "expires_at_micros": { "type": "integer", "description": "Expiry per TB_PRESENCE_TTL_SECS" }
      }
    },
    "SelectorBidSpec": {
      "type": "object",
      "required": ["selector_id", "clearing_price_usd_micros"],
      "properties": {
        "selector_id": { "type": "string", "description": "blake3(domain||domain_tier||interest_tags||presence_bucket||version)" },
        "clearing_price_usd_micros": { "type": "integer", "minimum": 0, "description": "Baseline price per MiB for this selector" },
        "shading_factor_bps": { "type": "integer", "minimum": 0, "maximum": 10000, "default": 0 },
        "slot_cap": { "type": "integer", "minimum": 0 },
        "max_pacing_ppm": { "type": "integer", "minimum": 0, "maximum": 1000000 }
      }
    }
  }
}
```

#### `ad_market.reserve_presence` — Request Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ReservePresenceRequest",
  "type": "object",
  "required": ["campaign_id", "presence_bucket_id", "slot_count"],
  "properties": {
    "campaign_id": {
      "type": "string",
      "minLength": 1,
      "maxLength": 256,
      "description": "Existing campaign ID from ad_market.register_campaign"
    },
    "presence_bucket_id": {
      "type": "string",
      "minLength": 1,
      "description": "Bucket ID from ad_market.list_presence_cohorts"
    },
    "slot_count": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1000000,
      "description": "Number of impression slots to reserve"
    },
    "expires_at_micros": {
      "type": "integer",
      "description": "Optional explicit expiry; defaults to bucket expiry or TB_PRESENCE_TTL_SECS"
    },
    "selector_budget": {
      "type": "array",
      "items": { "$ref": "#/definitions/SelectorBidSpec" },
      "description": "Optional per-selector bid overrides for this reservation"
    },
    "max_bid_usd_micros": {
      "type": "integer",
      "minimum": 0,
      "description": "Maximum bid cap for this reservation"
    }
  },
  "definitions": {
    "SelectorBidSpec": {
      "type": "object",
      "required": ["selector_id", "clearing_price_usd_micros"],
      "properties": {
        "selector_id": { "type": "string" },
        "clearing_price_usd_micros": { "type": "integer", "minimum": 0 },
        "shading_factor_bps": { "type": "integer", "minimum": 0, "maximum": 10000 },
        "slot_cap": { "type": "integer", "minimum": 0 },
        "max_pacing_ppm": { "type": "integer", "minimum": 0, "maximum": 1000000 }
      }
    }
  },
  "additionalProperties": false
}
```

#### `ad_market.reserve_presence` — Response Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ReservePresenceResponse",
  "type": "object",
  "required": ["status", "reservation_id", "expires_at_micros"],
  "properties": {
    "status": { "type": "string", "const": "ok" },
    "reservation_id": {
      "type": "string",
      "description": "Unique reservation key for cancellation/inspection"
    },
    "expires_at_micros": {
      "type": "integer",
      "description": "When this reservation expires (bucket expiry or custom)"
    },
    "reserved_budget_usd_micros": {
      "type": "integer",
      "description": "Budget committed to this reservation (slot_count × clearing price)"
    },
    "effective_selectors": {
      "type": "array",
      "items": { "$ref": "#/definitions/SelectorBidSpec" },
      "description": "Merged selector specs after applying reservation overrides"
    }
  }
}
```

### Presence & Privacy Error Codes

| Code | Name | Description | Resolution |
|------|------|-------------|------------|
| `-32034` | `INVALID_PRESENCE_BUCKET` | Presence bucket is expired, malformed, or not found | Check `expires_at_micros` against `TB_PRESENCE_TTL_SECS`; refresh bucket via `ad_market.list_presence_cohorts` |
| `-32035` | `FORBIDDEN_SELECTOR_COMBO` | Selector combination violates privacy policy (e.g., premium domain + tight presence without opt-in) | Review `presence_filters` and `domain_filters` in campaign; may require explicit opt-in in metadata |
| `-32036` | `UNKNOWN_SELECTOR` | Interest tag or domain tier not in governance registry | Query `governance.interest_tags` or DNS tier registry |
| `-32037` | `INSUFFICIENT_PRIVACY_BUDGET` | Request would exceed per-selector or per-family epsilon/delta limits, or `slot_count` exceeds available `ready_slots` | Wait for budget decay (per `PrivacyBudgetManager` cooldown), reduce request scope, or lower `slot_count` |
| `-32038` | `HOLDOUT_OVERLAP` | Reservation conflicts with active holdout assignment | Cancel conflicting reservation or wait for holdout window to close |
| `-32039` | `SELECTOR_WEIGHT_MISMATCH` | `selector_weights[]` in conversion record don't sum to 1,000,000 ppm | Adjust weights; total must equal exactly 1,000,000 ppm |

### Ad Claim Routing

#### `ad_market.register_claim_route` — Request Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "RegisterClaimRouteRequest",
  "type": "object",
  "required": ["domain", "role", "address"],
  "properties": {
    "domain": { "type": "string" },
    "role": {
      "type": "string",
      "enum": ["publisher", "host", "hardware", "verifier", "liquidity", "viewer"]
    },
    "address": { "type": "string" },
    "owner_account": {
      "type": "string",
      "description": "DNS owner account (must match current ownership record if present)"
    },
    "app_id": {
      "type": "string",
      "description": "Optional DID/app identity anchor for attribution"
    }
  },
  "additionalProperties": false
}
```

#### `ad_market.register_claim_route` — Response Schema
```json
{
  "status": "ok"
}
```

Notes:
- Claim routes are keyed by domain + role and persist in the marketplace metadata.
- Settlement breakdowns include the registered routes; ad payouts use the route when present and fall back to the derived viewer/host/hardware/verifier/liquidity defaults when missing or malformed.

#### `ad_market.claim_routes` — Request Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ClaimRoutesRequest",
  "type": "object",
  "required": ["domain"],
  "properties": {
    "domain": { "type": "string" },
    "provider": { "type": "string" },
    "domain_tier": {
      "type": "string",
      "enum": ["premium", "reserved", "community", "unverified"]
    },
    "presence_bucket_id": { "type": "string" },
    "interest_tags": {
      "type": "array",
      "items": { "type": "string" }
    }
  },
  "additionalProperties": false
}
```

#### `ad_market.claim_routes` — Response Schema
```json
{
  "status": "ok",
  "claim_routes": {
    "publisher": "addr1",
    "host": "addr2"
  }
}
```

### Ad Attribution API

#### `ad_market.attribution` — Request Schema
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "AdAttributionRequest",
  "type": "object",
  "properties": {
    "selector_id": { "type": "string" },
    "domain": { "type": "string" },
    "app_id": { "type": "string" },
    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 100 }
  },
  "additionalProperties": false
}
```

#### `ad_market.attribution` — Response Schema
```json
{
  "status": "ok",
  "items": [
    {
    "selector_id": "blake3(domain||domain_tier||interest_tags||presence_bucket||version)",
      "app_id": "did:tb:abcd...",
      "spend_usd_micros": 1200000,
      "conversion_value_usd_micros": 3500000,
      "conversion_count": 42,
      "roi_ppm": 2916666
    }
  ]
}
```

### Governance Knobs for Presence

| Parameter | Env Var | Default | Description |
|-----------|---------|---------|-------------|
| `presence_ttl_secs` | `TB_PRESENCE_TTL_SECS` | 86400 | Maximum age of presence proofs before expiry |
| `presence_radius_meters` | `TB_PRESENCE_RADIUS_METERS` | 500 | Default radius for presence bucket aggregation |
| `presence_proof_cache_size` | `TB_PRESENCE_PROOF_CACHE_SIZE` | 10000 | Maximum cached `PresenceReceipt` entries per node |
| `presence_min_crowd_size` | via governance params | 5 | Minimum crowd count for venue-grade attestations |
| `presence_min_confidence_bps` | via governance params | 8000 | Minimum confidence for presence targeting |

Dashboards must be refreshed (per `docs/operations.md#telemetry-wiring`) whenever new selectors or RPC knobs land; capture the `/wrappers` hash plus Grafana screenshots in every PR.

## Light-Client Streaming
- RPC: `light.subscribe`, `light.get_block_range`, `light.get_device_status`, `state_stream.subscribe`. CLI: `contract-cli light sync`, `contract-cli light snapshot`, `contract-cli light device-status`.
- Mobile heuristics (battery, bandwidth, overrides) persist under `~/.the_block/light_client.toml`.

## Bridge, DEX, and Identity APIs
- Bridge RPC: `bridge.submit_proof`, `bridge.challenge`, `bridge.status`, `bridge.claim_reward`. CLI mirrors the same set.
- DEX RPC/CLI: order placement, swaps, trust-line routing, escrow proofs, HTLC settlement.
- Identity RPC: DID registration, revocation, handle lookup; CLI uses `contract-cli identity`.

## Wallet APIs
- CLI supports multisig, hardware signers, remote signers, and escrow-hash configuration: see `cli/src/wallet.rs` and `node/src/bin/wallet.rs`.
- Commands include wallet creation/import, address derivation, signing, broadcast, and governance voting (where applicable). Use `--format json` for automation and run `wallet discover-signers --timeout <ms> [--json]` to probe local signer endpoints (`--json` emits `{"timeout_ms":<ms>,"signers":[]}` for automation hooks).
- Remote signer workflows emit telemetry and enforce multisig signer-set policies documented in `docs/security_and_privacy.md#remote-signers-and-key-management`.

## Schemas and Reference Files
- JSON schemas under `docs/spec/` define fee market inputs (`fee_v2.schema.json`) and DNS records. Keep them in sync with code when adding fields.
- Dependency inventory snapshots live in `docs/dependency_inventory*.json`; regenerate after dependency changes.
- Assets (`docs/assets/`) include RSA samples, scheduler diagrams, and architecture SVGs referenced across the docs.
