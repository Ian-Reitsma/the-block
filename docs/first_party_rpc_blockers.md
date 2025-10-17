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

## Proposed next steps

1. Harden the macro by mirroring serde_json’s diagnostics: add tests that assert
   on the emitted compile errors for invalid tokens so future changes surface
   friendly messages instead of type noise.
2. Update RPC handlers to clone owned `String` data before constructing JSON
   values or change the JSON object builder to accept references and clone
   internally.

With these blockers resolved we can rerun `FIRST_PARTY_ONLY=0 cargo check -p the_block`
and resume the staged removal of `jsonrpc-core`.
