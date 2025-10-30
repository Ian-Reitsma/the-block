# HTTP Gateway – Zero‑Fee Web Hosting
> **Review (2025-09-25):** Synced HTTP Gateway – Zero‑Fee Web Hosting guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The HTTP gateway is the public entry point for on‑chain web sites. It maps a
`SiteManifestTx` domain to its blob assets, executes optional `FuncTx` WASM
handlers, logs every read via `ReadAck`, and exports analytics without charging
visitors or publishers. The read acknowledgement format and audit workflow are
detailed in [docs/read_receipts.md](read_receipts.md).

Signed DNS TXT records advertise gateway policy and track read counters; see [docs/gateway_dns.md](gateway_dns.md) for publishing and retrieval semantics.

`blockctl gateway domain` now exposes first-party helpers for premium-domain auctions, including stake registration/withdrawal,
seller-driven cancellations, and audit-friendly stake-status queries (with per-transfer ledger references) layered over the
`dns.*` RPC methods.

### DNS TXT Verification

Nodes validating external domains fetch TXT records and require a `tb-verification=<node_id>` token before honoring on-chain DNS entries. Results are cached for one hour and exposed via `net dns verify <domain>` for manual checks. Operators may disable verification in development environments with `gateway_dns_disable_verify = true`.

Security considerations are catalogued under
[threat_model/hosting.md](threat_model/hosting.md).

## 1. Request Lifecycle

1. **Accept & Throttle** – `web/gateway.rs` accepts the TCP connection and runs a
   per‑IP token bucket. Exceeding the bucket returns HTTP 429 and logs
   `read_denied_total{reason="rate_limit"}`.
2. **Domain Stake Check** – the `Host` header is verified against the on‑chain
   stake table. Domains without an escrowed deposit receive HTTP 403.
3. **Manifest Resolve** – the published `SiteManifestTx` is fetched by domain
   name. The manifest maps paths to blob IDs and optional WASM function hashes.
4. **Static Blob Stream** – for ordinary paths the gateway pulls erasure‑coded
   shards via `storage/pipeline.rs`, reassembles the blob, and streams bytes to
   the client. No fees are charged and the client decrypts locally if needed.
5. **Dynamic Execution** – `"/api/"` paths invoke the referenced `FuncTx`. The
   WASM bytecode is loaded from the blob store, executed with deterministic fuel
   limits, and its output streamed back to the client.
6. **Campaign Match & ReadAck Append** – once the response body is sent, the
   gateway asks the first-party advertising marketplace for a matching campaign
   given the domain, provider metadata, and badge context. Provider badges come
   from the global registry maintained by `service_badge::provider_badges`, so
   physical-presence checks survive restarts and badge revocations without
   querying sled mid-request. Provider identity now flows through
   `storage::pipeline::provider_for_manifest`: manifests published with explicit
   provider lists always win, and multi-provider manifests hash the reservation
   key (domain + path + byte range) to select a stable entry. Gateway tests use
   the new `pipeline::override_manifest_providers_for_test` hook so multi-provider
   scenarios stay hermetic without touching the filesystem. The winning creative
   (if any) is recorded on the acknowledgement, the caller-supplied
   `X-TheBlock-Ack-*` headers are validated, and the Ed25519 signature over the
   manifest, path hash, byte count, timestamp, client hash, domain, provider, and
   campaign fields is verified before the fully signed `ReadAck` is pushed into
   the batching queue. `ReadAck` now carries an optional `badge_soft_intent`
   payload containing the ANN snapshot fingerprint, encrypted query ciphertext,
   IV, and nearest-neighbour distance; gateway parses
   `X-TheBlock-Ann-{Snapshot,Proof}` headers into that structure and threads it to
  the ad marketplace so wallets can audit badge intent. Selection traces expose
  `requested_kappa`, shading multipliers, shadow prices, and dual-token toggles,
  letting RPC/SDK consumers correlate pacing guidance with the receipt that
  cleared the impression. The gateway integration test suite now asserts those
  fields for every candidate in the receipt, so multi-creative auctions surface
  consistent shading telemetry rather than only reporting the winner. Wallets
  that supply additional ANN entropy see it preserved in the soft-intent
  payload, and verification rejects transcripts whose ciphertext or IV no longer
  match the mixed entropy parameters.

### WebSocket peer metrics

- The `/ws/peer_metrics` endpoint now upgrades connections through the in-house
  `httpd::Router::upgrade` API. Handshake validation (Upgrade/Connection headers,
  `Sec-WebSocket-*` tokens, and keep-alive negotiation) is handled by httpd before
  the runtime `ServerStream` is handed to the metrics publisher.
- Metrics snapshots are serialized to JSON text frames. The runtime layer
  automatically responds to incoming ping frames and closes the connection when
  either side emits a close control frame.
