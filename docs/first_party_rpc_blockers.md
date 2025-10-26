# First-Party RPC Migration Blockers

RPC handlers now build envelopes entirely through the first-party stack: the
`foundation_serialization::json!` macro mirrors serde_json’s token-munching
behaviour (nested objects, identifier keys, trailing commas), and the typed
`foundation_rpc::ResponsePayload<T>` helper keeps success/error decoding inside
the facade. Legacy serde_json references have been scrubbed from the production
RPC client.

## Immediate blockers

## Recent progress (2025-12-14)

- The WAN chaos pipeline stays entirely first party. `sim/src/chaos.rs` and the
  `chaos_lab` binary emit signed readiness attestations, while the metrics
  aggregator verifies `/chaos/attest` payloads through the existing
  `monitoring-build` crate and exposes `/chaos/status` plus the
  `chaos_readiness{module,scenario}`/`chaos_sla_breach_total` gauges via the
  foundation metrics facade. Release automation invokes `just chaos-suite` and
  `cargo xtask chaos`, both of which call the in-house binaries—no third-party
  chaos tooling, HTTP clients, or RPC stubs were introduced. The new
  `chaos_lab_attestations_flow_through_status` regression depends only on the
  first-party `tb-sim` crate to feed `/chaos/attest`, and Grafana’s generated
  dashboards (`monitoring/build.rs`) now surface a dedicated **Chaos** row without
  introducing external templating stacks. Additional regression coverage
  (`chaos_attestation_rejects_invalid_signature`) mutates signatures with the
  first-party crypto facade to confirm `/chaos/attest` rejects forged payloads,
  while the gossip relay and peer metrics store handle temp-dir failures using
  in-process fallbacks—no RPC shim or third-party filesystem helper was needed.

## Recent progress (2025-11-06)

- DNS stake management moved entirely onto first-party RPCs. `dns.register_stake`,
  `dns.withdraw_stake`, `dns.stake_status`, and `dns.cancel_sale` hand-build
  JSON envelopes, operate on sled-backed escrows, and drive ledger settlement
  through the existing `BlockchainLedger` facade—no serde/jsonrpc fallbacks or
  external clients required. Escrow records now retain per-transfer
  `ledger_events`/`tx_ref` metadata using the same handwritten codecs, and the
  RPC responses expose those fields directly so downstream tooling can reconcile
  ledger batches without glue code. CLI coverage (`gateway domain stake-*`,
  `cancel`) reuses the same builders, and integration tests exercise stake
  deposits, withdrawals, and cancellation without spawning mock servers.

## Recent progress (2025-11-05)

- Premium domain settlements now write ledger transaction references directly
  into the auction sale history; RPC/CLI surfaces reuse the existing
  first-party JSON builders so explorers can audit ledger-backed transfers
  without serde/jsonrpc fallbacks. Stake escrow enforcement relies solely on
  sled-backed helpers and deterministic JSON payloads.
- Ad readiness persistence lives entirely inside the node crate. The RPC layer
  continues to expose readiness snapshots via the first-party codecs while the
  handle replays events from sled on startup—no external storage drivers or
  async runtimes added.

## Recent progress (2025-11-03)

- Ad readiness gating stays within the existing RPC/runtime stack: governance
  parameters wire an `AdReadinessHandle` into the node, the gateway consults the
  shared handle before matching, and the new `ad_market.readiness` endpoint plus
  telemetry gauges reuse the first-party JSON/metrics facades—no serde_json,
  async runtimes, or external Prometheus clients were introduced. Integration
  tests drive the in-process RPC harness to prove readiness blockers clear once
  acknowledgements land.
- Premium domain auctions now ship entirely through handwritten cursor codecs
  persisted in `SimpleDb`. RPC handlers, CLI commands, and tests cover
  listing/bidding/completion/status flows without reintroducing serde or
  external databases. Resale royalties and protocol fees reuse the stored
  policy values, and regression suites remain fully first party.

## Recent progress (2025-11-02)

