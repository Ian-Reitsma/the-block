# Serialization Guardrails
> **Review (2025-10-10):** Captured the binary profile consolidation across node, crypto suite, telemetry, and harness tooling; the facade section below notes the new `BinaryProfile` identifiers and telemetry labels.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, and serialization facades are live with governance overrides enforced (2025-10-10). Governance, ledger, metrics aggregator, node runtime, and telemetry now encode exclusively through `foundation_serialization`.

The `foundation_serialization` crate fronts every workspace serialization call so
binary, JSON, TOML, and base58 payloads all use deterministic, auditable, and
first-party codecs. Direct `serde_json`, `serde_cbor`, or `bincode` calls are
considered legacy and must be routed through this facade before landing.

## Available helpers

The crate exposes dedicated modules for each supported format:

- `json` – streaming JSON encoder/decoder plus `Value` utilities.
- `binary` – compact encoder/decoder used for snapshot and state persistence.
  Named profiles (`canonical`, `transaction`, `gossip`, `storage_manifest`)
  surface through `codec::profiles` and are labelled in telemetry.
- `toml` – configuration loader backing operator- and test-facing config files.
- `base58` – first-party base58 encoder/decoder reused by the overlay store and
  tooling crates.

### JSON

```rust
use foundation_serialization::json::{self, Value};

let payload = json::to_string(&request)?;
let pretty = json::to_string_pretty(&request)?;
let decoded: Response = json::from_str(&payload)?;
let value: Value = json::value_from_str(&payload)?;
let bytes = json::to_vec(&request)?;
let same: Response = json::from_slice(&bytes)?;
```

`json::to_value`/`json::from_value` provide ergonomic bridges between strongly
typed structs and `Value`, while `json::to_vec_value`/`json::to_string_value`
render raw `Value` trees without requiring `Serialize`/`Deserialize` derives.
Tests and RPC helpers should use `json::value_from_str` when constructing manual
payloads so malformed fixtures fail loudly.

#### JSON `Value` convenience APIs

`Value` mirrors serde’s API surface and now exposes public numeric accessors so
callers can avoid ad-hoc pattern matching:

- `Value::as_object()` / `as_array()` / `as_str()` – borrow structured data.
- `Value::as_i64()` / `as_u64()` / `as_f64()` – extract numeric fields via the
  canonical `Number` wrapper.
- `Number::as_i64()` / `as_u64()` / `as_f64()` – convert to primitive types while
  rejecting non-finite or fractional inputs when a whole number is required.

These helpers back the metrics aggregator, governance history loader, and
ledger migration CLI. Use them instead of indexing into maps (`value["field"]`)
so tests surface missing or misspelled keys immediately.

### Binary

```rust
use foundation_serialization::binary;

let bytes = binary::encode(&snapshot)?;
let snapshot: Snapshot = binary::decode(&bytes)?;
```

Binary helpers guarantee deterministic layout and reject trailing data, making
it safe to persist validator and storage-engine state without third-party
codecs.

### TOML

```rust
use foundation_serialization::toml;

let config: OperatorConfig = toml::from_str(&contents)?;
```

Configuration loaders surface the shared `foundation_serialization::Error`
variant and reuse the TOML parser that backs runtime configuration and tests.

## Error model

All helpers return `foundation_serialization::Result<T>` using the shared
`Error` enum. Errors encode the failing codec (JSON, binary, or TOML), the
operation, and the underlying reason so downstream callers can attach context or
bubble the message into diagnostics.

## Review checklist

Before merging changes that touch persistence, network payloads, or operator
configuration:

1. Import `foundation_serialization` helpers instead of calling `serde_json`,
   `serde_cbor`, or `bincode` directly.
2. Select an existing helper (`json`, `binary`, `toml`, or `base58`) or extend
   the crate with new functionality and document it here.
3. Prefer `json::value_from_*` when parsing ad-hoc fixtures so malformed data
   fails the test harness.
4. When working with JSON trees, use the `Value::as_*` accessors rather than
   indexing into objects to avoid silent defaulting.
5. Update or add unit tests in `crates/foundation_serialization` for new helpers
   including corrupted payload cases.

Following this checklist keeps the serialization facade authoritative and
prevents format drift across the workspace.
