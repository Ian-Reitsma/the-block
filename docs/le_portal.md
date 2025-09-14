# Law-Enforcement Portal Logging and Warrant Canaries

The `le_portal` module records law-enforcement requests and publishes warrant
canaries so operators can prove absence of secret orders. All records are stored
as plain text files under a configurable base directory and never leak case
identifiers or messages in cleartext.

## Request Logging

`record_request` hashes the caller-supplied `case_id` with BLAKE3 and appends a
JSON line containing the timestamp, agency name, jurisdiction, caller language,
and resulting `case_hash` to `le_requests.log`
[`node/src/le_portal.rs#L17-L36`](../node/src/le_portal.rs#L17-L36). The function
returns the 64‑hex character hash so operators can audit inclusion without
exposing the original identifier. Requests are retrieved via
`list_requests`, which parses the log into `LeRequest` entries and gracefully
returns an empty list when no log exists
[`node/src/le_portal.rs#L38-L55`](../node/src/le_portal.rs#L38-L55).

To export requests, copy the log file or stream it through tooling that filters
by agency or time. Because only hashed case IDs are stored, disclosure does not
reveal sensitive details.

## Warrant Canary

`record_canary` writes a space-separated `<timestamp> <hash>` line to
`warrant_canary.log`, where the hash is BLAKE3 over the operator-supplied
message [`node/src/le_portal.rs#L57-L68`](../node/src/le_portal.rs#L57-L68). A
fresh canary should be published on a regular cadence (e.g., daily). Observers
verify freshness by recomputing the hash and checking that the latest timestamp
falls within the expected window. Absence or delay signals potential gag orders.

## Operational Guidance

1. **Base directory** – The RPC layer passes a filesystem root into each
   function, allowing tests and production deployments to isolate logs per
   instance. Use a secure location with restricted permissions.
2. **Exporting requests** – Periodically ship `le_requests.log` to an archival
   system. Provide agencies with the returned `case_hash` so they can verify
   their entry without revealing identifiers to outsiders.
3. **Canary verification** – Share the latest canary hash publicly (e.g., on a
   website or transparency log). Watchers can compare it against local hashes of
   the expected message to ensure no secret requests have been received.
4. **Privacy posture** – Only cryptographic hashes of case identifiers or
   messages are stored; no IPs or account data are logged. Operators must still
   disclose logging practices in privacy policies and comply with local laws.
5. **Governance enablement** – Nodes may expose portal RPCs only when governance
   policies permit. Review `docs/jurisdiction.md` for region‑specific
   requirements and retention policies.

## Audit Trail & Badge Policy

`record_action` appends an agency-tagged, jurisdiction-tagged, timestamped hash to `le_actions.log`
to provide a tamper-evident trail of law-enforcement operations
[`node/src/le_portal.rs#L69-L98`](../node/src/le_portal.rs#L69-L98). Access to
`record_le_request`, `warrant_canary`, and the new audit endpoints is gated by
service badges. Badges expire after a governance-controlled interval and can be
renewed without downtime. Metrics `badge_issued_total` and
`badge_revoked_total` track lifecycle events, while policy updates flow through
the governance parameters `BadgeExpirySecs`, `BadgeIssueUptime`, and
`BadgeRevokeUptime`.

## Evidence Submission Workflow

`record_evidence` accepts binary artifacts associated with a hashed case ID and
stores them under `evidence/<hash>` alongside a JSON log entry in
`le_evidence.log`. Entries from `record_request`, `record_action`, and
`record_evidence` are also appended to the chain state's `le_audit.log` for
on-chain attestation. The RPC method `le.upload_evidence` exposes this
functionality and is guarded behind service badges.

CLI utilities `service-badge issue` and `service-badge revoke` manage badge
lifecycle, while `service-badge verify` checks validity. Scripts in
`examples/badge_workflow/` demonstrate a full issuance and revocation flow.

## Example

```rust
use the_block::le_portal::{record_request, list_requests, record_canary};

let base = "/var/log/the-block";
let hash = record_request(base, "Agency", "case123", "US", "en")?;
println!("case hash: {hash}");
let entries = list_requests(base)?;
assert_eq!(entries[0].case_hash, hash);
record_canary(base, "no requests")?;
```

The companion tests in
[`node/tests/le_portal.rs`](../node/tests/le_portal.rs) exercise both logging
paths and verify canary hashing.