- Settlement-log and reward-accrual CLI commands now route through the first-party parser during tests, locking optional asset/relayer filters, cursor forwarding, and the default 50-row limit without clap stubs or serde_json helpers. The new `bridge_pending_dispute_persists_across_restart` regression keeps dispute persistence tests inside the sled-backed bridge crate, proving `pending_withdrawals` and `bridge.dispute_audit` retain challenged entries after a reopen. Monitoring gained `dashboards_include_bridge_remediation_legends_and_tooltips`, which inspects every generated Grafana JSON to ensure remediation panels keep their legends and descriptions first party—no external dashboard validators required.

## Recent progress (2025-10-26)

- Liquidity router coverage expanded entirely inside the node crate—multi-batch
  fairness, hop-limited trust fallbacks, and challenged withdrawal handling all
  run through in-process integration tests without introducing new RPC surfaces
  or external harnesses. Documentation updates reflect the slack-aware routing
  logic so downstream RPC contracts remain unchanged.

## Recent progress (2025-11-01)

- Bridge audit tooling stays fully first party: `bridge_dispute_audit_parser_defaults_limit_and_cursor` runs the command builder
  through `cli_core::parse::Parser` so the CLI retains its default 50-row page size and localhost fallback without leaning on
  clap-generated stubs, while `bridge_dispute_audit_serializes_optional_fields` proves `asset=None`/`cursor=None` envelopes emit
  JSON `null` via the existing transport helpers. Monitoring’s
  `dashboards_include_bridge_counter_panels` regression parses every generated Grafana JSON (dashboard/operator/telemetry/dev)
  to confirm the reward-claim, approval, settlement, and dispute panels keep their first-party queries and legends across
  templates—no external validators or dashboard tooling required.

## Recent progress (2025-10-31)

- Bridge remediation integration tests now allocate a per-case `RemediationSpoolSandbox`, wiring `TB_REMEDIATION_*_DIRS` env
  guards for page/throttle/quarantine/escalate targets through the in-house `sys::tempfile` helpers and verifying via
  `remediation_spool_sandbox_restores_environment` that each guard unwinds cleanly—retry-heavy suites stop writing to shared
  `/tmp` paths without external harnesses.
- Explorer payout ingestion now clamps regressions inside
  `metrics_aggregator::record_explorer_payout_metric`, emitting trace-only diagnostics while the
  `explorer_payout_counters_remain_monotonic_with_role_churn` and
  `explorer_payout_counters_are_peer_scoped` regressions alternate read/advertising role sets and peer IDs through the
  first-party HTTPd test harness to prove caches remain monotonic without third-party metrics tooling.

## Recent progress (2025-10-30)

- Explorer payout lookups now cover the legacy JSON path entirely inside the
  existing SQLite/`foundation_serialization` stack, and the CLI’s
  `explorer block-payouts` command exercises mutual-exclusion checks plus
  missing-block errors through the first-party mock transport—no serde_json or
  HTTP shims reinstated. Grafana gains a matching “Block Payouts” row generated
  by the in-house dashboard builder, keeping dashboards dependency-free.

## Recent progress (2025-10-29)

- Explorer `/blocks/:hash/payouts` reuses the first-party SQLite codec to emit per-role read/ad totals without serde_json fallbacks, and the CLI’s `explorer block-payouts` command forwards JSON-RPC envelopes exclusively through the in-house transport abstraction. Integration tests rely on the mock transport and stub SQLite backend, keeping FIRST_PARTY_ONLY coverage hermetic while exercising the new payloads.
- Governance and monitoring documentation now codify the response to rising `read_ack_processed_total{result="invalid_signature"}` or `invalid_privacy` counts without introducing external alert tooling; operators remain on the native dashboards/CLI, and the compute dashboard’s "Read Ack Outcomes" panel charts the first-party metric directly.

## Recent progress (2025-10-28)

- Gateway `ReadAck` submissions now ride entirely on first-party HTTP and
  signature tooling. The node verifies the `X-TheBlock-Ack-*` headers without
  serde helpers, derives the client hash locally, and enqueues acknowledgements
  only after the in-house Ed25519 backend validates the signature. Unit tests
  cover the transport with deterministic fixtures, and the documentation spells
  out the contract so SDKs can implement the headers without third-party JSON or
  crypto crates.

