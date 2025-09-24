# Jurisdiction Policies and Law-Enforcement Logging
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The `jurisdiction` crate provides region-specific policy packs and an optional
post-quantum (PQ) encrypted logging helper so operators can comply with local
regulations while keeping transparency logs intact.

## Policy Packs

A `PolicyPack` is a JSON file describing the default consent rules and feature
toggles for a particular region.  The loader accepts any path and deserializes
it into the following structure:

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

```rust
use jurisdiction::PolicyPack;
let pack = PolicyPack::load("/etc/the-block/policy.json")?;
assert!(pack.consent_required);
```

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
policy packs and Prometheus labels, or use orchestration tooling to mount the
appropriate pack per region.

Example packs live under [`examples/jurisdiction/`](../examples/jurisdiction/)
for quick testing.

Validate a custom pack before deployment with `tools/jurisdiction_check.rs`:

```
rustc tools/jurisdiction_check.rs && ./jurisdiction_check examples/jurisdiction/us.json
```

## Law-Enforcement Request Log

`log_law_enforcement_request` appends a metadata string to a log file so
operators can publish transparency reports.  When compiled with the `pq`
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