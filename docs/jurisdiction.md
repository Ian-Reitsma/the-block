# Jurisdiction Policies and Law-Enforcement Logging
> **Review (2025-09-25):** Synced Jurisdiction Policies and Law-Enforcement Logging guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The `jurisdiction` crate provides region-specific policy packs and an optional
post-quantum (PQ) encrypted logging helper so operators can comply with local
regulations while keeping transparency logs intact.

## Policy Packs

A `PolicyPack` is a JSON file describing the default consent rules and feature
toggles for a particular region.  The loader accepts any path and validates it
through handwritten JSON conversions (no serde derives remain) before building
the following structure:

```json
{
  "region": "US",
  "consent_required": true,
  "features": ["wallet", "staking"]
}
```

- `region` – free-form label such as `US` or `EU` used to select the pack.
- `consent_required` – whether users must opt in before the node enables
  optional modules.
- `features` – list of module names (e.g., `wallet`, `staking`) that are
  enabled when this pack is active.

The crate exposes helpers for every layer that needs to work with the raw JSON
value instead of touching serde:

```rust
use jurisdiction::{PolicyPack, SignedPack};

// Load from disk or from an in-memory JSON Value.
let pack = PolicyPack::load("/etc/the-block/policy.json")?;
let roundtrip = PolicyPack::from_json_value(&pack.to_json_value())?;
assert_eq!(pack, roundtrip);

// Signed registry entries convert through the same helpers.
let signed = SignedPack::from_json_slice(include_bytes!("/tmp/signed-pack.json"))?;
let json_value = signed.to_json_value();
```

Manual conversion keeps FIRST_PARTY_ONLY builds green while surfacing precise
errors (field name + expectation) when policy packs are malformed. Packs now
expose a typed `PolicyDiff` helper that records consent/feature deltas without
falling back to raw JSON: callers receive `Change<bool>`/`Change<Vec<String>>`
records, can render them via `PolicyDiff::to_json_value()`, or parse stored
diffs with `PolicyDiff::from_json_value()` when replaying governance history.

The crate also provides binary codecs implemented on top of the shared
`foundation_serialization::binary_cursor` helpers:

```rust
use jurisdiction::{encode_policy_pack, decode_policy_pack};

let bytes = encode_policy_pack(&pack);
let restored = decode_policy_pack(&bytes)?;
assert_eq!(restored, pack);
```

`encode_signed_pack`/`decode_signed_pack` and the matching
`encode_policy_diff`/`decode_policy_diff` helpers keep sled snapshots and
integration fixtures byte-stable without depending on serde or the legacy
`binary_codec` shim. Regression tests in
[`crates/jurisdiction/tests/codec.rs`](../crates/jurisdiction/tests/codec.rs)
cover pack/diff round-trips, and the workspace
[`tests/jurisdiction_dynamic.rs`](../tests/jurisdiction_dynamic.rs) suite
exercises the typed diff API end to end.

Policy packs live alongside the node configuration and can be swapped without
recompiling.  Governance may distribute canonical packs and validators can load
them at runtime.  Built‑in templates are available via `PolicyPack::template("US")`
or `PolicyPack::template("EU")` and can be used as starting points.

Governance proposals may update the active jurisdiction by voting on the
`jurisdiction_region` parameter.  When executed, the node records the change via
the law‑enforcement portal so auditors have a tamper‑evident trail of policy
changes.  See [`node/src/governance/params.rs`](../node/src/governance/params.rs)
for the runtime hook.

To run a node with a specific jurisdiction pack pass the country code or file
path via `--jurisdiction`:

```
the-block-node run --jurisdiction US
```

The active jurisdiction is exposed over RPC through `jurisdiction.status` and
can be updated at runtime with `jurisdiction.set` (admin token required).
Transactions processed while a policy is active are tagged with the region in
the `tx_jurisdiction_total{jurisdiction="US"}` metric.

For multi‑jurisdiction deployments run separate node instances with distinct
policy packs and telemetry labels, or use orchestration tooling to mount the
appropriate pack per region.

Example packs live under [`examples/jurisdiction/`](../examples/jurisdiction/)
for quick testing.

Persist signed registry entries with the dual-format helper so both legacy JSON
tools and the new sled snapshots stay in sync:

```rust
use jurisdiction::{load_signed_pack, persist_signed_pack};

persist_signed_pack("/etc/the-block/policy.json", &signed)?;
let restored = load_signed_pack("/etc/the-block/policy.json")?;
assert_eq!(restored, signed);
```

`persist_signed_pack` writes the JSON file alongside a `.bin` companion encoded
via the manual cursor helpers; `load_signed_pack` prefers JSON but falls back to
the binary snapshot so operators can drop serde tooling without losing history.
Both flows surface precise IO/validation errors and power the new codec
regression tests.

Validate a custom pack before deployment with `tools/jurisdiction_check.rs`:

```
rustc tools/jurisdiction_check.rs && ./jurisdiction_check examples/jurisdiction/us.json
```

## Law-Enforcement Request Log

`log_law_enforcement_request` appends a metadata string to a log file so
operators can publish transparency reports.  Each append now emits a
`diagnostics::log::info!` record that mirrors the on-disk write, allowing
aggregators and operators to trace law-enforcement activity without the third-
party `log` crate.  When compiled with the `pq`
feature flag, metadata is encrypted using the Kyber1024 KEM before being
base64‑encoded and written:

```rust
use jurisdiction::log_law_enforcement_request;
log_law_enforcement_request("/var/log/le.log", "case-123")?;
```

Without `pq`, the metadata is written verbatim.  Each call opens (or creates)
the target file and appends a line terminated with `\n`.  Logs should be stored
in a secure location with strict permissions; operators are responsible for
rotation and retention policies.

## Operational Notes

1. **Distribution** – Nodes load policy packs from disk; governance can publish
   signed packs to a registry so operators verify authenticity before use.
2. **PQ Encryption** – Enable the `pq` feature at compile time to ensure all log
   entries are post-quantum protected.  The helper generates an ephemeral key
   per call and prepends the ciphertext to the plaintext before encoding.
3. **Transparency** – The log contains metadata only and never records IP
   addresses or user content.  Provide external auditors with the raw file so
   they can hash and verify entries independently.
4. **Retention** – Jurisdictions often mandate minimum retention windows;
   document your schedule and publish warrant canaries via
   [`docs/le_portal.md`](le_portal.md).

The companion tests in
[`node/tests/jurisdiction_packs.rs`](../node/tests/jurisdiction_packs.rs)
exercise both the JSON loader and the log helper.