## Recent progress (2025-10-27)

- Bridge remediation spool artefacts now persist across acknowledgement retries,
  the restart suite verifies the cleanup path, the contract remediation CLI
  surfaces per-action `spool_artifacts` in filtered/JSON views, and monitoring
  now verifies both the latency overlays and the new
  `bridge_remediation_spool_artifacts` gauge/panel—all first-party plumbing with
  no third-party helpers.
- Bridge remediation dashboards now plot `bridge_remediation_ack_latency_seconds`
  alongside the policy gauge `bridge_remediation_ack_target_seconds`, and the
  metrics aggregator restores histogram samples after a restart. A new
  `BridgeRemediationAckLatencyHigh` alert fires when p95 breaches policy so
  operators are paged before escalations trigger.
- The `contract remediation bridge` CLI added `--playbook`, `--peer`, and
  `--json` flags so filtered or machine-readable action streams stay entirely on
  the first-party transport abstraction.

- Sweep remaining RPC responders for legacy helper usage (hand-written structs,
  bespoke payload builders) and migrate them to the shared request/response
  builders so tests cover the unified surface.
- RPC responses still depend on `Cow<'static, str>` message fields provided by
  `jsonrpc_core`. Converting them to first-party envelopes requires either new
  strongly-typed response structs or explicit `Cow` wrappers. ✅ *Resolved — the
  new `foundation_rpc::ResponsePayload<T>` wrapper exposes typed success/error
  branches while preserving the first-party `RpcError` struct, allowing
  handlers and clients to decode responses without the legacy envelope.*

## Recent progress (2025-10-25)

- Read acknowledgement privacy now ships entirely on first-party helpers. The
  gateway attaches readiness proofs from the new `zkp` crate, the node verifies
  them via `ReadAckPrivacyProof`, and operators can toggle enforcement with
  `node run --ack-privacy` or the `node.{get,set}_ack_privacy` RPCs—no external
  crypto libraries or transports required. Ad reservations hash a per-ack
  discriminator so duplicate fetches no longer clobber entries, and integration
  tests cover both the proof flow and collision-free settlements through the
  in-process RPC harness.

- Remediation follow-ups now queue automated retries and governance escalations
  without leaving the first-party engine. Actions persist `dispatch_attempts`,
  `auto_retry_count`, retry timestamps, and follow-up notes so CLI/tests can
  assert deterministic envelopes while HTTP hooks see explicit escalation
  context. The acknowledgement parser accepts plain-text payloads in addition to
  JSON, mapping strings like `"ack pager"` or `"closed: resolved"` into
  structured `BridgeDispatchAckRecord`s. Bridge alerts consume the stored
  acknowledgement counter to raise `BridgeRemediationAckPending` and
  `BridgeRemediationClosureMissing` without promtool or third-party parsers.

- Dispatch acknowledgement telemetry now stays first party: the metrics
  aggregator increments `bridge_remediation_dispatch_ack_total{action,playbook,target,state}`
  and persists acknowledgement timestamps/notes on remediation actions, while
  the CLI/integration tests drive HTTP responses through an in-process override
  client so acknowledgement flows remain hermetic without mock servers.
- Remediation payloads now ship operator-facing annotations, curated dashboard panel
  hints, deterministic response sequences, and the canonical
  `/remediation/bridge/dispatches` endpoint. The metrics aggregator appends every
  attempt to a first-party dispatch log so paging/governance hooks audit delivery
  without third-party queues. CLI integration tests capture the richer envelopes via
  the in-memory transport, locking the contract tooling to the first-party serializers.

## Recent progress (2025-10-24)

- Bridge remediation dispatches now emit `bridge_remediation_dispatch_total{action,playbook,target,status}`.
  The contract CLI tests drive the in-memory `BridgeRpcTransport` through spool success,
  spool failure, and unconfigured scenarios while Grafana/HTML dashboards gained a dispatch
  health panel, keeping first-party operators informed without third-party tooling.