- CLI consumers should construct the handshake with
  `runtime::ws::handshake_key`/`read_client_handshake` as demonstrated in
  `node/src/bin/net.rs`. Existing telemetry (`peer_metrics_active` gauge and
  send error counters) continues to fire around the new implementation.

## 2. Receipt Batching, Submission & Analytics

- `ReadBatcher::finalize()` drains queued `ReadAck`s, hashes each record (domain
  and campaign metadata included), and writes the resulting binary batch and
  Merkle root to disk. Roots anchor on‑chain so auditors can reconstruct traffic.
- `spawn_read_ack_worker()` inside `node/src/bin/node.rs` receives each
  acknowledgement from the gateway, attaches the current readiness snapshot,
  records telemetry via `read_ack_processed_total{result="ok|invalid_signature|invalid_privacy"}`,
  feeds the ledger’s per-role byte maps, and commits advertising reservations as
  pending settlements.
- The `analytics` RPC exposes per‑domain totals computed from finalized batches
  allowing site operators to verify pageviews or ad impressions.

## 3. Subsidy Issuance for Reads

- Finalized read batches mint `READ_SUB_CT` via the block coinbase, but the
  resulting CT is now split across viewer, host, hardware, verifier, liquidity,
  and miner accounts according to governance parameters. Advertising settlements
  debit the campaign budget at the same time and publish per-role `ad_*_ct`
  totals in the block header.
- Runtime telemetry counters `subsidy_bytes_total{type="read"}` and
  `read_ack_processed_total{result}` increment with every anchored batch and
  validation decision. Watch for growth in
  `read_ack_processed_total{result="invalid_signature"}` or
  `{result="invalid_privacy"}` to catch replayed signatures or mismatched proofs
  before they impact campaign settlements.

## 4. Advertising Marketplace Integration

- Campaign matching is gated behind governance-controlled readiness thresholds.
  The node instantiates an `AdReadinessHandle` shared by the gateway, RPC
  runtime, and telemetry summary; `attach_campaign_metadata` skips matching and
  increments `ad_readiness_skipped_total{reason}` until rolling unique-viewer,
  host, and provider counts clear the configured floor. The
  `ad_market.readiness` RPC and Prometheus gauges (`ad_readiness_ready`,
  `ad_readiness_unique_viewers`, `ad_readiness_total_usd_micros`,
  `ad_readiness_settlement_count`, `ad_readiness_ct_price_usd_micros`, and
  `ad_readiness_it_price_usd_micros`) expose current counters, oracle snapshots,
  and blockers.
- Readiness events persist to a sled namespace keyed by
  `SimpleDb::names::GATEWAY_AD_READINESS`; startup replays the surviving window
  before installing the global handle so readiness decisions survive restarts
  as long as acknowledgements remain inside the configured horizon.
- The `ad_market` crate now defaults to the sled-backed `SledMarketplace`,
  persisting campaign manifests, budgets, and distribution policies across
  restarts. RPC and CLI surfaces feed campaigns through the handwritten
  `campaign_from_value` converter so FIRST_PARTY_ONLY builds never depend on the
  `foundation_serde` stub.
- Gateway tests and integration suites use the shared
  `fuzz_dispatch_request`/`fuzz_runtime_config_with_admin` helpers to exercise
  `ad_market.register_campaign` without binding TCP sockets, keeping duplicate
  registration and invalid payload paths hermetic under the
  `integration-tests` feature.
- Reservations include a per-mebibyte CT price; when the acknowledgement is
  accepted the node commits the settlement, carves up the CT/IT based on the
  active distribution policy, records the USD micros plus oracle snapshot, and
  publishes per-role totals (`ad_viewer_ct`, `ad_host_it`, etc.) in the block
  header for explorer/CLI consumption.
- Pending settlements surface through the explorer and RPC snapshots so
  advertisers can reconcile impressions against debits without replaying the raw
  receipt files.

## 5. Abuse Prevention Summary

- **Rate limits** – per‑IP token buckets backed by an Xor8 filter (97 % load, 1.1×10⁻³ FP); governance knob `gateway.req_rate_per_ip`.
- **Stake deposits** – domains bond CT before serving content; slashable on
  abuse.
- **WASM fuel** – deterministic execution with `func.gas_limit_default`.
- **Auditability** – all reads recorded via `ReadAck`; batches with <10 % signed
  acks are discarded and can trigger slashing.

## 6. Operator Visibility

- `gateway.policy` reports current rate‑limit counters and last access time.
- `gateway.reads_since(epoch)` scans finalized batches for historical traffic.
- `analytics` RPC provides aggregated read counts and bytes, suitable for
  dashboarding or advertising audits.
