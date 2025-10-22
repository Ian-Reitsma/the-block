# First-Party RPC Migration Blockers

RPC handlers now build envelopes entirely through the first-party stack: the
`foundation_serialization::json!` macro mirrors serde_json’s token-munching
behaviour (nested objects, identifier keys, trailing commas), and the typed
`foundation_rpc::ResponsePayload<T>` helper keeps success/error decoding inside
the facade. Legacy serde_json references have been scrubbed from the production
RPC client.

## Immediate blockers

- Sweep remaining RPC responders for legacy helper usage (hand-written structs,
  bespoke payload builders) and migrate them to the shared request/response
  builders so tests cover the unified surface.
- RPC responses still depend on `Cow<'static, str>` message fields provided by
  `jsonrpc_core`. Converting them to first-party envelopes requires either new
  strongly-typed response structs or explicit `Cow` wrappers. ✅ *Resolved — the
  new `foundation_rpc::ResponsePayload<T>` wrapper exposes typed success/error
  branches while preserving the first-party `RpcError` struct, allowing
  handlers and clients to decode responses without the legacy envelope.*

## Recent progress (2025-10-23)

- Governance-backed bridge reward claims now emit first-party JSON. `bridge.claim_rewards`
  and `bridge.reward_claims` share typed request/response structs, while the CLI
  mirrors the same builders for `blockctl bridge claim` and `reward-claims` so
  payout reconciliation avoids serde fallbacks. Settlement proofs and dispute
  audits ride the same stack through `bridge.submit_settlement`,
  `bridge.settlement_log`, and `bridge.dispute_audit`, with CLI mirrors that reuse
  the shared JSON helpers. Partial channel reconfiguration (`bridge.configure_asset`)
  accepts optional fields/clear flags without clobbering existing values, and
  unit tests in `governance/src/store.rs` and `node/src/governance/store.rs`
  verify reward approvals persist across reopen.

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