- Explorer payout ingestion now updates `explorer_block_payout_read_total{role}` and
  `_ad_total{role}` counters directly through cached `CounterVec` handles in the metrics
  aggregator. The new Prometheus integration test proves both metrics advance on a
  second scrape, matching the Grafana queries without relying on external exporters,
  and documentation adds CLI automation snippets that stay on the first-party JSON stack.

## Recent progress (2025-10-23)

- Governance-backed bridge reward claims now emit first-party JSON. `bridge.claim_rewards`
  and `bridge.reward_claims` share typed request/response structs, while the CLI
  mirrors the same builders for `blockctl bridge claim` and `reward-claims` so
  payout reconciliation avoids serde fallbacks. Settlement proofs and dispute
  audits ride the same stack through `bridge.submit_settlement`,
  `bridge.settlement_log`, and `bridge.dispute_audit`, with CLI mirrors that reuse
  the shared JSON helpers. Cursor/limit pagination plus `next_cursor` propagation
  landed across these endpoints, keeping FIRST_PARTY_ONLY pagination logic inside
  the handwritten builders. Partial channel reconfiguration (`bridge.configure_asset`)
  accepts optional fields/clear flags without clobbering existing values, and
  unit tests in `governance/src/store.rs` and `node/src/governance/store.rs`
  verify reward approvals persist across reopen. Telemetry counters now include
  `BRIDGE_REWARD_CLAIMS_TOTAL`, `BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL`,
  `BRIDGE_SETTLEMENT_RESULTS_TOTAL{result,reason}`, and
  `BRIDGE_DISPUTE_OUTCOMES_TOTAL{kind,outcome}` alongside the existing challenge
  and slash metrics, with integration coverage in `node/tests/bridge_incentives.rs`.
- Reward accrual history shares the same first-party builders. `bridge.reward_accruals`
  paginates sled-backed `RewardAccrualRecord`s and the CLI forwards the
  identical envelope via `blockctl bridge reward-accruals`. Settlement proofs now
  compute a deterministic digest through `bridge_types::settlement_proof_digest`
  and maintain per-asset height watermarks so hash/height replays return typed
  errors without leaving the JSON/RPC façade.
- Bridge remediation dispatch uses the in-house HTTP client and filesystem
  spools. Configure `TB_REMEDIATION_*_URLS` or `TB_REMEDIATION_*_DIRS` to fan out
  JSON playbooks without introducing third-party paging libraries.

## Recent progress (2025-10-22)

- Bridge RPC now surfaces incentive accounting without serde fallbacks: duty and
  accounting records encode manually in `node/src/bridge/mod.rs`, new
  `bridge.relayer_accounting`/`bridge.duty_log` endpoints share typed request/
  response structs, and `blockctl bridge accounting`/`bridge duties` emit JSON
  through the shared helpers. The `bridge_incentives` integration suite locks
  the end-to-end flow under FIRST_PARTY_ONLY.
- CLI wallet tests now snapshot the `signer_metadata` vector end-to-end: the
  `fee_floor_warning` integration suite asserts the metadata array for ready and
  override previews, and the new `wallet_signer_metadata` module covers local,
  ephemeral, and session signers while checking the auto-bump telemetry event
  using first-party `JsonMap` builders. The suite no longer relies on mock RPC
  servers yet guarantees deterministic JSON output for FIRST_PARTY_ONLY runs.
- Wallet JSON previews now include a typed `signer_metadata` field, and unit
  tests assert on the JSON emitted for ready, needs-confirmation, ephemeral, and
  session flows while snapshotting the metadata array so FIRST_PARTY_ONLY runs
  cover the same payload the CLI prints in JSON mode. Service-badge and telemetry commands gained helper-backed tests that
  snapshot the JSON-RPC envelopes for `service_badge.verify`/`issue`/`revoke`
  and `telemetry.configure`, eliminating reliance on serde conversions or mock
  servers while keeping request construction on the shared builders. The mobile
  push notification and node difficulty examples have also been manualized,
  replacing their last `foundation_serialization::json!` literals with explicit
  `JsonMap` assembly so documentation tooling mirrors production payloads.

