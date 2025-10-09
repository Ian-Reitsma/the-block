# First-Party RPC Migration Blockers

The workspace currently fails to compile when `foundation_serialization::json!` is
expanded across the RPC modules. The legacy handler implementations rely on
`jsonrpc_core::types::response::Output` semantics, including borrowed string
literals and raw map/object literals that assume serde_json’s macro semantics.

## Immediate blockers

- `foundation_serialization::json!` lacks a faithful reproduction of
  `serde_json::json!`’s token-munching behaviour. Nested object literals (e.g.
  `{ "error": { "code": -32075, "message": "relay_only" } }`) are not parsed
  correctly, leading to compiler errors at every colon token.
- RPC responses still depend on `Cow<'static, str>` message fields provided by
  `jsonrpc_core`. Converting them to first-party envelopes requires either new
  strongly-typed response structs or explicit `Cow` wrappers.
- Downstream consumers expect `Value::to_string()` to be available; the new
  facade returns `foundation_serialization::json::Value` which does not implement
  `Display` or `ToString`.

## Proposed next steps

1. Port the `serde_json::json!` macro implementation (MIT/Apache 2.0) and
   replace the final conversion hooks with `foundation_serialization` helpers so
   nested literals behave identically to the legacy macro.
2. Introduce typed response wrappers in `foundation_rpc` that mirror the fields
   exposed today and provide `impl From<FirstPartyResponse> for Value` to keep
   older call-sites compiling while we finish the migration.
3. Add `Value::to_string_value` glue helpers to avoid sprinkling
   pretty-printing/pipeline calls across the RPC layer.
4. Update RPC handlers to clone owned `String` data before constructing JSON
   values or change the JSON object builder to accept references and clone
   internally.

With these blockers resolved we can rerun `FIRST_PARTY_ONLY=0 cargo check -p the_block`
and resume the staged removal of `jsonrpc-core`.
