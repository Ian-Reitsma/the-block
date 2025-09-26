# Serialization Guardrails
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; align codecs with the centralized wrapper before
> touching persistence or wire formats.

The `codec` crate fronts every workspace serialization call so that bincode, JSON, and CBOR payloads all use consistent
configuration, error handling, and telemetry. Direct `serde_json`, `serde_cbor`, or `bincode` calls are considered legacy
and must be routed through the wrapper before landing.

## Available codecs

The crate exposes a `Codec` enum plus named profiles under `codec::profiles`:

- `profiles::transaction()` – canonical bincode configuration for signed transactions and payload hashing.
- `profiles::gossip()` – bincode profile for gossip relay persistence.
- `profiles::storage_manifest()` – bincode profile for storage manifests.
- `profiles::json()` – canonical JSON encoder/decoder.
- `profiles::cbor()` – canonical CBOR encoder/decoder.

Helper APIs provide the common ergonomics:

```rust
let bytes = codec::serialize(profiles::transaction(), &payload)?;
let value: Payload = codec::deserialize(profiles::transaction(), &bytes)?;
let json = codec::serialize_to_string(profiles::json(), &value)?;
let pretty = codec::serialize_json_pretty(&value)?;
let parsed: serde_json::Value = codec::deserialize_from_str(profiles::json(), &json_str)?;
```

Types that implement `serde::Serialize`/`Deserialize` can opt into the blanket `CodecMessage` trait for convenience when a
`Codec` value is already in scope.

## Error model

All helpers return `codec::Result<T>` using the shared `codec::Error` type. Errors include the codec profile, operation
(`serialize`/`deserialize`), and any wrapped serde or UTF-8 failures so downstream callers can surface actionable messages.

## Telemetry hooks

When the crate’s optional `telemetry` feature is enabled it emits:

- `codec_payload_bytes` – histogram tracking serialized byte length with `codec`, `direction`, and optional `profile` labels.
- `codec_operation_fail_total` – counter incremented whenever a serialization or deserialization attempt fails.

These hooks power dashboards that highlight codec regressions and keep payload sizes within expected bounds.

## Review checklist

Before merging changes that touch persistence or network payloads:

1. Import `codec` helpers instead of calling `serde_json`, `serde_cbor`, or `bincode` directly.
2. Select an existing profile (`transaction`, `gossip`, `storage_manifest`, `json`, or `cbor`) or add a new one with docs.
3. Ensure human-facing output goes through `codec::serialize_to_string`/`serialize_json_pretty` to retain the unified
   error type.
4. Update or add tests in `crates/codec` for new formats, including corrupted payload cases.
5. Document any new profiles or helpers in this file and reference the relevant metrics labels when appropriate.

Following this checklist keeps the codec abstraction authoritative and prevents format drift across the workspace.