## Recent progress (2025-10-23)

- Contract CLI bridge commands now rely on the new `BridgeRpcTransport` trait.
  Production still wraps the existing `RpcClient`, but the CLI tests inject an
  in-memory `MockTransport` that records envelopes and returns canned responses.
  This removes the `JsonRpcMock` HTTP harness and async runtime dependency from
  the suite, keeping FIRST_PARTY_ONLY runs hermetic while preserving the
  end-to-end JSON builders.

## Recent progress (2025-10-21)

- Treasury CLI tests now rely solely on the manual builders: lifecycle coverage
  credits the store before executing disbursements, remote fetch tests validate
  `combine_treasury_fetch_results` with empty and populated history, and the
  suite no longer shells out to `JsonRpcMock` or calls
  `foundation_serialization::json::to_value`, keeping FIRST_PARTY_ONLY test
  runs entirely on the in-house facade.
- The contract CLI gained a shared `json_helpers` module; compute, service-badge,
  scheduler, telemetry, identity, config, bridge, and TLS commands now compose
  JSON-RPC payloads through explicit `JsonMap` builders and the helper’s
  envelope constructors. Governance disbursement listings serialize through a
  tiny typed wrapper, and the node runtime log sink plus staking/escrow wallet
  binary reuse the same manual builders, erasing the last `foundation_serialization::json!`
  macros from operator-facing RPC tooling.
- `telemetry::governance_webhook` no longer hides behind the `telemetry` feature;
  the node always posts to `GOV_WEBHOOK_URL` through the first-party HTTP
  client, so governance activation/rollback hooks fire on minimal builds without
  resorting to stub transports.
- The CLI networking stack (`contract net`, `gateway mobile-cache`, light-client
  device status, and wallet send) swapped every `foundation_serialization::json!`
  literal for explicit `JsonMap` builders and a reusable `RpcRequest` envelope,
  keeping JSON-RPC calls and error payloads on the first-party facade.
- `node/src/bin/net.rs` mirrors the same manual builders for peer stats, export,
  throttle, and backpressure utilities, eliminating macro literals from the
  operator tooling paths and keeping batch pagination deterministic.

## Recent progress (2025-10-20)

- Canonical transaction helpers now reuse the cursor encoder directly:
  `canonical_payload_bytes` forwards to `encode_raw_payload`,
  `verify_signed_tx` hashes signed transactions via the manual writer, the
  Python bindings decode with `decode_raw_payload`, and the CLI converts its
  payload struct before signing. RPC admission and fee regression tests no
  longer hit the `foundation_serde` stub when serializing RawTxPayload.
- Block, transaction, and gossip RPC-adjacent writers now call
  `StructWriter::write_struct` with inline `field_u8`/`field_u32` helpers so
  cursor layouts self-describe their field counts. The refreshed
  round-trip tests stop `Cursor(UnexpectedEof)` panics when RPC surfaces
  rehydrate blocks, blob transactions, or gossip payloads during
  snapshot/bootstrap flows.
- Peer statistics responders stopped using `foundation_serialization::json::to_value`;
  the new helper functions assemble drop/handshake maps and metric structs by hand,
  keeping `net.peer_stats_export_all` fully on the first-party JSON facade and
  removing the last serde-backed conversion from the networking RPC path.
- Compute-market RPC endpoints (`scheduler_stats`, `job_requirements`,
  `provider_hardware`, and settlement audit) now build responses via the shared
  JSON map helper, so capability snapshots, utilization maps, and audit rows no
  longer delegate to `json::to_value`. DEX escrow status/release handlers encode
  payment proofs and Merkle roots manually, eliminating the serde escape hatch
  while retaining the legacy payload shape, and fresh unit tests lock the sorted
  drop/handshake maps these responders consume.
- `peer_metrics_to_value` gained a focused regression that exercises nested drop
  and handshake maps plus throttle metadata, ensuring peer-stat RPC responses
  stay deterministic as we continue migrating bespoke builders to the shared
  helpers.
