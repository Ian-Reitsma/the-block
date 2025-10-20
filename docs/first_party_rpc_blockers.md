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

## Recent progress (2025-10-20)

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