- Ledger persistence and RPC startup checks now stay entirely on the cursor
  helpers: `MempoolEntryDisk` stores a cached `serialized_size`, the mempool
  rebuild reads that byte length before re-encoding, and new `ledger_binary`
  unit tests cover the legacy decode helpers (`decode_block_vec`,
  `decode_account_map_bytes`, `decode_emission_tuple`, and the older
  five-field mempool entry layout). This keeps RPC snapshot consumers and
  ledger exporters on the first-party stack without invoking `binary_codec`.

## Recent progress (2025-10-19)

- Provider-profile RPC/storage tests now compute their reference payloads with
  the first-party cursor helper instead of `binary_codec::serialize`, keeping
  the binary regression suite green under `FIRST_PARTY_ONLY`.
- Gossip peer telemetry tests and the aggregator failover harness reuse the
  shared `peer_snapshot_to_value` builder, so JSON assertions no longer trigger
  serde-derived fallbacks during unit or integration runs.

## Recent progress (2025-10-18)

- The node RPC client now constructs envelopes through manual JSON maps and
  decodes responses by inspecting `foundation_serialization::json::Value`
  payloads. This removed the last `foundation_serde` derive invocations from
  client-side calls (`mempool.stats`, `mempool.qos_event`, `stake.role`,
  `inflation.params`) so `FIRST_PARTY_ONLY` builds no longer trigger stub
  panics when issuing RPC requests or parsing acknowledgements.
- Treasury RPC handlers expose typed `gov.treasury.disbursements`,
  `gov.treasury.balance`, and `gov.treasury.balance_history` endpoints using the
  shared request/response structs, and the new `node/tests/rpc_treasury.rs`
  integration test drives the HTTP server to lock cursor pagination and balance
  semantics. `contract gov treasury fetch` now consumes these endpoints via the
  first-party JSON facade and reports transport failures with actionable
  messaging, keeping CLI automation inside the dependency boundary. The metrics
  aggregator reuses the same sled snapshots, tolerates legacy string-encoded
  balance history, and emits warnings when disbursement history lacks matching
  balance entries, closing the remaining treasury observability gaps.

## Recent progress (2025-10-16)

- `foundation_serialization::json!` now supports nested object literals,
  identifier keys, and trailing commas. The regression suite in
  `crates/foundation_serialization/src/json_impl.rs` covers these cases so
  future refactors keep macro parity with serde_json.
- `foundation_serialization::json::Value` implements `Display`, so
  `Value::to_string()` now mirrors the legacy `serde_json::Value` behaviour. The
  new `display_matches_compact_serializer` regression test locks the rendered
  output to the compact serializer to catch future drift.
- `foundation_rpc::Request` gained `with_id`, `with_badge`, and `with_params`
  builders plus `id()`/`params()` accessors so RPC callers can compose envelopes
  without hand-written structs.
- `foundation_rpc::Response::into_payload` decodes typed success payloads while
  preserving the original RPC error. The `ResponsePayload<T>` helper exposes
  `into_result`/`map`/`id` APIs, and `node/src/rpc/client.rs` now routes every
  client call through the typed wrapper instead of bespoke envelopes.
- Bridge RPC handlers now accept typed request/response structs and reuse a
  shared commitment decoder, while `governance::Params::to_value`/`deserialize`
  let governance responders clone parameter envelopes without bespoke JSON
  maps. Explorer/CLI surfaces share the sled-backed treasury snapshots, keeping
  treasury RPC responses entirely first party.

## Proposed next steps

1. Harden the macro by mirroring serde_json’s diagnostics: add tests that assert
   on the emitted compile errors for invalid tokens so future changes surface
   friendly messages instead of type noise.
2. Update RPC handlers to clone owned `String` data before constructing JSON
   values or change the JSON object builder to accept references and clone
   internally.

With these blockers resolved we can rerun `FIRST_PARTY_ONLY=0 cargo check -p the_block`
and resume the staged removal of `jsonrpc-core`.
